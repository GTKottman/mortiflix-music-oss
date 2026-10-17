//! Mortiflix FilmCraft: the Mortiflix layer on top of FilmCraft.
//!
//! The app walks a video through six stages (doc 30): 1 Script · 2 Look · 3 Shots · 4 Voice & music ·
//! 5 FilmCraft · 6 Render. Stages 1-4 are Mortiflix panels docked in FilmCraft's window; stage 5 is
//! FilmCraft's Edit mode with the Mortiflix chat docked right; stage 6 is FilmCraft's Export mode.
//!
//! This crate owns the Mortiflix state ([`state::MfxState`]), the stage bar ([`stagebar`]), one module
//! per part (each a `show(ui, ctx)`), and the supervisor of the Node "Mortiflix service" that does the AI
//! work ([`service`], protocol in `docs/mortiflix/protocol.md`). `filmcraft-ui-egui` hosts it: it
//! builds an [`MfxCtx`] per call, then runs the commands and automation ids the parts left in it.
//! Nothing here depends on FilmCraft's UI crate, so parts never touch FilmCraft code to draw.

#![cfg_attr(not(test), deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable))]

pub mod chat;
pub mod export;
pub mod into_filmcraft;
pub mod look;
pub mod new_project;
pub mod nodes;
pub mod preview;
pub mod script;
pub mod service;
pub mod setup;
pub mod stagebar;
pub mod state;
pub mod voice_music;

use egui::Color32;
use serde_json::Value;

pub use service::{Service, ServiceConfig, ServiceStatus};
pub use state::{MfxState, StageStatus, StatusKind};

/// The six stages: (name shown, command key). Index = `MfxState::stage`.
pub const STAGES: [(&str, &str); 6] =
    [("Script", "script"), ("Look", "look"), ("Shots", "shots"), ("Voice & music", "voiceMusic"), ("FilmCraft", "filmcraft"), ("Render", "render")];

/// Stage index of Voice & music (the stage that shows Get the video / Edit the video).
pub const STAGE_VOICE_MUSIC: u8 = 3;
/// Stage index of FilmCraft (final edits in FilmCraft's Edit mode).
pub const STAGE_FILMCRAFT: u8 = 4;
/// Stage index of Render (FilmCraft's Export mode).
pub const STAGE_RENDER: u8 = 5;

/// Height of the stage bar row under FilmCraft's header (doc 30).
pub const STAGE_BAR_HEIGHT: f32 = 44.0;

/// The colours the Mortiflix panels draw with, copied from FilmCraft's theme tokens each frame so
/// the panels follow the user's appearance (dark, light, darkest).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MfxTheme {
    pub app_bg: Color32,
    pub header_bg: Color32,
    pub panel_bg: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub accent: Color32,
    pub accent_hover: Color32,
    pub hover: Color32,
    pub separator: Color32,
    pub field_bg: Color32,
    pub tab_text: Color32,
    pub tab_text_active: Color32,
    /// Status colours: done / making, needs you, stopped.
    pub ok: Color32,
    pub warn: Color32,
    pub error: Color32,
}

impl Default for MfxTheme {
    fn default() -> Self {
        Self {
            app_bg: Color32::from_gray(10),
            header_bg: Color32::from_gray(24),
            panel_bg: Color32::from_gray(29),
            text: Color32::from_gray(225),
            text_dim: Color32::from_gray(170),
            text_faint: Color32::from_gray(120),
            accent: Color32::from_rgb(45, 140, 235),
            accent_hover: Color32::from_rgb(70, 160, 245),
            hover: Color32::from_gray(48),
            separator: Color32::from_gray(40),
            field_bg: Color32::from_gray(18),
            tab_text: Color32::from_gray(150),
            tab_text_active: Color32::from_gray(235),
            ok: Color32::from_rgb(80, 190, 110),
            warn: Color32::from_rgb(235, 175, 60),
            error: Color32::from_rgb(230, 80, 70),
        }
    }
}

/// Everything a Mortiflix part may use while it draws. Parts never call FilmCraft directly: they push
/// a command id (`ctx.command("file.import", json!({...}))`) and the host runs it after the part
/// returns, through the same `menus::invoke` the menus, shortcuts and control channel use.
pub struct MfxCtx<'a> {
    pub state: &'a mut MfxState,
    pub service: &'a mut Service,
    pub theme: MfxTheme,
    /// Commands to run after this call: (command id, params).
    pub commands: Vec<(String, Value)>,
    /// Automation ids for the widgets drawn (AGENTS: every interactive widget has one).
    pub auto: Vec<(String, egui::Response, String)>,
}

impl<'a> MfxCtx<'a> {
    pub fn new(state: &'a mut MfxState, service: &'a mut Service, theme: MfxTheme) -> Self {
        Self { state, service, theme, commands: Vec::new(), auto: Vec::new() }
    }
    /// Run a FilmCraft or Mortiflix command after this part returns.
    pub fn command(&mut self, id: &str, params: Value) {
        self.commands.push((id.to_string(), params));
    }
    /// Register a widget with the automation registry (`ui.click {id}` on the control channel).
    pub fn auto(&mut self, id: &str, response: &egui::Response, label: &str) {
        self.auto.push((id.to_string(), response.clone(), label.to_string()));
    }
}

/// What a stage command did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    /// The stage changed: the host switches FilmCraft's layout / mode.
    Moved { from: u8, to: u8 },
    /// Something still needs you: the pop-up asks first (`state.confirm_next`).
    Asked,
    /// Nothing to move to (Next on Render, Back on Script opens New project).
    Stay,
}

/// The Mortiflix side of the app: its state and the service connection.
#[derive(Default)]
pub struct Mortiflix {
    pub state: MfxState,
    pub service: Service,
}

impl Mortiflix {
    /// Turn Mortiflix on (the desktop app does at start): the stage bar shows and the stage
    /// commands switch FilmCraft's layout. Off (tests, FilmCraft alone), nothing of it draws.
    pub fn activate(&mut self) {
        self.state.active = true;
    }
    /// Start supervising the Node service (desktop only; a missing Node or service is a calm state,
    /// never an error).
    pub fn start_service(&mut self, config: ServiceConfig) {
        self.service.start(config);
    }
    /// Read what the service sent since the last frame and apply what the shared state tracks
    /// (statuses, chat messages, the AI's live line). Never blocks.
    pub fn poll(&mut self) {
        for msg in self.service.poll() {
            self.state.apply_service_message(&msg);
        }
        // A service that was restarted (it crashed or was stopped) has no project open: open the
        // one the app is on again, so the chat, nodes and preview carry on.
        let connects = self.service.connects();
        if connects != self.state.seen_connects {
            let again = self.state.seen_connects > 0;
            self.state.seen_connects = connects;
            if again && let Some(id) = self.state.project_id.clone() {
                self.service.request("project.open", serde_json::json!({"id": id}));
            }
        }
    }
    /// Go to a stage (0-5). `done`: the stage being left was finished with Next. Tells the service.
    /// Returns the stage it was on, or None when `to` is out of range.
    pub fn go_to(&mut self, to: u8, done: bool) -> Option<u8> {
        let from = self.state.go_to(to, done)?;
        self.service.request("stage.set", serde_json::json!({"stage": to, "key": stage_key(to), "from": from, "done": done}));
        Some(from)
    }
    /// The stage commands' logic (`stages.<what>`): `back`, `next`, `getVideo` (Voice & music
    /// straight to Render), `editVideo` (to FilmCraft), or a stage key (`script` … `render`).
    /// `force` skips the "still need you" question. None = unknown command.
    pub fn navigate(&mut self, what: &str, force: bool) -> Option<Nav> {
        let stage = self.state.stage;
        let ask = |s: &mut MfxState, to_render: bool| -> bool {
            let st = s.status(stage);
            if force || st.kind != StatusKind::NeedsYou {
                return false;
            }
            s.confirm_next = Some(state::NextConfirm { stage, need: st.need, to_render });
            true
        };
        let moved = |m: &mut Self, to: u8, done: bool| m.go_to(to, done).map_or(Nav::Stay, |from| Nav::Moved { from, to });
        Some(match what {
            "back" if stage == 0 => {
                self.state.show_new_project = true;
                Nav::Stay
            }
            "back" => moved(self, stage - 1, false),
            "next" if stage >= STAGE_RENDER => Nav::Stay,
            "next" if ask(&mut self.state, false) => Nav::Asked,
            "next" => moved(self, stage + 1, true),
            "getVideo" if ask(&mut self.state, true) => Nav::Asked,
            "getVideo" => moved(self, STAGE_RENDER, true),
            "editVideo" if ask(&mut self.state, false) => Nav::Asked,
            "editVideo" => moved(self, STAGE_FILMCRAFT, true),
            key => moved(self, stage_by_key(key)?, false),
        })
    }
    /// A context for drawing one part.
    pub fn ctx(&mut self, theme: MfxTheme) -> MfxCtx<'_> {
        MfxCtx::new(&mut self.state, &mut self.service, theme)
    }
}

/// The stage's display name ("Voice & music"), or "" out of range.
pub fn stage_name(stage: u8) -> &'static str {
    STAGES.get(usize::from(stage)).map_or("", |s| s.0)
}

/// The stage's command key ("voiceMusic"), or "" out of range.
pub fn stage_key(stage: u8) -> &'static str {
    STAGES.get(usize::from(stage)).map_or("", |s| s.1)
}

/// The stage with this command key.
pub fn stage_by_key(key: &str) -> Option<u8> {
    STAGES.iter().position(|s| s.1.eq_ignore_ascii_case(key)).and_then(|i| u8::try_from(i).ok())
}

/// PanelKind::MfxStage: the stage's main panel, by stage (Script, Look, Shots, Voice & music, then
/// the Into FilmCraft and Render parts).
pub fn show_stage(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    match ctx.state.stage {
        0 => script::show(ui, ctx),
        1 => look::show(ui, ctx),
        2 => nodes::show_stage(ui, ctx),
        3 => voice_music::show(ui, ctx),
        4 => into_filmcraft::show(ui, ctx),
        _ => export::show(ui, ctx),
    }
}

/// The calm placeholder every part stub draws: the part's name and the doc that specifies it.
pub fn placeholder(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str, text: &str) {
    let rect = ui.max_rect();
    let resp = ui.interact(rect, egui::Id::new(("mfx-placeholder", id)), egui::Sense::hover());
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(13.0), ctx.theme.text_faint);
    ctx.auto(&format!("mfx.{id}"), &resp, text);
}

/// When the service is not connected: one calm line and a Setup link. Returns true when it drew
/// (the caller then draws its offline state below, or nothing).
pub fn service_notice(ui: &mut egui::Ui, ctx: &mut MfxCtx) -> bool {
    if ctx.service.status().is_connected() {
        return false;
    }
    let line = ctx.service.status().line();
    ui.horizontal(|ui| {
        ui.colored_label(ctx.theme.text_dim, line);
        let setup = ui.link("Setup…");
        if setup.clicked() {
            ctx.command("mortiflix.setup", serde_json::json!({}));
        }
        ctx.auto("mfx.service.setup", &setup, "Setup…");
    });
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_keys_round_trip() {
        for i in 0..6u8 {
            assert_eq!(stage_by_key(stage_key(i)), Some(i));
        }
        assert_eq!(stage_by_key("nope"), None);
        assert_eq!(stage_name(9), "");
    }

    #[test]
    fn go_to_tells_the_service_and_rejects_out_of_range() {
        let mut m = Mortiflix::default();
        assert_eq!(m.go_to(2, false), Some(0));
        assert_eq!(m.state.stage, 2);
        assert_eq!(m.go_to(6, false), None);
        assert_eq!(m.state.stage, 2);
        // not started: the stage.set request is answered locally with an error, never left waiting
        assert!(m.service.take_reply(1).is_some_and(|r| !r.ok));
    }

    #[test]
    fn navigation_asks_first_and_get_video_goes_to_render() {
        let mut m = Mortiflix::default();
        assert_eq!(m.navigate("back", false), Some(Nav::Stay));
        assert!(m.state.show_new_project);
        assert_eq!(m.navigate("next", false), Some(Nav::Moved { from: 0, to: 1 }));
        assert_eq!(m.navigate("voiceMusic", false), Some(Nav::Moved { from: 1, to: 3 }));
        m.state.statuses[3] = StageStatus { kind: StatusKind::NeedsYou, need: 2, ..StageStatus::default() };
        assert_eq!(m.navigate("getVideo", false), Some(Nav::Asked));
        assert!(m.state.confirm_next.as_ref().is_some_and(|c| c.to_render && c.need == 2));
        assert_eq!(m.navigate("getVideo", true), Some(Nav::Moved { from: 3, to: STAGE_RENDER }));
        assert_eq!(m.state.status(3).kind, StatusKind::Done);
        assert_eq!(m.navigate("next", false), Some(Nav::Stay));
        assert_eq!(m.navigate("back", false), Some(Nav::Moved { from: 5, to: 4 }));
        assert_eq!(m.navigate("nonsense", false), None);
    }
}
