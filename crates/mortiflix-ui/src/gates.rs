//! The stage gates (owner, 2026-10-18: "the gates themselves can now be represented in the UI"):
//! PanelKind::MfxStage on stages 2-6. Each shows what must be true before Next, a box to check off
//! every piece of the stage (an OK sent at once, unchecking takes it back), and what Next makes.
//! Nothing of a stage is made before the gate before it passed (the service holds every step until
//! the steps before it are approved). The Script gate is [`crate::script::gate`].

use egui::{Color32, RichText, vec2};
use serde_json::json;

use crate::nodes::model::{Kind, NodeState};
use crate::{MfxCtx, STAGE_ANIMATIC, STAGE_MUSIC, STAGE_STYLE_FRAMES, STAGE_TRANSITIONS, STAGE_VOICE, stage_name};

/// What a stage checks off, and what pressing Next starts.
struct Gate {
    kind: Kind,
    /// The pieces, plural ("voice lines").
    noun: &'static str,
    /// What the next stage makes once this gate passes.
    next: &'static str,
}

fn gate_of(stage: u8) -> Option<Gate> {
    Some(match stage {
        STAGE_VOICE => Gate {
            kind: Kind::Line,
            noun: "voice lines",
            next: "The style frames are drawn in the voice's mood: the look and a few panels for you to check off.",
        },
        STAGE_STYLE_FRAMES => {
            Gate { kind: Kind::Frame, noun: "style frames", next: "Every cut between the scenes gets its transition, drawn in the look you checked off." }
        }
        STAGE_TRANSITIONS => Gate {
            kind: Kind::Transition,
            noun: "transitions",
            next: "You pick the motion (a motion pack and rule packs), then the animatic: every scene timed to the voice, in the look, cut with these transitions.",
        },
        STAGE_ANIMATIC => {
            Gate { kind: Kind::Shot, noun: "shots", next: "The music is written to the animatic's timing in Strudel, its MIDI under the animatic." }
        }
        STAGE_MUSIC => Gate { kind: Kind::Cue, noun: "music cues", next: "Edit the video in Mortiflix Studio, or get it rendered straight away." },
        _ => return None,
    })
}

/// The gate panel for the stage on screen.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let stage = ctx.state.stage;
    let Some(gate) = gate_of(stage) else { return };
    let t = ctx.theme;
    // nothing here may widen the panel: long lines wrap
    ui.set_max_width(ui.available_width());
    ui.label(RichText::new(format!("{} gate", stage_name(stage))).size(15.0).color(t.text).strong());
    ui.add_space(2.0);
    ui.label(RichText::new(format!("Nothing after {} is made until this passes.", stage_name(stage))).size(12.0).color(t.text_dim));
    if stage == STAGE_TRANSITIONS {
        // the transition pack picked before the transitions are made (owner, 2026-10-18)
        let c = &ctx.state.cuts;
        let words = if c.saved {
            format!("Transitions from: {}", c.packs.iter().find(|p| p.id == c.pack).map_or("the transitions library".to_string(), |p| p.name.clone()))
        } else {
            "Transitions: not picked yet".to_string()
        };
        ui.add_space(4.0);
        ui.label(RichText::new(words).size(12.0).color(t.text_dim));
        let b = ui.button("Transitions menu…");
        ctx.auto("mfx.gate.cutsMenu", &b, "Transitions menu…");
        if b.clicked() {
            ctx.state.cuts.show = true;
        }
    }
    if stage == STAGE_ANIMATIC {
        // the motion and rule packs picked before animating (owner, 2026-10-18)
        let m = &ctx.state.motion;
        let picked = m.motions.iter().find(|p| p.id == m.motion).map_or("the AI decides".to_string(), |p| p.name.clone());
        let rules = m.rules.iter().filter(|p| m.picked_rules.contains(&p.id)).map(|p| p.name.clone()).collect::<Vec<_>>();
        let words = if m.saved {
            format!("Motion: {picked}{}", if rules.is_empty() { String::new() } else { format!(" · rules: {}", rules.join(", ")) })
        } else {
            "Motion: not picked yet".to_string()
        };
        ui.add_space(4.0);
        ui.horizontal_top(|ui| {
            ui.add(egui::Label::new(RichText::new(words).size(12.0).color(t.text_dim)).wrap());
        });
        let b = ui.button("Motion menu…");
        ctx.auto("mfx.gate.motionMenu", &b, "Motion menu…");
        if b.clicked() {
            ctx.state.motion.show = true;
        }
    }
    ui.add_space(10.0);
    if crate::service_notice(ui, ctx) && !ctx.state.nodes.loaded {
        return;
    }
    // the pieces in film order (lines, cuts and cues by their order; shots by the chain)
    let g = &ctx.state.nodes.graph;
    let mut pieces: Vec<_> = g.nodes.iter().filter(|n| n.kind == gate.kind && n.state != NodeState::Cut).collect();
    if gate.kind == Kind::Shot {
        pieces.sort_by_key(|n| g.film.iter().position(|f| *f == n.id).unwrap_or(usize::MAX));
    } else {
        pieces.sort_by_key(|n| n.order);
    }
    let list: Vec<(String, String, String, bool, bool, Color32)> = pieces
        .iter()
        .map(|n| {
            let beat = n.beat.as_deref().map(|b| g.beat_title(b)).filter(|b| b.0 > 0).map(|b| format!("{} {}", b.0, b.1)).unwrap_or_default();
            let words = if n.title.trim().is_empty() { beat } else { n.title.trim().to_string() };
            let colour = n.colour.map_or(t.text_faint, |c| c.chip);
            (n.id.clone(), n.display_name(), words, !n.versions.is_empty(), n.state == NodeState::Ok, colour)
        })
        .collect();
    let (total, made, oks) = (list.len(), list.iter().filter(|p| p.3).count(), list.iter().filter(|p| p.4).count());
    let waiting = pieces.iter().filter(|n| matches!(n.state, NodeState::NeedsYou | NodeState::Remaking | NodeState::Making)).count();
    let rows = [
        (total > 0 && made == total, format!("Every one of the {total} {} is made ({made} so far)", gate.noun)),
        (waiting == 0, if waiting == 0 { "Nothing waits for a Fix".to_string() } else { format!("{waiting} wait for a Fix or need you") }),
        (total > 0 && oks == total, format!("Checked off: {oks} of {total} (Next takes the rest as they are)")),
    ];
    for (ok, text) in &rows {
        ui.horizontal_top(|ui| {
            ui.label(RichText::new(if *ok { "✓" } else { "○" }).size(13.0).color(if *ok { t.ok } else { t.text_faint }));
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(if *ok { t.text } else { t.text_dim })).wrap());
        });
    }
    // on Voice the lines are OK'd in the Voice list itself: no second list here (owner, 2026-10-19)
    let listed = stage != STAGE_VOICE;
    ui.add_space(10.0);
    if listed {
        ui.label(RichText::new(format!("Check off the {}", gate.noun)).size(13.0).color(t.text).strong());
    }
    ui.add_space(2.0);
    // the list gets the room left above Next; on Music the score's Strudel gets its own box below
    let list_h = if !listed {
        0.0
    } else if stage == STAGE_MUSIC {
        (ui.available_height() * 0.3).max(90.0)
    } else {
        (ui.available_height() - 120.0).max(80.0)
    };
    let strudel_h = if stage == STAGE_MUSIC { (ui.available_height() - list_h - 130.0).max(120.0) } else { 0.0 };
    let mut decide: Option<(String, &'static str)> = None;
    let mut select: Option<String> = None;
    egui::ScrollArea::vertical().id_salt(("mfx-gate", stage)).max_height(list_h).auto_shrink([false, true]).show(ui, |ui| {
        if !listed {
            return;
        }
        if list.is_empty() {
            ui.label(RichText::new(format!("No {} yet: they're made once the gate before this one passed.", gate.noun)).size(12.0).color(t.text_faint));
        }
        for (id, name, words, made, ok, colour) in &list {
            ui.horizontal(|ui| {
                let mut on = *ok;
                let b = ui.add_enabled(*made, egui::Checkbox::without_text(&mut on));
                let b = if *made { b.on_hover_text(if *ok { "Take the OK back" } else { "OK it as it is" }) } else { b.on_disabled_hover_text("Not made yet") };
                ctx.auto(&format!("mfx.gate.{id}"), &b, name);
                if b.changed() {
                    decide = Some((id.clone(), if on { "ok" } else { "clear" }));
                }
                ui.label(RichText::new("▌").color(*colour));
                let r = ui.add(
                    egui::Label::new(RichText::new(format!("{name}  {words}")).size(12.5).color(if *made { t.text } else { t.text_faint }))
                        .truncate()
                        .sense(egui::Sense::click()),
                );
                ctx.auto(&format!("mfx.gate.{id}.show"), &r, name);
                if r.clicked() {
                    select = Some(id.clone());
                }
            });
        }
    });
    if let Some((id, action)) = decide {
        let MfxCtx { state, service, .. } = ctx;
        if let Err(e) = state.nodes.decide(service, &id, action, "", true) {
            state.nodes.error = e;
        }
    }
    if let Some(id) = select {
        ctx.command("nodes.select", json!({"node": id, "frame": true}));
    }
    if stage == STAGE_MUSIC {
        ui.add_space(10.0);
        crate::score::strudel(ui, ctx, strudel_h - 40.0);
    }
    ui.add_space(12.0);
    let next = stage_name(stage.saturating_add(1));
    if stage == STAGE_MUSIC {
        ui.label(RichText::new("Then").size(13.0).color(t.text).strong());
    } else {
        ui.label(RichText::new(format!("Next: {next}")).size(13.0).color(t.text).strong());
    }
    ui.add(egui::Label::new(RichText::new(gate.next).size(12.0).color(t.text_dim)).wrap());
    ui.add_space(8.0);
    if stage != STAGE_MUSIC {
        let quiet = stage == STAGE_VOICE && oks < total;
        let b = ui.add(
            egui::Button::new(RichText::new(format!("Pass the gate: Next: {next}")).size(13.0).color(if quiet { t.text } else { Color32::WHITE }))
                .fill(if quiet { t.hover } else { t.accent })
                .min_size(vec2(0.0, 28.0))
                .corner_radius(4.0),
        );
        ctx.auto("mfx.gate.next", &b, "Pass the gate");
        if b.clicked() {
            ctx.command("stages.next", json!({}));
        }
    }
}
