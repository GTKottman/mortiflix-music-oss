//! The Transitions stage's centre (owner, 2026-10-18: each stage its own UI; the transitions picked
//! first, like the look and the motion). Before any transition is made: **Pick the transitions**, a
//! transition pack (its transitions are the ones the board picks from) or the transitions library,
//! and the owner's words; Make the transitions saves it (`transition.brief.set`) and the service
//! holds the transitions until then. After: **the board**, one row per cut: the outgoing scene's
//! style frame, the transition (its panel or its drawn picture, name, length, OK, Fix, Change) and
//! the incoming scene's style frame, in the video's shape. Nothing here plays video: the shots come
//! after the animatic's gate.
//!
//! Data: `packs.list {kind: "transition"}`, `transition.brief.get` / `.set`; the cuts and their
//! transition nodes from the node area's graph; changes through `nodes.pickTransition`,
//! `node.edit` (OK / Fix) (protocol: `docs/mortiflix/protocol.md`).

use std::path::PathBuf;

use egui::{Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::nodes::model::{Kind, NodeState};
use crate::nodes::panel::TRANSITIONS;
use crate::nodes::stills::{Stills, resolve};
use crate::{MfxCtx, STAGE_TRANSITIONS, Service, state::MfxState};

/// A transition pack as `packs.list` sends it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub description: String,
    /// (name, seconds, words)
    pub transitions: Vec<(String, f32, String)>,
    pub words: String,
}

/// The stage's state (in [`MfxState::cuts`], so the host knows the centre is the menu).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CutsState {
    /// Transitions menu… was pressed.
    pub show: bool,
    pub packs: Vec<Pack>,
    /// The chosen pack ("" = the transitions library).
    pub pack: String,
    pub words: String,
    pub saved: bool,
    pub loaded: bool,
    pub note: String,
    /// The Fix box open on a cut: (transition node, words).
    pub fix: Option<(String, String)>,
    project: Option<String>,
    reqs: [Option<u64>; 2],
    save: Option<u64>,
}

fn text(v: &Value, k: &str, n: usize) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").chars().take(n).collect()
}

pub fn parse_packs(v: &Value) -> Vec<Pack> {
    v.as_array()
        .into_iter()
        .flatten()
        .take(500)
        .filter_map(|p| {
            let id = text(p, "id", 120);
            (!id.is_empty()).then(|| Pack {
                name: text(p, "name", 80),
                description: text(p, "description", 600),
                transitions: p
                    .get("transitions")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .take(200)
                    .filter_map(|t| {
                        let name = text(t, "name", 80);
                        #[allow(clippy::cast_possible_truncation)]
                        let sec = t.get("sec").and_then(Value::as_f64).filter(|s| s.is_finite() && (0.0..=10.0).contains(s)).unwrap_or(0.5) as f32;
                        (!name.is_empty()).then(|| (name, sec, text(t, "words", 600)))
                    })
                    .collect(),
                words: text(p, "words", 4000),
                id,
            })
        })
        .collect()
}

/// Every frame: on the Transitions stage, read the packs and the saved pick once per project.
pub fn tick(state: &mut MfxState, service: &mut Service) {
    let c = &mut state.cuts;
    if c.project != state.project_id {
        *c = CutsState { project: state.project_id.clone(), ..CutsState::default() };
    }
    for (slot, req) in c.reqs.iter_mut().enumerate() {
        let Some(r) = req.and_then(|id| service.take_reply(id)) else { continue };
        *req = None;
        if slot == 0 {
            if r.ok {
                c.packs = parse_packs(&r.result);
            }
            continue;
        }
        c.loaded = true;
        if r.ok && r.result.is_object() {
            c.saved = true;
            c.pack = r.result.pointer("/transition_pack/id").and_then(Value::as_str).unwrap_or("").chars().take(120).collect();
            c.words = text(&r.result, "words", 2000);
        }
    }
    if let Some(id) = c.save
        && let Some(r) = service.take_reply(id)
    {
        c.save = None;
        if r.ok {
            c.saved = true;
            c.show = false;
            c.note = "Saved: the transitions are made from this now.".into();
        } else {
            c.note = format!("Couldn't save the transitions: {}", r.error.get("message").and_then(Value::as_str).unwrap_or("the studio said no"));
        }
    }
    if state.stage == STAGE_TRANSITIONS && !c.loaded && c.reqs.iter().all(Option::is_none) && service.status().is_connected() && state.project_id.is_some() {
        c.reqs = [Some(service.request("packs.list", json!({"kind": "transition"}))), Some(service.request("transition.brief.get", json!({})))];
    }
}

/// The centre is the menu: asked for, or no transition made yet and nothing picked.
pub fn wanted(state: &MfxState) -> bool {
    let c = &state.cuts;
    if c.show {
        return true;
    }
    let made = state.nodes.graph.nodes.iter().any(|n| n.kind == Kind::Transition && !n.versions.is_empty());
    c.loaded && !c.saved && !made
}

/// The Transitions centre: the menu or the board.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    ui.set_max_width(ui.available_width());
    if crate::service_notice(ui, ctx) {
        return;
    }
    if wanted(ctx.state) { menu(ui, ctx) } else { board(ui, ctx) }
}

fn heading(ui: &mut egui::Ui, ctx: &MfxCtx, words: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(words).size(13.0).color(ctx.theme.text).strong());
    ui.add_space(2.0);
}

fn menu(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Pick the transitions").size(15.0).color(t.text).strong());
        if ctx.state.cuts.show {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let back = ui.button("Back to the board");
                ctx.auto("mfx.cutsMenu.back", &back, "Back to the board");
                if back.clicked() {
                    ctx.state.cuts.show = false;
                }
            });
        }
    });
    ui.add(
        egui::Label::new(
            RichText::new("Every cut between the scenes gets its transition from this. Nothing is made until you press Make the transitions.")
                .size(12.0)
                .color(t.text_dim),
        )
        .wrap(),
    );
    egui::ScrollArea::vertical().id_salt("mfx-cuts-menu").auto_shrink([false, false]).show(ui, |ui| {
        heading(ui, ctx, "Transition pack");
        let packs = ctx.state.cuts.packs.clone();
        if packs.is_empty() {
            ui.label(RichText::new("You have no transition packs yet: the transitions library is used.").size(12.0).color(t.text_faint));
        } else {
            let c = &mut ctx.state.cuts;
            let shown = packs.iter().find(|p| p.id == c.pack).map_or("None: the transitions library".to_string(), |p| p.name.clone());
            let r = egui::ComboBox::from_id_salt("mfx-cuts-pack").width(320.0).selected_text(shown).show_ui(ui, |ui| {
                ui.selectable_value(&mut c.pack, String::new(), "None: the transitions library");
                for p in &packs {
                    ui.selectable_value(&mut c.pack, p.id.clone(), &p.name);
                }
            });
            ctx.auto("mfx.cutsMenu.pack", &r.response, "Transition pack");
        }
        let chosen = packs.iter().find(|p| p.id == ctx.state.cuts.pack).cloned();
        let list: Vec<(String, f32, String)> =
            chosen.as_ref().map_or_else(|| TRANSITIONS.iter().map(|(k, s)| ((*k).to_string(), *s, String::new())).collect(), |p| p.transitions.clone());
        if let Some(p) = &chosen
            && !p.description.is_empty()
        {
            ui.add(egui::Label::new(RichText::new(&p.description).size(12.0).color(t.text_dim)).wrap());
        }
        ui.add_space(4.0);
        // the transitions on offer, each with its picture
        ui.horizontal_wrapped(|ui| {
            for (name, sec, words) in list.iter().take(40) {
                let (r, resp) = ui.allocate_exact_size(vec2(96.0, 74.0), Sense::hover());
                let pic = Rect::from_min_size(r.min, vec2(96.0, 54.0));
                crate::nodes::panel::draw_transition(ui.painter(), pic, name, &t);
                let g = ui.painter().layout(format!("{name} · {sec:.1} s"), FontId::proportional(11.0), t.text_dim, 96.0);
                ui.painter().galley(pos2(r.min.x, pic.max.y + 3.0), g, t.text_dim);
                if !words.is_empty() {
                    resp.on_hover_text(words);
                }
            }
        });
        heading(ui, ctx, "In your own words");
        let r = ui.add(
            egui::TextEdit::multiline(&mut ctx.state.cuts.words)
                .hint_text("How the scenes should hand over: hard cuts on the beat, match cuts on shapes…")
                .desired_rows(2)
                .desired_width(f32::INFINITY),
        );
        ctx.auto("mfx.cutsMenu.words", &r, "In your own words");
        ui.add_space(14.0);
        let c = &ctx.state.cuts;
        let label = if c.saved { "Save the transitions" } else { "Make the transitions" };
        let b = ui.add_enabled(
            c.save.is_none(),
            egui::Button::new(RichText::new(label).size(13.0).color(Color32::WHITE)).fill(t.accent).min_size(vec2(0.0, 30.0)).corner_radius(4.0),
        );
        ctx.auto("mfx.cutsMenu.make", &b, label);
        if b.clicked() {
            let c = &ctx.state.cuts;
            let params = json!({"transition_pack": if c.pack.is_empty() { Value::Null } else { json!(c.pack) }, "words": c.words});
            let id = ctx.service.request("transition.brief.set", params);
            ctx.state.cuts.save = Some(id);
            ctx.state.cuts.note = "Saving the transitions…".into();
        }
        let c = &ctx.state.cuts;
        if !c.note.is_empty() {
            ui.add_space(4.0);
            ui.label(RichText::new(c.note.clone()).size(12.0).color(if c.note.starts_with("Couldn't") { t.error } else { t.text_dim }));
        }
    });
}

/// One scene's side of a cut: its style frame (the look it is drawn in), or its words in an empty box.
struct Side {
    label: String,
    words: String,
    still: Option<PathBuf>,
    colour: Color32,
}

/// One row of the board.
struct Row {
    cut: String,
    transition: Option<String>,
    name: String,
    sec: Option<f32>,
    still: Option<PathBuf>,
    state: Option<NodeState>,
    made: bool,
    from: Side,
    to: Side,
}

fn rows(ctx: &MfxCtx) -> Vec<Row> {
    let area = &ctx.state.nodes;
    let g = &area.graph;
    let project = area.project.as_deref();
    let path = |f: &str| resolve(f, &g.base, &area.studio, project);
    let side = |id: &str| -> Side {
        let n = g.get(id);
        let look = n.and_then(|n| n.look.as_deref()).and_then(|l| g.get(l));
        Side {
            label: n.map_or_else(|| id.to_string(), crate::nodes::model::Node::display_name),
            words: n.map(|n| n.title.trim().to_string()).unwrap_or_default(),
            still: look.and_then(|l| l.still()).and_then(&path),
            colour: n.and_then(|n| g.colour_of(n)).map_or(Color32::from_gray(60), |c| c.fill),
        }
    };
    g.cuts
        .iter()
        .map(|c| {
            let tnode = c.transition.as_deref().and_then(|t| g.get(t));
            let (name, sec, _) = area.cut_label(c);
            Row {
                cut: c.id.clone(),
                transition: c.transition.clone(),
                name,
                sec,
                still: tnode.and_then(|n| n.still()).and_then(&path),
                state: tnode.map(|n| n.state),
                made: tnode.is_some_and(|n| !n.versions.is_empty()),
                from: side(&c.from),
                to: side(&c.to),
            }
        })
        .collect()
}

/// A picture cropped to fill `r`, or the empty box.
fn picture(ui: &egui::Ui, r: Rect, path: Option<&PathBuf>, fill: Color32) -> bool {
    let p = ui.painter();
    p.rect_filled(r, 3.0, Color32::from_gray(24));
    let Some(path) = path else { return false };
    let stills = Stills::large(ui.ctx());
    stills.poll(ui.ctx());
    match stills.get(path) {
        Some(tex) => {
            let [tw, th] = tex.size().map(|x| x.max(1) as f32);
            let (fw, fh) = (r.width().max(1.0), r.height().max(1.0));
            let (sx, sy) = if tw / th > fw / fh { ((fw / fh) / (tw / th), 1.0) } else { (1.0, (tw / th) / (fw / fh)) };
            let uv = Rect::from_min_max(pos2((1.0 - sx) / 2.0, (1.0 - sy) / 2.0), pos2((1.0 + sx) / 2.0, (1.0 + sy) / 2.0));
            p.image(tex.id(), r, uv, Color32::WHITE);
            true
        }
        None => {
            p.rect_stroke(r, 3.0, Stroke::new(1.0, fill), StrokeKind::Inside);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            false
        }
    }
}

fn side_ui(ui: &mut egui::Ui, ctx: &MfxCtx, s: &Side, w: f32, h: f32) {
    let t = ctx.theme;
    ui.vertical(|ui| {
        ui.set_width(w);
        let (r, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
        if !picture(ui, r, s.still.as_ref(), s.colour) {
            // no look yet: the scene's words, never an empty box
            let g = ui.painter().layout(s.words.clone(), FontId::proportional(11.5), t.text_faint, w - 12.0);
            ui.painter().with_clip_rect(r).galley(r.min + vec2(6.0, 6.0), g, t.text_faint);
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new("▌").color(s.colour));
            ui.add(egui::Label::new(RichText::new(format!("{}  {}", s.label, s.words)).size(11.5).color(t.text_dim)).truncate());
        });
    });
}

fn board(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let list = rows(ctx);
    let c = &ctx.state.cuts;
    let using = c.packs.iter().find(|p| p.id == c.pack).map_or("the transitions library".to_string(), |p| p.name.clone());
    let pack_list: Vec<(String, f32)> = c
        .packs
        .iter()
        .find(|p| p.id == c.pack)
        .map_or_else(|| TRANSITIONS.iter().map(|(k, s)| ((*k).to_string(), *s)).collect(), |p| p.transitions.iter().map(|x| (x.0.clone(), x.1)).collect());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Transitions").size(15.0).color(t.text).strong());
        ui.label(RichText::new(format!("{} cuts · from {using}", list.len())).size(12.0).color(t.text_faint));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let b = ui.button("Transitions menu…").on_hover_text("The transition pack and your words the transitions are made from");
            ctx.auto("mfx.cuts.menu", &b, "Transitions menu…");
            if b.clicked() {
                ctx.state.cuts.show = true;
            }
        });
    });
    ui.add(
        egui::Label::new(
            RichText::new("Each cut: the scene before, the transition, the scene after. OK it, ask for a Fix, or change it.").size(12.0).color(t.text_dim),
        )
        .wrap(),
    );
    ui.add_space(6.0);
    if list.is_empty() {
        ui.add_space(20.0);
        let words = if ctx.state.nodes.loaded { "No cuts yet: they come with the transitions." } else { "Reading the cuts…" };
        ui.vertical_centered(|ui| ui.label(RichText::new(words).size(13.0).color(t.text_faint)));
        return;
    }
    // the video's shape: a vertical video's frames are tall and narrow
    let aspect = if ctx.state.look.view.aspect > 0.0 { ctx.state.look.view.aspect } else { 16.0 / 9.0 };
    let mid_w = 230.0;
    let side_w = ((ui.available_width() - mid_w - 40.0) / 2.0).clamp(60.0, 260.0);
    let h = (side_w / aspect).clamp(60.0, 240.0);
    let side_w = (h * aspect).min(side_w);
    let mut picks: Vec<(String, String, f32)> = Vec::new();
    let mut decide: Option<(String, &'static str, String)> = None;
    let mut select: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("mfx-cuts-board").auto_shrink([false, false]).show(ui, |ui| {
        for row in &list {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 14.0;
                side_ui(ui, ctx, &row.from, side_w, h);
                // the transition
                ui.vertical(|ui| {
                    ui.set_width(mid_w);
                    let ph = (h - 70.0).clamp(46.0, 130.0);
                    let (r, resp) = ui.allocate_exact_size(vec2(mid_w, ph), Sense::click());
                    if !picture(ui, r, row.still.as_ref(), t.separator) {
                        crate::nodes::panel::draw_transition(ui.painter(), r.shrink(6.0), &row.name, &t);
                    }
                    ctx.auto(&format!("mfx.cuts.{}.show", row.cut), &resp, &row.name);
                    if resp.clicked() {
                        select = row.transition.clone();
                    }
                    let len = row.sec.map(|s| format!(" · {s:.1} s")).unwrap_or_default();
                    ui.label(RichText::new(format!("→ {}{len}", row.name)).size(13.0).color(t.text).strong());
                    let state = match (row.made, row.state) {
                        (false, _) => "· Being made".to_string(),
                        (true, Some(s)) => format!("{} {}", s.glyph(), s.word()),
                        (true, None) => String::new(),
                    };
                    ui.label(RichText::new(state).size(11.5).color(t.text_faint));
                    ui.horizontal(|ui| {
                        if let Some(tid) = &row.transition {
                            let mut on = row.state == Some(NodeState::Ok);
                            let b = ui.add_enabled(row.made, egui::Checkbox::new(&mut on, "OK"));
                            ctx.auto(&format!("mfx.cuts.{}.ok", row.cut), &b, "OK");
                            if b.changed() {
                                decide = Some((tid.clone(), if on { "ok" } else { "clear" }, String::new()));
                            }
                            let f = ui.add_enabled(row.made, egui::Button::new("Fix…"));
                            ctx.auto(&format!("mfx.cuts.{}.fix", row.cut), &f, "Fix…");
                            if f.clicked() {
                                ctx.state.cuts.fix = Some((tid.clone(), String::new()));
                            }
                        }
                        let r = egui::ComboBox::from_id_salt(("mfx-cuts-change", &row.cut)).width(90.0).selected_text("Change").show_ui(ui, |ui| {
                            for (k, s) in &pack_list {
                                if ui.selectable_label(false, format!("{k} · {s:.1} s")).clicked() {
                                    picks.push((row.cut.clone(), k.clone(), *s));
                                }
                            }
                        });
                        ctx.auto(&format!("mfx.cuts.{}.change", row.cut), &r.response, "Change");
                    });
                    // the Fix box, under the transition it is about
                    if let Some((tid, mut words)) = ctx.state.cuts.fix.clone()
                        && row.transition.as_deref() == Some(tid.as_str())
                    {
                        let e = ui.add(egui::TextEdit::singleline(&mut words).hint_text("What should change").desired_width(mid_w));
                        ctx.auto(&format!("mfx.cuts.{}.fixWords", row.cut), &e, "What should change");
                        let send = ui.button("Send the Fix");
                        ctx.auto(&format!("mfx.cuts.{}.sendFix", row.cut), &send, "Send the Fix");
                        if send.clicked() && !words.trim().is_empty() {
                            decide = Some((tid.clone(), "fix", words.trim().to_string()));
                        }
                        ctx.state.cuts.fix = Some((tid, words));
                    }
                });
                side_ui(ui, ctx, &row.to, side_w, h);
            });
            ui.add_space(6.0);
            ui.painter().hline(ui.max_rect().x_range(), ui.cursor().min.y, Stroke::new(1.0, t.separator));
            ui.add_space(8.0);
        }
    });
    for (cut, kind, sec) in picks {
        ctx.command("nodes.pickTransition", json!({"cut": cut, "kind": kind, "sec": sec}));
    }
    if let Some((id, action, words)) = decide {
        let MfxCtx { state, service, .. } = ctx;
        match state.nodes.decide(service, &id, action, &words, true) {
            Ok(()) if action == "fix" => {
                state.cuts.fix = None;
                state.cuts.note = String::new();
            }
            Ok(()) => {}
            Err(e) => state.cuts.note = e,
        }
    }
    if let Some(id) = select {
        ctx.command("nodes.select", json!({"node": id, "frame": true}));
    }
    let note = ctx.state.cuts.note.clone();
    if !note.is_empty() {
        ui.label(RichText::new(note).size(12.0).color(t.text_dim));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transition_packs_are_read_safely() {
        let p = parse_packs(&json!([
            {"id": "paper", "name": "Paper", "transitions": [{"name": "Paper tear", "sec": 0.6, "words": "rips"}, {"sec": 2}, {"name": "Long", "sec": 99}]},
            {"name": "no id"}, 3
        ]));
        assert_eq!(p.len(), 1);
        assert_eq!(p[0].transitions, vec![("Paper tear".to_string(), 0.6, "rips".to_string()), ("Long".to_string(), 0.5, String::new())]);
    }

    #[test]
    fn the_centre_is_the_menu_until_transitions_are_picked_or_made() {
        let mut s = MfxState::default();
        assert!(!wanted(&s));
        s.cuts.loaded = true;
        assert!(wanted(&s));
        s.cuts.saved = true;
        assert!(!wanted(&s));
        s.cuts.show = true;
        assert!(wanted(&s));
    }
}
