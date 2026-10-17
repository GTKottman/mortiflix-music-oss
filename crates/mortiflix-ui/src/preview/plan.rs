//! The loose preview's edit list (doc 36 "How the loose preview is made fast"): every shot in film
//! order with the best media it has, at its planned time, plus the beats, the voice lines and the
//! music. Built from the service's `preview.plan` (the service picks the best media per shot: its
//! piece of the animatic, else its look's still, else a beat card, cut shots left out, voice and
//! music at their times) plus `graph.get` for what the panel shows around it (beat names, the
//! look's still to fall back to, shots being remade). The host turns it into Mortiflix Studio's "Loose
//! preview" sequence. Pure data: no egui, no Mortiflix Studio. Every value from the service is hostile
//! input: unknown or malformed fields fall back, and a file that isn't there falls back too.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Length of a shot with no timing yet (no motion, no beat target).
pub const DEFAULT_SHOT_SEC: f64 = 4.0;
/// Longest shot or line taken from the graph; longer values are clamped.
pub const MAX_SHOT_SEC: f64 = 600.0;
/// Shortest shot (one frame at 25 fps).
pub const MIN_SHOT_SEC: f64 = 0.04;
/// Most shots / lines read from one graph (a larger graph is cut, never a huge allocation).
pub const MAX_ITEMS: usize = 5000;

const VIDEO_EXT: [&str; 7] = ["mp4", "mov", "m4v", "mkv", "webm", "avi", "mxf"];
const IMAGE_EXT: [&str; 7] = ["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff"];
const AUDIO_EXT: [&str; 7] = ["wav", "mp3", "m4a", "aac", "flac", "ogg", "opus"];

/// What a shot shows in the loose preview.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Media {
    /// Its animatic piece: `file` from `from` to `to` seconds.
    Motion { file: String, from: f64, to: f64 },
    /// A still frame held for the shot's length.
    Still { file: String },
    /// Not made yet: a plain card in the beat's colour.
    #[default]
    Card,
}

impl Media {
    /// The caption's "what kind of picture it is" (doc 36).
    pub fn kind_text(&self) -> &'static str {
        match self {
            Media::Motion { .. } => "Moving · animatic",
            Media::Still { .. } => "Still frame · motion comes next",
            Media::Card => "Not made yet",
        }
    }
    pub fn file(&self) -> Option<&str> {
        match self {
            Media::Motion { file, .. } | Media::Still { file } => Some(file),
            Media::Card => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanShot {
    pub id: String,
    /// "Shot 4" (from the node's label `S04`).
    pub label: String,
    /// The shot's words (its narration line or name).
    pub title: String,
    pub beat: Option<String>,
    /// Beat colour (the block fill) and its chip colour (lines, text), `#rrggbb`.
    pub colour: String,
    pub chip: String,
    pub start: f64,
    pub len: f64,
    pub media: Media,
    /// The look's still, kept so the host can fall back to it when the motion won't play.
    pub still: Option<String>,
    pub version: u32,
    pub versions: u32,
    /// A new version is being made (an open edit request); the kept one keeps playing.
    pub making: bool,
    /// The last Fix failed: the node needs you.
    pub failed: bool,
    /// What went wrong with its media (shown in the caption with Remake / Show the file).
    pub problem: Option<String>,
}

impl PlanShot {
    pub fn end(&self) -> f64 {
        self.start + self.len
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanBeat {
    pub id: String,
    /// "2" (the beat number shown beside its colour).
    pub number: String,
    pub title: String,
    pub colour: String,
    pub chip: String,
    pub start: f64,
    pub end: f64,
    /// The beat's first shot (clicking the beat selects it).
    pub first_shot: Option<String>,
}

/// A voice line or the music: a sound file at a time.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PlanSound {
    pub id: String,
    /// None: nothing to play yet (the row shows only what exists).
    pub file: Option<String>,
    pub start: f64,
    pub len: f64,
    /// A stand-in (AI voice before your own take, the Strudel stand-in mix): dotted in the rows.
    pub stand_in: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Plan {
    pub shots: Vec<PlanShot>,
    pub beats: Vec<PlanBeat>,
    pub voice: Vec<PlanSound>,
    pub music: Option<PlanSound>,
    /// Length of the whole film so far (seconds).
    pub total: f64,
}

/// Counts for the legend: Moving · Still · Not made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub moving: usize,
    pub still: usize,
    pub not_made: usize,
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite())
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(500).collect()).unwrap_or_default()
}

fn ext_of(file: &str) -> String {
    Path::new(file).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default()
}

pub fn is_video(file: &str) -> bool {
    VIDEO_EXT.contains(&ext_of(file).as_str())
}
pub fn is_image(file: &str) -> bool {
    IMAGE_EXT.contains(&ext_of(file).as_str())
}
pub fn is_audio(file: &str) -> bool {
    AUDIO_EXT.contains(&ext_of(file).as_str())
}

/// `#rrggbb` → bytes; None for anything else.
pub fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let b = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
    Some([b(0)?, b(2)?, b(4)?])
}

/// The "not made yet" card colour: the beat's colour dimmed to 30 % on #0e0e0e (doc 36 states).
pub fn card_colour(hex: &str) -> String {
    let c = parse_hex(hex).unwrap_or([0x3a, 0x3a, 0x3a]);
    let mix = |x: u8| -> u8 {
        let v = 14.0 + (f32::from(x) - 14.0) * 0.3;
        v.round().clamp(0.0, 255.0) as u8
    };
    format!("#{:02x}{:02x}{:02x}", mix(c[0]), mix(c[1]), mix(c[2]))
}

/// A media path from the service: absolute, no `..`, no NUL, sane length. Anything else is refused.
pub fn clean_path(p: &str) -> Option<String> {
    let path = Path::new(p);
    let ok =
        !p.is_empty() && p.len() <= 4096 && !p.contains('\0') && path.is_absolute() && !path.components().any(|c| matches!(c, std::path::Component::ParentDir));
    ok.then(|| p.to_string())
}

fn path_of(v: Option<&Value>) -> Option<String> {
    v?.get("path").and_then(Value::as_str).and_then(clean_path)
}

/// `{hex, chip}` → (hex, chip), with greys for anything missing.
fn colours(c: Option<&Value>) -> (String, String) {
    let hex = c.and_then(|c| c.get("hex")).and_then(Value::as_str).filter(|h| parse_hex(h).is_some()).unwrap_or("#3a3a3a");
    let chip = c.and_then(|c| c.get("chip")).and_then(Value::as_str).filter(|h| parse_hex(h).is_some()).unwrap_or("#9a9a9a");
    (hex.to_string(), chip.to_string())
}

/// "S04" → "Shot 4"; anything else as it is.
fn shot_label(label: &str, id: &str) -> String {
    let src = if label.is_empty() { id } else { label };
    let digits: String = src.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    match digits.parse::<u32>() {
        Ok(n) if src.to_ascii_lowercase().starts_with('s') => format!("Shot {n}"),
        _ => src.to_string(),
    }
}

fn clamp_len(x: f64) -> f64 {
    if x.is_finite() && x > 0.0 { x.clamp(MIN_SHOT_SEC, MAX_SHOT_SEC) } else { DEFAULT_SHOT_SEC }
}

/// What `graph.get` adds: beat names and colours, each shot's look still, shots being remade.
#[derive(Default)]
struct Extras {
    beats: HashMap<String, (String, String, String)>,
    still: HashMap<String, String>,
    making: HashMap<String, bool>,
    failed: HashMap<String, bool>,
    versions: HashMap<String, u32>,
}

impl Extras {
    fn of(graph: Option<&Value>) -> Extras {
        let mut x = Extras::default();
        let Some(g) = graph else { return x };
        for b in g.get("beats").and_then(Value::as_array).into_iter().flatten().take(MAX_ITEMS) {
            let Some(id) = b.get("id").and_then(Value::as_str) else { continue };
            let (hex, chip) = colours(b.get("colour"));
            // the beat's short name (graph.get `name`), else its words
            let name = Some(text(b, "name")).filter(|n| !n.is_empty()).unwrap_or_else(|| text(b, "title"));
            x.beats.insert(id.chars().take(200).collect(), (name, hex, chip));
        }
        let nodes: Vec<&Value> = g.get("nodes").and_then(Value::as_array).map(|a| a.iter().take(MAX_ITEMS * 4).collect()).unwrap_or_default();
        let by_id: HashMap<&str, &Value> = nodes.iter().filter_map(|n| Some((n.get("id")?.as_str()?, *n))).collect();
        let plays = |n: &Value| -> Option<String> {
            let want = n.get("plays").and_then(Value::as_u64).or_else(|| n.get("current").and_then(Value::as_u64))?;
            let v = n.get("versions")?.as_array()?.iter().find(|v| v.get("v").and_then(Value::as_u64) == Some(want))?;
            path_of(Some(v))
        };
        for n in &nodes {
            if n.get("kind").and_then(Value::as_str) != Some("shot") {
                continue;
            }
            let Some(id) = n.get("id").and_then(Value::as_str) else { continue };
            if let Some(still) = n.get("look").and_then(Value::as_str).and_then(|l| by_id.get(l)).and_then(|l| plays(l)).filter(|f| is_image(f)) {
                x.still.insert(id.to_string(), still);
            }
            x.making.insert(id.to_string(), n.get("pending").is_some_and(Value::is_object));
            x.failed.insert(id.to_string(), n.get("failed").is_some_and(Value::is_object));
            let count = n.get("versions").and_then(Value::as_array).map_or(0, Vec::len);
            x.versions.insert(id.to_string(), u32::try_from(count).unwrap_or(u32::MAX));
        }
        x
    }
}

/// Does the file exist? (A motion file that has gone shows its still instead.)
pub type Exists<'a> = &'a dyn Fn(&str) -> bool;

impl Plan {
    /// Build the plan from the service's `preview.plan` result (`plan`) and, when there is one, its
    /// `graph.get` result (`graph`).
    pub fn from_service(plan: &Value, graph: Option<&Value>, exists: Exists) -> Plan {
        let extras = Extras::of(graph);
        let mut out = Plan::default();
        let mut t = 0.0f64;
        for s in plan.get("shots").and_then(Value::as_array).into_iter().flatten().take(MAX_ITEMS) {
            let Some(id) = s.get("node").and_then(Value::as_str).map(|x| x.chars().take(200).collect::<String>()) else { continue };
            if s.get("state").and_then(Value::as_str) == Some("cut") {
                continue;
            }
            let label = shot_label(&text(s, "label"), &id);
            let beat = s.get("beat").and_then(Value::as_str).map(str::to_string);
            let (colour, chip) = colours(s.get("colour"));
            let still = extras.still.get(&id).cloned().filter(|f| exists(f));
            let m = s.get("media");
            let kind = m.and_then(|m| m.get("kind")).and_then(Value::as_str).unwrap_or("card");
            let file = path_of(m);
            let dur = clamp_len(num(s, "dur_sec").unwrap_or(DEFAULT_SHOT_SEC));
            let mut problem = None;
            let fallback = |still: &Option<String>| still.clone().map_or(Media::Card, |file| Media::Still { file });
            let media = match (kind, file) {
                ("video", Some(f)) if exists(&f) => {
                    let from = m.and_then(|m| num(m, "from_sec")).unwrap_or(0.0).max(0.0);
                    Media::Motion { file: f, from, to: from + dur }
                }
                ("still", Some(f)) if exists(&f) => Media::Still { file: f },
                ("video" | "still", Some(f)) => {
                    log::warn!("loose preview: {id}: {f} is missing");
                    problem = Some(match &still {
                        Some(_) => format!("{label}'s motion won't play. Showing its still frame."),
                        None => format!("{label}'s picture won't play. Showing a card."),
                    });
                    fallback(&still)
                }
                _ => Media::Card,
            };
            let start = num(s, "start_sec").filter(|x| *x >= 0.0 && (*x - t).abs() < 0.01).unwrap_or(t);
            out.shots.push(PlanShot {
                label,
                title: text(s, "title"),
                beat,
                colour,
                chip,
                start,
                len: dur,
                media,
                still,
                version: s.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
                versions: extras.versions.get(&id).copied().unwrap_or(0),
                making: extras.making.get(&id).copied().unwrap_or(false),
                failed: extras.failed.get(&id).copied().unwrap_or(false),
                problem,
                id,
            });
            t = start + dur;
        }
        out.total = t;
        // beats: each run of shots with the same beat, in film order
        let mut i = 0;
        while let Some(first) = out.shots.get(i) {
            let beat = first.beat.clone();
            let mut j = i;
            while out.shots.get(j + 1).is_some_and(|s| s.beat == beat) {
                j += 1;
            }
            if let (Some(b), Some(last)) = (beat, out.shots.get(j)) {
                let no = plan
                    .get("shots")
                    .and_then(Value::as_array)
                    .and_then(|a| a.iter().find(|s| s.get("node").and_then(Value::as_str) == Some(first.id.as_str())))
                    .and_then(|s| s.get("beat_no"))
                    .and_then(Value::as_u64)
                    .map_or_else(|| b.trim_start_matches(|c: char| !c.is_ascii_digit()).trim_start_matches('0').to_string(), |n| n.to_string());
                let (title, hex, chip) = extras.beats.get(&b).cloned().unwrap_or_else(|| (String::new(), first.colour.clone(), first.chip.clone()));
                out.beats.push(PlanBeat {
                    id: b,
                    number: no,
                    title,
                    colour: hex,
                    chip,
                    start: first.start,
                    end: last.end(),
                    first_shot: Some(first.id.clone()),
                });
            }
            i = j + 1;
        }
        for v in plan.get("voice").and_then(Value::as_array).into_iter().flatten().take(MAX_ITEMS) {
            let Some(id) = v.get("node").and_then(Value::as_str) else { continue };
            if v.get("state").and_then(Value::as_str) == Some("cut") {
                continue;
            }
            let Some(start) = num(v, "start_sec").filter(|x| *x >= 0.0) else { continue };
            let file = path_of(Some(v)).filter(|f| is_audio(f) && exists(f));
            let len = clamp_len(num(v, "dur_sec").unwrap_or(2.0));
            out.voice.push(PlanSound { id: id.chars().take(200).collect(), file, start, len, stand_in: stand_in(v) });
        }
        out.music = plan.get("music").and_then(|m| {
            let file = path_of(Some(m)).filter(|f| is_audio(f) && exists(f))?;
            let start = num(m, "start_sec").unwrap_or(0.0).max(0.0);
            Some(PlanSound { id: text(m, "node"), file: Some(file), start, len: (out.total - start).max(0.0), stand_in: stand_in(m) })
        });
        out
    }

    /// What the host's sequence depends on: changes when a clip must change.
    pub fn signature(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for s in &self.shots {
            s.id.hash(&mut h);
            format!("{:?}", s.media).hash(&mut h);
            s.colour.hash(&mut h);
            s.start.to_bits().hash(&mut h);
            s.len.to_bits().hash(&mut h);
        }
        for v in self.voice.iter().chain(&self.music) {
            v.file.hash(&mut h);
            v.start.to_bits().hash(&mut h);
            v.len.to_bits().hash(&mut h);
        }
        h.finish()
    }

    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for s in &self.shots {
            match s.media {
                Media::Motion { .. } => c.moving += 1,
                Media::Still { .. } => c.still += 1,
                Media::Card => c.not_made += 1,
            }
        }
        c
    }

    /// The shot under time `t` (the last one at the very end).
    pub fn shot_at(&self, t: f64) -> Option<&PlanShot> {
        self.shots.iter().find(|s| t >= s.start && t < s.end()).or_else(|| self.shots.last().filter(|s| t >= s.end()))
    }

    pub fn shot(&self, id: &str) -> Option<&PlanShot> {
        self.shots.iter().find(|s| s.id == id)
    }

    pub fn beat(&self, id: &str) -> Option<&PlanBeat> {
        self.beats.iter().find(|b| b.id == id)
    }

    /// Where a node is in the film: a shot, a beat (its first shot), a voice line, or a shot of the
    /// node's beat. (start, end) in seconds.
    pub fn span_of(&self, node: &str) -> Option<(f64, f64)> {
        if let Some(s) = self.shot(node) {
            return Some((s.start, s.end()));
        }
        if let Some(b) = self.beat(node) {
            return Some((b.start, b.end));
        }
        self.voice.iter().find(|v| v.id == node).map(|v| (v.start, v.start + v.len))
    }
}

fn stand_in(n: &Value) -> bool {
    n.get("stand_in").or_else(|| n.get("standIn")).and_then(Value::as_bool).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn all(_: &str) -> bool {
        true
    }

    fn plan() -> Value {
        json!({"rev": 3, "range": [0, 11], "length_sec": 11, "fps": 30, "shots": [
            {"node": "s01", "label": "S01", "title": "They bet", "beat": "b01", "beat_no": 1, "colour": {"hex": "#380ea7", "chip": "#a98caf"}, "state": "ready", "version": 2,
             "start_sec": 0, "dur_sec": 3.5, "media": {"kind": "video", "path": "/st/state/p1/canvas/media/video/v.mp4", "from_sec": 2, "to_sec": 5.5}},
            {"node": "s02", "label": "S02", "beat": "b01", "beat_no": 1, "colour": {"hex": "#380ea7", "chip": "#a98caf"}, "start_sec": 3.5, "dur_sec": 3,
             "media": {"kind": "still", "path": "/st/state/p1/canvas/media/sf01/a.png"}},
            {"node": "s03", "label": "S03", "beat": "b02", "beat_no": 2, "colour": {"hex": "#803f17", "chip": "#da7639"}, "start_sec": 6.5, "dur_sec": 4.5,
             "media": {"kind": "card", "text": "The bet"}},
            {"node": "s04", "state": "cut", "dur_sec": 9},
            "junk", 7, {"label": "no node"}
        ], "voice": [
            {"node": "b01-01", "path": "/st/state/p1/canvas/media/b01-01/x.wav", "start_sec": 0.5, "dur_sec": 2},
            {"node": "b01-02", "path": null, "start_sec": 2.5, "dur_sec": null}
        ], "music": {"node": "music", "path": "/st/state/p1/canvas/media/music/m.wav", "start_sec": 0}})
    }

    fn graph() -> Value {
        json!({"beats": [{"id": "b01", "no": 1, "title": "Cold open", "colour": {"hex": "#380ea7", "chip": "#a98caf"}}, {"id": "b02", "no": 2, "title": "The bet"}],
               "nodes": [
                   {"id": "sf01", "kind": "frame", "plays": 1, "versions": [{"v": 1, "path": "/st/state/p1/canvas/media/sf01/a.png"}]},
                   {"id": "s01", "kind": "shot", "look": "sf01", "versions": [{"v": 1}, {"v": 2}]},
                   {"id": "s03", "kind": "shot", "pending": {"id": "E001"}, "versions": []}]})
    }

    #[test]
    fn the_service_plan_becomes_shots_beats_and_sounds() {
        let p = Plan::from_service(&plan(), Some(&graph()), &all);
        let ids: Vec<&str> = p.shots.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, ["s01", "s02", "s03"]);
        assert_eq!(p.shots[0].media, Media::Motion { file: "/st/state/p1/canvas/media/video/v.mp4".into(), from: 2.0, to: 5.5 });
        assert_eq!((p.shots[0].label.as_str(), p.shots[0].version, p.shots[0].versions), ("Shot 1", 2, 2));
        assert_eq!(p.shots[0].still.as_deref(), Some("/st/state/p1/canvas/media/sf01/a.png"));
        assert_eq!(p.shots[1].media, Media::Still { file: "/st/state/p1/canvas/media/sf01/a.png".into() });
        assert_eq!(p.shots[2].media, Media::Card);
        assert!(p.shots[2].making);
        assert!((p.total - 11.0).abs() < 1e-9);
        assert_eq!(p.counts(), Counts { moving: 1, still: 1, not_made: 1 });
        assert_eq!(p.beats.len(), 2);
        assert_eq!((p.beats[0].number.as_str(), p.beats[0].title.as_str()), ("1", "Cold open"));
        assert_eq!((p.beats[1].first_shot.as_deref(), p.beats[1].start), (Some("s03"), 6.5));
        assert_eq!(p.voice.len(), 2);
        assert!(p.voice[1].file.is_none());
        assert!(p.music.as_ref().is_some_and(|m| (m.len - 11.0).abs() < 1e-9));
        assert_eq!(p.shot_at(4.0).map(|s| s.id.as_str()), Some("s02"));
        assert_eq!(p.shot_at(999.0).map(|s| s.id.as_str()), Some("s03"));
        assert_eq!(p.span_of("b02").map(|s| s.0), Some(6.5));
        // without graph.get: no beat names, the rest the same
        let q = Plan::from_service(&plan(), None, &all);
        assert_eq!(q.shots.len(), 3);
        assert_eq!(q.beats[1].number, "2");
    }

    #[test]
    fn a_missing_motion_file_falls_back_to_the_still_with_a_problem() {
        let p = Plan::from_service(&plan(), Some(&graph()), &|f: &str| !f.ends_with(".mp4"));
        assert!(matches!(p.shots[0].media, Media::Still { .. }));
        assert_eq!(p.shots[0].problem.as_deref(), Some("Shot 1's motion won't play. Showing its still frame."));
        assert_ne!(p.signature(), Plan::from_service(&plan(), Some(&graph()), &all).signature());
    }

    #[test]
    fn hostile_plans_never_panic_and_paths_must_be_clean() {
        for g in [json!(null), json!(3), json!("x"), json!({}), json!({"shots": "no"}), json!({"shots": [{"node": "s1", "dur_sec": "x", "media": 5}]})] {
            let p = Plan::from_service(&g, Some(&g), &all);
            assert!(p.total.is_finite());
        }
        let evil = json!({"shots": [{"node": "s01", "dur_sec": 1e300, "media": {"kind": "video", "path": "/a/../../etc/passwd.mp4", "from_sec": -5}},
                                    {"node": "s02", "dur_sec": -1, "media": {"kind": "still", "path": "rel/x.png"}}]});
        let p = Plan::from_service(&evil, None, &all);
        assert_eq!(p.shots[0].media, Media::Card);
        assert!((p.shots[0].len - MAX_SHOT_SEC).abs() < 1e-9);
        assert_eq!(p.shots[1].media, Media::Card);
        assert!((p.shots[1].len - DEFAULT_SHOT_SEC).abs() < 1e-9);
        assert_eq!(parse_hex("#zzzzzz"), None);
        assert_eq!(parse_hex("#ffé"), None);
        assert_eq!(card_colour("#380ea7"), "#1b0e3c");
    }
}
