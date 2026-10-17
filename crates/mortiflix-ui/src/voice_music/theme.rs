//! Doc 33b: the music theme card and the per-beat music cues while the AI makes the look and the
//! shots. The chat (part 37, `chat.rs`) draws them inline as cards: [`show_theme_card`] after the
//! looks, [`show_cue_plan`] once the beats exist. Voice & music shows the same plan before the
//! score exists. Data: `music.theme.get` and `voicemusic.view` (its `cues`, coloured by their
//! beat's shots); changes: `music.theme.use`, `music.cue.set` (part 08's protocol).
//!
//! One cue per beat, in the beat's colour with its number written in it (never colour alone).
//! A change is sent at once as an edit request on that cue; only that cue changes.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use super::model::{beat_no, colour_of};
use crate::MfxCtx;

/// Re-read the theme this often while the card is on screen (seconds).
const REFRESH_SEC: f64 = 4.0;
/// Clip text on a beat colour fill (doc 33: `#d9d9d9`).
const CLIP_TEXT: Color32 = Color32::from_rgb(0xd9, 0xd9, 0xd9);

#[derive(Clone, Default)]
struct ThemeUi {
    /// `music.theme.get`'s theme (doc 33's shape), or null.
    theme: Value,
    /// `voicemusic.view` (its cues and shots).
    view: Value,
    reqs: [Option<u64>; 2],
    ops: Vec<(u64, String)>,
    seq: u64,
    fetched_at: Option<f64>,
    error: String,
    /// The cue whose popover is open, and its draft.
    open: Option<String>,
    feel: String,
    energy: u8,
    msg: Option<(String, bool)>,
}

fn key() -> egui::Id {
    egui::Id::new("mfx-vm-theme")
}

/// Fetch and keep the theme fresh; returns the state for this frame (written back by [`store`]).
fn load(ui: &egui::Ui, ctx: &mut MfxCtx) -> ThemeUi {
    let mut st: ThemeUi = ui.ctx().data_mut(|d| d.get_temp::<ThemeUi>(key())).unwrap_or_default();
    let now = ui.input(|i| i.time);
    for (slot, req) in st.reqs.iter_mut().enumerate() {
        let Some(r) = req.and_then(|id| ctx.service.take_reply(id)) else { continue };
        *req = None;
        st.fetched_at = Some(now);
        if !r.ok {
            st.error = r.error.get("message").and_then(Value::as_str).unwrap_or("The music theme isn't available.").chars().take(300).collect();
        } else if slot == 0 {
            st.theme = r.result.get("theme").cloned().unwrap_or(Value::Null);
            st.error.clear();
        } else {
            st.view = r.result;
        }
    }
    let mut changed = false;
    st.ops.retain(|(id, what)| match ctx.service.take_reply(*id) {
        Some(r) => {
            changed = true;
            st.msg = Some(if r.ok {
                (what.clone(), false)
            } else {
                (r.error.get("message").and_then(Value::as_str).unwrap_or("That didn't work.").chars().take(300).collect(), true)
            });
            false
        }
        None => true,
    });
    for (seq, ev) in ctx.service.events_since(st.seq) {
        st.seq = st.seq.max(*seq);
        if matches!(ev.get("event").and_then(Value::as_str), Some("node.changed" | "status")) {
            changed = true;
        }
    }
    let stale = st.fetched_at.is_none_or(|t| now - t > REFRESH_SEC);
    if st.reqs.iter().all(Option::is_none) && (changed || stale) && ctx.service.status().is_connected() {
        st.reqs = ["music.theme.get", "voicemusic.view"].map(|ty| Some(ctx.service.request(ty, json!({}))));
    }
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(REFRESH_SEC));
    st
}

fn store(ui: &egui::Ui, st: ThemeUi) {
    ui.ctx().data_mut(|d| d.insert_temp(key(), st));
}

fn send(st: &mut ThemeUi, ctx: &mut MfxCtx, ty: &str, params: Value, done: &str) {
    let id = ctx.service.request(ty, params);
    st.ops.push((id, done.to_string()));
}

fn message(ui: &mut egui::Ui, ctx: &MfxCtx, st: &ThemeUi) {
    if let Some((m, bad)) = &st.msg {
        ui.colored_label(if *bad { ctx.theme.error } else { ctx.theme.text_dim }, m);
    }
}

/// The theme card (doc 33b, chat): the theme in words and its samples, "Using this" on the one in
/// use. Shown by the chat after the three looks; the AI uses the first and carries on.
pub fn show_theme_card(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let mut st = load(ui, ctx);
    let t = ctx.theme;
    ui.label(egui::RichText::new("Mortiflix · music theme").color(t.text_dim).size(11.0));
    let theme = Some(st.theme.clone()).filter(Value::is_object).map(|raw| {
        let mut tl = super::model::Timeline::default();
        super::model::apply_theme(&mut tl, &json!({"theme": raw}));
        let (name, summary) = tl.theme.unwrap_or_default();
        json!({"name": name, "summary": summary})
    });
    match theme {
        None if !st.error.is_empty() => {
            ui.colored_label(t.text_faint, &st.error);
        }
        None => {
            ui.colored_label(t.text_faint, "The music theme comes after the look.");
        }
        Some(th) => {
            let name = th.get("name").and_then(Value::as_str).unwrap_or("Music theme");
            let summary = th.get("summary").and_then(Value::as_str).unwrap_or("");
            ui.label(egui::RichText::new(name).color(t.text).strong());
            if !summary.is_empty() {
                ui.colored_label(t.text_dim, summary);
            }
            let using = st.theme.get("using").and_then(Value::as_str).unwrap_or("").to_string();
            let samples: Vec<Value> = st.theme.get("samples").and_then(Value::as_array).cloned().unwrap_or_default();
            for s in samples.iter().take(8) {
                let id = s.get("id").and_then(Value::as_str).unwrap_or("");
                if id.is_empty() {
                    continue;
                }
                let name = s.get("name").and_then(Value::as_str).unwrap_or(id);
                let desc = s.get("desc").and_then(Value::as_str).unwrap_or("");
                ui.horizontal(|ui| {
                    let here = id == using;
                    let r = ui.radio(here, name);
                    ctx.auto(&format!("mfx.vm.theme.use.{id}"), &r, name);
                    if r.clicked() && !here {
                        send(&mut st, ctx, "music.theme.use", json!({"sample": id}), &format!("Using {name}."));
                    }
                    if here {
                        ui.colored_label(t.text_dim, "Using this");
                    }
                    if !desc.is_empty() {
                        ui.colored_label(t.text_faint, desc);
                    }
                });
            }
        }
    }
    message(ui, ctx, &st);
    store(ui, st);
}

/// The per-beat music cues (doc 33b): one block per beat in the beat's colour, with its number,
/// feel line and energy. Click a block to change its feel or energy; Send changes only that cue.
pub fn show_cue_plan(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let mut st = load(ui, ctx);
    let t = ctx.theme;
    let cues: Vec<Value> = st.view.get("cues").and_then(Value::as_array).cloned().unwrap_or_default();
    let shots: Vec<Value> = st.view.get("shots").and_then(Value::as_array).cloned().unwrap_or_default();
    let beat_colour = |beat: &str| shots.iter().find(|s| s.get("beat").and_then(Value::as_str) == Some(beat)).and_then(colour_of);
    if cues.is_empty() {
        ui.colored_label(t.text_faint, if st.error.is_empty() { "The music cues are planned once the script has its beats." } else { st.error.as_str() });
        store(ui, st);
        return;
    }
    ui.colored_label(t.text_dim, "Music cues, one per beat. Planned: the notes are written after the shots, to their exact cuts.");
    let w = ui.available_width().max(120.0);
    let n = cues.len().clamp(1, 64) as f32;
    let gap = 4.0;
    let bw = ((w - gap * (n - 1.0)) / n).max(60.0);
    let h = 64.0;
    let (row, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
    for (i, c) in cues.iter().take(64).enumerate() {
        let id = c.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        let beat = c.get("beat").and_then(Value::as_str).unwrap_or("");
        let fill = colour_of(c).or_else(|| beat_colour(beat)).unwrap_or(t.hover);
        let r = Rect::from_min_size(pos2(row.min.x + i as f32 * (bw + gap), row.min.y), vec2(bw, h));
        let resp = ui.interact(r, egui::Id::new(("mfx-vm-plan", &id)), Sense::click());
        let p = ui.painter_at(r);
        p.rect_filled(r, 3.0, t.field_bg);
        let band = Rect::from_min_size(r.min, vec2(r.width(), 18.0));
        p.rect_filled(band, 3.0, fill);
        let no = beat_no(beat).map_or(String::new(), |b| format!("{b} "));
        let title = if c.get("scored").and_then(Value::as_bool) == Some(true) { c.get("title").and_then(Value::as_str).unwrap_or("") } else { "" };
        p.text(pos2(band.min.x + 6.0, band.center().y), Align2::LEFT_CENTER, format!("{no}{} {}", title, id), FontId::proportional(11.0), CLIP_TEXT);
        let feel = c.get("feel").and_then(Value::as_str).unwrap_or("");
        let g = p.layout(
            if feel.is_empty() { "Feel: the AI picks".to_string() } else { feel.to_string() },
            FontId::proportional(11.0),
            t.text_dim,
            r.width() - 10.0,
        );
        p.galley(pos2(r.min.x + 5.0, band.max.y + 3.0), g, t.text_dim);
        let energy = c.get("energy").and_then(Value::as_u64).unwrap_or(3).clamp(1, 5);
        for k in 0..5u64 {
            let bar = Rect::from_min_size(pos2(r.min.x + 6.0 + k as f32 * 5.0, r.max.y - 6.0 - (k + 1) as f32 * 2.0), vec2(3.0, (k + 1) as f32 * 2.0));
            p.rect_filled(bar, 0.0, if k < energy { t.text } else { t.separator });
        }
        p.text(pos2(r.min.x + 34.0, r.max.y - 8.0), Align2::LEFT_CENTER, format!("energy {energy}"), FontId::proportional(10.0), t.text_faint);
        if st.open.as_deref() == Some(id.as_str()) {
            p.rect_stroke(r, 3.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
        }
        ctx.auto(&format!("mfx.vm.plan.{id}"), &resp, &format!("Music cue {id}"));
        if resp.clicked() {
            st.open = Some(id.clone());
            st.feel = feel.to_string();
            st.energy = u8::try_from(energy).unwrap_or(3);
        }
    }
    if let Some(id) = st.open.clone() {
        ui.add_space(6.0);
        ui.label(egui::RichText::new(format!("Music cue {id} · only this cue changes")).color(t.text));
        ui.horizontal(|ui| {
            ui.colored_label(t.text_dim, "Feel");
            let r = ui.add(egui::TextEdit::singleline(&mut st.feel).desired_width(ui.available_width().min(420.0)));
            ctx.auto("mfx.vm.plan.feel", &r, "Feel");
        });
        ui.horizontal(|ui| {
            ui.colored_label(t.text_dim, "Energy");
            for e in 1..=5u8 {
                let r = ui.selectable_label(st.energy == e, e.to_string());
                ctx.auto(&format!("mfx.vm.plan.energy{e}"), &r, &format!("Energy {e}"));
                if r.clicked() {
                    st.energy = e;
                }
            }
        });
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel");
            ctx.auto("mfx.vm.plan.cancel", &cancel, "Cancel");
            if cancel.clicked() {
                st.open = None;
            }
            let go = ui.add(egui::Button::new(egui::RichText::new("Send").color(Color32::WHITE)).fill(t.accent));
            ctx.auto("mfx.vm.plan.send", &go, "Send");
            if go.clicked() {
                let params = json!({"cue": id, "feel": st.feel.trim(), "energy": st.energy});
                send(&mut st, ctx, "music.cue.set", params, &format!("Sent: music cue {id} is planned again. Only this cue changes."));
                st.open = None;
            }
        });
    }
    message(ui, ctx, &st);
    store(ui, st);
}
