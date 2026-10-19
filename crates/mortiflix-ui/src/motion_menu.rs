//! The motion menu (owner, 2026-10-18: "before animating, we should be able to pick a motion pack
//! that tells the AI to use certain kinds of motions. and rule packs so the AI follows specific
//! rules"). Shown in the Animatic centre before any shot is animated (and from Motion menu… after):
//! a motion pack (or the AI decides), any rule packs, and the owner's own words. Animate with this
//! saves it (`motion.brief.set`); the service holds the animatic until then, and every session from
//! then on follows the rule packs.
//!
//! Data: `packs.list {kind}`, `motion.brief.get`, `motion.brief.set` (protocol: `docs/mortiflix/protocol.md`).

use egui::{Color32, RichText, vec2};
use serde_json::{Value, json};

use crate::nodes::model::Kind;
use crate::{MfxCtx, Service, state::MfxState};

/// A motion or rule pack as `packs.list` sends it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub description: String,
    /// Motion: its moves, easing and pace; rule: its rules.
    pub lines: Vec<String>,
    pub detail: String,
    pub words: String,
}

/// The menu's state (in [`MfxState::motion`], so the host knows whether the Animatic centre is the menu).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MotionState {
    /// Motion menu… was pressed.
    pub show: bool,
    pub motions: Vec<Pack>,
    pub rules: Vec<Pack>,
    /// The chosen motion pack ("" = the AI decides).
    pub motion: String,
    pub picked_rules: Vec<String>,
    pub words: String,
    /// The project has a saved motion (`motion.brief.get` answered with one).
    pub saved: bool,
    pub loaded: bool,
    pub note: String,
    project: Option<String>,
    reqs: [Option<u64>; 3],
    save: Option<u64>,
}

fn text(v: &Value, k: &str, n: usize) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").chars().take(n).collect()
}

fn texts(v: &Value, k: &str, most: usize) -> Vec<String> {
    v.get(k).and_then(Value::as_array).into_iter().flatten().take(most).filter_map(Value::as_str).map(|s| s.chars().take(300).collect()).collect()
}

pub fn parse_packs(v: &Value) -> Vec<Pack> {
    v.as_array()
        .into_iter()
        .flatten()
        .take(500)
        .filter_map(|p| {
            let id = text(p, "id", 120);
            if id.is_empty() {
                return None;
            }
            let rule = p.get("kind").and_then(Value::as_str) == Some("rule");
            let lines = if rule { texts(p, "rules", 60) } else { texts(p, "moves", 60) };
            let detail = if rule {
                String::new()
            } else {
                [("pace", text(p, "pace", 200)), ("easing", text(p, "easing", 200))]
                    .into_iter()
                    .filter(|(_, v)| !v.is_empty())
                    .map(|(k, v)| format!("{k} {v}"))
                    .collect::<Vec<_>>()
                    .join(" · ")
            };
            Some(Pack { name: text(p, "name", 80), description: text(p, "description", 600), lines, detail, words: text(p, "words", 4000), id })
        })
        .collect()
}

/// Every frame: read the packs and the saved motion once per project; take the save's answer.
pub fn tick(state: &mut MfxState, service: &mut Service) {
    let m = &mut state.motion;
    if m.project != state.project_id {
        *m = MotionState { project: state.project_id.clone(), ..MotionState::default() };
    }
    for (slot, req) in m.reqs.iter_mut().enumerate() {
        let Some(r) = req.and_then(|id| service.take_reply(id)) else { continue };
        *req = None;
        match slot {
            0 if r.ok => m.motions = parse_packs(&r.result),
            1 if r.ok => m.rules = parse_packs(&r.result),
            2 => {
                m.loaded = true;
                let b = &r.result;
                if r.ok && b.is_object() {
                    m.saved = true;
                    m.motion = b.pointer("/motion_pack/id").and_then(Value::as_str).unwrap_or("").chars().take(120).collect();
                    m.picked_rules = b
                        .get("rule_packs")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .take(20)
                        .filter_map(|r| r.get("id").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect();
                    m.words = text(b, "words", 2000);
                }
            }
            _ => {}
        }
    }
    if let Some(id) = m.save
        && let Some(r) = service.take_reply(id)
    {
        m.save = None;
        if r.ok {
            m.saved = true;
            m.show = false;
            m.note = "Saved: the animatic is made with this motion now.".into();
        } else {
            m.note = format!("Couldn't save the motion: {}", r.error.get("message").and_then(Value::as_str).unwrap_or("the studio said no"));
        }
    }
    if state.stage == crate::STAGE_ANIMATIC && !m.loaded && m.reqs.iter().all(Option::is_none) && service.status().is_connected() && state.project_id.is_some()
    {
        m.reqs = [
            Some(service.request("packs.list", json!({"kind": "motion"}))),
            Some(service.request("packs.list", json!({"kind": "rule"}))),
            Some(service.request("motion.brief.get", json!({}))),
        ];
    }
}

/// The Animatic centre is the menu: asked for, or nothing animated yet and no motion saved.
pub fn wanted(state: &MfxState) -> bool {
    let m = &state.motion;
    if m.show {
        return true;
    }
    let animated = state.nodes.graph.nodes.iter().any(|n| n.kind == Kind::Shot && !n.versions.is_empty());
    m.loaded && !m.saved && !animated
}

/// The menu, in the Animatic centre.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    ui.set_max_width(ui.available_width());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Pick the motion").size(15.0).color(t.text).strong());
        if ctx.state.motion.show {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let back = ui.button("Back to the animatic");
                ctx.auto("mfx.motionMenu.back", &back, "Back to the animatic");
                if back.clicked() {
                    ctx.state.motion.show = false;
                }
            });
        }
    });
    ui.add(
        egui::Label::new(RichText::new("The animatic is animated with this. Nothing moves until you press Animate with this.").size(12.0).color(t.text_dim))
            .wrap(),
    );
    ui.add_space(10.0);
    if crate::service_notice(ui, ctx) {
        return;
    }
    egui::ScrollArea::vertical().id_salt("mfx-motion-menu").auto_shrink([false, false]).show(ui, |ui| body(ui, ctx));
}

fn heading(ui: &mut egui::Ui, ctx: &MfxCtx, words: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(words).size(13.0).color(ctx.theme.text).strong());
    ui.add_space(2.0);
}

fn pack_detail(ui: &mut egui::Ui, ctx: &MfxCtx, p: &Pack) {
    let t = ctx.theme;
    if !p.description.is_empty() {
        ui.add(egui::Label::new(RichText::new(&p.description).size(12.0).color(t.text_dim)).wrap());
    }
    if !p.detail.is_empty() {
        ui.label(RichText::new(&p.detail).size(12.0).color(t.text_dim));
    }
    for l in p.lines.iter().take(8) {
        ui.add(egui::Label::new(RichText::new(format!("· {l}")).size(12.0).color(t.text_faint)).wrap());
    }
    if p.lines.len() > 8 {
        ui.label(RichText::new(format!("and {} more", p.lines.len() - 8)).size(11.5).color(t.text_faint));
    }
}

fn body(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    // Motion pack
    heading(ui, ctx, "Motion pack");
    let motions = ctx.state.motion.motions.clone();
    if motions.is_empty() {
        ui.label(RichText::new("You have no motion packs yet: the AI picks the motion (say how below if you like).").size(12.0).color(t.text_faint));
    } else {
        let m = &mut ctx.state.motion;
        let shown = motions.iter().find(|p| p.id == m.motion).map_or("None: the AI decides".to_string(), |p| p.name.clone());
        let r = egui::ComboBox::from_id_salt("mfx-motion-pack").width(320.0).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut m.motion, String::new(), "None: the AI decides");
            for p in &motions {
                ui.selectable_value(&mut m.motion, p.id.clone(), &p.name);
            }
        });
        ctx.auto("mfx.motionMenu.pack", &r.response, "Motion pack");
        if let Some(p) = motions.iter().find(|p| p.id == ctx.state.motion.motion) {
            ui.add_space(4.0);
            pack_detail(ui, ctx, p);
        }
    }

    // Rule packs
    heading(ui, ctx, "Rule packs");
    let rules = ctx.state.motion.rules.clone();
    if rules.is_empty() {
        ui.label(RichText::new("You have no rule packs yet.").size(12.0).color(t.text_faint));
    } else {
        ui.label(RichText::new("The AI follows every rule pack you tick, in every step from here on.").size(12.0).color(t.text_dim));
        for p in &rules {
            let mut on = ctx.state.motion.picked_rules.contains(&p.id);
            let r = ui.checkbox(&mut on, RichText::new(&p.name).size(13.0).color(t.text));
            ctx.auto(&format!("mfx.motionMenu.rule.{}", p.id), &r, &p.name);
            if r.changed() {
                let picked = &mut ctx.state.motion.picked_rules;
                picked.retain(|x| x != &p.id);
                if on {
                    picked.push(p.id.clone());
                }
            }
            if on {
                ui.indent(("mfx-motion-rule", &p.id), |ui| pack_detail(ui, ctx, p));
            }
        }
    }

    heading(ui, ctx, "In your own words");
    let r = ui.add(
        egui::TextEdit::multiline(&mut ctx.state.motion.words)
            .hint_text("How things should move: slow drifts, snappy pops, everything on the beat…")
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    ctx.auto("mfx.motionMenu.words", &r, "In your own words");

    ui.add_space(14.0);
    let m = &ctx.state.motion;
    let label = if m.saved { "Save the motion" } else { "Animate with this" };
    let b = ui.add_enabled(
        m.save.is_none(),
        egui::Button::new(RichText::new(label).size(13.0).color(Color32::WHITE)).fill(t.accent).min_size(vec2(0.0, 30.0)).corner_radius(4.0),
    );
    ctx.auto("mfx.motionMenu.animate", &b, label);
    if b.clicked() {
        let m = &ctx.state.motion;
        let params = json!({
            "motion_pack": if m.motion.is_empty() { Value::Null } else { json!(m.motion) },
            "rule_packs": m.picked_rules, "words": m.words,
        });
        let id = ctx.service.request("motion.brief.set", params);
        let m = &mut ctx.state.motion;
        m.save = Some(id);
        m.note = "Saving the motion…".into();
    }
    let m = &ctx.state.motion;
    if !m.note.is_empty() {
        ui.add_space(4.0);
        let col = if m.note.starts_with("Couldn't") { t.error } else { t.text_dim };
        ui.label(RichText::new(m.note.clone()).size(12.0).color(col));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motion_and_rule_packs_are_read_safely() {
        let p = parse_packs(&json!([
            {"id": "snappy", "kind": "motion", "name": "Snappy", "moves": ["whip pan", 3], "pace": "fast", "easing": "back out"},
            {"id": "safe", "kind": "rule", "name": "Safe text", "rules": ["Text 8% inside"]},
            {"name": "no id"}, 4
        ]));
        assert_eq!(p.len(), 2);
        assert_eq!((p[0].lines.clone(), p[0].detail.as_str()), (vec!["whip pan".to_string()], "pace fast · easing back out"));
        assert_eq!((p[1].lines.len(), p[1].detail.as_str()), (1, ""));
    }

    #[test]
    fn the_animatic_centre_is_the_menu_until_a_motion_is_saved_or_shots_exist() {
        let mut s = MfxState::default();
        assert!(!wanted(&s), "not read yet: the preview shows");
        s.motion.loaded = true;
        assert!(wanted(&s));
        s.motion.saved = true;
        assert!(!wanted(&s));
        s.motion.show = true;
        assert!(wanted(&s), "Motion menu… opens it again");
    }
}
