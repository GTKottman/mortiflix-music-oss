//! What the AI is working on right now (`work.active`, protocol.md): the pieces it makes or remakes,
//! the step and the plain-words line. Every place that shows a piece being worked on draws the same
//! working sign from here: a turning spinner and a sweep moving across, always with words ("Making…",
//! "Remaking v2…"), never colour alone. It animates only what the service says is being worked on
//! and stops the moment the piece leaves the list.

use std::collections::BTreeMap;

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// How often a working sign repaints (about 30 fps).
pub const FRAME: std::time::Duration = std::time::Duration::from_millis(33);

/// One piece being worked on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkNode {
    /// `making` (its first version) or `remaking` (a new version: your Fix).
    pub what: String,
    /// The version being made.
    pub v: u32,
    pub kind: String,
}

/// The step the runner is on.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkStep {
    pub key: String,
    pub name: String,
    /// The stage it belongs to (0-5).
    pub stage: u8,
    /// The canvas kind it makes (`shot`, `transition`, `frame`, `beat`), if any.
    pub kind: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Work {
    pub project: Option<String>,
    /// A session works on the project now. Nothing animates without one.
    pub running: bool,
    pub step: Option<WorkStep>,
    pub nodes: BTreeMap<String, WorkNode>,
    /// The piece the AI is on this moment (the newest activity names it).
    pub current: Option<String>,
    /// What it does, in plain words with progress ("Animating shot 4 of 8").
    pub doing: String,
    /// The steps still to come before the first shot, and about how long (stage 3 before any shot).
    pub before_shots: Vec<String>,
    pub shots_eta_min: Option<u32>,
}

fn text(v: &Value, key: &str, max: usize) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(max).collect()).unwrap_or_default()
}

impl Work {
    /// Read a `work.active` event's data (hostile input: bounded, unknown fields ignored).
    pub fn from_json(v: &Value) -> Work {
        let mut w = Work {
            project: Some(text(v, "project", 200)).filter(|p| !p.is_empty()),
            running: v.get("running").and_then(Value::as_bool).unwrap_or(false),
            current: Some(text(v, "current", 60)).filter(|c| !c.is_empty()),
            doing: text(v, "doing", 300),
            shots_eta_min: v.get("shots_eta_min").and_then(Value::as_u64).and_then(|n| u32::try_from(n.min(10_000)).ok()),
            ..Work::default()
        };
        w.step = v.get("step").filter(|s| s.is_object()).map(|s| WorkStep {
            key: text(s, "key", 60),
            name: text(s, "name", 120),
            stage: s.get("stage").and_then(Value::as_u64).and_then(|n| u8::try_from(n).ok()).filter(|n| *n < 6).unwrap_or(0),
            kind: text(s, "kind", 30),
        });
        for n in v.get("nodes").and_then(Value::as_array).into_iter().flatten().take(2000) {
            let id = text(n, "id", 60);
            if id.is_empty() {
                continue;
            }
            let what = if n.get("what").and_then(Value::as_str) == Some("remaking") { "remaking" } else { "making" };
            let v = n.get("v").and_then(Value::as_u64).and_then(|x| u32::try_from(x).ok()).unwrap_or(1).max(1);
            w.nodes.insert(id, WorkNode { what: what.into(), v, kind: text(n, "kind", 30) });
        }
        w.before_shots = v
            .get("before_shots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .take(20)
            .map(|s| s.chars().take(80).collect())
            .collect();
        if !w.running {
            w.nodes.clear();
            w.current = None;
        }
        w
    }
    /// This piece is being made or remade now.
    pub fn on(&self, id: &str) -> Option<&WorkNode> {
        if !self.running {
            return None;
        }
        self.nodes.get(id)
    }
    /// "Making…" / "Remaking v2…" for a piece being worked on.
    pub fn label(&self, id: &str) -> Option<String> {
        self.on(id).map(|n| if n.what == "remaking" { format!("Remaking v{}…", n.v) } else { "Making…".to_string() })
    }
    /// The runner is making pieces of this canvas kind (`transition`: the board).
    pub fn making_kind(&self, kind: &str) -> bool {
        self.running && self.step.as_ref().is_some_and(|s| s.kind == kind)
    }
    /// The stage the AI works in now: its step's, else the stage of the pieces it remakes (your Fixes).
    pub fn stage(&self) -> Option<u8> {
        if !self.running {
            return None;
        }
        self.step.as_ref().map(|s| s.stage).or_else(|| {
            self.nodes.values().find_map(|n| match n.kind.as_str() {
                "beat" => Some(0),
                "frame" => Some(1),
                "shot" | "transition" => Some(2),
                "line" | "cue" | "music" => Some(3),
                _ => None,
            })
        })
    }
    /// Anything animates (the caller keeps repainting while true).
    pub fn animating(&self) -> bool {
        self.running && (!self.nodes.is_empty() || self.step.is_some())
    }
}

/// A turning spinner: an arc of three quarters going round (one turn a second).
pub fn spinner(p: &Painter, c: Pos2, r: f32, col: Color32, time: f64) {
    #[allow(clippy::cast_possible_truncation)]
    let start = (time.rem_euclid(1.0) * std::f64::consts::TAU) as f32;
    let n = 20;
    let pts: Vec<Pos2> = (0..=n)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let a = start + i as f32 / n as f32 * std::f32::consts::TAU * 0.75;
            pos2(c.x + r * a.cos(), c.y + r * a.sin())
        })
        .collect();
    p.circle_stroke(c, r, Stroke::new((r * 0.28).max(1.2), col.gamma_multiply(0.25)));
    p.add(Shape::line(pts, Stroke::new((r * 0.28).max(1.2), col)));
}

/// A light band sweeping across `r` (a progress sweep with no end: the AI is on it).
pub fn sweep(p: &Painter, r: Rect, col: Color32, time: f64) {
    if r.width() < 2.0 || r.height() < 1.0 {
        return;
    }
    let band = (r.width() * 0.35).max(12.0);
    #[allow(clippy::cast_possible_truncation)]
    let f = (time * 0.6).rem_euclid(1.0) as f32;
    let x = r.min.x - band + f * (r.width() + band);
    let clip = p.with_clip_rect(r.intersect(p.clip_rect()));
    let steps = 8;
    for i in 0..steps {
        #[allow(clippy::cast_precision_loss)]
        let t = i as f32 / steps as f32;
        let alpha = 1.0 - (t * 2.0 - 1.0).abs();
        let w = band / steps as f32;
        clip.rect_filled(Rect::from_min_size(pos2(x + t * band, r.min.y), vec2(w + 0.5, r.height())), 0.0, col.gamma_multiply(0.35 * alpha));
    }
    // a bar along the bottom edge, so even a tiny piece shows it
    let bar = Rect::from_min_max(pos2(r.min.x, r.max.y - 3.0), r.max);
    clip.rect_filled(bar, 0.0, col.gamma_multiply(0.25));
    clip.rect_filled(Rect::from_min_max(pos2((x + band * 0.3).max(r.min.x), bar.min.y), pos2((x + band * 0.7).min(r.max.x), bar.max.y)), 0.0, col);
}

/// The working sign on a piece: sweep over `r`, and a spinner with the words in a dark pill at its
/// centre ("Making…", "Remaking v2…"). `size` = the words' size.
pub fn badge(p: &Painter, r: Rect, words: &str, col: Color32, size: f32, time: f64) {
    sweep(p, r, col, time);
    if r.width() < 24.0 || r.height() < 12.0 {
        return;
    }
    let font = egui::FontId::proportional(size);
    let g = p.layout_no_wrap(words.to_string(), font, Color32::WHITE);
    let sr = size * 0.45;
    let w = (g.size().x + sr * 2.0 + 14.0).min(r.width() - 4.0);
    let h = (g.size().y + 6.0).min(r.height());
    let pill = Rect::from_center_size(r.center(), vec2(w, h));
    p.rect_filled(pill, h / 2.0, Color32::from_black_alpha(190));
    spinner(p, pos2(pill.min.x + 5.0 + sr, pill.center().y), sr, col, time);
    let tp = p.with_clip_rect(pill.intersect(p.clip_rect()));
    tp.galley(pos2(pill.min.x + 9.0 + sr * 2.0, pill.center().y - g.size().y / 2.0), g, Color32::WHITE);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_what_the_service_says_and_nothing_runs_without_a_session() {
        let w = Work::from_json(
            &json!({"project": "p", "running": true, "step": {"key": "animatic", "stage": 2, "kind": "shot"}, "doing": "Animating shot 2 of 8",
            "nodes": [{"id": "s01", "what": "remaking", "v": 3}, {"id": "s02"}, {"what": "x"}, 7], "current": "s02", "before_shots": ["Transitions"], "shots_eta_min": 12}),
        );
        assert_eq!(w.label("s01").as_deref(), Some("Remaking v3…"));
        assert_eq!(w.label("s02").as_deref(), Some("Making…"));
        assert_eq!(w.label("s09"), None);
        assert!(w.making_kind("shot") && w.animating());
        assert_eq!(w.stage(), Some(2));
        assert_eq!((w.shots_eta_min, w.before_shots.len()), (Some(12), 1));
        let idle = Work::from_json(&json!({"project": "p", "running": false, "nodes": [{"id": "s01"}]}));
        assert!(idle.on("s01").is_none() && !idle.animating() && idle.stage().is_none());
        let fix = Work::from_json(&json!({"running": true, "nodes": [{"id": "b01-2", "kind": "line", "what": "remaking", "v": 2}]}));
        assert_eq!(fix.stage(), Some(3), "a Fix on a voice line works in Voice & music");
        assert_eq!(Work::from_json(&json!("junk")), Work::default());
    }

    #[test]
    fn signs_draw_at_any_size_without_panicking() {
        let ctx = egui::Context::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let p = ui.painter();
            for r in [Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 100.0)), Rect::from_min_size(pos2(0.0, 0.0), vec2(1.0, 0.0)), Rect::NOTHING] {
                badge(p, r, "Making…", Color32::LIGHT_BLUE, 12.0, 12.34);
                spinner(p, r.center(), 6.0, Color32::WHITE, 0.5);
            }
        });
        out.textures_delta.clear();
    }
}
