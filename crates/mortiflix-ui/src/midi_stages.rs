//! The MIDI studio's stage panels (owner, 2026-10-23; Design Docs/MIDI Studio - Plan v1.md). v1 has the nine
//! stages and their gates: each centre says what the stage will make. v2: each centre shows its stage's
//! document as the service wrote it (the service's `midi.get`), and Export opens the MIDI folder.

use egui::{FontId, RichText};
use serde_json::{Value, json};

use crate::MfxCtx;
use crate::service::Service;
use crate::state::MfxState;

/// One step of the song's pipeline as the service last sent it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiStep {
    /// `questions`, `idea`, `genre`, `song-map`… in pipeline order.
    pub key: String,
    /// The stage it belongs to ([`crate::MIDI_STAGES`] key): questions and idea are both `idea`.
    pub stage: String,
    /// `ready`, `working`, `changes`, `in_review`, `approved`, `blocked`…
    pub state: String,
    pub version: u32,
    /// The stage's document (brief.md, genre.md…), empty until the first version.
    pub doc: String,
}

/// The MIDI studio's stages as the service last sent them: every step, the tokens used, the MIDI folder.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MidiState {
    loaded_for: Option<String>,
    pub fetch_soon: bool,
    fetching: Option<u64>,
    pub steps: Vec<MidiStep>,
    /// Tokens the project's AI calls used so far.
    pub usage: u64,
    /// The MIDI files' folder, empty until Export wrote it.
    pub out: String,
}

impl MidiState {
    /// Events that can change a stage: read them again.
    pub fn on_message(&mut self, msg: &Value) {
        if matches!(msg.get("event").and_then(Value::as_str), Some("chat.message" | "status" | "project.changed")) {
            self.fetch_soon = true;
        }
    }
    /// The step called `key`.
    pub fn step(&self, key: &str) -> Option<&MidiStep> {
        self.steps.iter().find(|s| s.key == key)
    }
    /// The step whose document a stage shows: its last one (on Idea, the brief rather than the questions).
    pub fn main_step(&self, stage_key: &str) -> Option<&MidiStep> {
        self.steps.iter().rev().find(|s| s.stage == stage_key)
    }
}

/// Every frame in the MIDI studio: follow the open project, read the stages when they changed. Never blocks.
pub fn tick(state: &mut MfxState, service: &mut Service) {
    let st = &mut state.midi;
    if st.loaded_for != state.project_id {
        *st = MidiState { loaded_for: state.project_id.clone(), fetch_soon: state.project_id.is_some(), ..MidiState::default() };
    }
    if st.fetch_soon && st.fetching.is_none() && st.loaded_for.is_some() && service.status().is_connected() {
        st.fetch_soon = false;
        st.fetching = Some(service.request("midi.get", json!({})));
    }
    if let Some(id) = st.fetching
        && let Some(r) = service.take_reply(id)
    {
        st.fetching = None;
        if r.ok {
            apply(st, &r.result);
        }
    }
}

/// `midi.get`'s answer into the state. Missing or wrong-typed fields read as empty.
fn apply(st: &mut MidiState, result: &Value) {
    let text = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or_default().to_string();
    let steps = result.get("steps").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    st.steps = steps
        .iter()
        .take(64)
        .map(|s| MidiStep {
            key: text(s, "key"),
            stage: text(s, "stage"),
            state: text(s, "state"),
            version: s.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(0),
            doc: text(s, "doc"),
        })
        .collect();
    st.usage = result.get("usage").and_then(Value::as_u64).unwrap_or(0);
    st.out = text(result, "out");
}

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

/// What each stage's document is called in a line: (name, "is" or "are"). Index = stage.
const MADE: [(&str, &str); 9] = [
    ("brief", "is"),
    ("genre profile", "is"),
    ("song map", "is"),
    ("chords", "are"),
    ("shapes", "are"),
    ("melody", "is"),
    ("arrangement", "is"),
    ("feel", "is"),
    ("MIDI files", "are"),
];

/// True while the service is making (or remaking) a step.
fn making(step: &MidiStep) -> bool {
    matches!(step.state.as_str(), "ready" | "working" | "changes")
}

/// The one-line status while a stage's document is being made: "Making the song map…", "Making the song map v2…".
fn making_line(stage: u8, step: &MidiStep) -> String {
    let (name, _) = MADE.get(usize::from(stage)).copied().unwrap_or(("stage", "is"));
    if step.doc.is_empty() { format!("Making the {name}…") } else { format!("Making the {name} v{}…", step.version + 1) }
}

/// Where the Idea stage is, before the brief: the questions in the chat, or the brief being written.
fn idea_line(midi: &MidiState) -> String {
    let questions = midi.step("questions").map(|s| s.state.as_str()).unwrap_or_default();
    match (questions, midi.step("idea")) {
        ("ready" | "working" | "changes", _) => "Reading your idea…".into(),
        ("in_review", _) => "The four questions are in the chat. Take the suggested answer, or type your own.".into(),
        (_, Some(brief)) if making(brief) => making_line(0, brief),
        _ => String::new(),
    }
}

/// The centre of a MIDI stage: its document when the service has made it, else its name, what it makes and
/// where it is.
pub fn show_centre(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let stage = ctx.state.stage;
    let about = ABOUT.get(usize::from(stage)).copied().unwrap_or("");
    let step = ctx.state.midi.main_step(crate::stage_key(stage)).cloned();
    ui.painter().rect_filled(ui.max_rect(), 0.0, t.panel_bg);
    egui::ScrollArea::vertical().id_salt("mfx-midi-stage").show(ui, |ui| {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(560.0);
            if let Some(step) = step.as_ref().filter(|s| !s.doc.is_empty()) {
                ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| doc(ui, ctx, stage, step));
                return;
            }
            ui.label(RichText::new(format!("{} {}", stage + 1, crate::stage_name(stage))).font(FontId::proportional(22.0)).color(t.text));
            ui.add_space(10.0);
            ui.label(RichText::new(about).size(14.0).color(t.text_dim));
            let now = match &step {
                _ if stage == 0 => idea_line(&ctx.state.midi),
                Some(s) if making(s) => making_line(stage, s),
                _ => String::new(),
            };
            if !now.is_empty() {
                ui.add_space(24.0);
                ui.label(RichText::new(now).size(13.0).color(t.text));
            }
            tokens_line(ui, ctx);
        });
    });
    let resp = ui.interact(ui.max_rect(), egui::Id::new(("mfx-midi-stage", stage)), egui::Sense::hover());
    ctx.auto(&format!("mfx.midi.{}", crate::stage_key(stage)), &resp, about);
}

/// A stage's document: the status while a new version is made, the document, how to pass the gate, and on
/// Export the button to the MIDI folder.
fn doc(ui: &mut egui::Ui, ctx: &mut MfxCtx, stage: u8, step: &MidiStep) {
    let t = ctx.theme;
    if making(step) {
        ui.label(RichText::new(making_line(stage, step)).size(12.0).color(t.text_faint));
        ui.add_space(8.0);
    }
    crate::markdown::show(ui, &t, &step.doc);
    ui.add_space(24.0);
    let last = usize::from(stage) + 1 >= crate::MIDI_STAGES.len();
    if last && !ctx.state.midi.out.is_empty() {
        let out = ctx.state.midi.out.clone();
        let r = ui.button("Open the MIDI folder").on_hover_text(&out);
        ctx.auto("mfx.midi.export.openFolder", &r, "Open the MIDI folder");
        if r.clicked() {
            // the folder itself, in the file manager
            ctx.command("mortiflix.reveal", json!({"path": out, "open": true}));
        }
    } else if !last {
        let (name, is) = MADE.get(usize::from(stage)).copied().unwrap_or(("stage", "is"));
        let hint = if step.state == "approved" {
            format!("The {name} {is} OK'd.")
        } else {
            format!("Press Next when the {name} {is} right, or tell the chat what to change.")
        };
        ui.label(RichText::new(hint).size(12.0).color(t.text_faint));
    }
    tokens_line(ui, ctx);
}

/// "Tokens so far: 12.9 k", small, once the project has used any.
fn tokens_line(ui: &mut egui::Ui, ctx: &MfxCtx) {
    let used = ctx.state.midi.usage;
    if used > 0 {
        ui.add_space(12.0);
        ui.label(RichText::new(format!("Tokens so far: {}", crate::tokens::short(used))).size(11.0).color(ctx.theme.text_faint));
    }
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
        assert_eq!(MADE.len(), crate::MIDI_STAGES.len());
    }

    #[test]
    fn midi_get_fills_the_steps_and_the_folder() {
        let mut st = MidiState::default();
        let result = json!({
            "steps": [
                {"key": "questions", "stage": "idea", "state": "approved", "version": 1, "doc": null},
                {"key": "idea", "stage": "idea", "state": "in_review", "version": 1, "doc": "# Song · Brief v1"},
                {"key": "genre", "stage": "genre", "state": "working", "version": 0, "doc": null},
            ],
            "usage": 12869,
            "out": null,
        });
        apply(&mut st, &result);
        assert_eq!(st.steps.len(), 3);
        assert_eq!(st.main_step("idea").map(|s| s.key.as_str()), Some("idea"));
        assert_eq!(st.usage, 12869);
        assert!(st.out.is_empty());
        let genre = st.main_step("genre").cloned().unwrap_or_default();
        assert!(making(&genre));
        assert_eq!(making_line(1, &genre), "Making the genre profile…");
        assert_eq!(idea_line(&st), "");
        // a wrong answer leaves everything empty instead of failing
        apply(&mut st, &json!({"steps": "no", "usage": -1}));
        assert!(st.steps.is_empty() && st.usage == 0);
    }
}
