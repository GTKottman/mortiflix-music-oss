//! The Music stage's score (owner, 2026-10-18: "Music would obviously have strudel, and the MIDI stuff
//! would be pasted to the animatic"): the Strudel the score is written in (shown in the Music gate)
//! and its MIDI notes, drawn under the animatic on the preview's MIDI row. Read from the service's
//! `music.score` (notes already in seconds on the animatic's clock); read-only here.

use egui::{Color32, Painter, Rect, RichText, pos2};
use serde_json::{Value, json};

use crate::{MfxCtx, STAGE_MUSIC, Service, state::MfxState};

/// Re-read the score this often while the Music stage is on screen (seconds).
const REFRESH_SEC: f64 = 10.0;
/// Most notes kept (a long score's notes beyond this are not drawn).
const MAX_NOTES: usize = 20_000;
/// Longest Strudel text shown.
const MAX_CODE: usize = 200_000;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Note {
    /// Index into [`Score::channels`].
    pub ch: usize,
    pub key: u8,
    pub start: f64,
    pub dur: f64,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub colour: Color32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Score {
    pub code: String,
    pub file: String,
    pub channels: Vec<Channel>,
    pub notes: Vec<Note>,
    pub loaded: bool,
    req: Option<u64>,
    fetched_at: Option<std::time::Instant>,
    project: Option<String>,
    seq: u64,
}

fn colour(hex: &str) -> Option<Color32> {
    let h = hex.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(h, 16).ok()?;
    let [_, r, g, b] = v.to_be_bytes();
    Some(Color32::from_rgb(r, g, b))
}

impl Score {
    /// From the service's `music.score` answer. Every field is hostile input.
    pub fn parse(v: &Value) -> Self {
        let mut s = Self { loaded: true, ..Self::default() };
        if let Some(st) = v.get("strudel").filter(|x| x.is_object()) {
            s.code = st.get("text").and_then(Value::as_str).unwrap_or("").chars().take(MAX_CODE).collect();
            s.file = st.get("file").and_then(Value::as_str).unwrap_or("").chars().take(4096).collect();
        }
        let palette = [Color32::from_rgb(90, 160, 230), Color32::from_rgb(230, 150, 70), Color32::from_rgb(120, 200, 120), Color32::from_rgb(200, 110, 200)];
        for (i, c) in v.get("channels").and_then(Value::as_array).into_iter().flatten().take(64).enumerate() {
            let id = c.get("id").and_then(Value::as_str).unwrap_or("").chars().take(40).collect::<String>();
            let name = c.get("name").and_then(Value::as_str).unwrap_or("").chars().take(60).collect::<String>();
            let fallback = palette.get(i % palette.len()).copied().unwrap_or(Color32::GRAY);
            let colour = c.get("colour").and_then(Value::as_str).and_then(colour).unwrap_or(fallback);
            s.channels.push(Channel { id, name, colour });
        }
        for n in v.get("notes").and_then(Value::as_array).into_iter().flatten().take(MAX_NOTES) {
            let Some(ch) = n.get("ch").and_then(Value::as_str).and_then(|c| s.channels.iter().position(|x| x.id == c)) else { continue };
            let Some(key) = n.get("key").and_then(Value::as_u64).and_then(|k| u8::try_from(k).ok()).filter(|k| *k < 128) else { continue };
            let start = n.get("start_sec").and_then(Value::as_f64).filter(|x| x.is_finite() && *x >= 0.0);
            let dur = n.get("dur_sec").and_then(Value::as_f64).filter(|x| x.is_finite() && *x >= 0.0);
            if let (Some(start), Some(dur)) = (start, dur) {
                s.notes.push(Note { ch, key, start, dur });
            }
        }
        s
    }
}

/// Every frame: on the Music stage keep the score fresh (on its own clock, a node change, or a new project).
pub fn tick(state: &mut MfxState, service: &mut Service) {
    let now = std::time::Instant::now();
    let sc = &mut state.score;
    if let Some(id) = sc.req
        && let Some(r) = service.take_reply(id)
    {
        sc.req = None;
        let keep = (sc.project.clone(), sc.seq);
        *sc = if r.ok { Score::parse(&r.result) } else { Score { loaded: true, ..Score::default() } };
        (sc.project, sc.seq) = keep;
        sc.fetched_at = Some(now);
    }
    if state.stage != STAGE_MUSIC || !service.status().is_connected() || sc.req.is_some() {
        return;
    }
    let mut changed = sc.project != state.project_id;
    for (seq, ev) in service.events_since(sc.seq) {
        sc.seq = sc.seq.max(*seq);
        changed |= matches!(ev.get("event").and_then(Value::as_str), Some("node.changed"));
    }
    if changed || sc.fetched_at.is_none_or(|t| now.duration_since(t).as_secs_f64() > REFRESH_SEC) {
        sc.project.clone_from(&state.project_id);
        sc.req = Some(service.request("music.score", json!({})));
    }
}

/// The MIDI row under the animatic: every note at its time, its pitch across the row's height, in its
/// part's colour. `x_of` maps seconds to the row's x.
pub fn draw_midi(p: &Painter, row: Rect, score: &Score, x_of: impl Fn(f64) -> f32, faint: Color32) {
    p.rect_filled(row, 2.0, Color32::from_gray(18));
    if score.notes.is_empty() {
        let words = if score.loaded { "No MIDI yet: the score is written once the animatic passes its gate." } else { "Reading the score…" };
        p.text(row.left_center() + egui::vec2(6.0, 0.0), egui::Align2::LEFT_CENTER, words, egui::FontId::proportional(11.0), faint);
        return;
    }
    let (lo, hi) = score.notes.iter().fold((127u8, 0u8), |(lo, hi), n| (lo.min(n.key), hi.max(n.key)));
    let span = f32::from(hi.saturating_sub(lo).max(12));
    let step = (row.height() - 4.0) / (span + 1.0);
    for n in &score.notes {
        let x0 = x_of(n.start);
        let x1 = x_of(n.start + n.dur).max(x0 + 1.5);
        let y = row.max.y - 2.0 - f32::from(n.key.saturating_sub(lo)) * step;
        let col = score.channels.get(n.ch).map_or(faint, |c| c.colour);
        p.rect_filled(Rect::from_min_max(pos2(x0, y - step.max(1.5)), pos2(x1, y)), 0.5, col);
    }
}

/// The Strudel the score is written in (the Music gate), read-only.
pub fn strudel(ui: &mut egui::Ui, ctx: &mut MfxCtx, height: f32) {
    let t = ctx.theme;
    let sc = &ctx.state.score;
    ui.label(RichText::new("Strudel").size(13.0).color(t.text).strong());
    if sc.code.is_empty() {
        let words = if sc.loaded { "No score yet." } else { "Reading the score…" };
        ui.label(RichText::new(words).size(12.0).color(t.text_faint));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        for c in &sc.channels {
            ui.label(RichText::new(format!("▌{}", c.name)).size(11.5).color(c.colour));
        }
    });
    let code = sc.code.clone();
    egui::ScrollArea::both().id_salt("mfx-score-strudel").max_height(height).auto_shrink([false, true]).show(ui, |ui| {
        ui.add(egui::Label::new(RichText::new(code).monospace().size(11.5).color(t.text_dim)).extend());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_score_reads_its_parts_and_notes_and_survives_junk() {
        let s = Score::parse(&json!({"strudel": {"file": "/m/score.strudel.js", "text": "note(\"c3\")"},
            "channels": [{"id": "ch01", "name": "Floor", "colour": "#7a3f14"}, {"id": "ch02", "name": "Pad", "colour": "nope"}, 7],
            "notes": [{"ch": "ch01", "key": 48, "start_sec": 1.5, "dur_sec": 0.5}, {"ch": "ch09", "key": 48, "start_sec": 0, "dur_sec": 1},
                {"ch": "ch02", "key": 300, "start_sec": 0, "dur_sec": 1}, {"ch": "ch02", "key": 60, "start_sec": -1, "dur_sec": 1}, "x"]}));
        assert_eq!(s.code, "note(\"c3\")");
        assert_eq!(s.channels.len(), 3);
        assert_eq!(s.channels[0].colour, Color32::from_rgb(0x7a, 0x3f, 0x14));
        assert_eq!(s.notes, vec![Note { ch: 0, key: 48, start: 1.5, dur: 0.5 }]);
        assert!(Score::parse(&json!(null)).notes.is_empty());
    }
}
