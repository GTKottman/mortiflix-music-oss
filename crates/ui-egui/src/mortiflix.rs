//! Mortiflix FilmCraft inside FilmCraft's window: the stage bar row, the `Mfx*` dock panels, the
//! Stages menu and `stages.*` / `mortiflix.*` commands, and the stage ⇄ layout switch.
//!
//! Stages 1-4 use the "Mortiflix" workspace (chat left, preview and the stage's main panel, node
//! area along the bottom) in Edit mode; stage 5 is FilmCraft's Edit mode in the user's own editing
//! workspace with the Mortiflix chat docked right; stage 6 is Export mode. Clicking FilmCraft's Export
//! tab lights stage 6 and Edit lights stage 5. The parts themselves live in `filmcraft-mortiflix-ui`.

use craft_ui::docking::Zone;
use filmcraft_mortiflix_ui::{self as mfx, Mortiflix, Nav};
use serde_json::{Value, json};

use crate::FilmcraftApp;
use crate::dock::PanelKind;
use crate::menus::{MenuItem, UiCommand};
use crate::state::Mode;
use crate::theme::Tokens;

/// The workspace stages 1-4 use (`dock::workspace("Mortiflix")`).
pub const WORKSPACE: &str = "Mortiflix";

/// The Mortiflix side of the app plus what the host remembers for it.
#[derive(Default)]
pub struct Host {
    pub core: Mortiflix,
    /// The FilmCraft workspace to return to on stages 5-6.
    pub edit_workspace: String,
}

const fn cmd(id: &'static str, label: &'static str, menu: &'static [&'static str], shortcut: Option<&'static str>) -> UiCommand {
    UiCommand { id, label, menu, shortcut }
}

/// Stages ▸ … and the Mortiflix commands. Ctrl+Alt+Left/Right (doc 30's first pick) are taken in
/// FilmCraft's keymap (Slip Left/Right in the Timeline, Font Size Down/Up), so Back/Next are
/// Ctrl+Alt+Page Up/Page Down (Cmd+Option on a Mac). Rebindable in Keyboard Shortcuts.
pub const COMMANDS: &[UiCommand] = &[
    cmd("stages.back", "Back", &["Stages"], Some("Cmd+Alt+PageUp")),
    cmd("stages.next", "Next", &["Stages"], Some("Cmd+Alt+PageDown")),
    cmd("stages.script", "Script", &["Stages"], Some("Cmd+Alt+1")),
    cmd("stages.look", "Look", &["Stages"], Some("Cmd+Alt+2")),
    cmd("stages.shots", "Shots", &["Stages"], Some("Cmd+Alt+3")),
    cmd("stages.voiceMusic", "Voice & music", &["Stages"], Some("Cmd+Alt+4")),
    cmd("stages.filmcraft", "FilmCraft", &["Stages"], Some("Cmd+Alt+5")),
    cmd("stages.render", "Render", &["Stages"], Some("Cmd+Alt+6")),
    cmd("stages.getVideo", "Get the video", &[], None),
    cmd("stages.editVideo", "Edit the video", &[], None),
    cmd("stages.set", "Go to Stage", &[], None),
    cmd("mortiflix.state", "Mortiflix State", &[], None),
    cmd("mortiflix.newProject", "New Mortiflix Project", &[], None),
    cmd("mortiflix.setup", "Mortiflix Setup", &[], None),
    cmd("mortiflix.request", "Mortiflix Service Request", &[], None),
    cmd("mortiflix.reply", "Mortiflix Service Reply", &[], None),
    cmd("mortiflix.renderVideo", "Render the Video", &[], None),
    cmd("mortiflix.showNeedsYou", "Show What Needs You", &[], None),
    cmd("mortiflix.pickFile", "Mortiflix Pick File", &[], None),
];

/// The Mortiflix panels.
pub fn is_panel(p: PanelKind) -> bool {
    matches!(p, PanelKind::MfxChat | PanelKind::MfxPreview | PanelKind::MfxNodes | PanelKind::MfxStage | PanelKind::MfxAssets)
}

pub fn theme(t: &Tokens) -> mfx::MfxTheme {
    mfx::MfxTheme {
        app_bg: t.app_bg,
        header_bg: t.header_bg,
        panel_bg: t.panel_bg,
        text: t.text,
        text_dim: t.text_dim,
        text_faint: t.text_faint,
        accent: t.accent,
        accent_hover: t.accent_hover,
        hover: t.hover,
        separator: t.separator,
        field_bg: t.field_bg,
        tab_text: t.tab_text,
        tab_text_active: t.tab_text_active,
        ..mfx::MfxTheme::default()
    }
}

/// Height of the stage bar this frame (0 when Mortiflix is off).
pub fn bar_height(app: &FilmcraftApp) -> f32 {
    if app.mfx.core.state.active { mfx::STAGE_BAR_HEIGHT } else { 0.0 }
}

/// FilmCraft's Import / Edit / Export tabs show only on stages 5 and 6 (doc 30).
pub fn hides_mode_tabs(app: &FilmcraftApp) -> bool {
    app.mfx.core.state.active && app.mfx.core.state.stage < mfx::STAGE_FILMCRAFT
}

/// Start of every frame: read the service, keep the stage in step with FilmCraft's mode tabs.
pub fn frame_begin(app: &mut FilmcraftApp, ctx: &egui::Context) {
    app.mfx.core.service.set_waker(ctx);
    app.mfx.core.poll();
    let s = &app.mfx.core.state;
    if !s.active {
        return;
    }
    let follow = match app.ui.mode {
        Mode::Export if s.stage != mfx::STAGE_RENDER => Some("render"),
        Mode::Edit if s.stage == mfx::STAGE_RENDER => Some("filmcraft"),
        _ => None,
    };
    if let Some(key) = follow
        && let Some(Nav::Moved { to, .. }) = app.mfx.core.navigate(key, true)
    {
        apply_stage(app, to);
    }
}

/// Draw the stage bar in `rect`.
pub fn stage_bar(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: egui::Rect) {
    let theme = theme(&app.tokens);
    let mut c = app.mfx.core.ctx(theme);
    mfx::stagebar::show(ui, rect, &mut c);
    let (cmds, autos) = (std::mem::take(&mut c.commands), std::mem::take(&mut c.auto));
    finish(app, ui.ctx(), cmds, autos);
}

/// Draw a Mortiflix dock panel.
pub fn panel(app: &mut FilmcraftApp, ui: &mut egui::Ui, p: PanelKind, rect: egui::Rect) {
    let theme = theme(&app.tokens);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(rect);
    let mut c = app.mfx.core.ctx(theme);
    match p {
        PanelKind::MfxChat => mfx::chat::show(&mut child, &mut c),
        PanelKind::MfxAssets => mfx::chat::show_assets(&mut child, &mut c),
        PanelKind::MfxPreview => mfx::preview::show(&mut child, &mut c),
        PanelKind::MfxNodes => mfx::nodes::show(&mut child, &mut c),
        _ => mfx::show_stage(&mut child, &mut c),
    }
    let (cmds, autos) = (std::mem::take(&mut c.commands), std::mem::take(&mut c.auto));
    finish(app, ui.ctx(), cmds, autos);
}

/// The New project and Setup windows.
pub fn dialogs(app: &mut FilmcraftApp, ctx: &egui::Context) {
    let theme = theme(&app.tokens);
    for which in [0, 1] {
        let (open, title, id) = match which {
            0 => (app.mfx.core.state.show_new_project, "New project", "mfx-new-project"),
            _ => (app.mfx.core.state.show_setup, "Mortiflix Setup", "mfx-setup"),
        };
        if !open {
            continue;
        }
        let mut still_open = true;
        let mut cmds = Vec::new();
        let mut autos = Vec::new();
        egui::Window::new(title).id(egui::Id::new(id)).open(&mut still_open).collapsible(false).default_size([840.0, 560.0]).show(ctx, |ui| {
            ui.set_min_size(egui::vec2(480.0, 320.0));
            let mut c = app.mfx.core.ctx(theme);
            if which == 0 {
                mfx::new_project::show(ui, &mut c)
            } else {
                mfx::setup::show(ui, &mut c)
            }
            cmds = std::mem::take(&mut c.commands);
            autos = std::mem::take(&mut c.auto);
        });
        if !still_open {
            if which == 0 {
                app.mfx.core.state.show_new_project = false;
            } else {
                app.mfx.core.state.show_setup = false;
            }
        }
        finish(app, ctx, cmds, autos);
    }
}

/// Register the parts' automation ids and run the commands they asked for.
fn finish(app: &mut FilmcraftApp, ctx: &egui::Context, cmds: Vec<(String, Value)>, autos: Vec<(String, egui::Response, String)>) {
    for (id, resp, label) in &autos {
        app.auto.add_response(id, resp, label);
    }
    for (id, params) in cmds {
        if let Err(e) = crate::menus::invoke(app, ctx, &id, params) {
            app.ui.status = e;
        }
    }
}

/// Stages ▸ ticks the stage on screen.
pub fn menu(app: &FilmcraftApp, items: &mut [MenuItem]) {
    let key = format!("stages.{}", mfx::stage_key(app.mfx.core.state.stage));
    for it in items.iter_mut().filter(|it| it.path.first().is_some_and(|p| p == "Stages") && it.id != "stages.back" && it.id != "stages.next") {
        it.checked = Some(app.mfx.core.state.active && it.id == key);
    }
}

/// `stages.*` and `mortiflix.*` commands.
pub fn route(app: &mut FilmcraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if let Some(what) = id.strip_prefix("stages.") {
        return Some(stage_command(app, what, params));
    }
    let core = &mut app.mfx.core;
    let r = match id {
        "mortiflix.state" => Ok(json!({"state": core.state, "service": core.service.status(), "workspace": app.ui.workspace})),
        "mortiflix.newProject" => {
            core.state.show_new_project = true;
            Ok(json!({"dialog": "newProject"}))
        }
        "mortiflix.setup" => {
            core.state.show_setup = true;
            Ok(json!({"dialog": "setup"}))
        }
        "mortiflix.request" => {
            let ty = params.get("type").and_then(Value::as_str).filter(|t| !t.is_empty() && t.len() <= 100);
            match ty {
                Some(ty) => Ok(json!({"id": core.service.request(ty, params.get("params").cloned().unwrap_or(json!({})))})),
                None => Err("mortiflix.request needs a `type`".into()),
            }
        }
        "mortiflix.reply" => match params.get("id").and_then(Value::as_u64) {
            Some(rid) => {
                Ok(core.service.take_reply(rid).map_or(json!({"pending": true}), |r| json!({"id": r.id, "ok": r.ok, "result": r.result, "error": r.error})))
            }
            None => Err("mortiflix.reply needs an `id`".into()),
        },
        "mortiflix.renderVideo" => {
            app.ui.status = "Render the video: pick a preset in Export (doc 39)".into();
            Ok(Value::Null)
        }
        "mortiflix.showNeedsYou" => Ok(Value::Null),
        "mortiflix.pickFile" => return Some(pick_file(app, params)),
        _ => return None,
    };
    Some(r)
}

/// `mortiflix.pickFile {filter, exts, request: {type, params}, field}`: the OS file dialog (without
/// stopping the frame loop), then the service request with the chosen path in `params[field]`. A
/// part's Import… / Browse… button (Voice & music: Import a recording). Cancel sends nothing.
fn pick_file(app: &mut FilmcraftApp, params: &Value) -> Result<Value, String> {
    let ty = params
        .pointer("/request/type")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty() && t.len() <= 100)
        .ok_or("mortiflix.pickFile needs request.type")?
        .to_string();
    let mut req = params.pointer("/request/params").cloned().filter(Value::is_object).unwrap_or_else(|| json!({}));
    let field = params.get("field").and_then(Value::as_str).filter(|f| !f.is_empty() && f.len() <= 40).unwrap_or("file").to_string();
    let filter = params.get("filter").and_then(Value::as_str).unwrap_or("Files").chars().take(60).collect::<String>();
    let exts: Vec<String> = params
        .get("exts")
        .and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).take(32).map(|e| e.chars().take(10).collect()).collect())
        .unwrap_or_default();
    let exts: Vec<&str> = exts.iter().map(String::as_str).collect();
    app.pick_ui(crate::native_dialogs::FileDialog::open_file(&filter, &exts), move |app, paths| {
        if let Some(path) = paths.into_iter().next() {
            req[field.as_str()] = json!(path);
            app.mfx.core.service.request(&ty, req);
        }
    });
    Ok(json!({"dialog": "open"}))
}

fn stage_command(app: &mut FilmcraftApp, what: &str, params: &Value) -> Result<Value, String> {
    let force = params.get("force").and_then(Value::as_bool).unwrap_or(false);
    let key = if what == "set" {
        match params.get("stage") {
            Some(Value::Number(n)) => mfx::stage_key(n.as_u64().and_then(|n| u8::try_from(n).ok()).unwrap_or(u8::MAX)).to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        }
    } else {
        what.to_string()
    };
    app.mfx.core.activate();
    let nav = app.mfx.core.navigate(&key, force).ok_or_else(|| format!("unknown stage command `stages.{what}`"))?;
    if let Nav::Moved { to, .. } = nav {
        apply_stage(app, to);
    }
    let s = &app.mfx.core.state;
    Ok(json!({"stage": s.stage, "key": mfx::stage_key(s.stage), "asked": nav == Nav::Asked}))
}

/// Switch FilmCraft's layout and mode for a stage.
pub fn apply_stage(app: &mut FilmcraftApp, stage: u8) {
    if stage < mfx::STAGE_FILMCRAFT {
        if app.ui.workspace != WORKSPACE {
            app.mfx.edit_workspace = app.ui.workspace.clone();
            app.set_workspace(WORKSPACE);
        }
        app.ui.mode = Mode::Edit;
        return;
    }
    if app.ui.workspace == WORKSPACE {
        let back =
            if app.mfx.edit_workspace.is_empty() || app.mfx.edit_workspace == WORKSPACE { "Editing".to_string() } else { app.mfx.edit_workspace.clone() };
        app.set_workspace(&back);
    }
    let docked = app.ui.dock.contains(PanelKind::MfxChat) || app.ui.floating.iter().any(|g| g.panels.contains(&PanelKind::MfxChat));
    if !docked {
        // add it (Window ▸ Mortiflix Chat), then move it to the right edge
        app.show_panel(PanelKind::MfxChat);
        if let Err(e) = app.dock_to_edge(PanelKind::MfxChat, Zone::Right) {
            app.ui.status = e;
        }
    }
    app.ui.mode = if stage >= mfx::STAGE_RENDER { Mode::Export } else { Mode::Edit };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stage_commands_switch_layout_and_mode() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        // off until a stage command (or the desktop app) turns it on: FilmCraft alone is untouched
        assert_eq!(bar_height(&app), 0.0);
        assert!(!hides_mode_tabs(&app));
        crate::menus::invoke(&mut app, &ctx, "stages.shots", json!({})).unwrap();
        assert!(app.mfx.core.state.active);
        assert_eq!(app.ui.workspace, WORKSPACE);
        assert!(app.ui.dock.contains(PanelKind::MfxNodes) && app.ui.dock.contains(PanelKind::MfxChat));
        assert!(hides_mode_tabs(&app));
        crate::menus::invoke(&mut app, &ctx, "stages.editVideo", json!({})).unwrap();
        assert_eq!((app.mfx.core.state.stage, app.ui.mode), (mfx::STAGE_FILMCRAFT, Mode::Edit));
        assert_ne!(app.ui.workspace, WORKSPACE);
        assert!(app.ui.dock.contains(PanelKind::MfxChat) && app.ui.dock.contains(PanelKind::Timeline));
        crate::menus::invoke(&mut app, &ctx, "stages.next", json!({})).unwrap();
        assert_eq!((app.mfx.core.state.stage, app.ui.mode), (mfx::STAGE_RENDER, Mode::Export));
        // FilmCraft's Edit tab lights stage 5 again
        app.ui.mode = Mode::Edit;
        frame_begin(&mut app, &ctx);
        assert_eq!(app.mfx.core.state.stage, mfx::STAGE_FILMCRAFT);
        crate::menus::invoke(&mut app, &ctx, "stages.set", json!({"stage": 0})).unwrap();
        assert_eq!(app.ui.workspace, WORKSPACE);
        assert!(crate::menus::invoke(&mut app, &ctx, "stages.bogus", json!({})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "stages.set", json!({"stage": 99})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "stages.set", json!({"stage": -1})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.request", json!({"type": 5})).is_err());
        let r = crate::menus::invoke(&mut app, &ctx, "mortiflix.request", json!({"type": "state.get"})).unwrap();
        let reply = crate::menus::invoke(&mut app, &ctx, "mortiflix.reply", json!({"id": r["id"]})).unwrap();
        assert_eq!(reply["error"]["code"], "not_running");
    }
}
