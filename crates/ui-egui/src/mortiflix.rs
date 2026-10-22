//! Mortiflix Studio inside Mortiflix Studio's window: the stage bar row, the `Mfx*` dock panels, the
//! Stages menu and `stages.*` / `mortiflix.*` commands, and the stage ⇄ layout switch.
//!
//! Stages 1-4 use the "Mortiflix" workspace (chat left, preview and the stage's main panel, node
//! area along the bottom) in Edit mode; stage 5 is Mortiflix Studio's Edit mode in the user's own editing
//! workspace with the Mortiflix chat docked right; stage 6 is Export mode. Clicking Mortiflix Studio's Export
//! tab lights stage 6 and Edit lights stage 5. The parts themselves live in `mortiflix-ui`.

use craft_ui::docking::Zone;
use mortiflix_ui::{self as mfx, Mortiflix, Nav};
use serde_json::{Value, json};

use crate::StudioApp;
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
    /// The Mortiflix Studio workspace to return to on stages 5-6.
    pub edit_workspace: String,
    /// Stage 6 Render: the Export page (part 39, `mortiflix_export.rs`).
    pub export: crate::mortiflix_export::ExportHost,
    /// The OS window title last sent (sent again only when it changes).
    pub window_title: String,
}

/// The document title the header shows (and the OS window gets) while a Mortiflix project is open:
/// "<project title> – Mortiflix Studio", not Mortiflix Studio's own project name ("Mortiflix Studio Demo").
/// None = no Mortiflix project: Mortiflix Studio's own title.
pub fn doc_title(app: &StudioApp) -> Option<String> {
    let s = &app.mfx.core.state;
    if !s.active || s.project_id.is_none() {
        return None;
    }
    let name = s.project_title.trim();
    let app_name = mfx::app_name();
    Some(if name.is_empty() { app_name.to_string() } else { format!("{name} \u{2013} {app_name}") })
}

/// The OS window title follows the header's (Mortiflix Studio's unsaved mark kept).
fn window_title(app: &mut StudioApp, ctx: &egui::Context) {
    let base = doc_title(app).unwrap_or_else(|| mfx::app_name().to_string());
    let title = if app.session.is_dirty() { tlf!("{name} - Edited", name = base) } else { base };
    if title != app.mfx.window_title {
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
        app.mfx.window_title = title;
    }
}

const fn cmd(id: &'static str, label: &'static str, menu: &'static [&'static str], shortcut: Option<&'static str>) -> UiCommand {
    UiCommand { id, label, menu, shortcut }
}

/// Stages ▸ … and the Mortiflix commands. Ctrl+Alt+Left/Right (doc 30's first pick) are taken in
/// Mortiflix Studio's keymap (Slip Left/Right in the Timeline, Font Size Down/Up), so Back/Next are
/// Ctrl+Alt+Page Up/Page Down (Cmd+Option on a Mac). Rebindable in Keyboard Shortcuts.
pub const COMMANDS: &[UiCommand] = &[
    cmd("stages.back", "Back", &["Stages"], Some("Cmd+Alt+PageUp")),
    cmd("stages.next", "Next", &["Stages"], Some("Cmd+Alt+PageDown")),
    cmd("stages.script", "Script", &["Stages"], Some("Cmd+Alt+1")),
    cmd("stages.voice", "Voice", &["Stages"], Some("Cmd+Alt+2")),
    cmd("stages.styleFrames", "Style frames", &["Stages"], Some("Cmd+Alt+3")),
    cmd("stages.transitions", "Transitions", &["Stages"], Some("Cmd+Alt+4")),
    cmd("stages.animatic", "Animatic", &["Stages"], Some("Cmd+Alt+5")),
    cmd("stages.music", "Music", &["Stages"], Some("Cmd+Alt+6")),
    cmd("stages.edit", "Edit", &["Stages"], Some("Cmd+Alt+7")),
    cmd("stages.render", "Render", &["Stages"], Some("Cmd+Alt+8")),
    cmd("stages.getVideo", "Get the video", &[], None),
    cmd("stages.editVideo", "Edit the video", &[], None),
    cmd("stages.set", "Go to Stage", &[], None),
    cmd("mortiflix.state", "Mortiflix State", &[], None),
    cmd("mortiflix.newProject", "New Mortiflix Project", &[], None),
    cmd("mortiflix.setup", "Mortiflix…", &["Edit", "Preferences"], None),
    cmd("mortiflix.request", "Mortiflix Service Request", &[], None),
    cmd("mortiflix.reply", "Mortiflix Service Reply", &[], None),
    cmd("mortiflix.renderVideo", "Render the Video", &[], None),
    cmd("mortiflix.showNeedsYou", "Show What Needs You", &[], None),
    cmd("mortiflix.chooseFiles", "Choose Files for Mortiflix", &[], None),
    cmd("mortiflix.chooseFileAndSend", "Choose a File and Send It to Mortiflix", &[], None),
    cmd("mortiflix.reveal", "Show in Folder", &[], None),
    // The Script panel (stage 1): Ctrl/Cmd+Z there brings back the newest deleted line, saved or not
    // (panel-scoped "Mortiflix Stage" in the engine's MFX_STUDIO_PANEL table; a text field keeps its own).
    cmd("script.undo", "Undo in the Script", &[], None),
    // The node area (part 35): its keys are panel-scoped ("Mortiflix Nodes") in the engine's
    // MFX_STUDIO_PANEL table, so ←/→ walk the film there and step frames everywhere else.
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
    cmd("nodes.frameBeat1", "Frame Scene 1", &[], None),
    cmd("nodes.frameBeat2", "Frame Scene 2", &[], None),
    cmd("nodes.frameBeat3", "Frame Scene 3", &[], None),
    cmd("nodes.frameBeat4", "Frame Scene 4", &[], None),
    cmd("nodes.frameBeat5", "Frame Scene 5", &[], None),
    cmd("nodes.frameBeat6", "Frame Scene 6", &[], None),
    cmd("nodes.frameBeat7", "Frame Scene 7", &[], None),
    cmd("nodes.frameBeat8", "Frame Scene 8", &[], None),
    cmd("nodes.frameBeat9", "Frame Scene 9", &[], None),
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
    // Into Edit (doc 38): crate::mortiflix_into
    cmd("mortiflix.importEdit", "Bring the Video into Edit", &[], None),
    cmd("mortiflix.import.run", "Run the Mortiflix Studio Import", &[], None),
    cmd("mortiflix.restartEdit", "Start the Edit Again from Mortiflix…", &["File"], None),
    cmd("mortiflix.useVersion", "Use This Version", &[], None),
    cmd("mortiflix.timelineEdit", "Mortiflix Timeline Edit", &[], None),
];

/// What `mortiflix.chooseFiles` offers: the kinds of files a project's assets can be (doc 32a).
const ASSET_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "svg", "heic", "tif", "tiff", "bmp", "mp4", "mov", "mkv", "webm", "avi", "m4v", "wav", "mp3", "m4a", "aac", "flac",
    "ogg", "opus", "aif", "aiff", "ttf", "otf", "woff", "woff2", "pdf", "docx", "txt", "md", "ase", "aco", "json",
];

/// The Mortiflix panels.
pub fn is_panel(p: PanelKind) -> bool {
    matches!(p, PanelKind::MfxChat | PanelKind::MfxPreview | PanelKind::MfxNodes | PanelKind::MfxStage | PanelKind::MfxAssets | PanelKind::MfxTokens)
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
pub fn bar_height(app: &StudioApp) -> f32 {
    if app.mfx.core.state.active { mfx::STAGE_BAR_HEIGHT } else { 0.0 }
}

/// The stages before Edit own the layout (doc 30). The editor's tabs, menus and shortcuts follow
/// `menus::editor_hidden` (#16).
pub fn hides_mode_tabs(app: &StudioApp) -> bool {
    app.mfx.core.state.active && app.mfx.core.state.stage < mfx::edit_stage()
}

/// Start of every frame: read the service, keep the stage in step with Mortiflix Studio's mode tabs.
pub fn frame_begin(app: &mut StudioApp, ctx: &egui::Context) {
    app.mfx.core.service.set_waker(ctx);
    app.mfx.core.poll();
    // Ctrl/Cmd+S saves unsaved script changes first (Mortiflix Studio's own Save is for its project).
    if app.mfx.core.state.script.unsaved() && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::S)) {
        let core = &mut app.mfx.core;
        mfx::script::save(&mut core.state.script, &mut core.service);
    }
    script_go_on(app, ctx);
    window_title(app, ctx);
    crate::mortiflix_preview::frame(app, ctx);
    crate::mortiflix_into::frame(app, ctx);
    crate::mortiflix_export::frame(app, ctx);
    let s = &app.mfx.core.state;
    if !s.active {
        return;
    }
    if s.stage < mfx::edit_stage() && app.ui.mode == Mode::Edit {
        if app.ui.workspace != WORKSPACE {
            apply_stage(app, app.mfx.core.state.stage);
        }
        keep_mortiflix_only(app);
    }
    let s = &app.mfx.core.state;
    let follow = match app.ui.mode {
        Mode::Export if s.stage != mfx::render_stage() => Some("render"),
        Mode::Edit if s.stage == mfx::render_stage() => Some("edit"),
        _ => None,
    };
    if let Some(key) = follow
        && let Some(Nav::Moved { to, .. }) = app.mfx.core.navigate(key, true)
    {
        apply_stage(app, to);
    }
}

/// "Save your script changes?" was answered (saved and in, or discarded): carry on with what asked.
fn script_go_on(app: &mut StudioApp, ctx: &egui::Context) {
    use mfx::script::Leave;
    let core = &mut app.mfx.core;
    // the New project window makes its project itself (dropped if the window was closed meanwhile)
    if matches!(core.state.script.go, Some(Leave::NewProject)) {
        if !core.state.show_new_project {
            core.state.script.go = None;
        }
        return;
    }
    let Some(go) = core.state.script.take_go(None) else { return };
    let r = match go {
        Leave::Stage { what } => crate::menus::invoke(app, ctx, &format!("stages.{what}"), json!({})).map(|_| ()),
        Leave::Command { id, params } => crate::menus::invoke(app, ctx, &id, params).map(|_| ()),
        Leave::Request { ty, params } => {
            app.mfx.core.service.request(&ty, params);
            Ok(())
        }
        Leave::Quit => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            Ok(())
        }
        Leave::NewProject => Ok(()),
    };
    if let Err(e) = r {
        app.ui.status = e;
    }
}

/// Quitting (the window's close request) with unsaved script changes asks first: Save / Discard /
/// Keep editing. Returns whether the close must be cancelled. Mortiflix Studio's own "Save changes to …?"
/// comes after it, on the next close request.
pub fn intercept_quit(app: &mut StudioApp) -> bool {
    let s = &mut app.mfx.core.state;
    !app.quit_confirmed && s.active && s.script.ask(mfx::script::Leave::Quit)
}

/// Commands that open or close a project. With unsaved script changes they ask first and run again
/// once answered (doc 30: nothing typed is lost).
fn leaves_project(id: &str, params: &Value) -> Option<mfx::script::Leave> {
    use mfx::script::Leave;
    if matches!(id, "file.closeProject" | "file.open") || id.starts_with("file.openRecent.") {
        return Some(Leave::Command { id: id.to_string(), params: params.clone() });
    }
    if id == "mortiflix.request" {
        let ty = params.get("type").and_then(Value::as_str).filter(|t| matches!(*t, "project.open" | "project.new" | "project.close"))?;
        return Some(Leave::Request { ty: ty.to_string(), params: params.get("params").cloned().unwrap_or(json!({})) });
    }
    None
}

/// Draw the stage bar in `rect`.
pub fn stage_bar(app: &mut StudioApp, ui: &mut egui::Ui, rect: egui::Rect) {
    let theme = theme(&app.tokens);
    let mut c = app.mfx.core.ctx(theme);
    mfx::stagebar::show(ui, rect, &mut c);
    let (cmds, autos) = (std::mem::take(&mut c.commands), std::mem::take(&mut c.auto));
    finish(app, ui.ctx(), cmds, autos);
}

/// Draw Home's packs (by category) in `rect`.
pub fn home_packs(app: &mut StudioApp, ui: &mut egui::Ui, rect: egui::Rect) {
    let theme = theme(&app.tokens);
    let mut c = app.mfx.core.ctx(theme);
    mfx::home::show(ui, &mut c, rect);
    let (cmds, autos) = (std::mem::take(&mut c.commands), std::mem::take(&mut c.auto));
    finish(app, ui.ctx(), cmds, autos);
}

/// Draw a Mortiflix dock panel.
pub fn panel(app: &mut StudioApp, ui: &mut egui::Ui, p: PanelKind, rect: egui::Rect) {
    let theme = theme(&app.tokens);
    // Script, Voice and Style frames have no video yet: their own work is centre stage (owner, 2026-10-18)
    let own = mfx::own_centre(&app.mfx.core.state);
    if p == PanelKind::MfxPreview && !own {
        crate::mortiflix_preview::picture(app, ui, rect);
    }
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(8.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(rect);
    let mut c = app.mfx.core.ctx(theme);
    match p {
        PanelKind::MfxChat => mfx::chat::show(&mut child, &mut c),
        PanelKind::MfxAssets => mfx::chat::show_assets(&mut child, &mut c),
        PanelKind::MfxTokens => mfx::tokens::show(&mut child, &mut c),
        PanelKind::MfxPreview => {
            mfx::show_centre(&mut child, &mut c);
        }
        PanelKind::MfxNodes => mfx::nodes::show(&mut child, &mut c),
        _ => mfx::show_stage(&mut child, &mut c),
    }
    let (cmds, autos) = (std::mem::take(&mut c.commands), std::mem::take(&mut c.auto));
    finish(app, ui.ctx(), cmds, autos);
}

/// The Mortiflix panels' tab names follow what they show: the stage panel is the stage's gate, and the
/// centre panel carries the stage's name where it shows the stage's own work instead of the preview.
pub fn tab_titles(app: &StudioApp) -> Vec<(PanelKind, String)> {
    let s = &app.mfx.core.state;
    if !s.active || s.stage >= mfx::edit_stage() {
        return Vec::new();
    }
    let name = mfx::stage_name(s.stage);
    let mut titles = vec![(PanelKind::MfxStage, format!("{name} gate"))];
    if mfx::own_centre(s) {
        titles.push((PanelKind::MfxPreview, format!("{} {name}", mfx::branded("Mortiflix"))));
    }
    titles
}

/// Whisper (captions) and Kokoro (natural voices), from Mortiflix Studio's own catalogues, for the Setup
/// window's Tools step (it asks before downloading them into `<data dir>/models`).
pub fn model_packs() -> Vec<mfx::setup::ModelPack> {
    use mfx::setup::{ModelPack, PackFile};
    use mortiflix_speech::models as speech;
    use mortiflix_tts::catalog as kokoro;
    let Some(dir) = mortiflix_engine::transcript::models_dir() else { return Vec::new() };
    let mut packs = Vec::new();
    if let Some(m) = speech::find(speech::DEFAULT_MODEL) {
        packs.push(ModelPack {
            id: m.id.into(),
            name: "Whisper".into(),
            what: "captions and word timing".into(),
            license: m.license.into(),
            source: m.source.into(),
            dir: speech::model_dir(&dir, m),
            usable: mortiflix_engine::transcript::speech_available(),
            files: m.files.iter().map(|f| PackFile { name: f.name.into(), url: f.url.into(), sha256: f.sha256.into(), size: f.size }).collect(),
        });
    }
    let first = kokoro::VOICES.first().map_or("", |v| v.info.id);
    packs.push(ModelPack {
        id: kokoro::PACKAGE_ID.into(),
        name: "Natural voices".into(),
        what: "Kokoro narration voices".into(),
        license: kokoro::LICENSE.into(),
        source: kokoro::SOURCE.into(),
        dir: kokoro::package_dir(&dir),
        usable: !matches!(mortiflix_tts::voice_in(first, None), Err(mortiflix_tts::TtsError::Unavailable)),
        files: kokoro::FILES.iter().map(|f| PackFile { name: f.name.into(), url: f.url.into(), sha256: f.sha256.into(), size: f.size }).collect(),
    });
    packs
}

/// The New project and Setup windows.
pub fn dialogs(app: &mut StudioApp, ctx: &egui::Context) {
    let theme = theme(&app.tokens);
    if app.mfx.core.state.show_setup && !mfx::setup::has_model_packs(ctx) {
        mfx::setup::set_model_packs(ctx, model_packs());
    }
    for which in [0, 1] {
        let (open, id) = match which {
            0 => (app.mfx.core.state.show_new_project, "mfx-new-project"),
            _ => (app.mfx.core.state.show_setup, "mfx-setup"),
        };
        if !open {
            continue;
        }
        let mut still_open = true;
        let mut cmds = Vec::new();
        let mut autos = Vec::new();
        // New project: the title is a breadcrumb of its page and it sizes to the page (doc 31)
        let title = if which == 0 { mfx::new_project::window_title(ctx) } else { mfx::setup::window_title(ctx).to_string() };
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
fn finish(app: &mut StudioApp, ctx: &egui::Context, cmds: Vec<(String, Value)>, autos: Vec<(String, egui::Response, String)>) {
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
pub fn menu(app: &StudioApp, items: &mut [MenuItem]) {
    let key = format!("stages.{}", mfx::stage_key(app.mfx.core.state.stage));
    for it in items.iter_mut().filter(|it| it.path.first().is_some_and(|p| p == "Stages") && it.id != "stages.back" && it.id != "stages.next") {
        it.checked = Some(app.mfx.core.state.active && it.id == key);
    }
}

/// `stages.*` and `mortiflix.*` commands.
pub fn route(app: &mut StudioApp, ctx: &egui::Context, id: &str, params: &Value) -> Option<Result<Value, String>> {
    if app.mfx.core.state.active
        && let Some(leave) = leaves_project(id, params)
        && app.mfx.core.state.script.ask(leave)
    {
        return Some(Ok(json!({"asked": "script"})));
    }
    // File ▸ New ▸ Project… (Cmd/Ctrl+Alt+N) is Mortiflix's New project window here: the editor's own command
    // only emptied the hidden edit project, so nothing seemed to happen (owner, 2026-10-18). With a `name`
    // (Bring the video into Edit, agents) it stays the editor's command.
    if app.mfx.core.state.active && id == "file.newProject" && params.get("name").is_none() {
        app.mfx.core.state.show_new_project = true;
        return Some(Ok(json!({"dialog": "newProject"})));
    }
    if let Some(what) = id.strip_prefix("mortiflix.preview.") {
        return Some(crate::mortiflix_preview::command(app, what, params));
    }
    if let Some(what) = id.strip_prefix("stages.") {
        return Some(stage_command(app, what, params));
    }
    match id {
        // An OS file dialog for a Mortiflix part; the chosen files wait in `state.files_in` under
        // `purpose` until that part takes them. `paths` skips the dialog (agents, tests).
        "mortiflix.chooseFiles" => return Some(choose_files(app, params)),
        "mortiflix.reveal" => return Some(reveal(app, ctx, params)),
        _ => {}
    }
    if let Some(what) = id.strip_prefix("nodes.") {
        return Some(mfx::nodes::command(&mut app.mfx.core, what, params));
    }
    if id == "script.undo" {
        let core = &mut app.mfx.core;
        let undone = core.state.stage == 0 && core.state.script.undo_last(&mut core.service);
        return Some(Ok(json!({"undone": undone})));
    }
    if let Some(r) = crate::mortiflix_into::route(app, id, params) {
        return Some(r);
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
            // {step: 1 | "claude" | "keys" | "tools" | "narration"}: open on that step (Link Claude Code is step 1)
            core.state.setup_page = match params.get("step") {
                Some(Value::Number(n)) => {
                    n.as_u64().and_then(|n| ["claude", "keys", "tools", "narration"].get(usize::try_from(n.saturating_sub(1)).ok()?)).map(|s| (*s).to_string())
                }
                Some(Value::String(s)) => Some(s.chars().take(20).collect()),
                _ => None,
            };
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
        "mortiflix.renderVideo" => crate::mortiflix_export::route(app, params),
        "mortiflix.showNeedsYou" => Ok(Value::Null),
        "mortiflix.chooseFileAndSend" => return Some(choose_file_and_send(app, params)),
        _ => return None,
    };
    Some(r)
}

/// `mortiflix.chooseFiles {purpose, exts?, folder?, multiple?, paths?}`: an OS file (or folder)
/// dialog for a Mortiflix part; the chosen paths wait in `state.files_in` under `purpose` until that
/// part takes them. `exts` defaults to every kind of asset (doc 32a). With `paths` (agents, tests) no
/// dialog opens and those paths are delivered as if chosen.
fn choose_files(app: &mut StudioApp, params: &Value) -> Result<Value, String> {
    let purpose =
        params.get("purpose").and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 200).ok_or("mortiflix.chooseFiles needs a `purpose`")?.to_string();
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
fn reveal(app: &mut StudioApp, ctx: &egui::Context, params: &Value) -> Result<Value, String> {
    let path = params.get("path").and_then(Value::as_str).filter(|p| !p.is_empty() && p.len() <= 4096).ok_or("mortiflix.reveal needs a `path`")?;
    let open = params.get("open").and_then(Value::as_bool).unwrap_or(false);
    crate::panels::menu_dialogs::open_path(app, ctx, path, !open)?;
    Ok(json!({"path": path}))
}

/// Files dropped on the window during stages 1-4 (or with the New project window open) go to the
/// Mortiflix parts (chat, Assets, New project) instead of Mortiflix Studio's Project panel.
pub fn take_drop(app: &mut StudioApp, paths: &[String]) -> bool {
    let s = &app.mfx.core.state;
    if !(s.show_new_project || (s.active && s.stage < mfx::edit_stage())) {
        return false;
    }
    app.mfx.core.state.push_files("drop", paths.to_vec());
    true
}

/// `mortiflix.chooseFileAndSend {filter, exts, request: {type, params}, field}`: the OS file dialog (without
/// stopping the frame loop), then the service request with the chosen path in `params[field]`. A
/// part's Import… / Browse… button (Voice & music: Import a recording). Cancel sends nothing.
fn choose_file_and_send(app: &mut StudioApp, params: &Value) -> Result<Value, String> {
    let ty = params
        .pointer("/request/type")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty() && t.len() <= 100)
        .ok_or("mortiflix.chooseFileAndSend needs request.type")?
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
    // with `paths` (agents, tests; like mortiflix.chooseFiles) no dialog opens: the first path is
    // sent as if chosen
    if let Some(path) =
        params.get("paths").and_then(Value::as_array).and_then(|a| a.iter().filter_map(Value::as_str).find(|p| !p.is_empty() && p.len() <= 4096))
    {
        req[field.as_str()] = json!(path);
        let id = app.mfx.core.service.request(&ty, req);
        return Ok(json!({"sent": id}));
    }
    app.pick_ui(crate::native_dialogs::FileDialog::open_file(&filter, &exts), move |app, paths| {
        if let Some(path) = paths.into_iter().next() {
            req[field.as_str()] = json!(path);
            app.mfx.core.service.request(&ty, req);
        }
    });
    Ok(json!({"dialog": "open"}))
}

fn stage_command(app: &mut StudioApp, what: &str, params: &Value) -> Result<Value, String> {
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
        if what == "getVideo" {
            // bring the edit into Edit without opening it (part 06), then Export renders it
            if let Some(Err(e)) = crate::mortiflix_into::route(app, "mortiflix.import.run", &json!({"open_edit": false})) {
                app.ui.status = e;
            }
            crate::mortiflix_export::get_video(app);
        }
    }
    let s = &app.mfx.core.state;
    Ok(json!({"stage": s.stage, "key": mfx::stage_key(s.stage), "asked": nav == Nav::Asked}))
}

/// Stages 1-4 show only the Mortiflix panels (owner, 2026-10-16). A Mortiflix Studio panel that lands in
/// the Mortiflix workspace (Window ▸ Timeline, Shift+3, a sequence opened, a saved layout) is closed
/// again: Mortiflix Studio's own panels are on stage 5 Edit. Returns what it closed.
pub fn keep_mortiflix_only(app: &mut StudioApp) -> Vec<PanelKind> {
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
        app.ui.status = "Mortiflix Studio's own panels open on stage 5 Edit.".into();
    }
    extra
}

/// Switch Mortiflix Studio's layout and mode for a stage.
pub fn apply_stage(app: &mut StudioApp, stage: u8) {
    if stage < mfx::edit_stage() {
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
    app.ui.mode = if stage >= mfx::render_stage() { Mode::Export } else { Mode::Edit };
    crate::mortiflix_into::on_stage(app, stage);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn choose_files_delivers_paths_to_the_part_and_refuses_bad_params() {
        let mut app = StudioApp::new(mortiflix_engine::Session::default());
        let ctx = egui::Context::default();
        let r = crate::menus::invoke(&mut app, &ctx, "mortiflix.chooseFiles", json!({"purpose": "newProject.assets", "paths": ["/a.png", 5, ""]})).unwrap();
        assert_eq!(r["paths"], 1);
        assert_eq!(app.mfx.core.state.take_paths("newProject.assets"), vec!["/a.png".to_string()]);
        assert!(app.mfx.core.state.take_paths("newProject.assets").is_empty());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.chooseFiles", json!({})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.chooseFiles", json!({"purpose": 7})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.chooseFiles", json!({"purpose": "x".repeat(201)})).is_err());
        // no host dialog here: nothing is picked, nothing breaks
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.chooseFiles", json!({"purpose": "k", "exts": [1, "png"], "folder": "yes"})).is_ok());
        assert!(app.mfx.core.state.take_paths("k").is_empty());
    }

    #[test]
    fn the_title_names_the_mortiflix_project_not_studios_demo() {
        let mut app = StudioApp::new(mortiflix_engine::Session::default());
        assert_eq!(doc_title(&app), None);
        app.mfx.core.activate();
        app.mfx.core.state.apply_service_message(&json!({"event": "status", "data": {"project": "p1", "title": "Why the Sky Is Blue"}}));
        assert_eq!(doc_title(&app).as_deref(), Some("Why the Sky Is Blue \u{2013} Mortiflix Studio"));
        let ctx = egui::Context::default();
        window_title(&mut app, &ctx);
        assert!(app.mfx.window_title.starts_with("Why the Sky Is Blue \u{2013} Mortiflix Studio"));
    }

    #[test]
    fn stage_commands_switch_layout_and_mode() {
        let mut app = StudioApp::new(mortiflix_engine::Session::default());
        let ctx = egui::Context::default();
        // off until a stage command (or the desktop app) turns it on: Mortiflix Studio alone is untouched
        assert_eq!(bar_height(&app), 0.0);
        assert!(!hides_mode_tabs(&app));
        crate::menus::invoke(&mut app, &ctx, "stages.animatic", json!({"force": true})).unwrap(); // a stage set up for the test, past the gates
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
        // File ▸ New ▸ Project… and its shortcut open the New project window, the edit project untouched
        app.mfx.core.state.show_new_project = false;
        let rev = app.session.revision;
        assert_eq!(crate::menus::invoke(&mut app, &ctx, "file.newProject", json!({})).unwrap(), json!({"dialog": "newProject"}));
        assert!(app.mfx.core.state.show_new_project);
        assert_eq!(app.session.revision, rev);
        app.mfx.core.state.show_new_project = false;
        crate::menus::invoke(&mut app, &ctx, "stages.editVideo", json!({})).unwrap();
        assert_eq!((app.mfx.core.state.stage, app.ui.mode), (mfx::STAGE_EDIT, Mode::Edit));
        assert_ne!(app.ui.workspace, WORKSPACE);
        assert!(app.ui.dock.contains(PanelKind::MfxChat) && app.ui.dock.contains(PanelKind::Timeline));
        crate::menus::invoke(&mut app, &ctx, "stages.next", json!({})).unwrap();
        assert_eq!((app.mfx.core.state.stage, app.ui.mode), (mfx::STAGE_RENDER, Mode::Export));
        // Mortiflix Studio's Edit tab lights stage 5 again
        app.ui.mode = Mode::Edit;
        frame_begin(&mut app, &ctx);
        assert_eq!(app.mfx.core.state.stage, mfx::STAGE_EDIT);
        crate::menus::invoke(&mut app, &ctx, "stages.set", json!({"stage": 0})).unwrap();
        assert_eq!(app.ui.workspace, WORKSPACE);
        assert!(crate::menus::invoke(&mut app, &ctx, "stages.bogus", json!({})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "stages.set", json!({"stage": 99})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "stages.set", json!({"stage": -1})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.request", json!({"type": 5})).is_err());
        // files for a part: chosen paths wait under their purpose; hostile params are refused
        crate::menus::invoke(&mut app, &ctx, "mortiflix.chooseFiles", json!({"purpose": "chat.attach", "paths": ["/a/b.png", 7]})).unwrap();
        assert_eq!(app.mfx.core.state.take_files("chat.attach")[0].paths, ["/a/b.png"]);
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.chooseFiles", json!({})).is_err());
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.reveal", json!({"path": ""})).is_err());
        // drops go to Mortiflix on stages 1-4 only
        assert!(take_drop(&mut app, &["/x.png".into()]));
        crate::menus::invoke(&mut app, &ctx, "stages.edit", json!({})).unwrap();
        assert!(!take_drop(&mut app, &["/y.png".into()]), "stage 5: Mortiflix Studio imports it");
        let r = crate::menus::invoke(&mut app, &ctx, "mortiflix.request", json!({"type": "state.get"})).unwrap();
        let reply = crate::menus::invoke(&mut app, &ctx, "mortiflix.reply", json!({"id": r["id"]})).unwrap();
        assert_eq!(reply["error"]["code"], "not_running");
    }
}
