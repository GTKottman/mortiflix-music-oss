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
    /// The loose preview's sequence and playback (doc 36, `mortiflix_preview.rs`).
    pub preview: crate::mortiflix_preview::HostPreview,
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
    cmd("mortiflix.pickFiles", "Choose Files for Mortiflix", &[], None),
    cmd("mortiflix.reveal", "Show in Folder", &[], None),
    // The node area (part 35): its keys are panel-scoped ("Mortiflix Nodes") in the engine's
    // FILMCRAFT_PANEL table, so ←/→ walk the film there and step frames everywhere else.
    cmd("nodes.prev", "Previous Node", &[], None),
    cmd("nodes.next", "Next Node", &[], None),
    cmd("nodes.up", "Node Above", &[], None),
    cmd("nodes.down", "Node Below", &[], None),
    cmd("nodes.enter", "OK and On to the Next Shot", &[], None),
    cmd("nodes.fixKey", "Fix the Selected Node", &[], None),
    cmd("nodes.delete", "Cut It", &[], None),
    cmd("nodes.frameSelected", "Frame Selected Nodes", &[], None),
    cmd("nodes.frameAll", "Frame All Nodes", &[], None),
    cmd("nodes.zoomIn", "Zoom In on the Nodes", &[], None),
    cmd("nodes.zoomOut", "Zoom Out of the Nodes", &[], None),
    cmd("nodes.duplicate", "Duplicate Shot", &[], None),
    cmd("nodes.find", "Find a Shot", &[], None),
    cmd("nodes.selectAll", "Select All Shots", &[], None),
    cmd("nodes.escape", "Clear Search or Deselect", &[], None),
    cmd("nodes.undo", "Undo in the Node Area", &[], None),
    cmd("nodes.redo", "Redo in the Node Area", &[], None),
    cmd("nodes.addShot", "Add a Shot", &[], None),
    cmd("nodes.frameBeat1", "Frame Beat 1", &[], None),
    cmd("nodes.frameBeat2", "Frame Beat 2", &[], None),
    cmd("nodes.frameBeat3", "Frame Beat 3", &[], None),
    cmd("nodes.frameBeat4", "Frame Beat 4", &[], None),
    cmd("nodes.frameBeat5", "Frame Beat 5", &[], None),
    cmd("nodes.frameBeat6", "Frame Beat 6", &[], None),
    cmd("nodes.frameBeat7", "Frame Beat 7", &[], None),
    cmd("nodes.frameBeat8", "Frame Beat 8", &[], None),
    cmd("nodes.frameBeat9", "Frame Beat 9", &[], None),
    cmd("nodes.select", "Select a Node", &[], None),
    cmd("nodes.ok", "OK", &[], None),
    cmd("nodes.fix", "Fix", &[], None),
    cmd("nodes.cut", "Cut It", &[], None),
    cmd("nodes.bringBack", "Bring It Back", &[], None),
    cmd("nodes.move", "Move Nodes", &[], None),
    cmd("nodes.reorder", "Move a Shot in the Film", &[], None),
    cmd("nodes.moveInFilm", "Move a Shot Earlier or Later", &[], None),
    cmd("nodes.pickTransition", "Pick a Transition", &[], None),
    cmd("nodes.add", "New Shot", &[], None),
    cmd("nodes.useVersion", "Use This Version", &[], None),
    cmd("nodes.note", "Add a Note", &[], None),
    cmd("nodes.use", "Use in Shot", &[], None),
    cmd("nodes.play", "Play the Node", &[], None),
    cmd("nodes.show", "Show in the Node Area", &[], None),
    cmd("nodes.highlight", "Highlight Nodes", &[], None),
    cmd("nodes.wheel", "Mouse Wheel in the Node Area", &[], None),
    cmd("nodes.zoomTo", "Zoom the Nodes To", &[], None),
    cmd("nodes.tidy", "Put Nodes Back in Film Order", &[], None),
    cmd("nodes.refresh", "Reload the Nodes", &[], None),
    cmd("nodes.state", "Node Area State", &[], None),
];

/// What `mortiflix.pickFiles` offers: the kinds of files a project's assets can be (doc 32a).
const ASSET_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "svg", "heic", "tif", "tiff", "bmp", "mp4", "mov", "mkv", "webm", "avi", "m4v", "wav", "mp3", "m4a", "aac", "flac",
    "ogg", "opus", "aif", "aiff", "ttf", "otf", "woff", "woff2", "pdf", "docx", "txt", "md", "ase", "aco", "json",
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
    crate::mortiflix_preview::frame(app, ctx);
    let s = &app.mfx.core.state;
    if !s.active {
        return;
    }
    if s.stage < mfx::STAGE_FILMCRAFT && app.ui.mode == Mode::Edit {
        if app.ui.workspace != WORKSPACE {
            apply_stage(app, app.mfx.core.state.stage);
        }
        keep_mortiflix_only(app);
    }
    let s = &app.mfx.core.state;
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
    if p == PanelKind::MfxPreview {
        crate::mortiflix_preview::picture(app, ui, rect);
    }
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
        // New project: the title is a breadcrumb of its page and it sizes to the page (doc 31)
        let title = if which == 0 { mfx::new_project::window_title(ctx) } else { title.to_string() };
        let mut window = egui::Window::new(title).id(egui::Id::new(id)).open(&mut still_open).collapsible(false).default_size([840.0, 560.0]);
        if which == 0 {
            window = window
                .id(egui::Id::new((id, mfx::new_project::window_key(ctx))))
                .resizable(false)
                .pivot(egui::Align2::CENTER_TOP)
                .default_pos(ctx.content_rect().center_top() + egui::vec2(0.0, 90.0));
        }
        window.show(ctx, |ui| {
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
pub fn route(app: &mut FilmcraftApp, ctx: &egui::Context, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if let Some(what) = id.strip_prefix("mortiflix.preview.") {
        return Some(crate::mortiflix_preview::command(app, what, params));
    }
    if let Some(what) = id.strip_prefix("stages.") {
        return Some(stage_command(app, what, params));
    }
    match id {
        // An OS file dialog for a Mortiflix part; the chosen files wait in `state.files_in` under
        // `purpose` until that part takes them. `paths` skips the dialog (agents, tests).
        "mortiflix.pickFiles" => return Some(pick_files(app, params)),
        "mortiflix.reveal" => return Some(reveal(app, ctx, params)),
        _ => {}
    }
    if let Some(what) = id.strip_prefix("nodes.") {
        return Some(mfx::nodes::command(&mut app.mfx.core, what, params));
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
        _ => return None,
    };
    Some(r)
}

/// `mortiflix.pickFiles {purpose, exts?, folder?, multiple?, paths?}`: an OS file (or folder)
/// dialog for a Mortiflix part; the chosen paths wait in `state.files_in` under `purpose` until that
/// part takes them. `exts` defaults to every kind of asset (doc 32a). With `paths` (agents, tests) no
/// dialog opens and those paths are delivered as if chosen.
fn pick_files(app: &mut FilmcraftApp, params: &Value) -> Result<Value, String> {
    let purpose =
        params.get("purpose").and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 200).ok_or("mortiflix.pickFiles needs a `purpose`")?.to_string();
    if let Some(list) = params.get("paths").and_then(Value::as_array) {
        let paths: Vec<String> = list.iter().take(500).filter_map(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 4096).map(str::to_string).collect();
        let n = paths.len();
        app.mfx.core.state.push_files(&purpose, paths);
        return Ok(json!({"purpose": purpose, "paths": n}));
    }
    let dialog = if params.get("folder").and_then(Value::as_bool).unwrap_or(false) {
        crate::native_dialogs::FileDialog::Folder { at: None }
    } else {
        let asked: Vec<String> = params
            .get("exts")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).filter(|e| !e.is_empty() && e.len() <= 10).take(64).map(str::to_string).collect())
            .unwrap_or_default();
        let exts = if asked.is_empty() { ASSET_EXTS.iter().map(|e| (*e).to_string()).collect() } else { asked };
        if params.get("multiple").and_then(Value::as_bool).unwrap_or(true) {
            crate::native_dialogs::FileDialog::OpenFiles { exts }
        } else {
            crate::native_dialogs::FileDialog::OpenFile { filter: "Files".into(), exts }
        }
    };
    app.pick_ui(dialog, move |app, paths| app.mfx.core.state.push_files(&purpose, paths));
    Ok(json!({"dialog": "open"}))
}

/// Show a file in the OS file manager (`open: true` opens it in its own app instead).
fn reveal(app: &mut FilmcraftApp, ctx: &egui::Context, params: &Value) -> Result<Value, String> {
    let path = params.get("path").and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 4096).ok_or("mortiflix.reveal needs a `path`")?;
    let open = params.get("open").and_then(Value::as_bool).unwrap_or(false);
    crate::panels::menu_dialogs::open_path(app, ctx, path, !open)?;
    Ok(json!({"path": path}))
}

/// Files dropped on the window during stages 1-4 (or with the New project window open) go to the
/// Mortiflix parts (chat, Assets, New project) instead of FilmCraft's Project panel.
pub fn take_drop(app: &mut FilmcraftApp, paths: &[String]) -> bool {
    let s = &app.mfx.core.state;
    if !(s.show_new_project || (s.active && s.stage < mfx::STAGE_FILMCRAFT)) {
        return false;
    }
    app.mfx.core.state.push_files("drop", paths.to_vec());
    true
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

/// Stages 1-4 show only the Mortiflix panels (owner, 2026-10-16). A FilmCraft panel that lands in
/// the Mortiflix workspace (Window ▸ Timeline, Shift+3, a sequence opened, a saved layout) is closed
/// again: FilmCraft's own panels are on stage 5 FilmCraft. Returns what it closed.
pub fn keep_mortiflix_only(app: &mut FilmcraftApp) -> Vec<PanelKind> {
    if app.ui.workspace != WORKSPACE {
        return Vec::new();
    }
    let mut panels = Vec::new();
    app.ui.dock.panels(&mut panels);
    panels.extend(app.ui.floating.iter().flat_map(|g| g.panels.iter().copied()));
    let extra: Vec<PanelKind> = panels.into_iter().filter(|p| !is_panel(*p)).collect();
    for p in &extra {
        if let Err(e) = app.apply_dock(craft_ui::docking::Action::Close { panel: *p }) {
            log::warn!("closing {} on a Mortiflix stage: {e}", p.title());
        }
    }
    if !extra.is_empty() {
        app.ui.status = "FilmCraft's own panels open on stage 5 FilmCraft.".into();
    }
    extra
}

/// Switch FilmCraft's layout and mode for a stage.
pub fn apply_stage(app: &mut FilmcraftApp, stage: u8) {
    if stage < mfx::STAGE_FILMCRAFT {
        if app.ui.workspace != WORKSPACE {
            app.mfx.edit_workspace = app.ui.workspace.clone();
            app.set_workspace(WORKSPACE);
        }
        app.ui.mode = Mode::Edit;
        keep_mortiflix_only(app);
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
    fn pick_files_delivers_paths_to_the_part_and_refuses_bad_params() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        let r = crate::menus::invoke(&mut app, &ctx, "mortiflix.pickFiles", json!({"purpose": "newProject.assets", "paths": ["/a.png", 5, ""]})).unwrap();
        assert_eq!(r["paths"], 1);
        assert_eq!(app.mfx.core.state.take_paths("newProject.assets"), vec!["/a.png".to_string()]);
        assert!(app.mfx.core.state.take_paths("newProject.assets").is_empty());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.pickFiles", json!({})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.pickFiles", json!({"purpose": 7})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.pickFiles", json!({"purpose": "x".repeat(201)})).is_err());
        // no host dialog here: nothing is picked, nothing breaks
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.pickFiles", json!({"purpose": "k", "exts": [1, "png"], "folder": "yes"})).is_ok());
        assert!(app.mfx.core.state.take_paths("k").is_empty());
    }

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
        // stages 1-4 show only the Mortiflix panels: Window ▸ Timeline (Shift+3) doesn't stay there
        assert!(!app.ui.dock.contains(PanelKind::Timeline));
        crate::menus::invoke(&mut app, &ctx, "window.panel.Timeline", json!({})).unwrap();
        frame_begin(&mut app, &ctx);
        assert!(!app.ui.dock.contains(PanelKind::Timeline));
        assert!(app.ui.dock.contains(PanelKind::MfxChat) && app.ui.dock.contains(PanelKind::MfxAssets));
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
        // files for a part: chosen paths wait under their purpose; hostile params are refused
        crate::menus::invoke(&mut app, &ctx, "mortiflix.pickFiles", json!({"purpose": "chat.attach", "paths": ["/a/b.png", 7]})).unwrap();
        assert_eq!(app.mfx.core.state.take_files("chat.attach")[0].paths, ["/a/b.png"]);
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.pickFiles", json!({})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.reveal", json!({"path": ""})).is_err());
        // drops go to Mortiflix on stages 1-4 only
        assert!(take_drop(&mut app, &["/x.png".into()]));
        crate::menus::invoke(&mut app, &ctx, "stages.filmcraft", json!({})).unwrap();
        assert!(!take_drop(&mut app, &["/y.png".into()]), "stage 5: FilmCraft imports it");
        let r = crate::menus::invoke(&mut app, &ctx, "mortiflix.request", json!({"type": "state.get"})).unwrap();
        let reply = crate::menus::invoke(&mut app, &ctx, "mortiflix.reply", json!({"id": r["id"]})).unwrap();
        assert_eq!(reply["error"]["code"], "not_running");
    }
}
