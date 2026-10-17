//! Inline cards in the chat (doc 37 "Cards"): Script, Looks, Versions, Voice lines, Music themes,
//! Render. A card's buttons are small and secondary; a card never holds the view's one primary.
//! The card's data is the service's (`chat.message` `card: {kind, …}`, protocol.md): every field
//! is optional and every value is checked.

use std::collections::BTreeMap;

use egui::{Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use super::model::{self, Act, ActionButton};
use crate::MfxCtx;

/// What a press on a card asks the panel to do.
#[derive(Default)]
pub struct CardOut {
    /// Fold or unfold (Read it all, All 18 lines).
    pub toggle: bool,
    /// (question id, choice id): a pick on a Looks or Music card.
    pub answer: Option<(String, String)>,
    pub action: Option<ActionButton>,
    /// Change the words (Script): the Script panel with its first line open.
    pub change_words: bool,
    /// Hear the music themes: ask the service for its samples (`music.theme.samples`).
    pub samples: bool,
    /// Answer… on a question: (question id, its words); the composer answers it next.
    pub custom: Option<(String, String)>,
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(300).collect()).unwrap_or_default()
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v.get(key).and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// "4.2 s" from seconds.
fn secs(v: Option<&Value>) -> String {
    match v.and_then(Value::as_f64).filter(|s| s.is_finite() && *s >= 0.0 && *s < 1e7) {
        Some(s) if s < 60.0 => format!("{s:.1} s"),
        Some(s) => format!("{}:{:02}", (s / 60.0) as u64, (s % 60.0) as u64),
        None => String::new(),
    }
}

fn button(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str, label: &str) -> egui::Response {
    super::small_button(ui, ctx, id, label, false)
}

/// Draw one card under message `msg`.
pub fn show(
    ui: &mut egui::Ui,
    ctx: &mut MfxCtx,
    msg: usize,
    card: &Value,
    open: bool,
    answered: &BTreeMap<String, String>,
    samples: Option<&[Value]>,
    out: &mut CardOut,
) {
    let t = ctx.theme;
    let kind = text(card, "kind");
    // a question card repeats the message's own question (drawn as chips under it)
    if matches!(kind.as_str(), "question" | "confirm") {
        return;
    }
    let width = ui.available_width().min(460.0);
    egui::Frame::new().fill(t.field_bg).corner_radius(6.0).inner_margin(10.0).show(ui, |ui| {
        ui.set_width(width - 20.0);
        ui.spacing_mut().item_spacing.y = 4.0;
        let title = text(card, "title");
        if !title.is_empty() {
            ui.label(RichText::new(&title).size(12.0).color(t.text_dim).strong());
        }
        match kind.as_str() {
            "script" => script(ui, ctx, msg, card, open, out),
            "looks" => looks(ui, ctx, msg, card, answered, out),
            "versions" => versions(ui, ctx, msg, card, out),
            "shots" | "transitions" => node_list(ui, ctx, msg, card, open, out),
            "voice" => voice(ui, ctx, msg, card, open, out),
            "music" => music(ui, ctx, msg, card, answered, out),
            "themes" => themes(ui, ctx, msg, card, samples, out),
            "cues" => crate::voice_music::show_cue_plan(ui, ctx),
            "defaults" | "submission" => defaults(ui, ctx, msg, card, answered, out),
            "render" => render(ui, ctx, msg, card, out),
            _ => {}
        }
        let actions = model::actions_of(card.get("actions"));
        if !actions.is_empty() {
            ui.horizontal_wrapped(|ui| {
                for (k, b) in actions.iter().enumerate() {
                    if button(ui, ctx, &format!("mfx.chat.card.{msg}.action.{k}"), &b.label).clicked() {
                        out.action = Some(b.clone());
                    }
                }
            });
        }
    });
}

/// The beat square + number from a card row's own `no` / `colour` (else the graph's).
fn beat_of(ui: &mut egui::Ui, ctx: &MfxCtx, id: &str, row: &Value) {
    let mut info = model::node(&ctx.state.graph, id);
    if let Some(n) = row.get("no").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()) {
        info.beat_no = Some(n);
    }
    if let Some(c) = row.get("colour").and_then(model::hex_colour) {
        info.colour = Some(c);
    }
    super::beat_mark(ui, ctx, &info);
}

/// The beat square + number for a beat id (graph colour, else the fallback hue).
fn beat(ui: &mut egui::Ui, ctx: &MfxCtx, id: &str) {
    if id.is_empty() {
        return;
    }
    let info = model::node(&ctx.state.graph, id);
    super::beat_mark(ui, ctx, &info);
}

fn script(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, open: bool, out: &mut CardOut) {
    let t = ctx.theme;
    let beats = list(card, "beats");
    // The live script, every line editable in place (the Script panel's own editor and draft); the
    // card's own copy only until the script has been read.
    let scope = format!("chat.{msg}");
    let live = crate::script::editor(ui, ctx, &scope, true, open);
    if live && (ctx.state.script.unsaved() || ctx.state.script.saving) {
        ui.horizontal(|ui| crate::script::save_bar(ui, ctx, &scope));
    }
    for b in beats.iter().take(if live { 0 } else { 40 }) {
        let id = text(b, "id");
        let start = text(b, "start");
        let right = if start.is_empty() { secs(b.get("target_sec")) } else { start };
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 20.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            beat_of(ui, ctx, &id, b);
            let title = text(b, "title");
            if !title.is_empty() {
                ui.label(RichText::new(title).size(12.0).color(t.text).strong());
            }
            // words left-aligned in what is left, the time on the right edge
            let rest = (ui.available_width() - 48.0).max(20.0);
            if !open {
                ui.allocate_ui_with_layout(vec2(rest, 20.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(text(b, "words")).size(12.0).color(t.text_dim)).truncate());
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(right).size(11.0).color(t.text_faint).monospace());
            });
        });
        if open {
            ui.add(egui::Label::new(RichText::new(text(b, "words")).size(12.0).color(t.text_dim)).wrap());
            ui.add_space(4.0);
        }
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if button(ui, ctx, &format!("mfx.chat.card.{msg}.readAll"), if open { "Fold it" } else { "Read it all" }).clicked() {
            out.toggle = true;
        }
        if button(ui, ctx, &format!("mfx.chat.card.{msg}.changeWords"), "Change the words").clicked() {
            out.change_words = true;
        }
        let words = card.get("words").and_then(Value::as_u64).or_else(|| {
            let n: usize = beats.iter().map(|b| text(b, "words").split_whitespace().count()).sum();
            u64::try_from(n).ok().filter(|n| *n > 0)
        });
        let total: f64 = beats.iter().filter_map(|b| b.get("target_sec").and_then(Value::as_f64)).filter(|x| x.is_finite() && *x >= 0.0).sum();
        let length = if card.get("seconds").is_some() {
            secs(card.get("seconds"))
        } else if total > 0.0 {
            secs(Some(&json!(total)))
        } else {
            String::new()
        };
        let facts = match (words, length.is_empty()) {
            (Some(w), false) => format!("{w} words · {length}"),
            (Some(w), true) => format!("{w} words"),
            (None, _) => length,
        };
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(facts).size(11.0).color(t.text_faint));
        });
    });
}

/// A fixed 16:9 frame (rule: framed for real content, nothing drawn over it).
fn frame(ui: &mut egui::Ui, w: f32, fill: Color32, outline: Option<Color32>, still: Option<&str>) -> egui::Response {
    let (r, resp) = ui.allocate_exact_size(vec2(w, w * 9.0 / 16.0), Sense::click());
    ui.painter().rect_filled(r, 4.0, fill);
    if let Some(path) = still.map(std::path::Path::new).filter(|p| p.is_absolute()) {
        // the piece's still (graph.get `still`, the node area's decoder and cache), cropped to fill
        let stills = crate::nodes::stills::Stills::of(ui.ctx());
        stills.poll(ui.ctx());
        match stills.get(path) {
            Some(tex) => {
                let [tw, th] = tex.size().map(|x| x.max(1) as f32);
                let (fw, fh) = (r.width().max(1.0), r.height().max(1.0));
                let (sx, sy) = if tw / th > fw / fh { ((fw / fh) / (tw / th), 1.0) } else { (1.0, (tw / th) / (fw / fh)) };
                let uv = egui::Rect::from_min_max(pos2((1.0 - sx) / 2.0, (1.0 - sy) / 2.0), pos2((1.0 + sx) / 2.0, (1.0 + sy) / 2.0));
                ui.painter().image(tex.id(), r, uv, Color32::WHITE);
            }
            None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(250)),
        }
    }
    if let Some(c) = outline {
        ui.painter().rect_stroke(r.expand(2.0), 5.0, Stroke::new(2.0, c), egui::StrokeKind::Outside);
    } else if resp.hovered() {
        ui.painter().rect_stroke(r.expand(2.0), 5.0, Stroke::new(1.0, Color32::from_gray(120)), egui::StrokeKind::Outside);
    }
    resp
}

/// The picture of a node's version (`v`, else the one that plays) from `graph.get`: its `still` (a
/// shot version's first frame, PNG), else the version itself when it is a picture (a look).
fn still_of(graph: &Value, node: &str, v: Option<u64>) -> Option<String> {
    let raw = model::raw_node(graph, node)?;
    let want = v.or_else(|| raw.get("plays").or_else(|| raw.get("current")).and_then(Value::as_u64));
    let vs = raw.get("versions")?.as_array()?;
    let ver = vs.iter().find(|x| x.get("v").and_then(Value::as_u64) == want).or_else(|| vs.last())?;
    let picture = |p: &&str| {
        let ext = std::path::Path::new(p).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default();
        matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "svg")
    };
    ver.get("still").and_then(Value::as_str).filter(|x| !x.is_empty()).or_else(|| ver.get("path").and_then(Value::as_str).filter(picture)).map(str::to_string)
}

fn looks(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, answered: &BTreeMap<String, String>, out: &mut CardOut) {
    let t = ctx.theme;
    let question = text(card, "question");
    let looks = if card.get("frames").is_some() { list(card, "frames") } else { list(card, "looks") };
    // the look in use now (the Look stage), else the one the card was sent with
    let using = ctx.state.look.view.using.clone().filter(|_| question.is_empty()).unwrap_or_else(|| text(card, "using"));
    let picked = answered
        .get(if question.is_empty() { "look" } else { &question })
        .cloned()
        .or_else(|| looks.iter().find(|l| l.get("picked").and_then(Value::as_bool) == Some(true)).map(|l| text(l, "id")))
        .or_else(|| (!using.is_empty()).then_some(using));
    let n = looks.len().clamp(1, 3);
    let gap = 10.0;
    let w = ((ui.available_width() - gap * (n as f32 - 1.0)) / n as f32).max(40.0);
    for row in looks.chunks(3).take(4) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for l in row {
                let id = text(l, "id");
                // the Look stage's own record of it (its name, its picture now), else the card's
                let live = ctx.state.look.view.frame(&id).cloned();
                let name = Some(text(l, "name"))
                    .filter(|n| !n.is_empty())
                    .or_else(|| live.as_ref().map(|f| f.name.clone()).filter(|n| !n.is_empty()))
                    .unwrap_or_else(|| text(l, "label"));
                let on = picked.as_deref() == Some(id.as_str());
                ui.vertical(|ui| {
                    ui.set_width(w);
                    let beat_colour = model::node(&ctx.state.graph, &text(l, "beat")).colour.map(|c| c.gamma_multiply(0.5));
                    let fill = l.get("colour").and_then(model::hex_colour).or(beat_colour).unwrap_or(t.hover);
                    // the look's own picture: the card's (`still`, else its `path`, SVG too), else the graph's
                    let still = Some(text(l, "still"))
                        .filter(|x| !x.is_empty())
                        .or_else(|| live.as_ref().map(|f| f.path.clone()).filter(|x| !x.is_empty()))
                        .or_else(|| Some(text(l, "path")).filter(|x| !x.is_empty()))
                        .or_else(|| still_of(&ctx.state.graph, &id, None));
                    let r = frame(ui, w, fill, on.then_some(t.text), still.as_deref());
                    let r = r.on_hover_text(format!("Use {name}"));
                    if r.clicked() && !on {
                        if question.is_empty() {
                            // the Look stage's own pick: asks first when shots would be remade (look.use)
                            let MfxCtx { state, service, .. } = ctx;
                            crate::look::pick(state, service, &id);
                        } else {
                            out.answer = Some((question.clone(), id.clone()));
                        }
                    }
                    ctx.auto(&format!("mfx.chat.card.{msg}.look.{id}"), &r, &name);
                    let label = if on { format!("✓ {name}") } else { name.clone() };
                    ui.add(egui::Label::new(RichText::new(label).size(12.0).color(if on { t.text } else { t.text_dim })).truncate());
                });
            }
        });
    }
}

fn versions(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, out: &mut CardOut) {
    let t = ctx.theme;
    let mut node = text(card, "node");
    if node.is_empty() {
        node = list(card, "refs").iter().find_map(|r| r.as_str().map(str::to_string)).unwrap_or_default();
    }
    // part 08's card names the piece; its versions are in the graph
    let from_graph: Vec<Value>;
    let vs: &[Value] = if card.get("versions").is_some() {
        list(card, "versions")
    } else {
        let raw = model::raw_node(&ctx.state.graph, &node);
        let plays = raw.and_then(|r| r.get("plays").or_else(|| r.get("current"))).and_then(Value::as_u64);
        from_graph = raw
            .and_then(|r| r.get("versions"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .take(12)
                    .filter_map(|v| v.get("v").and_then(Value::as_u64).or_else(|| v.as_u64()))
                    .map(|v| json!({"v": v, "inUse": Some(v) == plays, "still": still_of(&ctx.state.graph, &node, Some(v))}))
                    .collect()
            })
            .unwrap_or_default();
        &from_graph
    };
    if vs.is_empty() && !node.is_empty() {
        ui.label(RichText::new(format!("A new version of {} is in.", model::node(&ctx.state.graph, &node).name)).size(12.0).color(t.text_dim));
    }
    let w = 84.0;
    // rows of as many as fit: a wrapped row of vertical groups overflowed the panel (run 2: the
    // fifth version went past the edge and widened the thread, so long messages below were cut off)
    let per_row = (((ui.available_width() + 10.0) / (w + 10.0)).floor() as usize).max(1);
    let shown: Vec<&Value> = vs.iter().take(12).collect();
    for row in shown.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            for v in row.iter().copied() {
                let n = v.get("v").and_then(Value::as_u64).unwrap_or(0);
                let in_use = v.get("inUse").and_then(Value::as_bool).unwrap_or(false);
                let making = v.get("making").and_then(Value::as_bool).unwrap_or(false);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    let still = Some(text(v, "still")).filter(|x| !x.is_empty() && !making);
                    let r = frame(ui, w, if making { t.panel_bg } else { t.hover }, in_use.then_some(t.text), still.as_deref());
                    if making {
                        // striped slot: the new version is on its way
                        let p = ui.painter().with_clip_rect(r.rect);
                        let mut x = r.rect.min.x - r.rect.height();
                        while x < r.rect.max.x {
                            p.line_segment([pos2(x, r.rect.max.y), pos2(x + r.rect.height(), r.rect.min.y)], Stroke::new(3.0, t.hover));
                            x += 10.0;
                        }
                    }
                    let label = match (in_use, making) {
                        (_, true) => format!("v{n} · making {}", text(v, "eta")).trim().to_string(),
                        (true, _) => format!("v{n} ✓ in use"),
                        _ => {
                            let l = text(v, "label");
                            if l.is_empty() { format!("v{n}") } else { format!("v{n} · {l}") }
                        }
                    };
                    ui.add(egui::Label::new(RichText::new(label).size(11.0).color(if in_use { t.text } else { t.text_dim })).truncate());
                });
            }
        });
    }
    ui.horizontal_wrapped(|ui| {
        if !node.is_empty() && button(ui, ctx, &format!("mfx.chat.card.{msg}.play"), "▶ Play it").clicked() {
            let info = model::node(&ctx.state.graph, &node);
            super::play(ctx, Some(&info));
        }
        for v in vs.iter().take(12) {
            let n = v.get("v").and_then(Value::as_u64).unwrap_or(0);
            let in_use = v.get("inUse").and_then(Value::as_bool).unwrap_or(false);
            let making = v.get("making").and_then(Value::as_bool).unwrap_or(false);
            if in_use || making || node.is_empty() || n == 0 {
                continue;
            }
            let label = format!("Use v{n}");
            if button(ui, ctx, &format!("mfx.chat.card.{msg}.use.{n}"), &label).clicked() {
                out.action = Some(ActionButton { label, act: Act::Request { ty: "node.pick".into(), params: json!({"node": node, "v": n}) }, primary: false });
            }
        }
    });
}

fn play_dot(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str, auto: &str) {
    let r =
        ui.add(egui::Button::new(RichText::new("▶").size(11.0)).min_size(vec2(22.0, 20.0)).fill(ctx.theme.hover).corner_radius(10.0)).on_hover_text("Play it");
    if r.clicked() && !id.is_empty() {
        if let Some(path) = id.strip_prefix("file:") {
            // a music theme sample: its own audio file, in the system player
            ctx.command("mortiflix.reveal", json!({"path": path, "open": true}));
        } else {
            let info = model::node(&ctx.state.graph, id);
            super::play(ctx, Some(&info));
        }
    }
    ctx.auto(auto, &r, "Play it");
}

fn voice(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, open: bool, out: &mut CardOut) {
    let t = ctx.theme;
    let lines = list(card, "lines");
    let shown = if open { lines.len().min(200) } else { lines.len().min(3) };
    for (k, l) in lines.iter().take(shown).enumerate() {
        let id = text(l, "id");
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 22.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            play_dot(ui, ctx, &id, &format!("mfx.chat.card.{msg}.line.{k}"));
            beat(ui, ctx, &text(l, "beat"));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(secs(l.get("seconds"))).size(11.0).color(t.text_faint));
                let words = format!("\u{201c}{}\u{201d}", text(l, "words"));
                let r = ui.add(egui::Label::new(RichText::new(words).size(12.0).color(t.text)).truncate().sense(Sense::click()));
                if r.clicked() && !id.is_empty() {
                    ctx.state.selected_node = Some(id.clone());
                }
            });
        });
    }
    if lines.len() > 3 {
        let label = if open { "Fewer lines".to_string() } else { format!("All {} lines", lines.len()) };
        if button(ui, ctx, &format!("mfx.chat.card.{msg}.allLines"), &label).clicked() {
            out.toggle = true;
        }
    }
}

/// A small made-up waveform from the theme's name (the real one comes with the audio).
fn wave(ui: &mut egui::Ui, seed: &str, col: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(90.0, 18.0), Sense::hover());
    let mut h: u32 = 2166136261;
    for b in seed.bytes() {
        h = (h ^ u32::from(b)).wrapping_mul(16777619);
    }
    let bars = 22;
    for i in 0..bars {
        h = h.wrapping_mul(1103515245).wrapping_add(12345);
        let a = 0.25 + f32::from(((h >> 16) & 0xff) as u8) / 255.0 * 0.75;
        let x = r.min.x + i as f32 * (r.width() / bars as f32);
        let half = r.height() * a / 2.0;
        ui.painter().line_segment([pos2(x, r.center().y - half), pos2(x, r.center().y + half)], Stroke::new(2.0, col));
    }
}

fn music(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, answered: &BTreeMap<String, String>, out: &mut CardOut) {
    let t = ctx.theme;
    let question = text(card, "question");
    let themes = list(card, "themes");
    let picked =
        answered.get(&question).cloned().or_else(|| themes.iter().find(|l| l.get("picked").and_then(Value::as_bool) == Some(true)).map(|l| text(l, "id")));
    for th in themes.iter().take(6) {
        let id = text(th, "id");
        let name = text(th, "name");
        let on = picked.as_deref() == Some(id.as_str());
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 24.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            play_dot(ui, ctx, &format!("theme:{id}"), &format!("mfx.chat.card.{msg}.theme.{id}.play"));
            let label = if on { format!("{id} · {name} ✓") } else { format!("{id} · {name}") };
            let r = ui
                .add(egui::Label::new(RichText::new(&label).size(12.0).color(if on { t.text } else { t.text_dim })).sense(Sense::click()))
                .on_hover_text("Use this theme");
            if r.clicked() && !on {
                out.answer = Some((if question.is_empty() { "music".into() } else { question.clone() }, id.clone()));
            }
            ctx.auto(&format!("mfx.chat.card.{msg}.theme.{id}"), &r, &name);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(bpm) = th.get("bpm").and_then(Value::as_u64) {
                    ui.label(RichText::new(format!("{bpm} bpm")).size(11.0).color(t.text_faint));
                }
                wave(ui, &name, if on { t.text_dim } else { t.text_faint });
            });
        });
    }
}

/// Shots or transitions that came in: one row each, a link to the piece.
fn node_list(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, open: bool, out: &mut CardOut) {
    let t = ctx.theme;
    let nodes = list(card, "nodes");
    let shown = if open { nodes.len().min(200) } else { nodes.len().min(5) };
    for (k, n) in nodes.iter().take(shown).enumerate() {
        let id = text(n, "id");
        let info = model::node(&ctx.state.graph, &id);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 20.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            beat(ui, ctx, info.beat.as_deref().unwrap_or(""));
            let r = ui.add(egui::Label::new(RichText::new(&info.name).size(12.0).color(super::LINK).underline()).sense(Sense::click()));
            if r.clicked() {
                ctx.state.selected_node = Some(id.clone());
            }
            ctx.auto(&format!("mfx.chat.card.{msg}.node.{k}"), &r, &info.name);
            ui.add(egui::Label::new(RichText::new(text(n, "title")).size(12.0).color(t.text_dim)).truncate());
        });
    }
    if nodes.len() > 5 {
        let label = if open { "Fewer".to_string() } else { format!("All {}", nodes.len()) };
        if button(ui, ctx, &format!("mfx.chat.card.{msg}.all"), &label).clicked() {
            out.toggle = true;
        }
    }
}

/// The music theme was set: hear the three sample themes and pick one (doc 33).
fn themes(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, samples: Option<&[Value]>, out: &mut CardOut) {
    let t = ctx.theme;
    if card.get("themes").is_some() {
        return music(ui, ctx, msg, card, &BTreeMap::new(), out);
    }
    // the theme in words and the one in use (part 05, doc 33b), then the samples to hear here
    crate::voice_music::show_theme_card(ui, ctx);
    match samples {
        None => {
            if button(ui, ctx, &format!("mfx.chat.card.{msg}.hearThemes"), "Hear the music themes").clicked() {
                out.samples = true;
            }
        }
        Some([]) => {
            ui.label(RichText::new("Getting the themes…").size(12.0).color(t.text_faint));
        }
        Some(list) => {
            for (k, sm) in list.iter().enumerate() {
                let id = text(sm, "id");
                ui.allocate_ui_with_layout(vec2(ui.available_width(), 24.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    play_dot(ui, ctx, &format!("file:{}", text(sm, "wav")), &format!("mfx.chat.card.{msg}.sample.{k}.play"));
                    ui.label(RichText::new(text(sm, "name")).size(12.0).color(t.text));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = format!("Use {}", text(sm, "name"));
                        if button(ui, ctx, &format!("mfx.chat.card.{msg}.sample.{k}.use"), "Use it").clicked() {
                            out.action =
                                Some(ActionButton { label, act: Act::Request { ty: "music.theme.use".into(), params: json!({"sample": id}) }, primary: false });
                        }
                        if let Some(bpm) = sm.get("bpm").and_then(Value::as_u64) {
                            ui.label(RichText::new(format!("{bpm} bpm")).size(11.0).color(t.text_faint));
                        }
                    });
                });
            }
        }
    }
    // the per-beat music cues once the beats exist (doc 33b)
    ui.add_space(6.0);
    crate::voice_music::show_cue_plan(ui, ctx);
}

/// Questions that went on with their defaults (or a submission's questions): each one's chips, the
/// default ticked; a press changes the answer (`chat.answer`).
fn defaults(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, answered: &BTreeMap<String, String>, out: &mut CardOut) {
    let t = ctx.theme;
    for item in list(card, "items").iter().take(12) {
        ui.label(RichText::new(format!("· {}", text(item, "label"))).size(12.0).color(t.text_dim));
    }
    for q in list(card, "questions").iter().take(12) {
        let Some(q) = model::question_of(q) else { continue };
        if !q.text.is_empty() {
            ui.label(RichText::new(&q.text).size(12.0).color(t.text));
        }
        let using = answered.get(&q.id).cloned().or(q.picked.clone()).or(q.default.clone());
        ui.horizontal_wrapped(|ui| {
            for c in &q.choices {
                let on = using.as_deref() == Some(c.id.as_str());
                let label = if on { format!("✓ {}", c.label) } else { c.label.clone() };
                let r = ui.add(
                    egui::Button::new(RichText::new(&label).size(12.0).color(if on { t.text } else { t.text_dim }))
                        .min_size(vec2(0.0, 22.0))
                        .fill(if on { t.hover } else { t.panel_bg })
                        .stroke(Stroke::new(1.0, if on { t.text_dim } else { t.separator.gamma_multiply(2.0) }))
                        .corner_radius(11.0),
                );
                if r.clicked() && !on {
                    out.answer = Some((q.id.clone(), c.id.clone()));
                }
                ctx.auto(&format!("mfx.chat.card.{msg}.q.{}.{}", q.id, c.id), &r, &c.label);
            }
            // A question with no choices (or room for own words) still needs a way in.
            if q.custom || q.choices.is_empty() {
                let label = if q.choices.is_empty() { "Answer…" } else { "Your own…" };
                let r = ui.add(
                    egui::Button::new(RichText::new(label).size(12.0).color(t.text))
                        .min_size(vec2(0.0, 22.0))
                        .fill(t.panel_bg)
                        .stroke(Stroke::new(1.0, t.separator.gamma_multiply(2.0)))
                        .corner_radius(11.0),
                );
                if r.clicked() {
                    out.custom = Some((q.id.clone(), q.text.clone()));
                }
                ctx.auto(&format!("mfx.chat.card.{msg}.q.{}.custom", q.id), &r, label);
            }
        });
        // Say what is in use, so an open question never looks blank.
        let line = match &using {
            Some(u) if !q.choices.is_empty() && q.choices.iter().all(|c| &c.id != u) => format!("Your words: {u}"),
            Some(u) if q.choices.is_empty() => format!("Going with: {u}"),
            Some(_) => String::new(),
            None if q.choices.is_empty() => "No answer yet: the AI picks one unless you say.".into(),
            None => String::new(),
        };
        if !line.is_empty() {
            ui.label(RichText::new(line).size(12.0).color(t.text_dim));
        }
    }
}

fn render(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, card: &Value, out: &mut CardOut) {
    let t = ctx.theme;
    let finished = card.get("finished").and_then(Value::as_bool).unwrap_or(false);
    let path = text(card, "path");
    if finished {
        ui.label(RichText::new(if path.is_empty() { "The video is ready.".to_string() } else { model::file_name(&path) }).size(13.0).color(t.text));
        ui.horizontal(|ui| {
            if !path.is_empty() {
                if button(ui, ctx, &format!("mfx.chat.card.{msg}.open"), "▶ Play").clicked() {
                    ctx.command("mortiflix.reveal", json!({"path": path, "open": true}));
                }
                if button(ui, ctx, &format!("mfx.chat.card.{msg}.reveal"), "Show in folder").clicked() {
                    ctx.command("mortiflix.reveal", json!({"path": path}));
                }
            }
        });
        return;
    }
    let done = card.get("done").and_then(Value::as_f64).filter(|d| d.is_finite()).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    ui.painter().rect_filled(r, 3.0, t.hover);
    ui.painter().rect_filled(Rect::from_min_size(r.min, vec2(r.width() * done, r.height())), 3.0, t.ok);
    let line = text(card, "line");
    ui.label(RichText::new(if line.is_empty() { format!("{:.0}%", done * 100.0) } else { line }).size(12.0).color(t.text_dim).font(FontId::proportional(12.0)));
    if button(ui, ctx, &format!("mfx.chat.card.{msg}.stop"), "Stop the render").clicked() {
        out.action = Some(ActionButton {
            label: "Stop the render".into(),
            act: Act::Request { ty: "render.cancel".into(), params: json!({"id": card.get("job")}) },
            primary: false,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_card_draws_its_frames() {
        let mut m = crate::Mortiflix::default();
        let card = json!({"kind": "looks", "frames": [{"id": "sf01", "label": "SF01 · Open", "beat": "b01"}, {"id": "sf02", "label": "SF02"}, {"id": "sf03", "label": "SF03"}, {"id": "sf04", "label": "SF04"}], "using": "sf01"});
        let ectx = egui::Context::default();
        let mut h = 0.0;
        let mut out = ectx.run_ui(egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(400.0, 800.0))), ..Default::default() }, |ui| {
            let mut ctx = m.ctx(crate::MfxTheme::default());
            let mut o = CardOut::default();
            let r = ui.scope(|ui| show(ui, &mut ctx, 0, &card, false, &BTreeMap::new(), None, &mut o)).response.rect;
            h = r.height();
        });
        out.textures_delta.clear();
        assert!(h > 100.0, "two rows of 16:9 frames, got {h}");
    }

    #[test]
    fn seconds_read_safely() {
        assert_eq!(secs(Some(&json!(4.21))), "4.2 s");
        assert_eq!(secs(Some(&json!(90))), "1:30");
        assert_eq!(secs(Some(&json!(-1))), "");
        assert_eq!(secs(Some(&json!("x"))), "");
        assert_eq!(secs(None), "");
    }
}
