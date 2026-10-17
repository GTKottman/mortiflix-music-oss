//! New project window and preset creator (docs 31, 32a attach assets, 33a music theme).
//!
//! One window, three pages that swap in place (never a window on a window, doc 31):
//! - **New project** (31a): Name, Preset [+] [✎] with its description and stages, The idea, Assets
//!   (Add files… or drop files from the desktop; how to use each one, doc 32a), Music (Let the AI
//!   pick / Choose…, doc 33a), folded Preset settings, Cancel / Create.
//! - **Music theme** (33a-2): who picks, mood, genre, tempo, instruments, like this song, three
//!   samples to hear first, More.
//! - **Preset creator** (31b): 8 steps on the left, one page on the right ([`creator`]).
//!
//! Presets are pipelines from the service (`presets.list` / `presets.save` / `presets.delete`);
//! Create sends `project.new` with the preset, the answers, the attached files and the music
//! theme, then goes to stage 1 Script. The draft (name, preset, idea, files, music) is kept while
//! the app runs, so closing the window loses nothing. The window's state lives in egui's memory
//! (`Win`); [`window_title`] tells the host what to call the window.

mod creator;
mod model;
mod music;

use egui::{Align, Color32, CornerRadius, FontId, Key, Layout, RichText, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

use crate::{MfxCtx, MfxTheme};
pub use model::{Asset, Draft, Kind, MusicTheme, Preset, PresetDraft, Use, parse_presets};

/// Width of the New project page (doc 40 clash 11: the window grows to 840 px).
pub const MAIN_W: f32 = 820.0;
/// Width of the preset creator (doc 31b: 1040 px).
pub const CREATOR_W: f32 = 1020.0;
const LABEL_W: f32 = 104.0;
/// File rows shown before the list scrolls (doc 32a "Many files").
const ROWS_SHOWN: f32 = 6.0;
const ROW_H: f32 = 44.0;
/// How long the "Saved your preset" note stays (doc 31: 4 s).
const TOAST_SECS: f64 = 4.0;

/// The dialog keys this window asks `mortiflix.pickFiles` for.
pub const PICK_ASSETS: &str = "newProject.assets";
pub const PICK_REFERENCE: &str = "newProject.reference";
pub const PICK_CREATOR_REFERENCE: &str = "newProject.creator.reference";
pub const PICK_PICTURES: &str = "newProject.creator.pictures";
pub const PICK_SKILL_DIR: &str = "newProject.creator.skillDir";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Main,
    Music,
    Creator,
}

/// Where the Music theme page was opened from (Done goes back there).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum MusicFrom {
    #[default]
    Main,
    Creator,
}

/// A request to the service and where it is at.
#[derive(Clone, Debug, Default, PartialEq)]
enum Load {
    #[default]
    Idle,
    Loading(u64),
    Loaded,
    Failed(String),
}

/// The window's state, kept in egui's memory between frames and between openings.
#[derive(Clone, Default)]
struct Win {
    page: Page,
    music_from: MusicFrom,
    draft: Draft,
    presets: Vec<Preset>,
    presets_load: Load,
    /// After a save: pick this preset once the list is back.
    pick_after_load: Option<String>,
    /// `presets.get` asked for each preset (intake, skills, look, voice, music theme).
    detail_reqs: Vec<u64>,
    settings_open: bool,
    /// Create was pressed with something missing.
    problem: Option<model::Problem>,
    create_req: Option<u64>,
    create_error: String,
    /// A file added twice: flash the first row's name (row, until).
    flash: Option<(usize, f64)>,
    toast: Option<(String, f64)>,
    focus_name: bool,
    music_ui: music::MusicUi,
    creator: Option<creator::Creator>,
}

fn state_id() -> egui::Id {
    egui::Id::new("mfx-new-project-window")
}

fn popup_id() -> egui::Id {
    egui::Id::new("mfx-new-project-popup-at-start")
}

/// Esc pressed for the page (not for a dropdown that was open when the frame began). Consumes it.
pub(crate) fn escape(ui: &mut egui::Ui) -> bool {
    let popup_before = ui.ctx().data_mut(|d| d.get_temp::<bool>(popup_id())).unwrap_or(false);
    if popup_before || egui::Popup::is_any_open(ui.ctx()) {
        return false;
    }
    ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape))
}

/// Which window size the page needs: the host keeps one window state per key, so the New project
/// page doesn't stay as wide as the creator after it.
pub fn window_key(ctx: &egui::Context) -> &'static str {
    let w = ctx.data_mut(|d| d.get_temp::<Win>(state_id())).unwrap_or_default();
    if w.page == Page::Creator { "creator" } else { "form" }
}

/// The window title for the page on screen (doc 31: the title is a breadcrumb).
pub fn window_title(ctx: &egui::Context) -> String {
    let w = ctx.data_mut(|d| d.get_temp::<Win>(state_id())).unwrap_or_default();
    match (w.page, &w.creator) {
        (Page::Music, Some(_)) if w.music_from == MusicFrom::Creator => "New preset / Music theme".into(),
        (Page::Music, _) => "New project / Music theme".into(),
        (Page::Creator, Some(c)) if c.change => format!("New project › Change the preset: {}", c.change_name),
        (Page::Creator, _) => "New project › New preset".into(),
        _ => "New project".into(),
    }
}

/// The New project window's content (drawn inside the window the host opens with `mortiflix.newProject`).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let id = state_id();
    let mut w = ui.ctx().data_mut(|d| d.get_temp::<Win>(id)).unwrap_or_else(|| Win { focus_name: true, ..Win::default() });
    // Esc that closes a dropdown must not also leave the page: note whether one was open first
    let popup = egui::Popup::is_any_open(ui.ctx());
    ui.ctx().data_mut(|d| d.insert_temp(popup_id(), popup));
    let now = ui.input(|i| i.time);
    w.poll(ctx, now);
    match w.page {
        Page::Main => main_page(ui, ctx, &mut w, now),
        Page::Music => music_page(ui, ctx, &mut w),
        Page::Creator => creator_page(ui, ctx, &mut w, now),
    }
    if w.toast.as_ref().is_some_and(|(_, until)| *until > now) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, w));
}

impl Win {
    fn preset(&self) -> Option<&Preset> {
        self.presets.iter().find(|p| p.slug == self.draft.preset)
    }

    /// Read the replies this window waits for; ask for the presets once the service is up.
    fn poll(&mut self, ctx: &mut MfxCtx, now: f64) {
        let connected = ctx.service.status().is_connected();
        if self.presets_load == Load::Idle && connected {
            self.presets_load = Load::Loading(ctx.service.request("presets.list", json!({})));
        }
        if let Load::Loading(rid) = self.presets_load
            && let Some(r) = ctx.service.take_reply(rid)
        {
            if r.ok {
                self.presets = parse_presets(&r.result);
                ctx.state.presets = r.result.get("presets").cloned().unwrap_or(r.result.clone());
                self.presets_load = Load::Loaded;
                self.detail_reqs =
                    self.presets.iter().filter(|p| !p.broken()).take(50).map(|p| ctx.service.request("presets.get", json!({"slug": p.slug}))).collect();
                self.after_presets_loaded();
            } else {
                let msg = r.error.get("message").and_then(Value::as_str).unwrap_or("the service didn't answer");
                self.presets_load = Load::Failed(msg.chars().take(200).collect());
            }
        }
        let mut got = false;
        self.detail_reqs.retain(|rid| match ctx.service.take_reply(*rid) {
            Some(r) => {
                if r.ok {
                    model::merge_detail(&mut self.presets, &r.result);
                    got = true;
                }
                false
            }
            None => true,
        });
        if got && let Some(p) = self.preset().cloned() {
            // the picked preset's intake arrived: its settings and idea show; typed answers stay
            let typed = self.draft.answers.clone();
            let music = (self.draft.music.clone(), self.draft.music_touched);
            self.draft.pick_preset(&p);
            for (k, v) in typed {
                if !v.is_empty() && (p.question(&k).is_some() || k == "music") {
                    self.draft.answers.insert(k, v);
                }
            }
            if music.1 {
                self.draft.music = music.0;
            }
        }
        // a service that comes back later gets asked again
        if !connected && matches!(self.presets_load, Load::Failed(_)) {
            self.presets_load = Load::Idle;
        }
        if let Some(rid) = self.create_req
            && let Some(r) = ctx.service.take_reply(rid)
        {
            self.create_req = None;
            if r.ok {
                let project = r.result.get("project").unwrap_or(&Value::Null);
                let pid = project.get("id").and_then(Value::as_str).map(|s| s.chars().take(200).collect::<String>());
                let title = project.get("title").and_then(Value::as_str).map(|s| s.chars().take(200).collect::<String>());
                ctx.state.project_title = title.unwrap_or_else(|| self.draft.name.trim().to_string());
                if pid.is_some() {
                    ctx.state.project_id = pid;
                }
                ctx.state.show_new_project = false;
                ctx.command("stages.set", json!({"stage": 0, "force": true}));
                // a fresh draft for the next project, same preset
                let preset = self.draft.preset.clone();
                self.draft = Draft::default();
                if let Some(p) = self.presets.iter().find(|p| p.slug == preset).cloned() {
                    self.draft.pick_preset(&p);
                }
                self.music_ui = music::MusicUi::default();
                self.problem = None;
                self.create_error.clear();
                self.focus_name = true;
            } else {
                let msg = r.error.get("message").and_then(Value::as_str).unwrap_or("it didn't start");
                self.create_error = format!("Couldn't make the project: {}", msg.chars().take(300).collect::<String>());
            }
        }
        music::poll(ctx, &mut self.music_ui, &mut self.draft.music);
        if let Some(c) = self.creator.as_mut() {
            c.poll(ctx);
        }
        if self.toast.as_ref().is_some_and(|(_, until)| *until <= now) {
            self.toast = None;
        }
    }

    fn after_presets_loaded(&mut self) {
        let want = self.pick_after_load.take().or_else(|| (!self.draft.preset.is_empty()).then(|| self.draft.preset.clone()));
        let found = want.as_ref().and_then(|s| self.presets.iter().find(|p| &p.slug == s && !p.broken()));
        let pick =
            found.or_else(|| self.presets.iter().find(|p| p.slug == "explainer" && !p.broken())).or_else(|| self.presets.iter().find(|p| !p.broken())).cloned();
        if let Some(p) = pick {
            // keep the answers when it's the preset already picked
            if p.slug != self.draft.preset || self.draft.answers.is_empty() {
                self.draft.pick_preset(&p);
            }
        }
    }

    fn open_creator(&mut self, change: bool) {
        let Some(p) = self.preset().cloned().or_else(|| self.presets.iter().find(|p| !p.broken()).cloned()) else { return };
        self.creator = Some(creator::Creator::new(&p, change));
        self.page = Page::Creator;
    }
}

// ------------------------------------------------------------------ small widgets

/// The one accent (blue) button of a page.
pub(crate) fn accent_button(ui: &mut egui::Ui, t: &MfxTheme, text: &str, enabled: bool) -> egui::Response {
    let b = egui::Button::new(RichText::new(text).color(Color32::WHITE).size(13.0))
        .fill(if enabled { t.accent } else { t.hover })
        .corner_radius(CornerRadius::same(4))
        .min_size(vec2(96.0, 28.0));
    ui.add_enabled(enabled, b)
}

/// A quiet button (the rest).
pub(crate) fn quiet_button(ui: &mut egui::Ui, t: &MfxTheme, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(t.text).size(13.0)).fill(t.field_bg).corner_radius(CornerRadius::same(4)).min_size(vec2(72.0, 28.0)))
}

/// A chip: an on/off pill. Picked chips say so with a tick, never colour alone.
pub(crate) fn chip(ui: &mut egui::Ui, t: &MfxTheme, on: bool, text: &str) -> egui::Response {
    let label = if on { format!("✓ {text}") } else { text.to_string() };
    let b = egui::Button::new(RichText::new(label).size(12.5).color(if on { t.text } else { t.text_dim }))
        .fill(if on { t.hover } else { t.field_bg })
        .stroke(Stroke::new(1.0, if on { t.accent } else { t.separator }))
        .corner_radius(CornerRadius::same(12))
        .min_size(vec2(0.0, 24.0));
    ui.add(b)
}

/// A fold header: a drawn triangle (right = folded, down = open) and its label.
pub(crate) fn fold(ui: &mut egui::Ui, t: &MfxTheme, open: bool, text: &str) -> egui::Response {
    let g = ui.painter().layout_no_wrap(text.to_string(), FontId::proportional(13.0), t.text_dim);
    let (rect, r) = ui.allocate_exact_size(vec2(g.size().x + 20.0, 20.0), Sense::click());
    let col = if r.hovered() { t.text } else { t.text_dim };
    let c = egui::pos2(rect.min.x + 6.0, rect.center().y);
    let pts = if open {
        vec![egui::pos2(c.x - 4.0, c.y - 2.5), egui::pos2(c.x + 4.0, c.y - 2.5), egui::pos2(c.x, c.y + 3.0)]
    } else {
        vec![egui::pos2(c.x - 2.5, c.y - 4.0), egui::pos2(c.x + 3.0, c.y), egui::pos2(c.x - 2.5, c.y + 4.0)]
    };
    ui.painter().add(egui::Shape::convex_polygon(pts, col, Stroke::NONE));
    ui.painter().galley(egui::pos2(rect.min.x + 16.0, rect.center().y - g.size().y / 2.0), g, col);
    r
}

/// The ✎ (Change this preset) button: a pencil drawn in code (the UI font has no ✎).
fn pencil_button(ui: &mut egui::Ui, t: &MfxTheme, enabled: bool) -> egui::Response {
    let r = ui.add_enabled(enabled, egui::Button::new("").min_size(vec2(28.0, 26.0)));
    let col = if enabled { t.text } else { t.text_faint };
    let c = r.rect.center();
    let (a, b) = (egui::pos2(c.x - 5.0, c.y + 5.0), egui::pos2(c.x + 4.0, c.y - 4.0));
    let p = ui.painter();
    p.line_segment([a, b], Stroke::new(3.0, col));
    p.line_segment([egui::pos2(c.x + 3.0, c.y - 5.0), egui::pos2(c.x + 5.5, c.y - 2.5)], Stroke::new(1.0, t.panel_bg));
    p.add(egui::Shape::convex_polygon(
        vec![egui::pos2(a.x - 1.5, a.y + 1.5), egui::pos2(a.x - 1.0, a.y - 2.0), egui::pos2(a.x + 2.0, a.y + 1.0)],
        col,
        Stroke::NONE,
    ));
    r
}

/// The left label column of a form row.
pub(crate) fn row_label(ui: &mut egui::Ui, t: &MfxTheme, text: &str, sub: Option<(&str, Color32)>) {
    ui.allocate_ui_with_layout(vec2(LABEL_W, 22.0), Layout::top_down(Align::Min), |ui| {
        ui.set_width(LABEL_W);
        ui.label(RichText::new(text).color(t.text_dim).size(13.0));
        if let Some((s, c)) = sub {
            ui.label(RichText::new(s).color(c).size(11.0));
        }
    });
}

/// Outline a field red (Create found it missing).
fn outline_red(ui: &egui::Ui, t: &MfxTheme, r: &egui::Response) {
    ui.painter().rect_stroke(r.rect.expand(2.0), 4.0, Stroke::new(1.5, t.error), StrokeKind::Outside);
}

/// Files dropped on the app this frame (paths only), and how many are hovering.
pub(crate) fn drops(ui: &egui::Ui) -> (Vec<String>, usize) {
    ui.input(|i| {
        let dropped = i.raw.dropped_files.iter().map(|f| f.path().to_string_lossy().to_string()).filter(|p| !p.is_empty()).collect();
        (dropped, i.raw.hovered_files.len())
    })
}

/// Ask the host for an OS file dialog; the paths come back in `state.picked` under `key`.
pub(crate) fn pick(ctx: &mut MfxCtx, key: &str, exts: &[&str], folder: bool) {
    ctx.command("mortiflix.pickFiles", json!({"key": key, "exts": exts, "folder": folder}));
}

/// A kind glyph in a fixed 16:9 frame (doc 32a: nothing is drawn over a thumbnail).
pub(crate) fn kind_glyph(ui: &mut egui::Ui, t: &MfxTheme, kind: Option<Kind>, size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 3.0, t.field_bg);
    p.rect_stroke(rect, 3.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    let c = rect.center();
    let s = rect.height() * 0.32;
    let col = t.text_dim;
    match kind {
        None => {
            p.text(c, egui::Align2::CENTER_CENTER, "!", FontId::proportional(rect.height() * 0.6), t.warn);
        }
        Some(Kind::Logo) => {
            p.circle_stroke(c, s, Stroke::new(1.5, col));
            p.circle_filled(c, s * 0.4, col);
        }
        Some(Kind::Image) => {
            let base = c.y + s * 0.8;
            p.add(egui::Shape::convex_polygon(
                vec![egui::pos2(c.x - s * 1.6, base), egui::pos2(c.x - s * 0.4, c.y - s * 0.6), egui::pos2(c.x + s * 0.6, base)],
                col,
                Stroke::NONE,
            ));
            p.circle_filled(egui::pos2(c.x + s, c.y - s * 0.5), s * 0.3, col);
        }
        Some(Kind::Footage) => {
            p.add(egui::Shape::convex_polygon(
                vec![egui::pos2(c.x - s * 0.6, c.y - s * 0.8), egui::pos2(c.x + s * 0.8, c.y), egui::pos2(c.x - s * 0.6, c.y + s * 0.8)],
                col,
                Stroke::NONE,
            ));
        }
        Some(Kind::Audio) | Some(Kind::Music) => {
            for (i, h) in [0.4f32, 0.9, 0.6, 1.0, 0.5, 0.8, 0.3].iter().enumerate() {
                let x = c.x - s * 1.5 + i as f32 * s * 0.5;
                p.line_segment([egui::pos2(x, c.y - s * h), egui::pos2(x, c.y + s * h)], Stroke::new(1.5, col));
            }
        }
        Some(Kind::Font) => {
            p.text(c, egui::Align2::CENTER_CENTER, "Aa", FontId::proportional(rect.height() * 0.5), col);
        }
        Some(Kind::Document) => {
            let page = egui::Rect::from_center_size(c, vec2(s * 1.4, s * 2.0));
            p.rect_stroke(page, 1.0, Stroke::new(1.0, col), StrokeKind::Inside);
            for i in 1..4 {
                let y = page.min.y + page.height() * i as f32 / 4.0;
                p.line_segment([egui::pos2(page.min.x + 3.0, y), egui::pos2(page.max.x - 3.0, y)], Stroke::new(1.0, col));
            }
        }
        Some(Kind::Brand) => {
            for (i, g) in [90u8, 140, 200].iter().enumerate() {
                p.rect_filled(egui::Rect::from_center_size(egui::pos2(c.x - s + i as f32 * s, c.y), vec2(s * 0.8, s * 1.4)), 1.0, Color32::from_gray(*g));
            }
        }
    }
}

// ------------------------------------------------------------------ 31a New project

fn main_page(ui: &mut egui::Ui, ctx: &mut MfxCtx, w: &mut Win, now: f64) {
    let t = ctx.theme;
    ui.set_width(MAIN_W);
    ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
    crate::service_notice(ui, ctx);

    // files picked in the dialog or dropped anywhere on the window (doc 32a)
    let picked = ctx.state.take_picked(PICK_ASSETS);
    let (dropped, hovering) = drops(ui);
    for list in [picked, dropped] {
        if !list.is_empty()
            && let Some(i) = model::add_assets(&mut w.draft.assets, &list)
        {
            w.flash = Some((i, now + 0.8));
        }
    }
    if hovering > 0 {
        ui.ctx().request_repaint();
    }
    let preset = w.preset().cloned();
    let problem_field = w.problem.as_ref().map(|p| p.field);

    // Name
    ui.horizontal(|ui| {
        row_label(ui, &t, "Name", None);
        let r = ui.add(
            egui::TextEdit::singleline(&mut w.draft.name).hint_text("The Rise of AI").desired_width(MAIN_W - LABEL_W - 16.0).font(FontId::proportional(14.0)),
        );
        if std::mem::take(&mut w.focus_name) {
            r.request_focus();
        }
        if problem_field == Some("name") {
            outline_red(ui, &t, &r);
        }
        if r.changed() && problem_field == Some("name") {
            w.problem = None;
        }
        ctx.auto("mfx.newProject.name", &r, "Name");
    });

    // Preset [v] [+] [✎]
    ui.horizontal(|ui| {
        row_label(ui, &t, "Preset", None);
        preset_combo(ui, ctx, w, preset.as_ref());
        let plus =
            ui.add_enabled(!w.presets.is_empty(), egui::Button::new(RichText::new("+").size(15.0)).min_size(vec2(28.0, 26.0))).on_hover_text("New preset");
        if plus.clicked() {
            w.open_creator(false);
        }
        ctx.auto("mfx.newProject.preset.new", &plus, "New preset");
        let pencil = pencil_button(ui, &t, preset.is_some()).on_hover_text("Change this preset");
        if pencil.clicked() {
            w.open_creator(true);
        }
        ctx.auto("mfx.newProject.preset.change", &pencil, "Change this preset");
    });
    if let Some(p) = &preset {
        ui.horizontal(|ui| {
            ui.add_space(LABEL_W + 8.0);
            ui.vertical(|ui| {
                ui.set_width(MAIN_W - LABEL_W - 16.0);
                if !p.description.is_empty() {
                    ui.label(RichText::new(&p.description).color(t.text_dim).size(12.0));
                }
                let stages = p.stage_line();
                if !stages.is_empty() {
                    ui.label(RichText::new(stages).color(t.text_faint).size(11.5));
                }
            });
        });
    }

    // The idea
    let mut idea_focused = false;
    ui.horizontal_top(|ui| {
        row_label(ui, &t, "The idea", None);
        let hint =
            preset.as_ref().and_then(|p| p.idea_question()).map(|q| if q.help.is_empty() { q.label.clone() } else { q.help.clone() }).unwrap_or_default();
        let r = ui.add(egui::TextEdit::multiline(&mut w.draft.idea).hint_text(hint).desired_rows(3).desired_width(MAIN_W - LABEL_W - 16.0));
        idea_focused = r.has_focus();
        if problem_field == Some("idea") {
            outline_red(ui, &t, &r);
        }
        if r.changed() && problem_field == Some("idea") {
            w.problem = None;
        }
        ctx.auto("mfx.newProject.idea", &r, "The idea");
    });

    // Assets (doc 32a)
    assets_section(ui, ctx, w, preset.as_ref(), hovering, now);

    // Music (doc 33a-1)
    ui.horizontal(|ui| {
        row_label(ui, &t, "Music", None);
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.set_width(MAIN_W - LABEL_W - 16.0);
                ui.label(RichText::new(w.draft.music.summary()).color(t.text).size(13.0));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let r = quiet_button(ui, &t, "Choose…");
                    if r.clicked() {
                        w.page = Page::Music;
                        w.music_from = MusicFrom::Main;
                    }
                    ctx.auto("mfx.newProject.music.choose", &r, "Choose…");
                });
            });
            let sub = if !w.draft.music.music {
                "No music in this video. Turn it on in Choose… › More.".to_string()
            } else if w.draft.music.ai_picks() {
                if w.draft.music_touched || preset.as_ref().and_then(|p| p.music_theme.as_ref()).is_none() {
                    "From the preset and your idea. You hear three themes after the look.".to_string()
                } else {
                    "From the preset's music theme.".to_string()
                }
            } else {
                "Your theme. Anything you left empty, the AI picks.".to_string()
            };
            ui.label(RichText::new(sub).color(t.text_faint).size(11.5));
        });
    });

    ui.separator();
    // ▸ Preset settings (folded)
    if let Some(p) = &preset {
        ui.horizontal(|ui| {
            let r = fold(ui, &t, w.settings_open, "Preset settings");
            if r.clicked() {
                w.settings_open = !w.settings_open;
            }
            ctx.auto("mfx.newProject.settings", &r, "Preset settings");
            ui.label(RichText::new(w.draft.settings_summary(p)).color(t.text_faint).size(12.0));
        });
        if w.settings_open {
            settings_rows(ui, ctx, w, p);
        }
    }

    // footer
    ui.separator();
    let busy = w.create_req.is_some();
    let mut create = false;
    ui.horizontal(|ui| {
        ui.set_width(MAIN_W);
        let (msg, col) = if let Some(p) = &w.problem {
            (p.message.clone(), t.error)
        } else if !w.create_error.is_empty() {
            (w.create_error.clone(), t.error)
        } else if busy {
            ("Starting…".to_string(), t.text_dim)
        } else if let Some((m, _)) = &w.toast {
            (m.clone(), t.ok)
        } else if w.draft.assets.is_empty() {
            ("Enter = Create · Esc = Cancel".to_string(), t.text_faint)
        } else {
            ("Your files are copied into the project when you press Create.".to_string(), t.text_faint)
        };
        ui.label(RichText::new(msg).color(col).size(12.0));
        if !w.create_error.is_empty() && !ctx.service.status().is_connected() {
            let s = ui.link("Setup…");
            if s.clicked() {
                ctx.command("mortiflix.setup", json!({}));
            }
            ctx.auto("mfx.newProject.setup", &s, "Setup…");
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let r = accent_button(ui, &t, if busy { "Starting…" } else { "Create" }, !busy);
            create |= r.clicked();
            ctx.auto("mfx.newProject.create", &r, "Create");
            let c = quiet_button(ui, &t, "Cancel");
            if c.clicked() {
                ctx.state.show_new_project = false;
            }
            ctx.auto("mfx.newProject.cancel", &c, "Cancel");
        });
    });

    // keys: Enter = Create (not while writing the idea), Esc = Cancel (the draft stays)
    let popup = egui::Popup::is_any_open(ui.ctx());
    if !popup && !idea_focused && ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.shift) {
        create = true;
    }
    if escape(ui) {
        ctx.state.show_new_project = false;
    }
    if create && !busy {
        try_create(ctx, w, preset.as_ref());
    }
}

fn try_create(ctx: &mut MfxCtx, w: &mut Win, preset: Option<&Preset>) {
    w.create_error.clear();
    w.problem = w.draft.problem(preset);
    if w.problem.is_some() {
        return;
    }
    let Some(p) = preset else { return };
    w.create_req = Some(ctx.service.request("project.new", w.draft.project_new_params(p)));
}

fn preset_combo(ui: &mut egui::Ui, ctx: &mut MfxCtx, w: &mut Win, preset: Option<&Preset>) {
    let t = ctx.theme;
    let connected = ctx.service.status().is_connected();
    let selected = match (&w.presets_load, preset) {
        (_, Some(p)) => p.name.clone(),
        (Load::Loading(_), _) => "Loading presets…".into(),
        (Load::Failed(e), _) => format!("Can't load presets: {e}"),
        (Load::Loaded, None) if w.presets.is_empty() => "No presets found. Reinstall Mortiflix or make one with +".into(),
        _ if !connected => "Presets load when the Mortiflix service runs".into(),
        _ => "Pick a preset".into(),
    };
    let mut picked: Option<Preset> = None;
    let mut new_preset = false;
    let enabled = !w.presets.is_empty();
    let inner = ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt("mfx-np-preset").selected_text(RichText::new(selected).size(13.0)).width(MAIN_W - LABEL_W - 96.0).height(480.0).show_ui(
            ui,
            |ui| {
                ui.set_min_width(MAIN_W - LABEL_W - 110.0);
                for (group, built) in [("Built in", true), ("Yours", false)] {
                    let rows: Vec<&Preset> = w.presets.iter().filter(|p| p.built_in() == built).collect();
                    if rows.is_empty() {
                        continue;
                    }
                    ui.label(RichText::new(group).color(t.text_faint).size(11.0));
                    for p in rows {
                        let here = Some(p.slug.as_str()) == preset.map(|x| x.slug.as_str());
                        let r = if p.broken() {
                            ui.add_enabled(false, egui::Button::selectable(false, format!("{}   Can't open: {}", p.name, p.error)))
                        } else {
                            let r = ui.add(
                                egui::Button::selectable(here, RichText::new(&p.name).size(13.0))
                                    .right_text(RichText::new(p.shape_line()).color(if here { t.text } else { t.text_faint }).size(11.5)),
                            );
                            if r.clicked() {
                                picked = Some(p.clone());
                            }
                            r
                        };
                        ctx.auto(&format!("mfx.newProject.preset.option.{}", p.slug), &r, &p.name);
                    }
                }
                ui.separator();
                let r = ui.add(egui::Button::selectable(false, "+ New preset…"));
                new_preset = r.clicked();
                ctx.auto("mfx.newProject.preset.option.new", &r, "+ New preset…");
            },
        )
    });
    ctx.auto("mfx.newProject.preset", &inner.inner.response, "Preset");
    if let Some(p) = picked {
        w.draft.pick_preset(&p);
        if w.problem.as_ref().is_some_and(|x| x.field == "preset") {
            w.problem = None;
        }
    }
    if new_preset {
        w.open_creator(false);
    }
}

fn assets_section(ui: &mut egui::Ui, ctx: &mut MfxCtx, w: &mut Win, preset: Option<&Preset>, hovering: usize, now: f64) {
    let t = ctx.theme;
    let needed = preset
        .and_then(|p| p.required_files())
        .map(|q| if q.label.is_empty() { "a file is needed".to_string() } else { format!("{} is needed", q.label.to_lowercase()) });
    let width = MAIN_W - LABEL_W - 16.0;
    ui.horizontal_top(|ui| {
        match &needed {
            Some(n) => row_label(ui, &t, "Assets", Some((n, t.warn))),
            None => row_label(ui, &t, "Assets", Some(("optional", t.text_faint))),
        }
        ui.vertical(|ui| {
            ui.set_width(width);
            // drop zone: text swaps in a fixed slot, nothing changes size while dragging
            let has = !w.draft.assets.is_empty();
            let h = if has { 44.0 } else { 64.0 };
            let (rect, zone) = ui.allocate_exact_size(vec2(width, h), Sense::hover());
            let p = ui.painter_at(rect);
            let dragging = hovering > 0;
            p.rect_filled(rect, 4.0, t.field_bg);
            let stroke = if dragging { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.separator) };
            p.rect_stroke(rect, 4.0, stroke, StrokeKind::Inside);
            let text = if dragging {
                format!("Drop to attach {hovering} file{}", if hovering == 1 { "" } else { "s" })
            } else if has {
                "⬆ Drop more files here".to_string()
            } else {
                "⬆ Drop files here from your desktop".to_string()
            };
            p.text(
                egui::pos2(rect.min.x + 14.0, rect.center().y - if has { 0.0 } else { 8.0 }),
                egui::Align2::LEFT_CENTER,
                text,
                FontId::proportional(13.0),
                if dragging { t.text } else { t.text_dim },
            );
            if !has {
                p.text(
                    egui::pos2(rect.min.x + 14.0, rect.center().y + 11.0),
                    egui::Align2::LEFT_CENTER,
                    "Pictures, logos, footage, sound, music, fonts, documents",
                    FontId::proportional(11.0),
                    t.text_faint,
                );
            }
            ctx.auto("mfx.newProject.assets.drop", &zone, "Drop files here");
            let add_rect = egui::Rect::from_min_size(egui::pos2(rect.max.x - 112.0, rect.center().y - 14.0), vec2(100.0, 28.0));
            // a child Ui inside the zone, so the button doesn't move the layout's cursor
            let mut inside = ui.new_child(egui::UiBuilder::new().max_rect(add_rect));
            let add = inside.add_sized(add_rect.size(), egui::Button::new(RichText::new("Add files…").size(13.0)).corner_radius(CornerRadius::same(4)));
            if add.clicked() {
                pick(ctx, PICK_ASSETS, &model::all_exts(), false);
            }
            ctx.auto("mfx.newProject.assets.add", &add, "Add files…");
            if !has {
                ui.label(
                    RichText::new("Anything you attach, Mortiflix sees from the first step. Tell it how to use each one; you can add more later in Assets.")
                        .color(t.text_faint)
                        .size(11.5),
                );
                return;
            }
            let n = w.draft.assets.len();
            ui.horizontal(|ui| {
                ui.set_width(width);
                ui.label(RichText::new(format!("{n} file{} · copied into the project", if n == 1 { "" } else { "s" })).color(t.text_faint).size(11.5));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(30.0);
                    ui.add_sized([210.0, 16.0], egui::Label::new(RichText::new("Where, or a note").color(t.text_faint).size(11.5)));
                    ui.add_sized([150.0, 16.0], egui::Label::new(RichText::new("How to use it").color(t.text_faint).size(11.5)));
                });
            });
            let mut remove = None;
            egui::ScrollArea::vertical().id_salt("mfx-np-assets").max_height(ROWS_SHOWN * ROW_H).auto_shrink([false, true]).show(ui, |ui| {
                for (i, a) in w.draft.assets.iter_mut().enumerate() {
                    let flash = w.flash.is_some_and(|(row, until)| row == i && until > now);
                    if asset_row(ui, ctx, &t, i, a, width - 14.0, flash) {
                        remove = Some(i);
                    }
                }
            });
            if let Some(i) = remove
                && i < w.draft.assets.len()
            {
                w.draft.assets.remove(i);
                w.flash = None;
            }
            if w.flash.is_some_and(|(_, until)| until > now) {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
        });
    });
    if w.problem.as_ref().is_some_and(|p| p.field == "assets") && w.draft.assets.iter().any(|a| a.error.is_empty()) {
        w.problem = None;
    }
}

/// One attached file. Returns true when ✕ was pressed.
fn asset_row(ui: &mut egui::Ui, ctx: &mut MfxCtx, t: &MfxTheme, i: usize, a: &mut Asset, width: f32, flash: bool) -> bool {
    let mut remove = false;
    ui.horizontal(|ui| {
        ui.set_width(width);
        ui.set_height(ROW_H - 6.0);
        kind_glyph(ui, t, a.error.is_empty().then_some(a.kind), vec2(64.0, 36.0));
        let text_w = (width - 64.0 - 150.0 - 210.0 - 60.0).max(120.0);
        ui.vertical(|ui| {
            ui.set_width(text_w);
            let name_col = if flash { t.accent_hover } else { t.text };
            ui.add(egui::Label::new(RichText::new(&a.name).color(name_col).size(13.0)).truncate()).on_hover_text(&a.path);
            if a.error.is_empty() {
                ui.label(RichText::new(&a.facts).color(t.text_faint).size(11.0));
            } else {
                ui.add(egui::Label::new(RichText::new(format!("⚠ {}", a.error)).color(t.error).size(11.0)).truncate()).on_hover_text(&a.error);
            }
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let x = ui.add(egui::Button::new(RichText::new("×").size(16.0)).frame(false)).on_hover_text("Remove");
            remove = x.clicked();
            ctx.auto(&format!("mfx.newProject.asset.{i}.remove"), &x, &format!("Remove {}", a.name));
            ui.add_enabled_ui(a.error.is_empty(), |ui| {
                let note = ui.add(egui::TextEdit::singleline(&mut a.note).hint_text("Anywhere it fits").desired_width(200.0));
                ctx.auto(&format!("mfx.newProject.asset.{i}.note"), &note, "Where, or a note");
                let combo = egui::ComboBox::from_id_salt(("mfx-np-use", i)).selected_text(a.how.label()).width(140.0).show_ui(ui, |ui| {
                    for u in Use::ALL {
                        let r = ui.selectable_value(&mut a.how, u, u.label());
                        ctx.auto(&format!("mfx.newProject.asset.{i}.use.{}", u.key()), &r, u.label());
                    }
                });
                ctx.auto(&format!("mfx.newProject.asset.{i}.use"), &combo.response, "How to use it");
            });
        });
    });
    remove
}

fn settings_rows(ui: &mut egui::Ui, ctx: &mut MfxCtx, w: &mut Win, p: &Preset) {
    let t = ctx.theme;
    let questions: Vec<model::Question> = p.settings_questions().cloned().collect();
    if questions.is_empty() {
        ui.label(RichText::new("This preset has nothing else to set.").color(t.text_faint).size(12.0));
        return;
    }
    for q in questions {
        let label = if q.label.is_empty() { q.id.clone() } else { q.label.clone() };
        let value = w.draft.answers.entry(q.id.clone()).or_default();
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(vec2(220.0, 22.0), Layout::top_down(Align::Min), |ui| {
                ui.set_width(220.0);
                ui.add(egui::Label::new(RichText::new(&label).color(t.text_dim).size(12.5)).wrap());
            });
            let id = format!("mfx.newProject.setting.{}", q.id);
            match q.kind.as_str() {
                "choice" => {
                    let shown = if value.is_empty() { "Let the AI decide".to_string() } else { value.clone() };
                    let combo = egui::ComboBox::from_id_salt(("mfx-np-setting", q.id.as_str())).selected_text(shown).width(MAIN_W - 260.0).show_ui(ui, |ui| {
                        for c in &q.choices {
                            let r = ui.selectable_value(value, c.clone(), c);
                            ctx.auto(&format!("{id}.{}", c), &r, c);
                        }
                    });
                    ctx.auto(&id, &combo.response, &label);
                }
                "long" => {
                    let r = ui.add(egui::TextEdit::multiline(value).desired_rows(2).desired_width(MAIN_W - 260.0).hint_text(q.help.clone()));
                    ctx.auto(&id, &r, &label);
                }
                _ => {
                    let r = ui.add(egui::TextEdit::singleline(value).desired_width(MAIN_W - 260.0).hint_text(q.help.clone()));
                    ctx.auto(&id, &r, &label);
                }
            }
        });
    }
}

// ------------------------------------------------------------------ 33a-2 Music theme

fn music_page(ui: &mut egui::Ui, ctx: &mut MfxCtx, w: &mut Win) {
    let from_creator = w.music_from == MusicFrom::Creator && w.creator.is_some();
    let preset = w.preset().cloned();
    let back = if from_creator {
        let Some(c) = w.creator.as_mut() else { return };
        let finish = c.music_choices.clone();
        music::page(ui, ctx, &mut c.music_ui, &mut c.draft.theme, &[], &finish, &mut c.draft.music_choice, PICK_CREATOR_REFERENCE)
    } else {
        let finish = preset.as_ref().and_then(|p| p.question("music")).map(|q| q.choices.clone()).unwrap_or_default();
        let before = w.draft.music.clone();
        let mut choice = w.draft.music_choice().clone();
        let out = music::page(ui, ctx, &mut w.music_ui, &mut w.draft.music, &w.draft.assets, &finish, &mut choice, PICK_REFERENCE);
        *w.draft.music_choice() = choice;
        if w.draft.music != before {
            w.draft.music_touched = true;
        }
        out
    };
    if back {
        w.page = if from_creator { Page::Creator } else { Page::Main };
    }
}

// ------------------------------------------------------------------ 31b Preset creator

fn creator_page(ui: &mut egui::Ui, ctx: &mut MfxCtx, w: &mut Win, now: f64) {
    let Some(c) = w.creator.as_mut() else {
        w.page = Page::Main;
        return;
    };
    match c.show(ui, ctx, &w.presets) {
        creator::Out::Stay => {}
        creator::Out::Music => {
            w.page = Page::Music;
            w.music_from = MusicFrom::Creator;
        }
        creator::Out::Leave => {
            w.creator = None;
            w.page = Page::Main;
        }
        creator::Out::Saved { slug, name } => {
            w.creator = None;
            w.page = Page::Main;
            w.pick_after_load = Some(slug);
            w.presets_load = Load::Idle;
            w.toast = Some((format!("Saved your preset \"{name}\" and picked it"), now + TOAST_SECS));
        }
        creator::Out::Deleted { name } => {
            w.creator = None;
            w.page = Page::Main;
            w.draft.preset.clear();
            w.presets_load = Load::Idle;
            w.toast = Some((format!("Deleted \"{name}\""), now + TOAST_SECS));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MfxState, Service};

    fn run_frames(state: &mut MfxState, service: &mut Service, frames: usize, events: Vec<egui::Event>) -> Vec<(String, Value)> {
        let ectx = egui::Context::default();
        let mut cmds = Vec::new();
        for i in 0..frames {
            let input = egui::RawInput { events: if i == 0 { events.clone() } else { Vec::new() }, ..egui::RawInput::default() };
            ectx.run_ui(input, |ui| {
                let mut c = MfxCtx::new(state, service, MfxTheme::default());
                show(ui, &mut c);
                cmds.append(&mut c.commands);
            })
            .textures_delta
            .clear();
        }
        cmds
    }

    fn put(ectx: &egui::Context, w: Win) {
        ectx.data_mut(|d| d.insert_temp(state_id(), w));
    }

    fn explainer() -> Preset {
        parse_presets(&json!([{"slug": "explainer", "name": "Explainer video", "source": "built-in", "description": "An explainer.",
            "shared_skills": ["music"],
            "intake": [{"id": "topic", "type": "long", "required": true}, {"id": "length", "type": "choice", "choices": ["60 s"], "default": "60 s"},
                {"id": "music", "type": "choice", "choices": ["Original score", "No music"], "default": "Original score"}],
            "steps": [{"key": "brief", "name": "Brief"}, {"key": "music", "name": "Music", "work": ["music"]}, {"key": "final", "name": "Final"}]}]))
        .remove(0)
    }

    #[test]
    fn every_page_draws_without_the_service() {
        let mut state = MfxState::default();
        let mut service = Service::default();
        // the window with nothing loaded: no panic, presets wait for the service
        run_frames(&mut state, &mut service, 2, Vec::new());
        let ectx = egui::Context::default();
        for page in [Page::Main, Page::Music, Page::Creator] {
            let p = explainer();
            let mut w = Win { page, presets: vec![p.clone()], presets_load: Load::Loaded, ..Win::default() };
            w.draft.pick_preset(&p);
            w.draft.assets.push(Asset { path: "/x.svg".into(), name: "x.svg".into(), ..Asset::default() });
            w.draft.assets.push(Asset { path: "/y.heic".into(), name: "y.heic".into(), error: "Can't read".into(), ..Asset::default() });
            w.settings_open = true;
            if page == Page::Creator {
                w.creator = Some(creator::Creator::new(&p, false));
            }
            put(&ectx, w);
            for step in 0..8 {
                if let Some(mut w) = ectx.data_mut(|d| d.get_temp::<Win>(state_id()))
                    && let Some(c) = w.creator.as_mut()
                {
                    c.step = step;
                    put(&ectx, w);
                }
                ectx.run_ui(egui::RawInput::default(), |ui| {
                    let mut c = MfxCtx::new(&mut state, &mut service, MfxTheme::default());
                    show(ui, &mut c);
                })
                .textures_delta
                .clear();
            }
            assert!(!window_title(&ectx).is_empty());
        }
    }

    #[test]
    fn create_checks_first_then_sends_project_new() {
        let mut state = MfxState::default();
        let mut service = Service::default();
        let p = explainer();
        let mut w = Win { presets: vec![p.clone()], presets_load: Load::Loaded, ..Win::default() };
        w.draft.pick_preset(&p);
        let mut c = MfxCtx::new(&mut state, &mut service, MfxTheme::default());
        try_create(&mut c, &mut w, Some(&p));
        assert_eq!(w.problem.as_ref().map(|x| x.field), Some("name"));
        assert!(w.create_req.is_none());
        w.draft.name = "The Rise of AI".into();
        w.draft.idea = "How five companies raced".into();
        try_create(&mut c, &mut w, Some(&p));
        assert!(w.problem.is_none());
        assert!(w.create_req.is_some());
        // the service isn't running: the window says so instead of waiting forever
        w.poll(&mut c, 0.0);
        assert!(w.create_req.is_none());
        assert!(w.create_error.starts_with("Couldn't make the project"), "{}", w.create_error);
    }

    #[test]
    fn picked_files_land_in_the_assets_list() {
        let dir = std::env::temp_dir().join(format!("mfx-np-pick-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("company-logos.svg");
        std::fs::write(&f, b"<svg/>").unwrap();
        let mut state = MfxState::default();
        let mut service = Service::default();
        state.push_picked(PICK_ASSETS, vec![f.to_string_lossy().to_string()]);
        let ectx = egui::Context::default();
        ectx.run_ui(egui::RawInput::default(), |ui| {
            let mut c = MfxCtx::new(&mut state, &mut service, MfxTheme::default());
            show(ui, &mut c);
        })
        .textures_delta
        .clear();
        let w = ectx.data_mut(|d| d.get_temp::<Win>(state_id())).unwrap();
        assert_eq!(w.draft.assets.len(), 1);
        assert_eq!(w.draft.assets[0].kind, Kind::Logo);
        assert!(state.picked.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn titles_follow_the_page() {
        let ectx = egui::Context::default();
        assert_eq!(window_title(&ectx), "New project");
        let p = explainer();
        put(&ectx, Win { page: Page::Creator, creator: Some(creator::Creator::new(&p, false)), ..Win::default() });
        assert_eq!(window_title(&ectx), "New project › New preset");
        put(&ectx, Win { page: Page::Creator, creator: Some(creator::Creator::new(&p, true)), ..Win::default() });
        assert_eq!(window_title(&ectx), "New project › Change the preset: Explainer video");
        put(&ectx, Win { page: Page::Music, ..Win::default() });
        assert_eq!(window_title(&ectx), "New project / Music theme");
    }
}
