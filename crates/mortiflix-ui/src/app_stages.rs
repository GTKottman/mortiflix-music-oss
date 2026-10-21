//! The app studio's stage panels (owner, 2026-10-21; Design Docs/App Studio - Plan v1.md). v1 has the ten
//! stages and their gates only: each centre says what the stage will make, and each stage's own work comes
//! one version at a time.

use egui::{FontId, RichText};

use crate::MfxCtx;

/// What each stage makes, in the owner's plan's words. Index = stage.
const ABOUT: [&str; 10] = [
    "The chat asks what problem the app solves, who uses it, what it must do and what it must never do. Out comes a one-page brief.",
    "Every feature of the app, one at a time. OK, Fix or Cut each one.",
    "Every screen is a node; the noodles are how you move between them.",
    "Three or four visual directions. Pick one, and it becomes the app's design system: colours, type and buttons.",
    "Every screen drawn in the picked look. Click a screen and type a change.",
    "A clickable fake app made from the mockups. Click through it like the real thing.",
    "The stack, the data, the API and the tests, with a plain-words summary on top.",
    "The build split into small tickets, in order, each tied to its features and screens.",
    "The AI builds the app ticket by ticket, tests each one, and shows its progress live.",
    "Run it, package it, then an installer or a deploy.",
];

/// The hint in the stage bar for each stage.
pub fn hint(stage: u8) -> &'static str {
    match stage {
        0 => "Tell the chat about your app. Nothing is made until you press Next.",
        1 => "Check each feature: OK, Fix or Cut it.",
        2 => "Check the screens and how they connect.",
        3 => "Pick a look for the app.",
        4 => "Check each screen: OK or Fix it.",
        5 => "Click through the prototype.",
        6 => "Check the tech plan.",
        7 => "Check the tasks and their order.",
        8 => "Watch the build. Fix anything that's off.",
        _ => "Ship the app.",
    }
}

/// The centre of an app stage: its name, what it makes, and (Requirements) what a user story is.
pub fn show_centre(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let stage = ctx.state.stage;
    let about = ABOUT.get(usize::from(stage)).copied().unwrap_or("");
    ui.painter().rect_filled(ui.max_rect(), 0.0, t.panel_bg);
    egui::ScrollArea::vertical().id_salt("mfx-app-stage").show(ui, |ui| {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(560.0);
            ui.label(RichText::new(format!("{} {}", stage + 1, crate::stage_name(stage))).font(FontId::proportional(22.0)).color(t.text));
            ui.add_space(10.0);
            ui.label(RichText::new(about).size(14.0).color(t.text_dim));
            if stage == 1 {
                ui.add_space(24.0);
                user_stories(ui, ctx);
            }
            ui.add_space(24.0);
            ui.label(RichText::new("This stage's own work comes in a later version.").size(12.0).color(t.text_faint));
        });
    });
    let resp = ui.interact(ui.max_rect(), egui::Id::new(("mfx-app-stage", stage)), egui::Sense::hover());
    ctx.auto(&format!("mfx.app.{}", crate::stage_key(stage)), &resp, about);
}

/// What a user story and its "done when" checks are, in plain words with an example (owner, 2026-10-21:
/// "people don't really understand that").
fn user_stories(ui: &mut egui::Ui, ctx: &MfxCtx) {
    let t = ctx.theme;
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.label(RichText::new("What's a user story?").size(15.0).color(t.text));
        ui.add_space(4.0);
        ui.label(RichText::new("Each feature is written as one sentence from the point of view of the person using the app:").size(13.0).color(t.text_dim));
        ui.add_space(4.0);
        ui.label(RichText::new("As a [who], I want [what], so that [why].").size(13.0).color(t.text));
        ui.add_space(4.0);
        ui.label(RichText::new("For example: As a home cook, I want to save a recipe, so that I can find it again later.").size(13.0).color(t.text_dim));
        ui.add_space(14.0);
        ui.label(RichText::new("What's a \"done when\" check?").size(15.0).color(t.text));
        ui.add_space(4.0);
        ui.label(
            RichText::new("The tests that prove the feature works. The AI builds against them and tests against them. For saving a recipe:")
                .size(13.0)
                .color(t.text_dim),
        );
        ui.add_space(4.0);
        for check in ["A Save button is on every recipe.", "Saved recipes show up in a \"Saved\" list.", "They're still there after closing the app."] {
            ui.label(RichText::new(format!("•  {check}")).size(13.0).color(t.text_dim));
        }
    });
}

/// The gate side of an app stage: one line on how to pass it.
pub fn show_stage(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let name = crate::stage_name(ctx.state.stage);
    let next = crate::stage_name(ctx.state.stage.saturating_add(1));
    let text = if next.is_empty() { format!("{name} is the last stage.") } else { format!("Press Next when {name} is right. {next} opens after it.") };
    // wrapped: the gate panel is narrow (a one-line placeholder ran off both edges)
    ui.add_space(16.0);
    let resp = ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.add(egui::Label::new(RichText::new(&text).size(13.0).color(ctx.theme.text_dim)).wrap())
    });
    ctx.auto("mfx.app.gate", &resp.inner, &text);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_app_stage_says_what_it_makes() {
        assert_eq!(ABOUT.len(), crate::APP_STAGES.len());
        assert!(ABOUT.iter().all(|a| !a.is_empty()));
        for i in 0..10u8 {
            assert!(!hint(i).is_empty());
        }
    }
}
