//! The Voice & music stage's data: the project graph (`graph.get`: every piece with its state,
//! verdict and versions) and the stage's view (`voicemusic.view`: shots, voice lines and cues at
//! their seconds, with the mix and your takes), on one time axis in seconds (doc 34, doc 19 §1).
//!
//! Everything here is pure and tested: the service's JSON is hostile input, so every number is
//! checked (finite, bounded) and every missing field has a calm default.

use egui::Color32;
use serde_json::Value;

/// Longest video the lanes lay out (seconds); anything later is clamped.
pub const MAX_SEC: f64 = 4.0 * 3600.0;
/// A planned shot (before the animatic) with no length from the service: the lanes show it this long.
pub const PLANNED_SHOT_SEC: f64 = 4.0;
/// An unhurried narration pace, for a line that has no timing yet (the booth's own rule).
const WORDS_PER_SEC: f64 = 2.6;
/// Default "Quieter under the voice" (doc 22 §3: about 18 dB under the voice).
pub const DEFAULT_DUCK_DB: f32 = -18.0;
/// The most pieces of one kind laid out (hostile input never allocates without bound).
const MAX_PIECES: usize = 2000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Shot,
    Line,
    Cue,
}

impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Kind::Shot => "shot",
            Kind::Line => "line",
            Kind::Cue => "cue",
        }
    }
}

/// Where one piece is at. The lanes mark each with a word or a sign, never colour alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceState {
    /// A music cue planned per beat (doc 33b): feel and energy, no notes yet.
    Planned,
    /// Not made yet: a striped "making" clip.
    Making,
    /// Made, no verdict yet.
    Ready,
    /// A Fix (new take, swap, new words) is being made; the old version plays meanwhile.
    Remaking,
    /// OK'd: the AI won't touch it.
    Ok,
    /// Cut it: kept, but out of the video.
    Cut,
    /// The last attempt failed: "! couldn't make it".
    Failed,
}

impl PieceState {
    /// The short mark on a clip and in lists.
    pub fn mark(self) -> &'static str {
        match self {
            PieceState::Planned => "planned",
            PieceState::Making => "making",
            PieceState::Ready => "",
            PieceState::Remaking => "↻",
            PieceState::Ok => "✓",
            PieceState::Cut => "cut",
            PieceState::Failed => "!",
        }
    }
    /// The words for the detail panel.
    pub fn words(self) -> &'static str {
        match self {
            PieceState::Planned => "Planned: the notes are written after the shots, to their exact cuts",
            PieceState::Making => "Still being made",
            PieceState::Ready => "Ready to check",
            PieceState::Remaking => "Remaking: the old version plays meanwhile",
            PieceState::Ok => "OK: the AI won't touch it",
            PieceState::Cut => "Cut: out of the video (kept, never deleted)",
            PieceState::Failed => "Couldn't make it",
        }
    }
}

/// One take of a line: a version the AI made (`v2`) or a recording of yours (`own1`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Take {
    pub id: String,
    pub own: bool,
    /// The version (AI) or the booth's take number (yours).
    pub no: u32,
    pub label: String,
    pub sec: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LineInfo {
    pub words: String,
    pub gain_db: f32,
    /// Per-line voice override.
    pub voice: Option<String>,
    /// The take that plays.
    pub take: Option<String>,
    pub takes: Vec<Take>,
    /// The start was set by hand (a timing lock).
    pub moved: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CueInfo {
    pub gain_db: f32,
    pub duck_db: f32,
    pub bars: Option<(f64, f64)>,
    /// Bars set by hand (Shorter / Longer).
    pub len_bars: Option<f64>,
    pub energy: Option<u8>,
    pub feel: String,
}

impl CueInfo {
    /// The cue's length in bars: set by hand, else its bars.
    pub fn bars_long(&self) -> Option<f64> {
        self.len_bars.or(self.bars.map(|(a, b)| b - a + 1.0)).filter(|n| n.is_finite() && *n >= 1.0)
    }
}

/// One clip on a lane.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub id: String,
    pub kind: Kind,
    /// "#7", "3·3", "M03".
    pub label: String,
    pub title: String,
    pub beat: Option<String>,
    pub beat_no: Option<u32>,
    /// The beat's Mortiflix Studio label fill (doc 11 §6).
    pub colour: Option<Color32>,
    pub start: f64,
    pub end: f64,
    pub state: PieceState,
    /// Version numbers, oldest first, and the one that plays.
    pub versions: Vec<u32>,
    pub current: Option<u32>,
    /// Why the last attempt failed.
    pub error: String,
    pub line: Option<LineInfo>,
    pub cue: Option<CueInfo>,
}

impl Piece {
    pub fn secs(&self) -> f64 {
        (self.end - self.start).max(0.0)
    }
    pub fn own_take(&self) -> bool {
        self.line.as_ref().and_then(|l| l.take.as_deref()).is_some_and(|t| t.starts_with("own"))
    }
    /// The clip's text on the lane: marks first ("✓ 1·1", "↻ v2 M04", "● 3·2").
    pub fn clip_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mark = self.state.mark();
        if !mark.is_empty() {
            parts.push(mark.to_string());
        }
        if self.own_take() {
            parts.push("●".into());
        } else if self.versions.len() > 1
            && let Some(v) = self.current
        {
            parts.push(format!("v{v}"));
        }
        parts.push(self.label.clone());
        if self.kind == Kind::Cue && !self.title.is_empty() {
            parts.push(self.title.clone());
        }
        parts.join(" ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Beat {
    pub id: String,
    pub no: u32,
    pub title: String,
    pub colour: Option<Color32>,
}

/// The stage on one time axis.
#[derive(Clone, Debug, PartialEq)]
pub struct Timeline {
    pub shots: Vec<Piece>,
    pub lines: Vec<Piece>,
    pub cues: Vec<Piece>,
    pub beats: Vec<Beat>,
    pub duration: f64,
    pub fps: f64,
    /// The studio's narration engine (`voice.list`): `none`, `elevenlabs`, `qwen`, `own`; "" unknown.
    pub narration: String,
    /// (id, label) of the voices a line can use.
    pub voices: Vec<(String, String)>,
    /// (name, summary) of the music theme (`music.theme.get`).
    pub theme: Option<(String, String)>,
    /// The mixed music exists (`voicemusic.view` `music`).
    pub has_music: bool,
    /// `voicemusic.view` answered (else times are estimated and levels, timing and takes are read-only).
    pub has_view: bool,
    /// The Voice stage: only the voice lines (the music is made after the animatic's gate).
    pub voice_only: bool,
}

impl Default for Timeline {
    fn default() -> Self {
        Self {
            shots: Vec::new(),
            lines: Vec::new(),
            cues: Vec::new(),
            beats: Vec::new(),
            duration: 0.0,
            fps: 30.0,
            narration: String::new(),
            voices: Vec::new(),
            theme: None,
            has_music: false,
            has_view: false,
            voice_only: false,
        }
    }
}

fn s<'a>(v: &'a Value, k: &str) -> &'a str {
    v.get(k).and_then(Value::as_str).unwrap_or("")
}

fn short(text: &str, max: usize) -> String {
    text.chars().take(max).collect()
}

/// A finite, non-negative number of seconds, clamped to [`MAX_SEC`].
fn sec(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|x| x.is_finite() && *x >= 0.0).map(|x| x.min(MAX_SEC))
}

fn db(v: Option<&Value>, lo: f32, hi: f32, dflt: f32) -> f32 {
    v.and_then(Value::as_f64).filter(|x| x.is_finite()).map_or(dflt, |x| (x as f32).clamp(lo, hi))
}

fn uint(v: Option<&Value>) -> Option<u32> {
    v.and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok())
}

/// `#380ea7` → a colour.
pub fn hex_colour(hex: &str) -> Option<Color32> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let p = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
    Some(Color32::from_rgb(p(0)?, p(2)?, p(4)?))
}

/// A node's `colour: {hex}`.
pub fn colour_of(v: &Value) -> Option<Color32> {
    v.get("colour").and_then(|c| c.get("hex")).and_then(Value::as_str).and_then(hex_colour)
}

/// `b03` → 3.
pub fn beat_no(beat: &str) -> Option<u32> {
    beat.strip_prefix('b').and_then(|d| d.parse::<u32>().ok())
}

/// `b03-2` → "3·2" (the line's number as doc 34 writes it); anything else as it is.
pub fn line_label(id: &str) -> String {
    match id.split_once('-') {
        Some((b, n)) if beat_no(b).is_some() => format!("{}·{n}", beat_no(b).unwrap_or(0)),
        _ => short(id, 20),
    }
}

/// "0:38.6".
pub fn fmt_time(sec: f64) -> String {
    let t = if sec.is_finite() { sec.clamp(0.0, MAX_SEC) } else { 0.0 };
    let tenths = (t * 10.0).round() as u64;
    format!("{}:{:02}.{}", tenths / 600, (tenths / 10) % 60, tenths % 10)
}

/// "2.4 s".
pub fn fmt_secs(sec: f64) -> String {
    format!("{:.1} s", if sec.is_finite() { sec } else { 0.0 })
}

/// "-3.0 dB" ("0.0 dB", never "-0.0").
pub fn fmt_db(db: f32) -> String {
    let v = if db.abs() < 0.05 { 0.0 } else { db };
    format!("{v:.1} dB")
}

/// The words for a voice engine id (Settings › Narration's names).
pub fn voice_label(id: &str) -> String {
    match id {
        "elevenlabs" => "ElevenLabs".into(),
        "qwen" => "This computer (Qwen3-TTS)".into(),
        "own" => "Your own voice".into(),
        "none" => "No narration".into(),
        other => short(other, 60),
    }
}

fn state_of(n: &Value, has_media: bool) -> (PieceState, String) {
    let pending = n.get("pending").is_some_and(|p| !p.is_null());
    let failed = n.get("failed").filter(|f| !f.is_null());
    let st = s(n, "state");
    let owner = s(n, "owner");
    if st == "cut" || owner == "cut" {
        return (PieceState::Cut, String::new());
    }
    if pending {
        return (if has_media { PieceState::Remaking } else { PieceState::Making }, String::new());
    }
    if st == "needs you" || failed.is_some() {
        let why = failed.map(|f| short(f.get("note").and_then(Value::as_str).filter(|x| !x.is_empty()).unwrap_or(s(f, "text")), 300)).unwrap_or_default();
        return (PieceState::Failed, why);
    }
    if !has_media {
        return (PieceState::Making, String::new());
    }
    if owner == "ok" || st == "approved" {
        return (PieceState::Ok, String::new());
    }
    (PieceState::Ready, String::new())
}

/// (version numbers, the one that plays, its entry). `plays` (graph.get) wins over `current`.
fn versions(n: &Value) -> (Vec<u32>, Option<u32>, Option<&Value>) {
    let list: Vec<&Value> = n.get("versions").and_then(Value::as_array).map(|a| a.iter().take(MAX_PIECES).collect()).unwrap_or_default();
    let nums: Vec<u32> = list.iter().filter_map(|v| uint(v.get("v"))).collect();
    let current = uint(n.get("plays")).or(uint(n.get("current")));
    let cur = current.and_then(|c| list.iter().find(|v| uint(v.get("v")) == Some(c)).copied()).or(list.last().copied());
    (nums, current, cur)
}

fn has_file(v: Option<&Value>) -> bool {
    v.is_some_and(|v| ["path", "file"].iter().any(|k| v.get(*k).and_then(Value::as_str).is_some_and(|f| !f.is_empty())))
}

/// Build the time axis from `graph.get` and, when the service has it, `voicemusic.view`.
pub fn build(graph: &Value, view: Option<&Value>) -> Timeline {
    let mut tl = Timeline::default();
    let list: Vec<&Value> = graph.get("nodes").and_then(Value::as_array).map(|a| a.iter().collect()).unwrap_or_default();
    let of = |kind: &str| -> Vec<&Value> {
        let mut v: Vec<&Value> = list
            .iter()
            .copied()
            .filter(|n| s(n, "kind") == kind && n.get("missing").and_then(Value::as_bool) != Some(true) && !s(n, "id").is_empty())
            .take(MAX_PIECES)
            .collect();
        v.sort_by_key(|n| n.get("order").and_then(Value::as_i64).unwrap_or(i64::MAX));
        v
    };
    let node = |id: &str| list.iter().copied().find(|n| s(n, "id") == id);
    let view_list =
        |k: &str| -> Vec<&Value> { view.and_then(|v| v.get(k)).and_then(Value::as_array).map(|a| a.iter().take(MAX_PIECES).collect()).unwrap_or_default() };
    if let Some(fps) = graph.get("fps").and_then(Value::as_f64).filter(|f| f.is_finite() && *f >= 1.0 && *f <= 240.0) {
        tl.fps = fps;
    }

    for b in of("beat") {
        let id = short(s(b, "id"), 40);
        let Some(no) = uint(b.get("beat_no")).or(beat_no(&id)) else { continue };
        tl.beats.push(Beat { id, no, title: short(s(b, "title"), 200), colour: colour_of(b) });
    }
    let beat_title = |beat: &Option<String>| beat.as_ref().and_then(|b| tl.beats.iter().find(|x| &x.id == b)).map(|b| b.title.clone()).unwrap_or_default();
    let beat_of = |n: &Value| n.get("beat").and_then(Value::as_str).map(|b| short(b, 40));
    let piece = |n: &Value, kind: Kind, label: String, start: f64, end: f64| {
        let (nums, current, cur) = versions(n);
        let media = if kind == Kind::Cue { cur.is_some() } else { has_file(cur) };
        let (state, error) = state_of(n, media);
        let beat = beat_of(n);
        Piece {
            id: short(s(n, "id"), 40),
            kind,
            label,
            title: short(s(n, "title"), 200),
            beat_no: uint(n.get("beat_no")).or(beat.as_deref().and_then(beat_no)),
            beat,
            colour: colour_of(n),
            start,
            end: end.max(start),
            state,
            versions: nums,
            current,
            error,
            line: None,
            cue: None,
        }
    };

    // Shots: the film order and seconds the service laid out (cut shots are out of the film);
    // without the view, back to back by their versions' lengths.
    let vshots = view_list("shots");
    if vshots.is_empty() {
        let mut t = 0.0;
        for (i, n) in of("shot").into_iter().enumerate() {
            if s(n, "state") == "cut" {
                continue;
            }
            let (_, _, cur) = versions(n);
            let len = cur.and_then(|v| Some(sec(v.get("to_sec"))? - sec(v.get("from_sec"))?)).filter(|d| *d > 0.0).unwrap_or(PLANNED_SHOT_SEC);
            let start = t;
            t = (t + len).min(MAX_SEC);
            tl.shots.push(piece(n, Kind::Shot, format!("#{}", i + 1), start, t));
        }
    } else {
        for (i, v) in vshots.into_iter().enumerate() {
            let id = s(v, "node");
            let Some(start) = sec(v.get("start_sec")) else { continue };
            let end = (start + sec(v.get("dur_sec")).unwrap_or(PLANNED_SHOT_SEC)).min(MAX_SEC);
            let mut p = match node(id) {
                Some(n) => piece(n, Kind::Shot, format!("#{}", i + 1), start, end),
                None => piece(v, Kind::Shot, format!("#{}", i + 1), start, end),
            };
            if p.id.is_empty() {
                p.id = short(id, 40);
            }
            // a still or a beat card stands in: the shot itself isn't made yet
            if s(v.get("media").unwrap_or(&Value::Null), "kind") != "video" && p.state == PieceState::Ready {
                p.state = PieceState::Making;
            }
            tl.shots.push(p);
        }
    }

    // Voice lines: their seconds and mix from the view (a hand-set start wins there); cut lines
    // stay on the lane (so they can be put back) at their script timing.
    let vvoice = view_list("voice");
    let mut prev_end = 0.0f64;
    for n in of("line") {
        let id = short(s(n, "id"), 40);
        let x = vvoice.iter().copied().find(|v| s(v, "node") == id).unwrap_or(&Value::Null);
        let words = short(if s(x, "words").is_empty() { s(n, "title") } else { s(x, "words") }, 2000);
        let (n0, n1) = (sec(n.get("start_sec")), sec(n.get("end_sec")));
        let est = (words.split_whitespace().count() as f64 / WORDS_PER_SEC).max(1.0);
        let len = sec(x.get("dur_sec")).filter(|d| *d > 0.0).or(match (n0, n1) {
            (Some(a), Some(b)) if b > a => Some(b - a),
            _ => None,
        });
        let start = sec(x.get("start_sec")).or(n0).unwrap_or(prev_end + 0.5).min(MAX_SEC);
        let end = (start + len.unwrap_or(est)).min(MAX_SEC);
        prev_end = end;
        let mut p = piece(n, Kind::Line, line_label(&id), start, end);
        p.title = beat_title(&p.beat);
        let own: Vec<&Value> = x.get("takes").and_then(Value::as_array).map(|a| a.iter().take(200).collect()).unwrap_or_default();
        let kept_own = own.iter().find(|t| t.get("kept").and_then(Value::as_bool) == Some(true)).and_then(|t| uint(t.get("take")));
        if kept_own.is_some() {
            // A kept take of yours is copied in as the line's newest version: shown once, as yours.
            p.versions.pop();
        }
        let mut takes: Vec<Take> = p.versions.iter().map(|v| Take { id: format!("v{v}"), own: false, no: *v, label: format!("v{v}"), sec: None }).collect();
        takes.extend(own.iter().filter_map(|t| {
            let no = uint(t.get("take"))?;
            Some(Take { id: format!("own{no}"), own: true, no, label: format!("your take {no}"), sec: sec(t.get("seconds")) })
        }));
        let take = kept_own.map(|n| format!("own{n}")).or(p.current.map(|c| format!("v{c}")));
        p.line = Some(LineInfo {
            words,
            gain_db: db(x.get("gain_db"), -60.0, 12.0, 0.0),
            voice: x.get("voice").and_then(Value::as_str).filter(|v| !v.is_empty()).map(|v| short(v, 120)),
            take,
            takes,
            moved: x.get("timing_locked").and_then(Value::as_bool) == Some(true),
        });
        tl.lines.push(p);
    }

    // Cues: one per beat, planned before the score (doc 33b), then the score's sections at their
    // seconds. A cue without seconds spans its beat's shots, else follows the one before.
    let vcues = view_list("cues");
    let cue_rows: Vec<&Value> = if vcues.is_empty() { of("cue") } else { vcues };
    let mut prev_end = 0.0f64;
    for c in cue_rows {
        let id = short(if s(c, "node").is_empty() { s(c, "id") } else { s(c, "node") }, 40);
        if id.is_empty() {
            continue;
        }
        let g = node(&id).filter(|n| s(n, "kind") == "cue");
        let beat = beat_of(c).or_else(|| g.and_then(beat_of));
        let span = beat.as_ref().and_then(|b| {
            let shots: Vec<&Piece> = tl.shots.iter().filter(|s| s.beat.as_ref() == Some(b)).collect();
            Some((shots.first()?.start, shots.iter().map(|s| s.end).fold(0.0, f64::max)))
        });
        let (start, end) = match (sec(c.get("start_sec")), sec(c.get("end_sec")), span) {
            (Some(a), Some(b), _) if b > a => (a, b),
            (_, _, Some((a, b))) if b > a => (a, b),
            _ => (prev_end, (prev_end + PLANNED_SHOT_SEC).min(MAX_SEC)),
        };
        prev_end = end;
        let planned = g.is_none() || c.get("scored").and_then(Value::as_bool) == Some(false);
        let mut p = piece(g.unwrap_or(c), Kind::Cue, short(&id.to_uppercase(), 20), start, end);
        p.id = id;
        if p.beat.is_none() {
            p.beat_no = beat.as_deref().and_then(beat_no);
            p.beat = beat;
        }
        if p.colour.is_none() {
            p.colour = colour_of(c)
                .or_else(|| tl.beats.iter().find(|b| Some(&b.id) == p.beat.as_ref()).and_then(|b| b.colour))
                .or_else(|| tl.shots.iter().find(|s| s.beat.is_some() && s.beat == p.beat).and_then(|s| s.colour));
        }
        if planned {
            p.state = PieceState::Planned;
            p.title = short(s(c, "feel"), 120);
        } else if p.title.is_empty() {
            p.title = short(s(c, "title"), 120);
        }
        let bars = c
            .get("bars")
            .or(g.and_then(|n| n.get("bars")))
            .and_then(Value::as_array)
            .and_then(|b| Some((b.first()?.as_f64()?, b.get(1)?.as_f64()?)))
            .filter(|(a, b)| a.is_finite() && b.is_finite() && b >= a);
        let energy = c.get("energy").or(g.and_then(|n| n.get("intensity"))).and_then(Value::as_u64).and_then(|e| u8::try_from(e.clamp(1, 5)).ok());
        p.cue = Some(CueInfo {
            gain_db: db(c.get("gain_db"), -60.0, 12.0, 0.0),
            duck_db: db(c.get("duck_db"), -40.0, 0.0, DEFAULT_DUCK_DB),
            bars,
            len_bars: c.get("len_bars").and_then(Value::as_f64).filter(|n| n.is_finite() && *n >= 1.0 && *n <= 512.0),
            energy,
            feel: short(s(c, "feel"), 300),
        });
        tl.cues.push(p);
    }

    let view_len = view.and_then(|v| sec(v.get("length_sec"))).unwrap_or(0.0);
    tl.duration = tl.all().map(|p| p.end).fold(view_len, f64::max);
    if let Some(v) = view {
        tl.has_view = true;
        tl.has_music = v.get("music").is_some_and(|m| m.is_object());
    }
    tl
}

/// The studio's voices (`voice.list`): the engine in use and the ones a line can switch to.
pub fn apply_voices(tl: &mut Timeline, list: &Value) {
    tl.narration = short(s(list, "engine"), 20);
    tl.voices = list
        .get("engines")
        .and_then(Value::as_array)
        .map(|a| a.iter().take(20).filter_map(Value::as_str).filter(|e| *e != "none" && !e.is_empty()).map(|e| (short(e, 40), voice_label(e))).collect())
        .unwrap_or_default();
}

/// The music theme in words (`music.theme.get` `{theme}`): the sample in use, else the genre.
pub fn apply_theme(tl: &mut Timeline, reply: &Value) {
    let Some(t) = reply.get("theme").filter(|t| t.is_object()) else {
        tl.theme = None;
        return;
    };
    let using = s(t, "using");
    let sample = t.get("samples").and_then(Value::as_array).and_then(|a| a.iter().take(16).find(|x| !using.is_empty() && s(x, "id") == using));
    let words: Vec<String> = t
        .get("mood")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .chain(std::iter::once(t.get("genre").unwrap_or(&Value::Null)))
        .chain(t.get("instruments").and_then(|i| i.get("use")).and_then(Value::as_array).into_iter().flatten())
        .take(16)
        .filter_map(Value::as_str)
        .filter(|w| !w.is_empty())
        .map(|w| short(w, 60))
        .collect();
    let name = sample.map(|x| s(x, "name")).filter(|n| !n.is_empty()).or(Some(s(t, "genre")).filter(|g| !g.is_empty())).unwrap_or("The AI's pick");
    tl.theme = Some((short(name, 120), short(&words.join(" · "), 300)));
}

/// How a line sits in its shot (doc 34: "✓ Fits shot 7, 1.0 s to spare" / "0.4 s too long for shot 7").
#[derive(Clone, Debug, PartialEq)]
pub enum Fit {
    Fits { shot: String, spare: f64 },
    TooLong { shot: String, over: f64 },
    NoShot,
}

impl Fit {
    pub fn text(&self) -> String {
        match self {
            Fit::Fits { shot, spare } => format!("✓ Fits shot {shot}, {} to spare", fmt_secs(*spare)),
            Fit::TooLong { shot, over } => format!("! {} too long for shot {shot}", fmt_secs(*over)),
            Fit::NoShot => "No shot under this line yet".into(),
        }
    }
}

/// The cut a cue lands on (doc 34: "✓ Lands on the cut into shot 8" / "misses the cut by 0.21 s").
#[derive(Clone, Debug, PartialEq)]
pub enum Landing {
    OnCut { into: String },
    Misses { by: f64 },
    End,
}

impl Landing {
    pub fn text(&self) -> String {
        match self {
            Landing::OnCut { into } => format!("✓ Lands on the cut into shot {into}"),
            Landing::Misses { by } => format!("! misses the cut by {by:.2} s"),
            Landing::End => "✓ Ends with the video".into(),
        }
    }
}

impl Timeline {
    pub fn all(&self) -> impl Iterator<Item = &Piece> {
        self.shots.iter().chain(&self.lines).chain(&self.cues)
    }
    pub fn find(&self, kind: Kind, id: &str) -> Option<&Piece> {
        let list = match kind {
            Kind::Shot => &self.shots,
            Kind::Line => &self.lines,
            Kind::Cue => &self.cues,
        };
        list.iter().find(|p| p.id == id)
    }
    /// A line or cue (what the chat's "talking about" and the node area can select).
    pub fn find_any(&self, id: &str) -> Option<&Piece> {
        self.lines.iter().chain(&self.cues).chain(&self.shots).find(|p| p.id == id)
    }
    /// The shot playing at `t`.
    pub fn shot_at(&self, t: f64) -> Option<&Piece> {
        self.shots.iter().find(|s| t >= s.start && t < s.end).or_else(|| self.shots.last().filter(|s| (t - s.end).abs() < 1e-6))
    }
    pub fn beat(&self, id: Option<&str>) -> Option<&Beat> {
        id.and_then(|b| self.beats.iter().find(|x| x.id == b))
    }
    /// The seconds a beat spans (its shots, else its lines and cues).
    pub fn beat_span(&self, beat: &str) -> Option<(f64, f64)> {
        let of: Vec<&Piece> = self.shots.iter().filter(|p| p.beat.as_deref() == Some(beat)).collect();
        let of = if of.is_empty() { self.all().filter(|p| p.beat.as_deref() == Some(beat)).collect() } else { of };
        let a = of.iter().map(|p| p.start).fold(f64::INFINITY, f64::min);
        let b = of.iter().map(|p| p.end).fold(0.0, f64::max);
        (a.is_finite() && b > a).then_some((a, b))
    }
    pub fn fit(&self, line: &Piece) -> Fit {
        let Some(shot) = self.shot_at(line.start) else { return Fit::NoShot };
        let n = shot.label.trim_start_matches('#').to_string();
        let spare = shot.end - line.end;
        if spare >= -1e-6 { Fit::Fits { shot: n, spare: spare.max(0.0) } } else { Fit::TooLong { shot: n, over: -spare } }
    }
    pub fn landing(&self, cue: &Piece) -> Landing {
        let film_end = self.shots.last().map_or(self.duration, |s| s.end);
        if cue.end >= film_end - 1.0 / self.fps {
            return Landing::End;
        }
        let near = self.shots.iter().skip(1).min_by(|a, b| (a.start - cue.end).abs().total_cmp(&(b.start - cue.end).abs()));
        match near {
            Some(s) if (s.start - cue.end).abs() <= 1.0 / self.fps => Landing::OnCut { into: s.label.trim_start_matches('#').to_string() },
            Some(s) => Landing::Misses { by: (s.start - cue.end).abs() },
            None => Landing::End,
        }
    }
    /// (OK'd, out of) for lines and for cues; cut and planned pieces don't count.
    pub fn tally(&self) -> ((usize, usize), (usize, usize)) {
        let t = |v: &[Piece]| {
            let live: Vec<&Piece> = v.iter().filter(|p| !matches!(p.state, PieceState::Cut | PieceState::Planned)).collect();
            (live.iter().filter(|p| p.state == PieceState::Ok).count(), live.len())
        };
        (t(&self.lines), t(&self.cues))
    }
    /// "7 of 13 lines OK · 2 of 5 cues OK".
    pub fn tally_text(&self) -> String {
        let ((lo, lt), (co, ct)) = self.tally();
        let mut parts = Vec::new();
        if lt > 0 {
            parts.push(format!("{lo} of {lt} {} OK", if lt == 1 { "line" } else { "lines" }));
        }
        if ct > 0 {
            parts.push(format!("{co} of {ct} {} OK", if ct == 1 { "cue" } else { "cues" }));
        }
        parts.join(" · ")
    }
    /// What has no OK yet (the move-on confirm lists them): labels, and whether anything is still being made.
    pub fn not_ok(&self) -> Vec<String> {
        self.lines
            .iter()
            .chain(&self.cues)
            .filter(|p| !matches!(p.state, PieceState::Ok | PieceState::Cut | PieceState::Planned))
            .map(|p| format!("{} {}", if p.kind == Kind::Line { "line" } else { "cue" }, p.label))
            .collect()
    }
    /// Pieces being made or remade: the stage keeps reading the view.
    pub fn busy(&self) -> usize {
        self.lines.iter().chain(&self.cues).filter(|p| matches!(p.state, PieceState::Making | PieceState::Remaking)).count()
    }
    pub fn failed(&self) -> usize {
        self.lines.iter().chain(&self.cues).filter(|p| p.state == PieceState::Failed).count()
    }
    /// The next / previous line from `id` (← / →), or the first one.
    pub fn step_line(&self, id: Option<&str>, dir: i32) -> Option<&Piece> {
        let i = id.and_then(|id| self.lines.iter().position(|l| l.id == id));
        let n = self.lines.len();
        if n == 0 {
            return None;
        }
        let at = match i {
            None => 0,
            Some(i) if dir < 0 => i.saturating_sub(1),
            Some(i) => (i + 1).min(n - 1),
        };
        self.lines.get(at)
    }
    /// No narration in this video (the Voice lane is gone, doc 34).
    pub fn no_narration(&self) -> bool {
        self.narration == "none" && self.lines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph() -> Value {
        json!({"fps": 30, "nodes": [
            {"id": "b01", "kind": "beat", "order": 0, "beat_no": 1, "title": "In 1956 ten people met.", "colour": {"hex": "#380ea7"}},
            {"id": "b02", "kind": "beat", "order": 1, "beat_no": 2, "title": "The bet.", "colour": {"hex": "#803f17"}},
            {"id": "s02", "kind": "shot", "order": 1, "beat": "b01", "versions": [{"v": 1, "path": "/v.mp4", "from_sec": 4, "to_sec": 7}], "plays": 1, "state": "approved", "owner": "ok"},
            {"id": "s01", "kind": "shot", "order": 0, "beat": "b01", "versions": [{"v": 1, "path": "/v.mp4", "from_sec": 0, "to_sec": 4}], "plays": 1, "state": "ready"},
            {"id": "s03", "kind": "shot", "order": 2, "beat": "b02", "versions": [], "plays": null},
            {"id": "s09", "kind": "shot", "order": 3, "missing": true},
            {"id": "b01-1", "kind": "line", "order": 0, "beat": "b01", "title": "In 1956, ten people met.", "start_sec": 0.4, "end_sec": 3.2,
             "versions": [{"v": 1, "path": "/a.wav"}, {"v": 2, "path": "/b.wav"}], "plays": 2, "state": "approved", "owner": "ok"},
            {"id": "b01-2", "kind": "line", "order": 1, "beat": "b01", "title": "They had one summer and a lot more words than fit", "start_sec": 4.2, "end_sec": 7.6,
             "versions": [{"v": 1, "path": "/c.wav"}], "plays": 1, "state": "generating", "pending": {"id": "E001"}},
            {"id": "b02-1", "kind": "line", "order": 2, "beat": "b02", "title": "Then came the bet.", "versions": [{"v": 1, "path": null}], "plays": 1, "state": "ready"},
            {"id": "b02-2", "kind": "line", "order": 3, "beat": "b02", "title": "x", "state": "needs you", "failed": {"note": "voice engine down"}, "versions": [{"v": 1, "path": "/d.wav"}], "plays": 1},
            {"id": "m01", "kind": "cue", "order": 0, "beat": "b01", "title": "Cold open", "bars": [1, 4], "intensity": 9, "versions": [{"v": 1}], "plays": 1, "state": "ready"},
            {"id": "m02", "kind": "cue", "order": 1, "beat": "b02", "title": "The bet", "versions": [{"v": 1}], "plays": 1, "state": "cut"},
            "junk", 7, {"kind": "line"}
        ]})
    }

    #[test]
    fn without_the_view_lays_out_shots_lines_and_cues_on_one_axis() {
        let tl = build(&graph(), None);
        assert_eq!(
            tl.shots.iter().map(|s| (s.id.as_str(), s.start, s.end)).collect::<Vec<_>>(),
            vec![("s01", 0.0, 4.0), ("s02", 4.0, 7.0), ("s03", 7.0, 11.0)]
        );
        assert_eq!(tl.shots[2].state, PieceState::Making, "a planned shot is still being made");
        assert_eq!(tl.shots[1].state, PieceState::Ok);
        assert_eq!(tl.beats.len(), 2);
        let l = &tl.lines;
        assert_eq!(l.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(), vec!["1·1", "1·2", "2·1", "2·2"]);
        assert_eq!((l[0].start, l[0].end, l[0].state), (0.4, 3.2, PieceState::Ok));
        assert_eq!(l[0].clip_text(), "✓ v2 1·1");
        assert_eq!(l[0].line.as_ref().unwrap().take.as_deref(), Some("v2"));
        assert_eq!(l[1].state, PieceState::Remaking);
        assert_eq!(l[2].state, PieceState::Making, "no clip file yet");
        assert!((l[2].start - 8.1).abs() < 1e-9, "no timing: half a second after the line before");
        assert_eq!((l[3].state, l[3].error.as_str()), (PieceState::Failed, "voice engine down"));
        assert_eq!((tl.cues[0].start, tl.cues[0].end), (0.0, 7.0), "no seconds: the cue spans its beat's shots");
        assert_eq!(tl.cues[0].cue.as_ref().and_then(|c| c.energy), Some(5), "hostile intensity clamped");
        assert_eq!(tl.cues[0].cue.as_ref().and_then(CueInfo::bars_long), Some(4.0));
        assert_eq!(tl.cues[1].state, PieceState::Cut);
        assert_eq!(tl.tally(), ((1, 4), (0, 1)));
        assert_eq!(tl.tally_text(), "1 of 4 lines OK · 0 of 1 cue OK");
        assert!(!tl.has_view);
    }

    #[test]
    fn the_view_sets_seconds_mix_takes_and_planned_cues() {
        let view = json!({"length_sec": 20.0, "music": {"path": "/m.wav"},
            "shots": [{"node": "s01", "start_sec": 0, "dur_sec": 5, "media": {"kind": "video"}}, {"node": "s02", "start_sec": 5, "dur_sec": 3, "media": {"kind": "video"}},
                      {"node": "s03", "start_sec": 8, "dur_sec": 6, "media": {"kind": "still"}}, {"node": "zz", "start_sec": "x"}],
            "voice": [{"node": "b01-1", "words": "In 1956, ten met.", "start_sec": 1.0, "dur_sec": 2.8, "gain_db": -3, "voice": "qwen", "timing_locked": true,
                       "takes": [{"take": 1, "kept": true, "seconds": 2.5}, {"take": "x"}]}],
            "cues": [{"node": "m01", "id": "m01", "start_sec": 0, "end_sec": 8, "duck_db": -12, "gain_db": "loud", "len_bars": 5, "scored": true, "beat": "b01"},
                     {"id": "m02", "node": "m02", "beat": "b02", "feel": "One held pad.", "energy": 2, "scored": false, "state": "planned"}]});
        let tl = build(&graph(), Some(&view));
        assert_eq!(tl.shots.iter().map(|s| (s.start, s.end)).collect::<Vec<_>>(), vec![(0.0, 5.0), (5.0, 8.0), (8.0, 14.0)]);
        assert_eq!(tl.shots[2].state, PieceState::Making, "a still stands in");
        let l = &tl.lines[0];
        assert_eq!((l.start, l.end), (1.0, 3.8));
        let info = l.line.as_ref().unwrap();
        assert!(info.moved);
        assert_eq!((info.gain_db, info.voice.as_deref(), info.words.as_str()), (-3.0, Some("qwen"), "In 1956, ten met."));
        assert_eq!(info.takes.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(), vec!["v1", "own1"], "the kept take's copy (v2) shows once, as yours");
        assert_eq!(info.take.as_deref(), Some("own1"));
        assert_eq!(l.clip_text(), "✓ ● 1·1");
        let c = &tl.cues;
        assert_eq!((c[0].start, c[0].end, c[0].state), (0.0, 8.0, PieceState::Ready));
        let ci = c[0].cue.as_ref().unwrap();
        assert_eq!((ci.duck_db, ci.gain_db, ci.bars_long()), (-12.0, 0.0, Some(5.0)));
        assert_eq!((c[1].state, c[1].title.as_str(), c[1].start, c[1].end), (PieceState::Planned, "One held pad.", 8.0, 14.0));
        assert_eq!(c[1].colour, Some(Color32::from_rgb(0x80, 0x3f, 0x17)), "a planned cue takes its beat's colour");
        assert_eq!(tl.tally(), ((1, 4), (0, 1)), "planned cues have nothing to OK yet");
        assert_eq!(tl.duration, 20.0);
        assert!(tl.has_view && tl.has_music);
    }

    #[test]
    fn voices_and_theme_in_words() {
        let mut tl = Timeline::default();
        apply_voices(&mut tl, &json!({"engine": "own", "engines": ["none", "elevenlabs", "qwen", "own", 5]}));
        assert_eq!(tl.narration, "own");
        assert_eq!(tl.voices.iter().map(|v| v.1.as_str()).collect::<Vec<_>>(), vec!["ElevenLabs", "This computer (Qwen3-TTS)", "Your own voice"]);
        apply_theme(
            &mut tl,
            &json!({"theme": {"mood": ["calm"], "genre": "ambient", "instruments": {"use": ["piano"]}, "samples": [{"id": "b", "name": "Cold machines"}], "using": "b"}}),
        );
        assert_eq!(tl.theme, Some(("Cold machines".into(), "calm · ambient · piano".into())));
        apply_theme(&mut tl, &json!({"theme": null}));
        assert_eq!(tl.theme, None);
    }

    #[test]
    fn fit_landing_and_steps() {
        let tl = build(&graph(), None);
        assert_eq!(tl.fit(&tl.lines[0]).text(), "✓ Fits shot 1, 0.8 s to spare");
        let over = tl.fit(&tl.lines[1]);
        assert!(matches!(over, Fit::TooLong { ref shot, over } if shot == "2" && (over - 0.6).abs() < 1e-9));
        assert_eq!(over.text(), "! 0.6 s too long for shot 2");
        assert_eq!(tl.landing(&tl.cues[0]), Landing::OnCut { into: "3".into() });
        let mut c = tl.cues[0].clone();
        c.end = 7.21;
        assert!(matches!(tl.landing(&c), Landing::Misses { by } if (by - 0.21).abs() < 1e-9));
        assert_eq!(tl.landing(&tl.cues[1]), Landing::End);
        assert_eq!(tl.step_line(None, 1).map(|l| l.id.as_str()), Some("b01-1"));
        assert_eq!(tl.step_line(Some("b01-1"), -1).map(|l| l.id.as_str()), Some("b01-1"));
        assert_eq!(tl.step_line(Some("b01-1"), 1).map(|l| l.id.as_str()), Some("b01-2"));
        assert_eq!(tl.beat_span("b01"), Some((0.0, 7.0)));
        assert_eq!(tl.busy(), 2);
        assert_eq!(tl.failed(), 1);
        assert_eq!(tl.not_ok().first().map(String::as_str), Some("line 1·2"));
    }

    #[test]
    fn hostile_and_empty_input_never_panics() {
        for v in [
            json!(null),
            json!([]),
            json!({"nodes": 3}),
            json!({"nodes": [{"id": "s1", "kind": "shot", "versions": [{"v": -1, "from_sec": 1e308, "to_sec": f64::MAX}], "plays": 99}]}),
        ] {
            for view in [json!({"voice": 5, "cues": {}, "shots": [{"node": 1, "start_sec": 1e300, "dur_sec": -4}]}), json!({"length_sec": "x"})] {
                let tl = build(&v, Some(&view));
                assert!(tl.duration.is_finite() && tl.duration <= MAX_SEC);
            }
        }
        assert_eq!(fmt_time(38.64), "0:38.6");
        assert_eq!(fmt_time(f64::NAN), "0:00.0");
        assert_eq!(fmt_time(3725.0), "62:05.0");
        assert_eq!(fmt_db(-0.01), "0.0 dB");
        assert_eq!(line_label("b03-3"), "3·3");
        assert_eq!(line_label("intro"), "intro");
        assert_eq!(hex_colour("#zz0000"), None);
        assert_eq!(hex_colour("#ÿÿÿ"), None);
        assert_eq!(beat_no("b07"), Some(7));
    }
}
