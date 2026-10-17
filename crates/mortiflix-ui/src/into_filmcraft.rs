//! Into FilmCraft (doc 38): "Edit the video" builds one FilmCraft project from the node graph, once.
//!
//! The service prepares one media file per piece and version and sends the hand-off plan
//! (`filmcraft.plan`, see `docs/mortiflix/protocol.md`). The host runs [`build`] in-process inside
//! one undo step ("Mortiflix: Bring … into FilmCraft"): a sequence at the project's size and rate,
//! bins per beat and kind (Shots › 3 Winter, Voice, Music, Music stems, Your assets, Older versions,
//! Cut on the board), every current shot on V1 back to back, voice lines on their own audio track at
//! their times, the music cut at its cues, FilmCraft's own transitions for the board's dissolves and
//! dips, clip and item labels = the beat's colour, a Chapter marker per beat (`3 Winter`), a Comment
//! marker per music cue, caption clips for the voice lines and the project notes.
//!
//! After that it is never rebuilt by itself: a new version of a piece (made from here or from the
//! node area) is swapped in with `clip.replaceFromBin` ([`swap`]), keeping the clip's place and
//! trims, and the old one moves to Older versions. The Mortiflix panel on stage 5 ([`show`]) is
//! about the clicked clip: its beat, times, versions (Use this one), the talk, and the ask box. A
//! trim FilmCraft can make itself is done at once as one undo step named "Mortiflix: …"; anything
//! that needs new media becomes an edit request on that node (`node.edit` fix).
//!
//! Nothing here touches FilmCraft directly: [`build`], [`swap`] and [`use_version`] take an
//! executor (the host passes `Session::execute`), and the panel pushes commands through `MfxCtx`.

use std::collections::BTreeMap;

use egui::{Color32, RichText, Sense, Stroke, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{MfxCtx, MfxState, Service};

/// Most pieces, beats, cues and transitions read from one plan.
pub const MAX_PIECES: usize = 2000;
pub const MAX_BEATS: usize = 500;
/// FilmCraft's tick rate (`filmcraft_time::Tick`): 254 016 000 000 per second.
pub const TICKS_PER_SECOND: i64 = 254_016_000_000;
/// Longest time read from a plan (a day).
const MAX_SECONDS: f64 = 86_400.0;
/// How often the panel asks the service for new versions while the edit is open.
const POLL_SECONDS: f64 = 4.0;
/// Most talk lines and change rows kept.
const MAX_TALK: usize = 400;

/// Runs one FilmCraft command (the host passes `Session::execute`).
pub type Exec<'a> = dyn FnMut(&str, Value) -> Result<Value, String> + 'a;

// ---------------------------------------------------------------- the plan (from the service)

/// The hand-off plan (`filmcraft.plan`). Every value is hostile: [`Plan::parse`] bounds it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Plan {
    pub project: String,
    pub title: String,
    pub rev: u64,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub seconds: f64,
    /// Where the project is saved (`<project>/out/filmcraft/<title>.fcproj`).
    pub fcproj: String,
    pub beats: Vec<PlanBeat>,
    pub pieces: Vec<Piece>,
    pub transitions: Vec<PlanTransition>,
    pub cues: Vec<Cue>,
    pub notes: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanBeat {
    pub id: String,
    pub number: u32,
    pub name: String,
    /// FilmCraft label name (Violet, Mango, ...).
    pub colour: Option<String>,
    /// The beat's first script line (the marker comment).
    pub comment: String,
    pub start_sec: Option<f64>,
}

/// One piece at one version: a shot, a voice line, the music, a stem, a sound or an attached asset.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Piece {
    pub node: String,
    /// shot, line, music, stem, sound, asset
    pub kind: String,
    pub label: String,
    pub title: String,
    pub beat: Option<String>,
    pub version: u32,
    /// This is the node's current version (older ones go to the Older versions bin).
    pub current: bool,
    /// Cut on the board: in the bin only.
    pub cut: bool,
    pub file: String,
    pub seconds: Option<f64>,
    /// Where it goes on the sequence; None = bin only.
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    /// A voice line's words (the caption).
    pub text: String,
    /// Still a preview render (the Export render makes it full quality).
    pub preview: bool,
    /// Where in the media file the piece starts (a shot cut from the animatic).
    pub source_in_sec: Option<f64>,
    /// video, still, card (no picture yet: a colour matte), audio
    pub media: String,
    /// The FilmCraft label the clip and item carry (the beat's colour).
    pub colour: Option<String>,
    /// The bin path ("Shots/3 Winter"); None = by kind.
    pub bin: Option<String>,
    /// A card's colour (#rrggbb).
    pub card_hex: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PlanTransition {
    pub node: String,
    pub label: String,
    pub name: String,
    pub from: Option<String>,
    pub to: Option<String>,
    /// FilmCraft's own transition (cross_dissolve, dip_to_black...), or None for a plain cut.
    pub effect: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cue {
    pub node: String,
    pub name: String,
    pub beat: Option<String>,
    pub colour: Option<String>,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
}

fn clean_sec(v: Option<f64>) -> Option<f64> {
    v.filter(|s| s.is_finite() && *s >= 0.0 && *s <= MAX_SECONDS)
}

fn short(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl Plan {
    /// Read the service's `handoff.plan` reply (protocol: sequence, clips, markers, captions,
    /// your_assets, older_versions, cut_on_board) into a plan, then bound it ([`Plan::parse`]).
    pub fn from_handoff(v: &Value) -> Result<Plan, String> {
        if !v.is_object() {
            return Err("the plan from Mortiflix couldn't be read".into());
        }
        let s = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
        let f = |o: &Value, k: &str| o.get(k).and_then(Value::as_f64);
        let u = |o: &Value, k: &str| o.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(1);
        let opt = |o: &Value, k: &str| o.get(k).and_then(Value::as_str).filter(|x| !x.is_empty()).map(str::to_string);
        let list = |k: &str| v.get(k).and_then(Value::as_array).map(|a| a.iter().take(MAX_PIECES).collect::<Vec<_>>()).unwrap_or_default();
        let seq = v.get("sequence").cloned().unwrap_or(Value::Null);
        let num_u32 = |o: &Value, k: &str| o.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
        let mut plan = Plan {
            project: s(v, "project"),
            title: opt(v, "title").unwrap_or_else(|| s(&seq, "name")),
            rev: v.get("rev").and_then(Value::as_u64).unwrap_or(0),
            width: num_u32(&seq, "width"),
            height: num_u32(&seq, "height"),
            fps: f(&seq, "fps").unwrap_or(30.0),
            seconds: f(&seq, "length_sec").unwrap_or(0.0),
            fcproj: s(v, "fcproj"),
            notes: s(v, "notes"),
            ..Plan::default()
        };
        // beats: the chapter markers ("3 Winter" at the beat's first shot)
        for m in list("markers") {
            let name = s(m, "name");
            if s(m, "kind") == "chapter" {
                let (no, rest) = name.split_once(' ').unwrap_or(("", name.as_str()));
                let number = no.parse::<u32>().unwrap_or(u32::try_from(plan.beats.len() + 1).unwrap_or(0));
                let rest = if no.parse::<u32>().is_ok() { rest.to_string() } else { name.clone() };
                plan.beats.push(PlanBeat {
                    id: s(m, "node"),
                    number,
                    name: rest,
                    colour: opt(m, "label"),
                    comment: s(m, "comment"),
                    start_sec: f(m, "time_sec"),
                });
            } else {
                plan.cues.push(Cue { node: s(m, "node"), name, beat: None, colour: opt(m, "label"), start_sec: f(m, "time_sec"), end_sec: None });
            }
        }
        let caption =
            |node: &str| list("captions").into_iter().find(|c| c.get("node").and_then(Value::as_str) == Some(node)).map(|c| (s(c, "text"), f(c, "end_sec")));
        let beat_of_bin = |bin: &str, beats: &[PlanBeat]| -> Option<String> {
            let tag = bin.strip_prefix("Shots/")?;
            let no = tag.split(' ').next()?.parse::<u32>().ok()?;
            beats.iter().find(|b| b.number == no).map(|b| b.id.clone())
        };
        let media_of = |c: &Value| c.get("media").cloned().unwrap_or(Value::Null);
        for c in list("clips") {
            let kind = s(c, "kind");
            let media = media_of(c);
            let mut piece = Piece {
                node: s(c, "node"),
                kind: kind.clone(),
                label: s(c, "node").to_uppercase(),
                title: s(c, "name"),
                version: u(c, "version"),
                current: true,
                file: s(&media, "path"),
                start_sec: f(c, "start_sec"),
                seconds: f(c, "dur_sec"),
                preview: c.get("preview").and_then(Value::as_bool).unwrap_or(false),
                source_in_sec: f(&media, "from_sec"),
                media: s(&media, "kind"),
                colour: opt(c, "label"),
                bin: opt(c, "bin"),
                card_hex: opt(&media, "colour"),
                ..Piece::default()
            };
            match kind.as_str() {
                "transition" => {
                    let native = c.get("native").and_then(Value::as_bool).unwrap_or(false);
                    let name = s(c, "transition");
                    let effect = if native && !name.eq_ignore_ascii_case("cut") { Some(name.clone()) } else { None };
                    plan.transitions.push(PlanTransition {
                        node: piece.node.clone(),
                        label: piece.title.clone(),
                        name,
                        from: opt(c, "from"),
                        to: opt(c, "to"),
                        effect: if native { effect } else { None },
                    });
                    continue;
                }
                "line" => {
                    if let Some((text, end)) = caption(&piece.node) {
                        piece.text = text;
                        piece.end_sec = end;
                    }
                    if piece.end_sec.is_none() {
                        piece.end_sec = piece.start_sec.zip(piece.seconds).map(|(a, b)| a + b);
                    }
                }
                "music" => {
                    // the cue clips: their times from the clip, names from the comment markers
                    let cues = c.get("cues").and_then(Value::as_array).cloned().unwrap_or_default();
                    for cue in cues.iter().take(MAX_BEATS) {
                        let node = s(cue, "node");
                        let (start, end) = (f(cue, "start_sec"), f(cue, "end_sec"));
                        match plan.cues.iter_mut().find(|x| x.node == node) {
                            Some(x) => {
                                x.end_sec = end;
                                x.start_sec = x.start_sec.or(start);
                            }
                            None => plan.cues.push(Cue {
                                node: node.clone(),
                                name: node.to_uppercase(),
                                beat: None,
                                colour: opt(cue, "label"),
                                start_sec: start,
                                end_sec: end,
                            }),
                        }
                    }
                }
                _ => {}
            }
            if kind == "shot" {
                piece.beat = piece.bin.as_deref().and_then(|b| beat_of_bin(b, &plan.beats));
            }
            if kind != "shot" {
                piece.media = if piece.media.is_empty() { "audio".into() } else { piece.media };
            }
            plan.pieces.push(piece);
        }
        let side = |o: &Value, cut: bool| Piece {
            node: s(o, "node"),
            kind: if cut { "shot".into() } else { "older".into() },
            label: s(o, "node").to_uppercase(),
            title: s(o, "name"),
            version: u(o, "version"),
            current: cut,
            cut,
            file: s(o, "path"),
            source_in_sec: f(o, "from_sec"),
            seconds: f(o, "to_sec").zip(f(o, "from_sec").or(Some(0.0))).map(|(b, a)| b - a).filter(|d| *d > 0.0),
            colour: opt(o, "label"),
            media: "video".into(),
            ..Piece::default()
        };
        for o in list("older_versions") {
            plan.pieces.push(side(o, false));
        }
        for o in list("cut_on_board") {
            plan.pieces.push(side(o, true));
        }
        for a in list("your_assets") {
            plan.pieces.push(Piece {
                node: format!("asset:{}", s(a, "id")),
                kind: "asset".into(),
                label: s(a, "name"),
                title: s(a, "name"),
                version: 1,
                current: true,
                file: s(a, "path"),
                bin: opt(a, "bin"),
                media: s(a, "kind"),
                ..Piece::default()
            });
        }
        if plan.fcproj.is_empty() {
            plan.fcproj = default_fcproj(&plan);
        }
        let value = serde_json::to_value(&plan).map_err(|e| e.to_string())?;
        Plan::parse(&value)
    }

    /// Read and bound a plan from the service's reply.
    pub fn parse(v: &Value) -> Result<Plan, String> {
        let mut p: Plan = serde_json::from_value(v.clone()).map_err(|e| format!("the plan from Mortiflix couldn't be read: {e}"))?;
        p.title = short(p.title.trim(), 120);
        if p.title.is_empty() {
            p.title = "Mortiflix".into();
        }
        if !(1.0..=240.0).contains(&p.fps) {
            p.fps = 30.0;
        }
        if !(16..=8192).contains(&p.width) || !(16..=8192).contains(&p.height) {
            (p.width, p.height) = (1920, 1080);
        }
        p.seconds = clean_sec(Some(p.seconds)).unwrap_or(0.0);
        p.notes = short(&p.notes, 4000);
        p.beats.truncate(MAX_BEATS);
        for b in &mut p.beats {
            b.name = short(&b.name, 60);
            b.comment = short(&b.comment, 300);
            b.start_sec = clean_sec(b.start_sec);
            b.colour = b.colour.take().filter(|c| label_rgb(c).is_some());
        }
        p.pieces.truncate(MAX_PIECES);
        p.pieces.retain(|x| !x.node.is_empty());
        for x in &mut p.pieces {
            x.label = short(&x.label, 80);
            x.title = short(&x.title, 200);
            x.text = short(&x.text, 500);
            x.start_sec = clean_sec(x.start_sec);
            x.end_sec = clean_sec(x.end_sec);
            x.seconds = clean_sec(x.seconds);
            x.source_in_sec = clean_sec(x.source_in_sec);
            x.colour = x.colour.take().filter(|c| label_rgb(c).is_some());
            x.bin = x.bin.take().map(|b| short(&b, 200)).filter(|b| !b.is_empty());
            x.card_hex = x.card_hex.take().filter(|h| h.len() == 7 && h.starts_with('#') && h.chars().skip(1).all(|c| c.is_ascii_hexdigit()));
        }
        // a card has no file (a colour matte is made for it); everything else needs one
        p.pieces.retain(|x| x.media == "card" || !x.file.is_empty());
        p.transitions.truncate(MAX_BEATS);
        p.cues.truncate(MAX_BEATS);
        for c in &mut p.cues {
            c.name = short(&c.name, 80);
            c.start_sec = clean_sec(c.start_sec);
            c.end_sec = clean_sec(c.end_sec);
            c.colour = c.colour.take().filter(|x| label_rgb(x).is_some());
        }
        Ok(p)
    }
}

/// Where the project is saved when the service names no `.fcproj`: beside the project's own
/// record (`<studio>/state/<id>/filmcraft/<title>.fcproj`, found from a piece's media path).
fn default_fcproj(plan: &Plan) -> String {
    let title: String = plan.title.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.')).collect();
    let title = if title.trim().is_empty() { "Mortiflix".to_string() } else { title.trim().to_string() };
    for p in &plan.pieces {
        if let Some((state, _)) = p.file.split_once("/canvas/media/") {
            return format!("{state}/filmcraft/{title}.fcproj");
        }
    }
    String::new()
}

/// FilmCraft's label colours (`filmcraft_project::Label::rgb`), by name.
pub fn label_rgb(name: &str) -> Option<Color32> {
    let rgb = match name.to_ascii_lowercase().as_str() {
        "violet" => [0x38, 0x0e, 0xa7],
        "iris" => [0x1d, 0x4a, 0x64],
        "caribbean" => [0x35, 0x54, 0x18],
        "lavender" => [0x6b, 0x1c, 0x82],
        "cerulean" => [0x23, 0x53, 0x5a],
        "forest" => [0x40, 0x4a, 0x11],
        "rose" => [0x80, 0x18, 0x36],
        "mango" => [0x80, 0x3f, 0x17],
        "purple" => [0x59, 0x0d, 0xb0],
        "blue" => [0x19, 0x2d, 0x94],
        "teal" => [0x1f, 0x4d, 0x45],
        "magenta" => [0x79, 0x1c, 0x56],
        "tan" => [0x6c, 0x5b, 0x47],
        "green" => [0x29, 0x5c, 0x2d],
        "brown" => [0x58, 0x3d, 0x14],
        "yellow" => [0x6e, 0x66, 0x28],
        _ => return None,
    };
    Some(Color32::from_rgb(rgb[0], rgb[1], rgb[2]))
}

/// The frame rate as an exact fraction (NTSC rates are n·1000/1001).
fn fps_fraction(fps: f64) -> (i128, i128) {
    for (num, den) in [(24_000, 1001), (30_000, 1001), (60_000, 1001), (120_000, 1001)] {
        if (fps - num as f64 / den as f64).abs() < 0.005 {
            return (num, den);
        }
    }
    if (fps - fps.round()).abs() < 1e-6 {
        return (fps.round() as i128, 1);
    }
    ((fps * 1000.0).round() as i128, 1000)
}

/// The frame at `sec` seconds (the nearest), at `fps`.
pub fn frame_at(sec: f64, fps: f64) -> Option<i64> {
    let sec = clean_sec(Some(sec))?;
    let (num, den) = fps_fraction(fps);
    if num <= 0 {
        return None;
    }
    let f = (sec * num as f64 / den as f64).round();
    if (0.0..1e12).contains(&f) { Some(f as i64) } else { None }
}

/// The exact ticks of `frames` frames at `fps`.
pub fn ticks_of_frames(frames: i64, fps: f64) -> Option<i64> {
    let (num, den) = fps_fraction(fps);
    if num <= 0 {
        return None;
    }
    let t = i128::from(frames).checked_mul(i128::from(TICKS_PER_SECOND))?.checked_mul(den)?.checked_div(num)?;
    i64::try_from(t).ok()
}

// ---------------------------------------------------------------- the hand-off state

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// Not brought into FilmCraft yet.
    #[default]
    NotYet,
    /// The service is preparing the media (progress events).
    Preparing,
    /// Placed: the edit is open.
    Done,
    /// It could not be brought in (the reason is in `error`).
    Failed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowState {
    #[default]
    Waiting,
    Working,
    Done,
    /// Some pieces of it failed (the rest came in).
    Failed,
    /// Nothing of this kind in the project.
    None,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Failure {
    pub node: String,
    pub label: String,
    pub why: String,
    /// The owner chose Leave it out.
    pub left_out: bool,
}

/// One row of the Import summary (Shots, Transitions, Voice, Music, Sounds, Your assets, Captions, Beats).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Row {
    pub key: String,
    pub name: String,
    pub count: u32,
    pub state: RowState,
    /// One plain line of what and where.
    pub line: String,
    pub failed: Vec<Failure>,
}

/// A clip Mortiflix placed: which node and version it shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Placed {
    pub node: String,
    pub kind: String,
    pub label: String,
    pub name: String,
    pub beat: Option<String>,
    pub version: u32,
    pub item: u64,
    pub clip: u64,
    /// V1, A1...
    pub track: String,
    pub text: String,
    /// Its label (the beat's colour).
    pub colour: Option<String>,
}

/// An item Mortiflix imported (every version), and the bin it is in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BinItem {
    pub node: String,
    pub version: u32,
    pub item: u64,
    pub bin: String,
}

/// A row of the bin tree the Import summary shows.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BinRow {
    pub name: String,
    pub depth: u8,
    pub count: u32,
    pub colour: Option<String>,
}

/// A track the hand-off made, for the host to name ("Shots", "Voice"...).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TrackName {
    pub video: bool,
    /// 0-based index among the sequence's video or audio tracks.
    pub index: usize,
    pub name: String,
}

/// A timeline clip the user selected (the host fills these each frame on stage 5).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Selected {
    pub clip: u64,
    pub name: String,
    pub start_sec: f64,
    pub end_sec: f64,
    /// "V1 Shots"
    pub track: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TalkLine {
    /// "you" or "mortiflix"
    pub who: String,
    pub node: String,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChangeState {
    #[default]
    Making,
    Done,
    NeedsYou,
}

/// A change asked for from this panel (or a new version that arrived).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Change {
    pub node: String,
    pub label: String,
    pub beat: Option<String>,
    pub text: String,
    pub state: ChangeState,
    pub from_version: u32,
}

/// What the service says about a node now (from `node.list`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NodeNow {
    pub current: u32,
    pub versions: Vec<u32>,
    /// An edit request is being made.
    pub making: bool,
    /// The last edit request failed (its note).
    pub failed: String,
}

/// Everything the hand-off and the stage-5 panel remember. Lives in `MfxState::handoff`, so the
/// control channel sees it (`mortiflix.state`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Handoff {
    pub phase: Phase,
    pub plan_request: Option<u64>,
    pub progress_done: u32,
    pub progress_total: u32,
    pub progress_line: String,
    /// Last service event read.
    pub event_seq: u64,
    pub rows: Vec<Row>,
    pub title: String,
    pub project: String,
    pub rev: u64,
    pub fcproj: String,
    pub fps: f64,
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    pub sequence: Option<u64>,
    pub beats: Vec<PlanBeat>,
    pub bins: Vec<BinRow>,
    pub bin_ids: BTreeMap<String, u64>,
    pub tracks: Vec<TrackName>,
    pub placed: Vec<Placed>,
    pub items: Vec<BinItem>,
    pub error: String,
    pub previews: u32,
    pub selected: Vec<Selected>,
    /// The clip picked when several are selected.
    pub focus: Option<u64>,
    pub ask: String,
    pub talk: Vec<TalkLine>,
    pub changes: Vec<Change>,
    /// File ▸ Start the edit again from Mortiflix asks first.
    pub confirm_restart: bool,
    pub swaps_request: Option<u64>,
    pub swaps_asked: Vec<(u64, String)>,
    pub nodes: BTreeMap<String, NodeNow>,
    pub last_poll: f64,
    /// The Import summary is open (it folds away once the edit is open).
    pub show_summary: bool,
    /// Imported for Get the video (`mortiflix.import.run {open_edit: false}`): no toast, no panel.
    pub quiet: bool,
    /// The plan the host should build now (taken by [`take_jobs`]).
    #[serde(skip)]
    pub jobs: Vec<Job>,
}

/// Work the host does with FilmCraft's session.
#[derive(Clone, Debug, PartialEq)]
pub enum Job {
    /// Build the project from this plan (one undo step).
    Build(Box<Plan>),
    /// A new version of a piece arrived: swap it in.
    Swap(Box<Piece>),
}

const ROWS: [(&str, &str); 8] = [
    ("shots", "Shots"),
    ("transitions", "Transitions"),
    ("voice", "Voice"),
    ("music", "Music"),
    ("sounds", "Sounds"),
    ("assets", "Your assets"),
    ("captions", "Captions"),
    ("beats", "Beats"),
];

impl Handoff {
    fn fresh_rows() -> Vec<Row> {
        ROWS.iter().map(|(k, n)| Row { key: (*k).into(), name: (*n).into(), ..Row::default() }).collect()
    }
    fn row(&mut self, key: &str) -> Option<&mut Row> {
        self.rows.iter_mut().find(|r| r.key == key)
    }
    fn fail(&mut self, key: &str, node: &str, label: &str, why: String) {
        if let Some(r) = self.row(key) {
            r.state = RowState::Failed;
            if r.failed.len() < 50 {
                r.failed.push(Failure { node: node.into(), label: label.into(), why: short(&why, 300), left_out: false });
            }
        }
    }
    fn set_row(&mut self, key: &str, count: u32, line: String) {
        if let Some(r) = self.row(key) {
            r.count = count;
            r.line = line;
            r.state = if r.state == RowState::Failed {
                RowState::Failed
            } else if count == 0 {
                RowState::None
            } else {
                RowState::Done
            };
        }
    }
    /// The beat's number, name and colour.
    pub fn beat(&self, id: &str) -> Option<&PlanBeat> {
        self.beats.iter().find(|b| b.id == id)
    }
    /// "3 Winter"
    pub fn beat_tag(&self, id: Option<&str>) -> Option<String> {
        id.and_then(|b| self.beat(b)).map(|b| format!("{} {}", b.number, b.name))
    }
    /// The placed clip for a timeline clip id.
    pub fn placed_clip(&self, clip: u64) -> Option<&Placed> {
        self.placed.iter().find(|p| p.clip == clip)
    }
    fn talk(&mut self, who: &str, node: &str, text: String) {
        self.talk.push(TalkLine { who: who.into(), node: node.into(), text });
        if self.talk.len() > MAX_TALK {
            self.talk.remove(0);
        }
    }
    /// "1 making · 2 done · 1 needs you"
    pub fn changes_summary(&self) -> String {
        let n = |k: ChangeState| self.changes.iter().filter(|c| c.state == k).count();
        let mut parts = Vec::new();
        for (k, w) in [(ChangeState::Making, "making"), (ChangeState::Done, "done"), (ChangeState::NeedsYou, "needs you")] {
            let c = n(k);
            if c > 0 {
                parts.push(format!("{c} {w}"));
            }
        }
        if parts.is_empty() { "none yet".into() } else { parts.join(" · ") }
    }
    /// How many changes are being made (the header line).
    pub fn making(&self) -> usize {
        self.changes.iter().filter(|c| c.state == ChangeState::Making).count()
    }
}

/// Take what the host must run now.
pub fn take_jobs(state: &mut MfxState) -> Vec<Job> {
    std::mem::take(&mut state.handoff.jobs)
}

/// Ask the service for the plan (Edit the video). Starts over only when `again` (File ▸ Start the
/// edit again from Mortiflix, after its confirm); otherwise a second call while preparing or done
/// does nothing.
pub fn start(state: &mut MfxState, service: &mut Service, again: bool) -> bool {
    let h = &mut state.handoff;
    if !again && matches!(h.phase, Phase::Preparing | Phase::Done) {
        return false;
    }
    let keep_talk = std::mem::take(&mut h.talk);
    let seq = h.event_seq;
    *h = Handoff { phase: Phase::Preparing, rows: Handoff::fresh_rows(), show_summary: true, talk: keep_talk, event_seq: seq, ..Handoff::default() };
    let params = match &state.project_id {
        Some(p) => json!({"project": p}),
        None => json!({}),
    };
    h.plan_request = Some(service.request("handoff.plan", params));
    h.progress_line = "Asking Mortiflix for the pieces".into();
    true
}

/// What the host tells the service once the project is built (`handoff.done`): which clip and item
/// shows which node at which version, and where the project is saved.
pub fn done_params(h: &Handoff) -> Value {
    let clips: Vec<Value> = h
        .placed
        .iter()
        .filter(|p| p.kind != "music" || h.placed.iter().find(|x| x.kind == "music").map(|x| x.clip) == Some(p.clip))
        .map(|p| json!({"node": p.node, "version": p.version, "clip": p.clip, "item": p.item}))
        .collect();
    let mut map = json!({"clips": clips, "rev": h.rev});
    if !h.fcproj.is_empty() {
        map["fcproj"] = json!(h.fcproj);
    }
    let mut params = json!({"map": map});
    if !h.project.is_empty() {
        params["project"] = json!(h.project);
    }
    params
}

/// Read the service's replies and events (`node.changed` → `handoff.swaps`). Never blocks.
/// `now` = seconds (egui time).
pub fn poll(state: &mut MfxState, service: &mut Service, now: f64) {
    let h = &mut state.handoff;
    let mut last = h.event_seq;
    let mut changed = false;
    for (seq, ev) in service.events_since(h.event_seq) {
        last = last.max(*seq);
        let data = ev.get("data").unwrap_or(&Value::Null);
        match ev.get("event").and_then(Value::as_str) {
            Some("node.changed") if h.phase == Phase::Done => {
                if read_changed(h, data) {
                    changed = true;
                }
            }
            Some("handoff.progress") | Some("filmcraft.progress") => {
                let num = |k: &str| data.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
                h.progress_total = num("total");
                h.progress_done = num("done").min(h.progress_total);
                if let Some(l) = data.get("line").and_then(Value::as_str) {
                    h.progress_line = short(l, 200);
                }
            }
            _ => {}
        }
    }
    h.event_seq = last;
    if let Some(id) = h.plan_request
        && let Some(r) = service.take_reply(id)
    {
        h.plan_request = None;
        let plan = if r.ok { Plan::from_handoff(&r.result) } else { Err(error_text(&r.error)) };
        match plan {
            Ok(p) => h.jobs.push(Job::Build(Box::new(p))),
            Err(e) => {
                h.phase = Phase::Failed;
                h.error = e;
            }
        }
    }
    if h.phase != Phase::Done {
        return;
    }
    // new versions: when a node changed, and now and then in case an event was missed
    let due = now - h.last_poll >= POLL_SECONDS * 4.0 || now < h.last_poll;
    if h.swaps_request.is_none() && service.status().is_connected() && (changed || due) {
        h.last_poll = now;
        let params = if h.project.is_empty() { json!({}) } else { json!({"project": h.project}) };
        h.swaps_request = Some(service.request("handoff.swaps", params));
    }
    if let Some(id) = h.swaps_request
        && let Some(r) = service.take_reply(id)
    {
        h.swaps_request = None;
        let swaps = r.result.get("swaps").and_then(Value::as_array).cloned().unwrap_or_default();
        for sw in swaps.iter().take(50) {
            if let Some(p) = swap_piece(h, sw)
                && !h.swaps_asked.iter().any(|(v, n)| *n == p.node && u64::from(p.version) == *v)
            {
                h.swaps_asked.push((u64::from(p.version), p.node.clone()));
                h.jobs.push(Job::Swap(Box::new(p)));
            }
        }
    }
}

/// One `handoff.swaps` row as a piece to swap in (its bin and colour from the clip it replaces).
fn swap_piece(h: &Handoff, sw: &Value) -> Option<Piece> {
    let node = sw.get("node").and_then(Value::as_str)?.to_string();
    let version = sw.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok())?;
    let file = sw.get("path").and_then(Value::as_str).filter(|p| !p.is_empty())?.to_string();
    let old = h.placed.iter().find(|p| p.node == node)?;
    if old.version == version {
        return None;
    }
    let (from, to) = (clean_sec(sw.get("from_sec").and_then(Value::as_f64)), clean_sec(sw.get("to_sec").and_then(Value::as_f64)));
    let bin = h.items.iter().find(|i| i.node == node && i.version == old.version).map(|i| i.bin.clone()).filter(|b| b != "Older versions");
    Some(Piece {
        node: node.clone(),
        kind: old.kind.clone(),
        label: old.label.clone(),
        title: old.name.rsplit_once(" · v").map_or(old.name.clone(), |(b, _)| b.to_string()),
        beat: old.beat.clone(),
        version,
        current: true,
        file,
        source_in_sec: from,
        seconds: to.zip(from.or(Some(0.0))).map(|(b, a)| b - a).filter(|d| *d > 0.0),
        media: if old.kind == "shot" { "video".into() } else { "audio".into() },
        colour: old.colour.clone(),
        bin,
        ..Piece::default()
    })
}

fn error_text(e: &Value) -> String {
    let m = e.get("message").and_then(Value::as_str).unwrap_or("Mortiflix didn't answer");
    short(m, 300)
}

/// Read a `node.changed` event into what each placed node is at now; a node being made again
/// becomes a Change. True when a placed node changed.
fn read_changed(h: &mut Handoff, data: &Value) -> bool {
    let Some(list) = data.get("nodes").and_then(Value::as_array) else { return false };
    let mut any = false;
    for n in list.iter().take(MAX_PIECES) {
        let Some(id) = n.get("id").and_then(Value::as_str) else { continue };
        let Some(label) = h.placed.iter().find(|p| p.node == id).map(|p| p.name.rsplit_once(" · v").map_or(p.name.clone(), |(b, _)| b.to_string())) else {
            continue;
        };
        any = true;
        let current = n.get("current").and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok()).unwrap_or(0);
        let st = n.get("state").and_then(Value::as_str).unwrap_or("");
        let making = st == "generating";
        let failed = if st == "needs you" { "it needs you in Mortiflix".to_string() } else { String::new() };
        let beat = h.placed.iter().find(|p| p.node == id).and_then(|p| p.beat.clone());
        if making && !h.changes.iter().any(|c| c.node == id && c.state == ChangeState::Making) {
            h.changes.push(Change {
                node: id.into(),
                label: label.clone(),
                beat,
                text: "a new version".into(),
                state: ChangeState::Making,
                from_version: current,
            });
        }
        if !failed.is_empty()
            && let Some(c) = h.changes.iter_mut().rev().find(|c| c.node == id && c.state == ChangeState::Making)
        {
            c.state = ChangeState::NeedsYou;
            c.text = format!("{}: {failed}", c.text);
        }
        let mut versions = h.nodes.get(id).map(|x| x.versions.clone()).unwrap_or_default();
        if current > 0 && !versions.contains(&current) {
            versions.push(current);
        }
        h.nodes.insert(id.to_string(), NodeNow { current, versions, making, failed });
    }
    any
}

// ---------------------------------------------------------------- building the project

fn bin_for(p: &Piece, h: &Handoff) -> Vec<String> {
    if p.cut {
        return vec!["Cut on the board".into()];
    }
    if !p.current {
        return vec!["Older versions".into()];
    }
    if let Some(b) = &p.bin {
        let parts: Vec<String> = b.split('/').filter(|x| !x.trim().is_empty()).take(4).map(|x| short(x.trim(), 80)).collect();
        if !parts.is_empty() {
            return parts;
        }
    }
    match p.kind.as_str() {
        "shot" => match h.beat_tag(p.beat.as_deref()) {
            Some(tag) => vec!["Shots".into(), tag],
            None => vec!["Shots".into()],
        },
        "transition" => vec!["Transitions".into()],
        "line" => vec!["Voice".into()],
        "music" => vec!["Music".into()],
        "stem" => vec!["Music stems".into()],
        "sound" => vec!["Sounds".into()],
        _ => vec!["Your assets".into()],
    }
}

/// The name a clip and its bin item carry: "Shot 7 · v2", "B03-L1 · And then the money...".
pub fn clip_name(p: &Piece) -> String {
    let base = if !p.title.is_empty() {
        p.title.clone()
    } else if p.kind == "line" && !p.text.is_empty() {
        format!("{} · {}", p.label.to_uppercase(), short(&p.text, 40))
    } else {
        p.label.clone()
    };
    if matches!(p.kind.as_str(), "stem" | "asset" | "older") { base } else { format!("{base} · v{}", p.version) }
}

fn ensure_bin(h: &mut Handoff, path: &[String], exec: &mut Exec) -> Result<Option<u64>, String> {
    let mut parent: Option<u64> = None;
    let mut key = String::new();
    for (depth, name) in path.iter().enumerate() {
        if !key.is_empty() {
            key.push('/');
        }
        key.push_str(name);
        if let Some(id) = h.bin_ids.get(&key) {
            parent = Some(*id);
            continue;
        }
        let mut params = json!({"name": name});
        if let Some(p) = parent {
            params["parent"] = json!(p);
        }
        let r = exec("file.newBin", params)?;
        let id = r.get("bin").and_then(Value::as_u64).ok_or("FilmCraft made no bin")?;
        h.bin_ids.insert(key.clone(), id);
        // a beat's bin ("3 Winter") carries the beat's colour
        let no = name.split(' ').next().and_then(|n| n.parse::<u32>().ok());
        let colour = if depth > 0 { no.and_then(|n| h.beats.iter().find(|b| b.number == n)).and_then(|b| b.colour.clone()) } else { None };
        h.bins.push(BinRow { name: name.clone(), depth: u8::try_from(depth).unwrap_or(u8::MAX), count: 0, colour });
        parent = Some(id);
    }
    Ok(parent)
}

fn import_one(h: &mut Handoff, p: &Piece, exec: &mut Exec) -> Result<u64, String> {
    let path = bin_for(p, h);
    let bin = ensure_bin(h, &path, exec)?;
    let item = if p.media == "card" {
        // no picture yet: a colour matte the length of the shot stands in
        let colour = p.card_hex.clone().unwrap_or_else(|| "#202020".into());
        let r = exec("file.newColorMatte", json!({"color": colour, "seconds": p.seconds.unwrap_or(3.0).clamp(0.1, 600.0)}))?;
        let item = r.get("item").and_then(Value::as_u64).ok_or("FilmCraft made no colour matte")?;
        if let Some(b) = bin {
            let _ = exec("project.moveToBin", json!({"items": [item], "bin": b}));
        }
        item
    } else {
        let mut params = json!({"paths": [p.file]});
        if let Some(b) = bin {
            params["bin"] = json!(b);
        }
        let r = exec("file.import", params)?;
        let item = r.get("items").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_u64);
        let Some(item) = item else {
            let why = r.get("errors").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_str).unwrap_or("FilmCraft couldn't read it");
            return Err(short(why, 300));
        };
        item
    };
    // a shot cut from a longer file (the animatic): the item's In/Out are the shot's own range, so
    // the bin and From Bin replaces show just that shot
    if p.media == "video"
        && let Some((tin, tout)) = source_range(p, h.fps)
    {
        let _ = exec("project.setMarks", json!({"item": item, "in": tin, "out": tout}));
    }
    // a readable bin: the item carries the clip's name (the rename is cosmetic: never fatal)
    let _ = exec("clip.rename", json!({"item": item, "name": clip_name(p)}));
    let bin_name = path.last().cloned().unwrap_or_default();
    if let Some(row) = h.bins.iter_mut().rev().find(|b| b.name == bin_name) {
        row.count = row.count.saturating_add(1);
    }
    h.items.push(BinItem { node: p.node.clone(), version: p.version, item, bin: path.join("/") });
    Ok(item)
}

/// The piece's range in its file, in ticks (In, Out), when it is cut from a longer one.
fn source_range(p: &Piece, fps: f64) -> Option<(i64, i64)> {
    let from = p.source_in_sec.unwrap_or(0.0);
    let len = p.seconds?;
    let fin = frame_at(from, fps)?;
    let fout = frame_at(from + len, fps)?;
    if fout <= fin || (fin == 0 && p.source_in_sec.is_none()) {
        return None;
    }
    Some((ticks_of_frames(fin, fps)?, ticks_of_frames(fout, fps)?))
}

fn first_clip(r: &Value) -> Option<u64> {
    r.get("clips").and_then(Value::as_array).and_then(|a| a.first()).and_then(Value::as_u64)
}

/// Build the FilmCraft project from the plan with `exec` (the host wraps it in one undo step).
/// A piece that fails is listed on its row; the rest still comes in. Only a sequence that can't be
/// made fails the whole hand-off.
pub fn build(h: &mut Handoff, plan: &Plan, exec: &mut Exec) {
    h.title = plan.title.clone();
    h.project = plan.project.clone();
    h.rev = plan.rev;
    h.fcproj = plan.fcproj.clone();
    h.fps = plan.fps;
    h.seconds = plan.seconds;
    (h.width, h.height) = (plan.width, plan.height);
    h.beats = plan.beats.clone();
    h.rows = Handoff::fresh_rows();
    for r in &mut h.rows {
        r.state = RowState::Working;
    }
    h.previews = u32::try_from(plan.pieces.iter().filter(|p| p.kind == "shot" && p.current && !p.cut && p.preview).count()).unwrap_or(u32::MAX);
    let fps = plan.fps;
    let on_timeline = |p: &Piece| p.current && !p.cut && p.start_sec.is_some();
    let has = |k: &str| plan.pieces.iter().any(|p| p.kind == k && on_timeline(p));

    // 1. The sequence: V1 Shots, then one audio track per kind that has something to place.
    let audio_kinds: Vec<(&str, &str)> = [("line", "Voice"), ("music", "Music"), ("sound", "Sounds")].into_iter().filter(|(k, _)| has(k)).collect();
    let seq = exec(
        "file.newSequence",
        json!({"name": plan.title, "width": plan.width, "height": plan.height, "fps": fps, "video": 1, "audio": audio_kinds.len().max(1)}),
    );
    match seq.as_ref().ok().and_then(|r| r.get("sequence").and_then(Value::as_u64)) {
        Some(id) => h.sequence = Some(id),
        None => {
            h.phase = Phase::Failed;
            h.error = match seq {
                Err(e) => format!("FilmCraft couldn't make the sequence: {e}"),
                Ok(_) => "FilmCraft made no sequence".into(),
            };
            return;
        }
    }
    h.tracks = vec![TrackName { video: true, index: 0, name: "Shots".into() }];
    let mut audio_track: BTreeMap<&str, String> = BTreeMap::new();
    for (i, (k, name)) in audio_kinds.iter().enumerate() {
        h.tracks.push(TrackName { video: false, index: i, name: (*name).into() });
        audio_track.insert(k, format!("A{}", i + 1));
    }

    // 2. Every piece and version into its bin.
    let mut item_of: BTreeMap<(String, u32), u64> = BTreeMap::new();
    for p in &plan.pieces {
        let row = row_of(&p.kind);
        match import_one(h, p, exec) {
            Ok(item) => {
                item_of.insert((p.node.clone(), p.version), item);
            }
            Err(e) => h.fail(row, &p.node, &p.label, e),
        }
    }

    // 3. On the timeline.
    let mut place = |h: &mut Handoff, p: &Piece, track: &str, video: bool, extra: Value| -> Option<u64> {
        let item = *item_of.get(&(p.node.clone(), p.version))?;
        let frame = frame_at(p.start_sec?, fps)?;
        let mut params = json!({"item": item, "track": track, "frame": frame, "insert": false});
        params[if video { "audio" } else { "video" }] = json!(false);
        if let (Some(o), Some(e)) = (params.as_object_mut(), extra.as_object()) {
            o.extend(e.clone());
        }
        match exec("timeline.place", params).map(|r| first_clip(&r)) {
            Ok(Some(clip)) => Some(clip),
            Ok(None) => {
                h.fail(row_of(&p.kind), &p.node, &p.label, "FilmCraft placed nothing".into());
                None
            }
            Err(e) => {
                h.fail(row_of(&p.kind), &p.node, &p.label, e);
                None
            }
        }
    };
    let mut placed: Vec<Placed> = Vec::new();
    let beats = plan.beats.clone();
    let colour_of =
        move |p: &Piece| p.colour.clone().or_else(|| p.beat.as_deref().and_then(|b| beats.iter().find(|x| x.id == b)).and_then(|b| b.colour.clone()));
    let beats = plan.beats.clone();
    let mk = |p: &Piece, item: u64, clip: u64, track: &str, name: String| Placed {
        node: p.node.clone(),
        kind: p.kind.clone(),
        label: p.label.clone(),
        name,
        // a piece without a beat of its own belongs to the beat with its colour
        beat: p.beat.clone().or_else(|| p.colour.as_deref().and_then(|c| beats.iter().find(|b| b.colour.as_deref() == Some(c))).map(|b| b.id.clone())),
        version: p.version,
        item,
        clip,
        track: track.into(),
        text: if p.text.is_empty() { p.title.clone() } else { p.text.clone() },
        colour: colour_of(p),
    };
    for p in plan.pieces.iter().filter(|p| p.kind == "shot" && on_timeline(p)) {
        // a shot cut from the animatic plays just its range; a still or card plays for the shot's length
        let mut extra = json!({});
        if let Some(len) = p.seconds.and_then(|l| frame_at(l, fps)).filter(|f| *f > 0).and_then(|f| ticks_of_frames(f, fps)) {
            extra["duration"] = json!(len);
        }
        if p.media == "video"
            && let Some((tin, _)) = source_range(p, fps)
        {
            extra["sourceIn"] = json!(tin);
        }
        if let Some(clip) = place(h, p, "V1", true, extra) {
            let item = item_of.get(&(p.node.clone(), p.version)).copied().unwrap_or(0);
            placed.push(mk(p, item, clip, "V1", clip_name(p)));
        }
    }
    for kind in ["line", "sound"] {
        let Some(track) = audio_track.get(kind).cloned() else { continue };
        for p in plan.pieces.iter().filter(|p| p.kind == kind && on_timeline(p)) {
            if let Some(clip) = place(h, p, &track, false, json!({})) {
                let item = item_of.get(&(p.node.clone(), p.version)).copied().unwrap_or(0);
                placed.push(mk(p, item, clip, &track, clip_name(p)));
            }
        }
    }
    // The music: one clip per cue, cut at section changes (the last cue runs to the end of the
    // music); without timed cues, the whole music once.
    let mut music_clips = 0u32;
    if let (Some(track), Some(m)) = (audio_track.get("music").cloned(), plan.pieces.iter().find(|p| p.kind == "music" && on_timeline(p))) {
        let timed: Vec<&Cue> = plan.cues.iter().filter(|c| c.start_sec.is_some()).collect();
        let mut done_cues = false;
        if !timed.is_empty() {
            done_cues = true;
            for (i, c) in timed.iter().enumerate() {
                let Some(start) = c.start_sec.and_then(|s| frame_at(s, fps)) else { continue };
                let Some(source_in) = ticks_of_frames(start, fps) else { continue };
                let mut extra = json!({"sourceIn": source_in});
                let last = i + 1 == timed.len();
                if !last && let Some(end) = c.end_sec.and_then(|e| frame_at(e, fps)).filter(|e| *e > start) {
                    extra["duration"] = json!(ticks_of_frames(end - start, fps));
                }
                let mut piece = (*m).clone();
                piece.start_sec = c.start_sec;
                match place(h, &piece, &track, false, extra) {
                    Some(clip) => {
                        let item = item_of.get(&(m.node.clone(), m.version)).copied().unwrap_or(0);
                        let mut pl = mk(m, item, clip, &track, format!("{} · v{}", c.name, m.version));
                        if c.beat.is_some() {
                            pl.beat = c.beat.clone();
                        }
                        if c.colour.is_some() {
                            pl.colour = c.colour.clone();
                        }
                        placed.push(pl);
                        music_clips += 1;
                    }
                    None => {
                        done_cues = false;
                        break;
                    }
                }
            }
        }
        if !done_cues && music_clips == 0 {
            // clear a half-made cue failure: the whole music is the fallback
            if let Some(r) = h.row("music") {
                r.failed.clear();
                r.state = RowState::Working;
            }
            if let Some(clip) = place(h, m, &track, false, json!({})) {
                let item = item_of.get(&(m.node.clone(), m.version)).copied().unwrap_or(0);
                placed.push(mk(m, item, clip, &track, clip_name(m)));
                music_clips = 1;
            }
        }
    }
    for pl in &placed {
        let _ = exec("clip.rename", json!({"clip": pl.clip, "name": pl.name}));
    }

    // 4. The board's transitions: FilmCraft's own, on the cut after the shot they leave.
    let mut transitions = 0u32;
    for t in &plan.transitions {
        let Some(effect) = &t.effect else { continue };
        let from = t.from.as_deref().unwrap_or_default();
        let Some(clip) = placed.iter().find(|p| p.kind == "shot" && p.node.eq_ignore_ascii_case(from)).map(|p| p.clip) else {
            h.fail("transitions", &t.node, &t.label, format!("{} isn't on the timeline (cut on the board?)", from.to_uppercase()));
            continue;
        };
        let r = exec("sequence.applyVideoTransition", json!({"clip": clip, "edge": "out", "effect": effect}))
            .or_else(|_| exec("sequence.applyVideoTransition", json!({"clip": clip, "edge": "out", "effect": "cross_dissolve"})));
        match r {
            Ok(_) => transitions += 1,
            Err(e) => h.fail("transitions", &t.node, &t.label, e),
        }
    }

    // 5. Colours = beats: every item and clip of a beat carries its label.
    let mut by_colour: BTreeMap<String, (Vec<u64>, Vec<u64>)> = BTreeMap::new();
    for p in &plan.pieces {
        if let (Some(c), Some(i)) = (colour_of(p), item_of.get(&(p.node.clone(), p.version))) {
            by_colour.entry(c).or_default().0.push(*i);
        }
    }
    for pl in &placed {
        if let Some(c) = &pl.colour {
            by_colour.entry(c.clone()).or_default().1.push(pl.clip);
        }
    }
    for (colour, (items, clips)) in &by_colour {
        let _ = exec("project.select", json!({"items": items}));
        let _ = exec("timeline.select", json!({"clips": clips}));
        if let Err(e) = exec("edit.label", json!({"label": colour})) {
            h.fail("beats", "", colour, e);
        }
    }
    let _ = exec("timeline.select", json!({"clips": []}));
    let _ = exec("project.select", json!({"items": []}));

    // 6. Markers: a Chapter per beat, a Comment per music cue.
    let mut chapters = 0u32;
    for b in &plan.beats {
        let Some(frame) = b.start_sec.and_then(|s| frame_at(s, fps)) else { continue };
        let mut params = json!({"frame": frame, "name": format!("{} {}", b.number, b.name), "comment": b.comment});
        if let Some(c) = &b.colour {
            params["color"] = json!(c);
        }
        match exec("markers.addChapter", params) {
            Ok(_) => chapters += 1,
            Err(e) => h.fail("beats", &b.id, &b.name, e),
        }
    }
    let mut cue_markers = 0u32;
    for c in &plan.cues {
        let Some(frame) = c.start_sec.and_then(|s| frame_at(s, fps)) else { continue };
        let mut params = json!({"frame": frame, "name": c.name, "comment": "Music cue"});
        if let Some(col) = &c.colour {
            params["color"] = json!(col);
        }
        if exec("markers.add", params).is_ok() {
            cue_markers += 1;
        }
    }

    // 7. Captions: one per voice line on the timeline, its words at its time.
    let mut captions = 0u32;
    for p in plan.pieces.iter().filter(|p| p.kind == "line" && on_timeline(p) && !p.text.is_empty()) {
        if !placed.iter().any(|x| x.node == p.node) {
            continue;
        }
        let (Some(start), Some(frame)) = (p.start_sec, p.start_sec.and_then(|s| frame_at(s, fps))) else { continue };
        let Some(time) = ticks_of_frames(frame, fps) else { continue };
        let len = p.end_sec.map(|e| e - start).or(p.seconds).filter(|l| *l > 0.05).unwrap_or(3.0).min(30.0);
        match exec("captions.add", json!({"text": p.text, "time": time, "durationSeconds": len})) {
            Ok(_) => captions += 1,
            Err(e) => h.fail("captions", &p.node, &p.label, e),
        }
    }

    // 8. The notes: which Mortiflix project and graph revision.
    if !plan.notes.is_empty() {
        let _ = exec("project.setNotes", json!({"text": plan.notes}));
    }

    // The rows' counts and lines.
    let count = |k: &str| u32::try_from(placed.iter().filter(|p| p.kind == k).count()).unwrap_or(u32::MAX);
    let n_items =
        |k: &str| u32::try_from(plan.pieces.iter().filter(|p| p.kind == k && item_of.contains_key(&(p.node.clone(), p.version))).count()).unwrap_or(0);
    let (shots, lines, sounds) = (count("shot"), count("line"), count("sound"));
    let older = u32::try_from(plan.pieces.iter().filter(|p| !p.current && !p.cut).count()).unwrap_or(0);
    let cut = u32::try_from(plan.pieces.iter().filter(|p| p.cut && p.current).count()).unwrap_or(0);
    let mut shot_line = format!("on V1 Shots, in board order, no gaps · {older} older version{}", plural(older));
    if cut > 0 {
        shot_line.push_str(&format!(" · {cut} cut"));
    }
    if h.previews > 0 {
        shot_line.push_str(&format!(" · {} preview{}: the render in Export makes them full quality", h.previews, plural(h.previews)));
    }
    h.set_row("shots", shots, shot_line);
    h.set_row("transitions", transitions, "FilmCraft's own, changeable in Effect Controls".into());
    let a = |k: &str| audio_track.get(k).cloned().unwrap_or_default();
    h.set_row("voice", lines, format!("one clip per line on {} Voice, at its time", a("line")));
    let stems = n_items("stem");
    h.set_row("music", music_clips, format!("{} Music, cut at section changes · {stems} stem{} in the bin", a("music"), plural(stems)));
    h.set_row("sounds", sounds, format!("on {} Sounds", a("sound")));
    let assets = n_items("asset");
    h.set_row("assets", assets, "in the Your assets bin".into());
    h.set_row("captions", captions, "a caption clip per voice line".into());
    h.set_row("beats", chapters, format!("a Chapter marker per beat · {cue_markers} music cue marker{}", plural(cue_markers)));
    h.placed = placed;
    h.phase = Phase::Done;
    h.show_summary = true;
}

fn plural(n: u32) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn row_of(kind: &str) -> &'static str {
    match kind {
        "shot" | "older" => "shots",
        "transition" => "transitions",
        "line" => "voice",
        "music" | "stem" => "music",
        "sound" => "sounds",
        _ => "assets",
    }
}

/// A new version arrived: import it next to the old one, move the old one to Older versions, and
/// replace every clip of that node with it (place and trims kept: `keepSourceIn`).
pub fn swap(h: &mut Handoff, piece: &Piece, exec: &mut Exec) -> Result<String, String> {
    let clips: Vec<u64> = h.placed.iter().filter(|p| p.node == piece.node && p.kind != "music").map(|p| p.clip).collect();
    if clips.is_empty() {
        return Err(format!("{} isn't on the timeline", piece.label));
    }
    let item = match h.items.iter().find(|i| i.node == piece.node && i.version == piece.version) {
        Some(i) => i.item,
        None => import_one(h, piece, exec)?,
    };
    let mut old: Vec<u64> = h.placed.iter().filter(|p| p.node == piece.node).map(|p| p.item).collect();
    old.dedup();
    let colour = piece.colour.clone().or_else(|| h.placed.iter().find(|p| p.node == piece.node).and_then(|p| p.colour.clone()));
    replace(h, &piece.node, piece.version, item, &clips, exec)?;
    // the new item and its clips keep the beat's colour (the host restores the selection after)
    if let Some(c) = colour {
        let _ = exec("project.select", json!({"items": [item]}));
        let _ = exec("timeline.select", json!({"clips": clips}));
        let _ = exec("edit.label", json!({"label": c}));
    }
    // the old version goes to Older versions (it stays: nothing is thrown away)
    if let Ok(Some(older)) = ensure_bin(h, &["Older versions".to_string()], exec) {
        let _ = exec("project.moveToBin", json!({"items": old, "bin": older}));
        for i in h.items.iter_mut().filter(|i| old.contains(&i.item)) {
            let from = std::mem::replace(&mut i.bin, "Older versions".into());
            let from = from.rsplit('/').next().unwrap_or_default().to_string();
            if let Some(b) = h.bins.iter_mut().find(|b| b.name == from) {
                b.count = b.count.saturating_sub(1);
            }
            if let Some(b) = h.bins.iter_mut().find(|b| b.name == "Older versions") {
                b.count = b.count.saturating_add(1);
            }
        }
    }
    let label = h.placed.iter().find(|p| p.node == piece.node).map(|p| p.name.clone()).unwrap_or_else(|| piece.label.clone());
    for c in h.changes.iter_mut().filter(|c| c.node == piece.node && c.state == ChangeState::Making) {
        c.state = ChangeState::Done;
    }
    h.talk("mortiflix", &piece.node, format!("{label} is on the timeline, in the same spot. The old version is in Older versions."));
    Ok(label)
}

/// Use an older (or newer) version that is already in the bin: no AI, instant.
pub fn use_version(h: &mut Handoff, node: &str, version: u32, exec: &mut Exec) -> Result<String, String> {
    let item = h.items.iter().find(|i| i.node == node && i.version == version).map(|i| i.item).ok_or_else(|| format!("v{version} isn't in the bin"))?;
    let clips: Vec<u64> = h.placed.iter().filter(|p| p.node == node && p.kind != "music").map(|p| p.clip).collect();
    if clips.is_empty() {
        return Err("that piece isn't on the timeline".into());
    }
    replace(h, node, version, item, &clips, exec)?;
    Ok(h.placed.iter().find(|p| p.node == node).map(|p| p.name.clone()).unwrap_or_default())
}

fn replace(h: &mut Handoff, node: &str, version: u32, item: u64, clips: &[u64], exec: &mut Exec) -> Result<(), String> {
    exec("clip.replaceFromBin", json!({"clips": clips, "item": item, "keepSourceIn": false}))?;
    for p in h.placed.iter_mut().filter(|p| p.node == node && clips.contains(&p.clip)) {
        p.version = version;
        p.item = item;
        p.name = match p.name.rsplit_once(" · v") {
            Some((base, _)) => format!("{base} · v{version}"),
            None => format!("{} · v{version}", p.name),
        };
        let _ = exec("clip.rename", json!({"clip": p.clip, "name": p.name}));
    }
    Ok(())
}

/// A trim FilmCraft can make itself, from plain words: "trim 0.5 s off the start", "cut 12 frames
/// from the end", "shorter by 1 second". Returns the undo label and the steps.
pub fn trim_steps(text: &str, clip: u64, name: &str, fps: f64) -> Option<(String, Vec<(String, Value)>)> {
    let t = text.to_ascii_lowercase();
    if !["trim", "shorter", "cut ", "shave", "tighten"].iter().any(|w| t.contains(w)) {
        return None;
    }
    let words: Vec<&str> = t.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
    let mut amount: Option<f64> = None;
    let mut frames_unit = false;
    for (i, w) in words.iter().enumerate() {
        let num = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let Ok(v) = num.parse::<f64>() else { continue };
        if !v.is_finite() || v <= 0.0 || v > 600.0 {
            continue;
        }
        let unit = if num.len() < w.len() { &w[num.len()..] } else { words.get(i + 1).copied().unwrap_or("") };
        frames_unit = unit.starts_with('f');
        amount = Some(v);
        break;
    }
    let amount = amount?;
    let frames = if frames_unit { amount.round() as i64 } else { frame_at(amount, fps)? };
    if frames <= 0 {
        return None;
    }
    let head = ["start", "beginning", "head", "front", "in point"].iter().any(|w| t.contains(w));
    let (edge, delta, where_) = if head { ("in", frames, "start") } else { ("out", -frames, "end") };
    let unit = if frames_unit { "frames" } else { "s" };
    let label = format!("Mortiflix: Trim {name} ({amount} {unit} off the {where_})");
    Some((label, vec![("timeline.trim".into(), json!({"clip": clip, "edge": edge, "mode": "regular", "deltaFrames": delta}))]))
}

// ---------------------------------------------------------------- the panel (stage 5)

/// What PanelKind::MfxStage shows on stage 5: the Import summary while it comes in, then the
/// Mortiflix panel for the clicked clip (with the summary folded above it).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    egui::ScrollArea::vertical().id_salt("mfx-into-scroll").auto_shrink([false, false]).show(ui, |ui| {
        let phase = ctx.state.handoff.phase;
        if ctx.state.handoff.confirm_restart {
            restart_confirm(ui, ctx);
            ui.add_space(12.0);
        }
        match phase {
            Phase::NotYet => from_mortiflix(ui, ctx),
            Phase::Preparing | Phase::Failed => summary(ui, ctx),
            Phase::Done => {
                let open = ctx.state.handoff.show_summary;
                let title = format!("{} is in FilmCraft", ctx.state.handoff.title);
                let r =
                    ui.add(egui::Button::new(RichText::new(format!("{title}  ·  {}", if open { "hide" } else { "show what came in" })).strong()).frame(false));
                if r.clicked() {
                    ctx.state.handoff.show_summary = !open;
                }
                ctx.auto("mfx.into.summaryFold", &r, &title);
                if open {
                    summary(ui, ctx);
                }
                ui.separator();
                clicked_panel(ui, ctx);
            }
        }
    });
}

/// Import before anything came in: the "From Mortiflix" card, saying what is still missing.
fn from_mortiflix(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    ui.label(RichText::new("From Mortiflix").strong().size(15.0).color(t.text));
    ui.add_space(4.0);
    let missing = if ctx.state.project_id.is_none() {
        Some("Make or open a Mortiflix project first.")
    } else if ctx.state.status(crate::STAGE_VOICE_MUSIC).kind != crate::StatusKind::Done {
        Some("Voice & music isn't done yet: what is made so far comes in.")
    } else {
        None
    };
    ui.colored_label(t.text_dim, "Everything you OK'd is placed on one FilmCraft timeline: shots, voice, music, captions and a marker per beat.");
    if let Some(m) = missing {
        ui.colored_label(t.warn, m);
    }
    crate::service_notice(ui, ctx);
    ui.add_space(8.0);
    let r = accent_button(ui, ctx, "Bring it into FilmCraft");
    if r.clicked() {
        ctx.command("mortiflix.importEdit", json!({}));
    }
    ctx.auto("mfx.into.bringIn", &r, "Bring it into FilmCraft");
}

fn accent_button(ui: &mut egui::Ui, ctx: &MfxCtx, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(ctx.theme.accent).min_size(vec2(0.0, 28.0)))
}

fn state_icon(s: RowState) -> (&'static str, &'static str) {
    match s {
        RowState::Waiting => ("○", "waiting"),
        RowState::Working => ("◐", "working"),
        RowState::Done => ("✓", "done"),
        RowState::Failed => ("!", "failed"),
        RowState::None => ("–", "none"),
    }
}

fn swatch(ui: &mut egui::Ui, colour: Option<&str>) {
    let (rect, _) = ui.allocate_exact_size(vec2(4.0, 14.0), Sense::hover());
    if let Some(c) = colour.and_then(label_rgb) {
        ui.painter().rect_filled(rect, 1.0, c);
    }
}

/// The Import summary (38b): rows, progress, how it's placed, the bins.
fn summary(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let h = &ctx.state.handoff;
    let any_failed = h.rows.iter().any(|r| r.state == RowState::Failed);
    let title = match h.phase {
        Phase::Preparing => format!("Bringing {} into FilmCraft", if h.title.is_empty() { "the video" } else { &h.title }),
        Phase::Failed => "It couldn't come into FilmCraft".to_string(),
        _ if any_failed => "One piece didn't come in".to_string(),
        _ => format!("{} is in FilmCraft", h.title),
    };
    if h.phase != Phase::Done {
        ui.label(RichText::new(title).strong().size(15.0).color(t.text));
        ui.colored_label(t.text_dim, "Everything you OK'd, placed the way you need it. Edit opens by itself when it's done.");
        ui.add_space(6.0);
    }
    if h.phase == Phase::Failed {
        ui.colored_label(t.error, format!("! {}", h.error));
        let r = ui.button("Try again");
        if r.clicked() {
            ctx.command("mortiflix.importEdit", json!({"again": true}));
        }
        ctx.auto("mfx.into.tryAgain", &r, "Try again");
        crate::service_notice(ui, ctx);
        return;
    }
    let rows = h.rows.clone();
    let mut actions: Vec<(String, &'static str, usize, usize)> = Vec::new();
    for (ri, r) in rows.iter().enumerate() {
        let (icon, word) = state_icon(r.state);
        let colour = match r.state {
            RowState::Done => t.ok,
            RowState::Failed => t.error,
            RowState::Working => t.accent,
            _ => t.text_faint,
        };
        ui.horizontal(|ui| {
            ui.colored_label(colour, icon).on_hover_text(word);
            ui.label(RichText::new(&r.name).color(t.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if !matches!(r.state, RowState::Waiting | RowState::Working) {
                    ui.colored_label(t.text_dim, r.count.to_string());
                }
            });
        });
        if !r.line.is_empty() {
            ui.horizontal(|ui| {
                ui.add_space(18.0);
                ui.colored_label(t.text_faint, &r.line);
            });
        }
        for (fi, f) in r.failed.iter().enumerate().filter(|(_, f)| !f.left_out).take(8) {
            ui.horizontal_wrapped(|ui| {
                ui.add_space(18.0);
                ui.colored_label(t.error, format!("{}: {}", f.label, f.why));
                if !f.node.is_empty() {
                    let again = ui.small_button("Make it again in Mortiflix");
                    if again.clicked() {
                        actions.push((f.node.clone(), "again", ri, fi));
                    }
                    ctx.auto(&format!("mfx.into.failed.{}.again", f.node), &again, "Make it again in Mortiflix");
                }
                let leave = ui.small_button("Leave it out");
                if leave.clicked() {
                    actions.push((f.node.clone(), "leave", ri, fi));
                }
                ctx.auto(&format!("mfx.into.failed.{ri}.{fi}.leave"), &leave, "Leave it out");
            });
        }
    }
    for (node, what, ri, fi) in actions {
        if what == "again" {
            ctx.service.request("node.edit", json!({"node": node, "action": "fix", "text": "It didn't come into FilmCraft: make it again."}));
        }
        if let Some(f) = ctx.state.handoff.rows.get_mut(ri).and_then(|r| r.failed.get_mut(fi)) {
            f.left_out = true;
        }
    }
    let h = &ctx.state.handoff;
    if h.phase == Phase::Preparing {
        ui.add_space(6.0);
        let frac = if h.progress_total > 0 { h.progress_done as f32 / h.progress_total as f32 } else { 0.0 };
        ui.add(egui::ProgressBar::new(frac).desired_height(6.0));
        let count = if h.progress_total > 0 { format!("{} of {} · ", h.progress_done, h.progress_total) } else { String::new() };
        ui.colored_label(t.text_dim, format!("{count}{}", h.progress_line));
    }
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let done = ctx.state.handoff.phase == Phase::Done;
        let label = if any_failed { "Go to Edit without it" } else { "Go to Edit" };
        let r = ui.add_enabled(done, egui::Button::new(RichText::new(label).strong()));
        let r = if done { r } else { r.on_disabled_hover_text("Edit opens by itself when it's done") };
        if r.clicked() {
            ctx.state.handoff.show_summary = false;
            ctx.command("window.panel.Timeline", json!({}));
        }
        ctx.auto("mfx.into.goToEdit", &r, label);
        let back = ui.link("Back to Mortiflix");
        if back.clicked() {
            ctx.command("stages.voiceMusic", json!({}));
        }
        ctx.auto("mfx.into.back", &back, "Back to Mortiflix");
    });
    let h = &ctx.state.handoff;
    if h.phase == Phase::Done {
        ui.add_space(10.0);
        ui.label(RichText::new("How it's placed").strong().color(t.text));
        ui.colored_label(t.text_dim, format!("{}×{} · {} fps · {}", h.width, h.height, trim_fps(h.fps), clock(h.seconds)));
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(t.text_faint, "Clip colours = beats");
            for b in &h.beats {
                swatch(ui, b.colour.as_deref());
                ui.colored_label(t.text_dim, format!("{} {}", b.number, b.name));
            }
        });
        for tr in &h.tracks {
            let id = if tr.video { format!("V{}", tr.index + 1) } else { format!("A{}", tr.index + 1) };
            let n = h.placed.iter().filter(|p| p.track == id).count();
            ui.colored_label(t.text_dim, format!("{id} {} · {n} clip{}", tr.name, if n == 1 { "" } else { "s" }));
        }
        let caps = h.rows.iter().find(|r| r.key == "captions").map_or(0, |r| r.count);
        if caps > 0 {
            ui.colored_label(t.text_dim, format!("C1 Captions · {caps} caption{}", plural(caps)));
        }
        ui.add_space(10.0);
        ui.label(RichText::new("Bins").strong().color(t.text));
        ui.colored_label(t.text_dim, format!("{}  sequence", h.title));
        for b in &h.bins {
            ui.horizontal(|ui| {
                ui.add_space(f32::from(b.depth) * 14.0);
                swatch(ui, b.colour.as_deref());
                ui.colored_label(t.text_dim, &b.name);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.colored_label(t.text_faint, b.count.to_string());
                });
            });
        }
        ui.colored_label(t.text_faint, "Nothing is thrown away: older versions and cut shots wait in their bins.");
    }
}

fn trim_fps(fps: f64) -> String {
    if (fps - fps.round()).abs() < 1e-6 { format!("{}", fps.round() as i64) } else { format!("{fps:.3}") }
}

/// "1:30", "0:38.5"
pub fn clock(sec: f64) -> String {
    if !sec.is_finite() || sec < 0.0 {
        return "0:00".into();
    }
    let whole = sec.floor() as u64;
    let tenth = ((sec - sec.floor()) * 10.0).round() as u64;
    let (m, s) = (whole / 60, whole % 60);
    if tenth == 0 || tenth >= 10 { format!("{m}:{s:02}") } else { format!("{m}:{s:02}.{tenth}") }
}

fn restart_confirm(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    egui::Frame::new().stroke(Stroke::new(1.0, t.warn)).inner_margin(8.0).corner_radius(4.0).show(ui, |ui| {
        ui.label(RichText::new("Start the edit again from Mortiflix?").strong().color(t.text));
        ui.colored_label(
            t.text_dim,
            "A new sequence is built from the board. Your trims, moves and other hand edits in this one are not carried over (it stays in the project).",
        );
        ui.horizontal(|ui| {
            let go = ui.button("Start again");
            if go.clicked() {
                ctx.state.handoff.confirm_restart = false;
                ctx.command("mortiflix.importEdit", json!({"again": true}));
            }
            ctx.auto("mfx.into.restart.go", &go, "Start again");
            let keep = ui.button("Keep my edit");
            if keep.clicked() {
                ctx.state.handoff.confirm_restart = false;
            }
            ctx.auto("mfx.into.restart.keep", &keep, "Keep my edit");
        });
    });
}

/// The Mortiflix panel (38a): what you clicked, its versions, the talk, the ask box, the changes.
fn clicked_panel(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    crate::service_notice(ui, ctx);
    let h = &ctx.state.handoff;
    let sel = h.selected.clone();
    if sel.is_empty() {
        ui.add_space(8.0);
        ui.colored_label(t.text_dim, "Click a clip on the timeline to change it.");
        changes_list(ui, ctx);
        return;
    }
    let focus = if sel.len() == 1 { sel.first().map(|s| s.clip) } else { h.focus.filter(|f| sel.iter().any(|s| s.clip == *f)) };
    let Some(focus) = focus else {
        ui.colored_label(t.text, format!("{} clips selected. Which one?", sel.len()));
        for s in sel.iter().take(20) {
            let r = ui.selectable_label(false, format!("{} · {}", s.name, s.track));
            if r.clicked() {
                ctx.state.handoff.focus = Some(s.clip);
            }
            ctx.auto(&format!("mfx.into.pick.{}", s.clip), &r, &s.name);
        }
        changes_list(ui, ctx);
        return;
    };
    let Some(s) = sel.iter().find(|s| s.clip == focus).cloned() else { return };
    let Some(p) = h.placed_clip(focus).cloned() else {
        ui.label(RichText::new(&s.name).strong().color(t.text));
        ui.colored_label(t.text_dim, format!("{}–{} · {}", clock(s.start_sec), clock(s.end_sec), s.track));
        ui.colored_label(t.text_dim, "Mortiflix didn't make this clip. It can trim or move it, not make a new version.");
        ask_box(ui, ctx, None, &s);
        changes_list(ui, ctx);
        return;
    };
    // What you clicked
    let beat = p.beat.as_deref().and_then(|b| h.beat(b)).cloned();
    ui.label(RichText::new(&p.name).strong().size(15.0).color(t.text));
    if let Some(b) = &beat {
        ui.horizontal(|ui| {
            swatch(ui, b.colour.as_deref());
            ui.colored_label(t.text, format!("{} {}", b.number, b.name));
        });
    }
    ui.colored_label(t.text_dim, format!("{}–{} · {}", clock(s.start_sec), clock(s.end_sec), s.track));
    if p.kind == "line" && !p.text.is_empty() {
        ui.colored_label(t.text_dim, format!("“{}”", short(&p.text, 200)));
    }
    // Versions, newest first
    ui.add_space(8.0);
    ui.label(RichText::new("Versions").strong().color(t.text));
    let now = h.nodes.get(&p.node).cloned().unwrap_or_default();
    let mut versions: Vec<u32> = h.items.iter().filter(|i| i.node == p.node).map(|i| i.version).collect();
    versions.extend(now.versions.iter().copied());
    versions.sort_unstable();
    versions.dedup();
    versions.reverse();
    if now.making {
        ui.horizontal(|ui| {
            ui.colored_label(t.accent, format!("v{} · making", now.current.max(p.version).saturating_add(1)));
            ui.add(egui::Spinner::new().size(12.0));
        });
    }
    if !now.failed.is_empty() {
        ui.colored_label(t.error, format!("The new version failed: {}", now.failed));
    }
    let bin_versions: Vec<u32> = ctx.state.handoff.items.iter().filter(|i| i.node == p.node).map(|i| i.version).collect();
    let in_bin = |v: u32| bin_versions.contains(&v);
    let mut use_v = None;
    for v in versions.iter().take(20) {
        ui.horizontal(|ui| {
            ui.colored_label(t.text, format!("v{v}"));
            if *v == p.version {
                ui.colored_label(t.ok, "On the timeline");
            } else if in_bin(*v) {
                let r = ui.small_button("Use this one");
                if r.clicked() {
                    use_v = Some(*v);
                }
                ctx.auto(&format!("mfx.into.use.v{v}"), &r, "Use this one");
            } else {
                ui.colored_label(t.text_faint, "coming in");
            }
        });
    }
    if let Some(v) = use_v {
        ctx.command("mortiflix.useVersion", json!({"node": p.node, "version": v}));
    }
    // Talk about this clip
    let talk: Vec<TalkLine> = ctx.state.handoff.talk.iter().filter(|l| l.node == p.node).rev().take(12).cloned().collect();
    if !talk.is_empty() {
        ui.add_space(8.0);
        for l in talk.iter().rev() {
            let who = if l.who == "you" { "You" } else { "Mortiflix" };
            ui.horizontal_wrapped(|ui| {
                ui.colored_label(t.text_faint, format!("{who}:"));
                ui.colored_label(t.text_dim, &l.text);
            });
        }
    }
    ask_box(ui, ctx, Some(&p), &s);
    changes_list(ui, ctx);
}

fn ask_box(ui: &mut egui::Ui, ctx: &mut MfxCtx, p: Option<&Placed>, s: &Selected) {
    let t = ctx.theme;
    let name = p.map_or(s.name.clone(), |p| p.name.rsplit_once(" · v").map_or(p.name.clone(), |(b, _)| b.to_string()));
    ui.add_space(8.0);
    ui.horizontal_wrapped(|ui| {
        if p.is_some() {
            let r = ui.small_button("New version");
            if r.clicked() {
                ctx.state.handoff.ask = format!("Make a new version of {name}: ");
            }
            ctx.auto("mfx.into.chip.newVersion", &r, "New version");
        }
        let r = ui.small_button("Trim it");
        if r.clicked() {
            ctx.state.handoff.ask = "Trim 0.5 s off the end".into();
        }
        ctx.auto("mfx.into.chip.trim", &r, "Trim it");
        if let Some(p) = p {
            let r = ui.small_button("Show it in Mortiflix");
            if r.clicked() {
                ctx.state.selected_node = Some(p.node.clone());
                let stage = if matches!(p.kind.as_str(), "line" | "music" | "sound") { "stages.voiceMusic" } else { "stages.shots" };
                ctx.command(stage, json!({}));
            }
            ctx.auto("mfx.into.chip.show", &r, "Show it in Mortiflix");
        }
    });
    let hint = format!("What should change in {name}?");
    let edit = ui.add(egui::TextEdit::multiline(&mut ctx.state.handoff.ask).hint_text(hint.as_str()).desired_rows(2).desired_width(f32::INFINITY));
    ctx.auto("mfx.into.ask", &edit, &hint);
    let enter = edit.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command);
    let send = accent_button(ui, ctx, "Send");
    ctx.auto("mfx.into.send", &send, "Send");
    if (send.clicked() || enter) && !ctx.state.handoff.ask.trim().is_empty() {
        let text = short(ctx.state.handoff.ask.trim(), 2000);
        ctx.state.handoff.ask.clear();
        send_request(ctx, p, s, &name, &text);
    }
    let _ = t;
}

/// Send: a trim FilmCraft can make itself is done at once (one undo step "Mortiflix: …"); anything
/// else becomes an edit request on that node.
fn send_request(ctx: &mut MfxCtx, p: Option<&Placed>, s: &Selected, name: &str, text: &str) {
    let node = p.map(|p| p.node.clone()).unwrap_or_default();
    ctx.state.handoff.talk("you", &node, text.to_string());
    let fps = if ctx.state.handoff.fps > 0.0 { ctx.state.handoff.fps } else { 30.0 };
    if let Some((label, steps)) = trim_steps(text, s.clip, name, fps) {
        let steps: Vec<Value> = steps.into_iter().map(|(id, params)| json!({"id": id, "params": params})).collect();
        ctx.command("mortiflix.timelineEdit", json!({"label": label, "steps": steps}));
        ctx.state.handoff.talk("mortiflix", &node, format!("Done in FilmCraft: {}. Undo takes it back.", label.trim_start_matches("Mortiflix: ")));
        return;
    }
    let Some(p) = p else {
        ctx.state.handoff.talk(
            "mortiflix",
            &node,
            "Mortiflix didn't make this clip, so it can't make a new version. Ask for a trim, e.g. \"trim 0.5 s off the start\".".into(),
        );
        return;
    };
    ctx.service.request("node.edit", json!({"node": p.node, "action": "fix", "text": text, "project": ctx.state.handoff.project}));
    let h = &mut ctx.state.handoff;
    h.changes.push(Change {
        node: p.node.clone(),
        label: name.to_string(),
        beat: p.beat.clone(),
        text: short(text, 300),
        state: ChangeState::Making,
        from_version: p.version,
    });
    if h.changes.len() > MAX_TALK {
        h.changes.remove(0);
    }
    h.talk("mortiflix", &p.node, format!("Making v{} of {name}. It lands in the same spot; the old one stays in Older versions.", p.version.saturating_add(1)));
    h.last_poll = f64::NEG_INFINITY;
}

fn changes_list(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    ui.add_space(10.0);
    let summary = ctx.state.handoff.changes_summary();
    let changes = ctx.state.handoff.changes.clone();
    let r = egui::CollapsingHeader::new(RichText::new(format!("Changes  {summary}")).color(t.text)).id_salt("mfx-into-changes").default_open(false).show(
        ui,
        |ui| {
            if changes.is_empty() {
                ui.colored_label(t.text_faint, "Nothing asked yet.");
            }
            for c in changes.iter().rev().take(50) {
                let beat = c.beat.as_deref().and_then(|b| ctx.state.handoff.beat(b)).cloned();
                ui.horizontal_wrapped(|ui| {
                    swatch(ui, beat.as_ref().and_then(|b| b.colour.as_deref()));
                    let (word, colour) = match c.state {
                        ChangeState::Making => ("making", t.accent),
                        ChangeState::Done => ("done", t.ok),
                        ChangeState::NeedsYou => ("needs you", t.warn),
                    };
                    ui.colored_label(t.text, &c.label);
                    ui.colored_label(t.text_dim, &c.text);
                    ui.colored_label(colour, word);
                });
            }
        },
    );
    ctx.auto("mfx.into.changes", &r.header_response, "Changes");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        let v = json!({
            "project": "p1", "title": "The Rise of AI", "rev": 2, "width": 1920, "height": 1080, "fps": 30, "seconds": 7.5,
            "fcproj": "/tmp/x.fcproj", "notes": "Mortiflix project p1",
            "beats": [{"id": "b01", "number": 1, "name": "Cold open", "colour": "Violet", "comment": "In 2012", "start_sec": 0},
                      {"id": "b02", "number": 2, "name": "The bet", "colour": "Mango", "comment": "Then", "start_sec": 3}],
            "pieces": [
                {"node": "s01", "kind": "shot", "label": "S01", "title": "Shot 1", "beat": "b01", "version": 1, "current": true, "file": "/m/s01-v1.mp4", "seconds": 3, "start_sec": 0},
                {"node": "s02", "kind": "shot", "label": "S02", "title": "Shot 2", "beat": "b02", "version": 1, "current": false, "file": "/m/s02-v1.mp4", "seconds": 2, "start_sec": 3},
                {"node": "s02", "kind": "shot", "label": "S02", "title": "Shot 2", "beat": "b02", "version": 2, "current": true, "file": "/m/s02-v2.mp4", "seconds": 2, "start_sec": 3},
                {"node": "s03", "kind": "shot", "label": "S03", "title": "Shot 3", "beat": "b02", "version": 1, "current": true, "cut": true, "file": "/m/s03-v1.mp4", "start_sec": null},
                {"node": "b01-l1", "kind": "line", "label": "b01-l1", "beat": "b01", "version": 1, "current": true, "file": "/m/l1.wav", "start_sec": 0.4, "end_sec": 2.4, "text": "In 2012 a network saw cats."},
                {"node": "music", "kind": "music", "label": "Music", "version": 1, "current": true, "file": "/m/music.wav", "start_sec": 0},
                {"node": "stem:bass.wav", "kind": "stem", "label": "bass", "version": 1, "current": true, "file": "/m/bass.wav"},
                {"node": "bad", "kind": "shot", "label": "S09", "version": 1, "current": true, "file": "", "start_sec": 0}
            ],
            "transitions": [{"node": "t01", "label": "T01", "name": "Cross Dissolve", "from": "s01", "to": "s02", "effect": "cross_dissolve"},
                            {"node": "t02", "label": "T02", "name": "Cut", "from": "s02", "to": "s03", "effect": null}],
            "cues": [{"node": "m01", "name": "M01 · Intro", "beat": "b01", "colour": "Violet", "start_sec": 0, "end_sec": 4},
                     {"node": "m02", "name": "M02 · Build", "beat": "b02", "colour": "Mango", "start_sec": 4, "end_sec": 8}]
        });
        Plan::parse(&v).unwrap()
    }

    /// A fake FilmCraft: records every command and answers like the real one.
    struct Fake {
        calls: Vec<(String, Value)>,
        next: u64,
        fail: Vec<&'static str>,
    }
    impl Fake {
        fn exec(&mut self, id: &str, p: Value) -> Result<Value, String> {
            self.calls.push((id.to_string(), p.clone()));
            if self.fail.iter().any(|f| *f == id || p.to_string().contains(f)) {
                return Err(format!("{id} failed"));
            }
            self.next += 1;
            Ok(match id {
                "file.newSequence" => json!({"sequence": 900}),
                "file.newBin" => json!({"bin": self.next}),
                "file.import" => json!({"items": [self.next], "errors": []}),
                "file.newColorMatte" => json!({"item": self.next}),
                "timeline.place" => json!({"clips": [self.next]}),
                _ => Value::Null,
            })
        }
        fn count(&self, id: &str) -> usize {
            self.calls.iter().filter(|c| c.0 == id).count()
        }
    }

    #[test]
    fn hostile_plans_are_bounded() {
        let p = Plan::parse(&json!({"fps": -3, "width": 0, "seconds": 1e300, "beats": [{"colour": "Neon", "start_sec": -1}], "pieces": [{"node": "x", "file": "/f", "start_sec": f64::MAX}]})).unwrap();
        assert_eq!((p.fps, p.width, p.height, p.seconds), (30.0, 1920, 1080, 0.0));
        assert_eq!(p.beats[0].colour, None);
        assert_eq!(p.beats[0].start_sec, None);
        assert_eq!(p.pieces[0].start_sec, None);
        assert!(Plan::parse(&json!({"pieces": "nope"})).is_err());
        assert!(Plan::parse(&json!(null)).is_err());
        assert_eq!(frame_at(f64::NAN, 30.0), None);
        assert_eq!(frame_at(-1.0, 30.0), None);
        assert_eq!(frame_at(1.5, 30.0), Some(45));
        assert_eq!(frame_at(1.0, 29.97), Some(30));
        assert_eq!(ticks_of_frames(30, 30.0), Some(TICKS_PER_SECOND));
        assert_eq!(ticks_of_frames(30_000, 29.97), Some(1001 * TICKS_PER_SECOND));
        assert_eq!(ticks_of_frames(i64::MAX, 30.0), None);
    }

    #[test]
    fn build_places_colours_and_marks_everything() {
        let mut f = Fake { calls: Vec::new(), next: 0, fail: Vec::new() };
        let mut h = Handoff::default();
        let p = plan();
        assert_eq!(p.pieces.len(), 7, "the piece without a file is dropped");
        build(&mut h, &p, &mut |id, v| f.exec(id, v));
        assert_eq!(h.phase, Phase::Done, "{}", h.error);
        assert_eq!(h.sequence, Some(900));
        // a sequence with V1 and two audio tracks (voice, music)
        let seq = &f.calls.iter().find(|c| c.0 == "file.newSequence").unwrap().1;
        assert_eq!((seq["fps"].as_f64(), seq["video"].as_u64(), seq["audio"].as_u64()), (Some(30.0), Some(1), Some(2)));
        assert_eq!(h.tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Shots", "Voice", "Music"]);
        // every piece and version imported, into its bin
        assert_eq!(f.count("file.import"), 7);
        let bins: Vec<&str> = h.bins.iter().map(|b| b.name.as_str()).collect();
        for b in ["Shots", "1 Cold open", "2 The bet", "Older versions", "Cut on the board", "Voice", "Music", "Music stems"] {
            assert!(bins.contains(&b), "{b} in {bins:?}");
        }
        assert_eq!(h.bins.iter().find(|b| b.name == "2 The bet").unwrap().colour.as_deref(), Some("Mango"));
        // shots on V1 at their frames (S02 v2, not v1; the cut S03 not at all), the line on A1 at 12
        let places: Vec<&Value> = f.calls.iter().filter(|c| c.0 == "timeline.place").map(|c| &c.1).collect();
        assert_eq!(places.iter().filter(|p| p["track"] == "V1").map(|p| p["frame"].as_i64().unwrap()).collect::<Vec<_>>(), [0, 90]);
        assert!(places.iter().any(|p| p["track"] == "A1" && p["frame"] == 12 && p["video"] == false));
        // the music cut at its cues: the first with a duration, the last to the end of the music
        let music: Vec<&&Value> = places.iter().filter(|p| p["track"] == "A2").collect();
        assert_eq!(music.len(), 2);
        assert_eq!(music[0]["duration"].as_i64(), Some(4 * TICKS_PER_SECOND));
        assert_eq!(music[1]["sourceIn"].as_i64(), Some(4 * TICKS_PER_SECOND));
        assert!(music[1].get("duration").is_none());
        assert_eq!(h.placed.iter().filter(|p| p.kind == "shot").map(|p| p.name.as_str()).collect::<Vec<_>>(), ["Shot 1 · v1", "Shot 2 · v2"]);
        // the dissolve on Shot 1's out edge; the cut is no transition
        assert_eq!(f.count("sequence.applyVideoTransition"), 1);
        // colours: a label per beat colour
        let labels: Vec<&str> = f.calls.iter().filter(|c| c.0 == "edit.label").map(|c| c.1["label"].as_str().unwrap()).collect();
        assert_eq!(labels, ["Mango", "Violet"]);
        // markers: a chapter per beat, a comment per cue
        let ch: Vec<String> = f.calls.iter().filter(|c| c.0 == "markers.addChapter").map(|c| c.1["name"].as_str().unwrap().to_string()).collect();
        assert_eq!(ch, ["1 Cold open", "2 The bet"]);
        assert_eq!(f.count("markers.add"), 2);
        assert_eq!(f.count("captions.add"), 1);
        assert_eq!(f.count("project.setNotes"), 1);
        let row = |k: &str| h.rows.iter().find(|r| r.key == k).unwrap().clone();
        assert_eq!((row("shots").count, row("shots").state), (2, RowState::Done));
        assert_eq!(row("sounds").state, RowState::None);
        assert_eq!(row("beats").count, 2);
    }

    #[test]
    fn a_failed_piece_is_listed_and_the_rest_comes_in() {
        let mut f = Fake { calls: Vec::new(), next: 0, fail: vec!["/m/l1.wav"] };
        let mut h = Handoff::default();
        build(&mut h, &plan(), &mut |id, v| f.exec(id, v));
        assert_eq!(h.phase, Phase::Done);
        let voice = h.rows.iter().find(|r| r.key == "voice").unwrap();
        assert_eq!(voice.state, RowState::Failed);
        assert_eq!(voice.failed[0].node, "b01-l1");
        assert_eq!(h.placed.iter().filter(|p| p.kind == "shot").count(), 2);
        // no sequence: the whole hand-off fails, calmly
        let mut f = Fake { calls: Vec::new(), next: 0, fail: vec!["file.newSequence"] };
        let mut h = Handoff::default();
        build(&mut h, &plan(), &mut |id, v| f.exec(id, v));
        assert_eq!(h.phase, Phase::Failed);
        assert!(h.error.contains("sequence"));
    }

    #[test]
    fn a_new_version_swaps_in_and_the_old_one_moves_to_older_versions() {
        let mut f = Fake { calls: Vec::new(), next: 0, fail: Vec::new() };
        let mut h = Handoff::default();
        build(&mut h, &plan(), &mut |id, v| f.exec(id, v));
        let clip = h.placed.iter().find(|p| p.node == "s01").unwrap().clip;
        let piece = Piece {
            node: "s01".into(),
            kind: "shot".into(),
            label: "S01".into(),
            title: "Shot 1".into(),
            beat: Some("b01".into()),
            version: 2,
            current: true,
            file: "/m/s01-v2.mp4".into(),
            ..Piece::default()
        };
        f.calls.clear();
        let name = swap(&mut h, &piece, &mut |id, v| f.exec(id, v)).unwrap();
        assert_eq!(name, "Shot 1 · v2");
        let rep = &f.calls.iter().find(|c| c.0 == "clip.replaceFromBin").unwrap().1;
        assert_eq!(rep["clips"], json!([clip]));
        assert_eq!(rep["keepSourceIn"], false, "the new item's own In point (its range in the file)");
        assert_eq!(f.count("project.moveToBin"), 1);
        assert!(f.calls.iter().any(|c| c.0 == "edit.label" && c.1["label"] == "Violet"), "the new version keeps the beat colour");
        assert_eq!(h.bins.iter().find(|b| b.name == "Older versions").unwrap().count, 2);
        assert_eq!(h.placed.iter().find(|p| p.node == "s01").unwrap().version, 2);
        // back to v1 from the bin, no AI
        f.calls.clear();
        use_version(&mut h, "s01", 1, &mut |id, v| f.exec(id, v)).unwrap();
        assert_eq!(f.count("file.import"), 0);
        assert_eq!(h.placed.iter().find(|p| p.node == "s01").unwrap().name, "Shot 1 · v1");
        assert!(use_version(&mut h, "s01", 7, &mut |id, v| f.exec(id, v)).is_err());
        assert!(swap(&mut h, &Piece { node: "zz".into(), ..Piece::default() }, &mut |id, v| f.exec(id, v)).is_err());
    }

    #[test]
    fn trims_from_plain_words() {
        let (label, steps) = trim_steps("Trim 0.5 s off the start", 7, "Shot 7", 30.0).unwrap();
        assert_eq!(label, "Mortiflix: Trim Shot 7 (0.5 s off the start)");
        assert_eq!(steps[0].1, json!({"clip": 7, "edge": "in", "mode": "regular", "deltaFrames": 15}));
        let (_, steps) = trim_steps("cut 12 frames from the end", 7, "Shot 7", 30.0).unwrap();
        assert_eq!(steps[0].1["deltaFrames"], -12);
        let (_, steps) = trim_steps("make it 1s shorter", 7, "Shot 7", 30.0).unwrap();
        assert_eq!((steps[0].1["edge"].as_str(), steps[0].1["deltaFrames"].as_i64()), (Some("out"), Some(-30)));
        assert!(trim_steps("make the sky purple", 7, "x", 30.0).is_none());
        assert!(trim_steps("trim it a bit", 7, "x", 30.0).is_none());
        assert!(trim_steps("trim 1e309 s", 7, "x", 30.0).is_none());
        assert!(trim_steps("trim -3 s", 7, "x", 30.0).is_none());
    }

    #[test]
    fn node_changes_become_swaps_and_changes() {
        let mut f = Fake { calls: Vec::new(), next: 0, fail: Vec::new() };
        let mut h = Handoff::default();
        build(&mut h, &plan(), &mut |id, v| f.exec(id, v));
        let changed = json!({"nodes": [{"id": "s02", "kind": "shot", "current": 2, "state": "generating"}, {"id": "ghost", "current": 9}]});
        assert!(read_changed(&mut h, &changed));
        assert!(!h.nodes.contains_key("ghost"));
        assert_eq!(h.changes_summary(), "1 making");
        assert!(read_changed(&mut h, &json!({"nodes": [{"id": "s02", "current": 2, "state": "needs you"}]})));
        assert_eq!(h.changes[0].state, ChangeState::NeedsYou);
        assert!(!read_changed(&mut h, &json!({"nodes": "x"})));
        // a handoff.swaps row: a piece in the same bin, at its range in the file
        let p = swap_piece(&h, &json!({"node": "s01", "version": 2, "path": "/m/s01-v2.mp4", "from_sec": 4, "to_sec": 6.5})).unwrap();
        assert_eq!((p.version, p.source_in_sec, p.seconds), (2, Some(4.0), Some(2.5)));
        assert_eq!(p.bin.as_deref(), Some("Shots/1 Cold open"));
        assert!(swap_piece(&h, &json!({"node": "s01", "version": 1, "path": "/m/x.mp4"})).is_none(), "already on the timeline");
        assert!(swap_piece(&h, &json!({"node": "s01", "version": 3})).is_none(), "no file");
        let done = done_params(&h);
        assert_eq!(done["map"]["clips"].as_array().unwrap().len(), 4, "shots, the line and the music once");
        assert_eq!(clock(38.5), "0:38.5");
        assert_eq!(clock(90.0), "1:30");
        assert_eq!(clock(f64::NAN), "0:00");
    }

    #[test]
    fn the_services_handoff_plan_is_read() {
        let v = json!({
            "project": "p1", "title": "The Rise of AI", "rev": 4,
            "sequence": {"name": "The Rise of AI", "width": 1920, "height": 1080, "fps": 30, "length_sec": 5.5},
            "clips": [
                {"node": "s01", "kind": "shot", "track": "V1", "bin": "Shots/1 Cold open", "name": "S01 · Shot 1", "label": "Violet", "version": 1, "start_sec": 0, "dur_sec": 3, "media": {"kind": "video", "path": "/m/anim.mp4", "from_sec": 0, "to_sec": 3}},
                {"node": "s02", "kind": "shot", "track": "V1", "bin": "Shots/2 The bet", "name": "S02 · Shot 2", "label": "Mango", "version": 1, "start_sec": 3, "dur_sec": 2.5, "media": {"kind": "video", "path": "/m/anim.mp4", "from_sec": 3, "to_sec": 5.5}},
                {"node": "s03", "kind": "shot", "track": "V1", "bin": "Shots/2 The bet", "name": "S03 · Shot 3", "label": "Mango", "version": null, "start_sec": 5.5, "dur_sec": 2, "media": {"kind": "card", "text": "Shot 3", "colour": "#803f17"}, "preview": true},
                {"node": "t01", "kind": "transition", "native": true, "transition": "Cross Dissolve", "from": "s01", "to": "s02", "at_sec": 3, "name": "T01 · Cross Dissolve"},
                {"node": "t02", "kind": "transition", "native": false, "transition": "Light leak", "from": "s02", "to": "s03", "name": "T02 · Light leak", "bin": "Transitions"},
                {"node": "b01-l1", "kind": "line", "track": "A1", "bin": "Voice", "name": "B01-L1 · In 2012", "label": "Violet", "version": 1, "start_sec": 0.4, "dur_sec": 2, "media": {"kind": "audio", "path": "/m/l1.wav"}},
                {"node": "music", "kind": "music", "track": "A2", "bin": "Music", "name": "Music", "version": 1, "start_sec": 0, "dur_sec": null, "media": {"kind": "audio", "path": "/m/music.wav"},
                 "cues": [{"node": "m01", "start_sec": 0, "end_sec": 3, "label": "Violet"}, {"node": "m02", "start_sec": 3, "end_sec": 8, "label": "Mango"}]}
            ],
            "markers": [{"kind": "chapter", "node": "b01", "time_sec": 0, "name": "1 Cold open", "label": "Violet", "comment": "In 2012"},
                        {"kind": "chapter", "node": "b02", "time_sec": 3, "name": "2 The bet", "label": "Mango", "comment": "Then"},
                        {"kind": "comment", "node": "m01", "time_sec": 0, "name": "M01 · Intro", "label": "Violet"}],
            "captions": [{"node": "b01-l1", "text": "In 2012 a network saw cats.", "start_sec": 0.4, "end_sec": 2.4}],
            "your_assets": [{"id": "a1", "name": "logo.png", "kind": "image", "path": "/m/logo.png", "bin": "Your assets/image"}],
            "older_versions": [{"node": "s01", "version": 0, "name": "S01 v0", "label": "Violet", "path": "/m/old.mp4", "from_sec": null, "to_sec": null}],
            "cut_on_board": [{"node": "s06", "version": 1, "name": "S06 · Shot 6", "label": "Teal", "path": "/m/anim.mp4", "from_sec": 13.5, "to_sec": 15.5}],
            "notes": "Mortiflix project p1", "previews": 1
        });
        let p = Plan::from_handoff(&v).unwrap();
        assert_eq!((p.title.as_str(), p.fps, p.seconds, p.rev), ("The Rise of AI", 30.0, 5.5, 4));
        assert_eq!(
            p.beats.iter().map(|b| (b.number, b.name.as_str(), b.colour.as_deref())).collect::<Vec<_>>(),
            [(1, "Cold open", Some("Violet")), (2, "The bet", Some("Mango"))]
        );
        let s02 = p.pieces.iter().find(|x| x.node == "s02").unwrap();
        assert_eq!((s02.beat.as_deref(), s02.source_in_sec, s02.seconds), (Some("b02"), Some(3.0), Some(2.5)));
        assert_eq!(p.pieces.iter().find(|x| x.node == "s03").unwrap().media, "card");
        assert_eq!(p.transitions.iter().map(|t| t.effect.clone()).collect::<Vec<_>>(), [Some("Cross Dissolve".to_string()), None]);
        let line = p.pieces.iter().find(|x| x.kind == "line").unwrap();
        assert_eq!((line.text.as_str(), line.end_sec), ("In 2012 a network saw cats.", Some(2.4)));
        assert_eq!(p.cues.iter().map(|c| (c.name.as_str(), c.end_sec)).collect::<Vec<_>>(), [("M01 · Intro", Some(3.0)), ("M02", Some(8.0))]);
        assert!(p.pieces.iter().any(|x| x.node == "s06" && x.cut && x.source_in_sec == Some(13.5)));
        assert!(p.pieces.iter().any(|x| x.kind == "older" && !x.current));
        assert!(p.pieces.iter().any(|x| x.kind == "asset" && x.bin.as_deref() == Some("Your assets/image")));
        assert_eq!(p.fcproj, "", "no media under a project record: nowhere to save");
        let mut v2 = v.clone();
        v2["clips"][0]["media"]["path"] = json!("/studio/state/p1/canvas/media/video/abc.mp4");
        assert_eq!(Plan::from_handoff(&v2).unwrap().fcproj, "/studio/state/p1/filmcraft/The Rise of AI.fcproj");
        // built: the animatic shots play their own range; the card is a colour matte; older ones in their bin
        let mut f = Fake { calls: Vec::new(), next: 0, fail: Vec::new() };
        let mut h = Handoff::default();
        build(&mut h, &p, &mut |id, v| f.exec(id, v));
        assert_eq!(h.phase, Phase::Done, "{}", h.error);
        let v1: Vec<&Value> = f.calls.iter().filter(|c| c.0 == "timeline.place" && c.1["track"] == "V1").map(|c| &c.1).collect();
        assert_eq!(v1.len(), 3);
        assert_eq!(v1[1]["sourceIn"].as_i64(), Some(3 * TICKS_PER_SECOND));
        assert_eq!(v1[1]["duration"].as_i64(), Some(TICKS_PER_SECOND * 5 / 2));
        assert!(v1[2].get("sourceIn").is_none());
        assert_eq!(f.count("file.newColorMatte"), 1);
        assert_eq!(f.count("project.setMarks"), 3, "s01, s02 and the cut s06 carry their ranges");
        assert!(h.bins.iter().any(|b| b.name == "1 Cold open" && b.colour.as_deref() == Some("Violet")));
        assert!(h.bins.iter().any(|b| b.name == "Older versions" && b.count == 1));
        assert_eq!(h.previews, 1);
        assert!(Plan::from_handoff(&json!([1, 2])).is_err());
        assert!(Plan::from_handoff(&json!({"clips": "x", "sequence": {"fps": "fast"}})).is_ok_and(|p| p.fps == 30.0));
    }

    #[test]
    fn start_asks_once_and_a_dead_service_fails_calmly() {
        let mut state = MfxState::default();
        let mut svc = Service::default();
        assert!(start(&mut state, &mut svc, false));
        assert!(!start(&mut state, &mut svc, false), "already preparing");
        poll(&mut state, &mut svc, 0.0);
        assert_eq!(state.handoff.phase, Phase::Failed);
        assert!(!state.handoff.error.is_empty());
        assert!(start(&mut state, &mut svc, false), "after a failure it can try again");
        assert!(take_jobs(&mut state).is_empty());
    }
}
