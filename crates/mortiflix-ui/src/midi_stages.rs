//! The MIDI studio's stage panels (owner, 2026-10-23; Design Docs/MIDI Studio - Plan v1.md). v1 has the nine
//! stages and their gates: each centre says what the stage will make, and each stage's own work comes one
//! version at a time.

use egui::{FontId, RichText};

use crate::MfxCtx;

/// What each stage makes, in the plan's words. Index = stage.
const ABOUT: [&str; 9] = [
    "The chat asks what the song is for, the feel, how long, and a song it should sound like. Out comes a one-page brief.",
    "A genre profile from sources: tempo range, keys, drum patterns, instruments, song form and signature moves.",
    "The sections in order as nodes (Intro, Verse, Chorus…), each with its bars and energy, plus tempo, key, meter and the list of parts.",
    "The chord progression for each section, played on piano. OK, Fix or Cut each one.",
    "Short shapes for each part over the chords: drums, bass, keys… Love, Like or Skip. The ones you love repeat most.",
    "The lead over the chords, or the vocal melody when there are lyrics.",
    "The whole song, built a part at a time while you watch, from the shapes you picked. Click bars and type a change: only those bars change.",
    "Velocity, timing, swing and fills at the end of sections, with a before and after to compare.",
    "The song as one MIDI file, one per part and one per section, plus the steps to bring it into your DAW.",
];

/// The hint in the stage bar for each stage.
pub fn hint(stage: u8) -> &'static str {
    match stage {
        0 => "Answer the questions in the chat, then check the brief.",
        1 => "Check the genre profile.",
        2 => "Check the sections and their order.",
        3 => "Listen to each section's chords: OK, Fix or Cut them.",
        4 => "Love, Like or Skip each shape.",
        5 => "Listen to the melody: OK or Fix it.",
        6 => "Watch the song come together. Fix any bars that are off.",
        7 => "Compare before and after.",
        _ => "Get the MIDI files.",
    }
}

/// The centre of a MIDI stage: its name and what it makes.
pub fn show_centre(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let stage = ctx.state.stage;
    let about = ABOUT.get(usize::from(stage)).copied().unwrap_or("");
    ui.painter().rect_filled(ui.max_rect(), 0.0, t.panel_bg);
    egui::ScrollArea::vertical().id_salt("mfx-midi-stage").show(ui, |ui| {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(560.0);
            ui.label(RichText::new(format!("{} {}", stage + 1, crate::stage_name(stage))).font(FontId::proportional(22.0)).color(t.text));
            ui.add_space(10.0);
            ui.label(RichText::new(about).size(14.0).color(t.text_dim));
            ui.add_space(24.0);
            ui.label(RichText::new("This stage's own work comes in a later version.").size(12.0).color(t.text_faint));
        });
    });
    let resp = ui.interact(ui.max_rect(), egui::Id::new(("mfx-midi-stage", stage)), egui::Sense::hover());
    ctx.auto(&format!("mfx.midi.{}", crate::stage_key(stage)), &resp, about);
}

/// The node area in the MIDI studio: the song's sections come at Song map.
pub fn show_nodes(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.vertical(|ui| {
            if crate::service_notice(ui, ctx) {
                return;
            }
            let open = ctx.state.project_id.is_some();
            let text = if open {
                "Your song's sections appear here as nodes at 3 Song map. The noodles between them are the order they play in."
            } else {
                "No project open yet. Your song's sections appear here as nodes."
            };
            ui.add(egui::Label::new(RichText::new(text).size(13.0).color(t.text_dim)).wrap());
            if !open {
                ui.add_space(6.0);
                let r = ui.button("New project…");
                ctx.auto("mfx.nodes.newProject", &r, "New project…");
                if r.clicked() {
                    ctx.command("mortiflix.newProject", serde_json::json!({}));
                }
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_midi_stage_says_what_it_makes() {
        assert_eq!(ABOUT.len(), crate::MIDI_STAGES.len());
        assert!(ABOUT.iter().all(|a| !a.is_empty()));
        for i in 0..9u8 {
            assert!(!hint(i).is_empty());
        }
    }
}
