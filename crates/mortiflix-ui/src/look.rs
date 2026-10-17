//! Stage 2 Look: the looks to pick from (doc 11 §9.3 Style, doc 30 stage 2, doc 37's looks card,
//! doc 33: the music theme is part of the look).
//!
//! The Look panel (PanelKind::MfxStage on stage 2) shows the 3-5 style frames large, as pictures
//! (PNG / JPEG / WebP, and SVG drawn in pure Rust by [`crate::nodes::stills`]), the one in use ticked
//! and outlined. Click a frame to use it; **Ask for changes** on a frame opens the chat about that
//! frame (typed words become its Fix); **Make more looks** asks the AI for more frames. Under them:
//! the palette swatches and fonts of the look in use, and the music theme row (the theme in use, its
//! 15 s samples, Change… opens the Music theme page in a small window).
//!
//! Picking another look when shots are already made asks first ("This change makes some things
//! again", doc 30), listing the shots that get remade in the new look; nothing changes until
//! **Change it**. The chat's looks card picks through the same path ([`pick`]).
//!
//! Data: `look.get` (frames, the one in use, palette, fonts, shots and the look each was made in),
//! `look.use {frame, confirm?}`, `look.more`, event `look.changed`; `music.theme.get` /
//! `music.theme.use` / `music.theme.set` for the theme row (protocol: `docs/mortiflix/protocol.md`).
//! The state lives in [`crate::MfxState::look`], so the panel and the chat card agree.

use egui::{Align, Color32, FontId, Layout, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::new_project::MusicTheme;
use crate::new_project::music::{self as theme_page, MusicUi};
use crate::state::MfxState;
use crate::{MfxCtx, Service};

/// Most looks / shots read from the service (hostile input stays bounded).
const MAX_FRAMES: usize = 16;
const MAX_SHOTS: usize = 500;
/// A frame card is at least this wide before the grid adds a column.
const CARD_MIN_W: f32 = 280.0;
const GAP: f32 = 14.0;
/// Where a Browse… on the theme page delivers its file.
const PICK_THEME_REFERENCE: &str = "look.theme.reference";

fn text(v: &Value, key: &str, max: usize) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(max).collect()).unwrap_or_default()
}

fn rgb(c: Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

fn colour([r, g, b]: [u8; 3]) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// One look (a style frame) as `look.get` sends it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Frame {
    pub id: String,
    pub label: String,
    pub name: String,
    pub words: String,
    /// The picture of the version in use (absolute), empty while it is being made.
    pub path: String,
    pub versions: u32,
    pub state: String,
    /// A Fix on it is under way.
    pub pending: bool,
    /// Asked for with Make more looks, not made yet.
    pub making: bool,
    pub palette: Vec<[u8; 3]>,
    pub fonts: Vec<String>,
}

/// A shot and the look it was made in.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shot {
    pub id: String,
    pub label: String,
    pub title: String,
    pub beat_no: Option<u32>,
    pub colour: Option<[u8; 3]>,
    pub made: bool,
    pub cut: bool,
    pub look: String,
    /// Being remade because the look changed.
    pub changed: bool,
    pub remaking: bool,
}

/// `look.get`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct View {
    pub loaded: bool,
    pub using: Option<String>,
    pub frames: Vec<Frame>,
    pub shots: Vec<Shot>,
    pub can_more: bool,
}

impl View {
    /// From a `look.get` result; unknown or malformed fields are skipped.
    pub fn parse(v: &Value) -> Self {
        let list = |k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let hexes = |f: &Value| -> Vec<[u8; 3]> {
            f.get("palette").and_then(Value::as_array).into_iter().flatten().take(8).filter_map(crate::chat::model::hex_colour).map(rgb).collect()
        };
        let frames = list("frames")
            .iter()
            .take(MAX_FRAMES)
            .filter_map(|f| {
                let id = text(f, "id", 24);
                (!id.is_empty()).then(|| Frame {
                    label: text(f, "label", 80),
                    name: text(f, "name", 80),
                    words: text(f, "words", 400),
                    path: text(f, "path", 4096),
                    versions: f.get("versions").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                    state: text(f, "state", 24),
                    pending: f.get("pending").and_then(Value::as_bool).unwrap_or(false),
                    making: f.get("making").and_then(Value::as_bool).unwrap_or(false),
                    palette: hexes(f),
                    fonts: f
                        .get("fonts")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .take(4)
                        .filter_map(Value::as_str)
                        .map(|s| s.chars().take(60).collect())
                        .collect(),
                    id,
                })
            })
            .collect();
        let shots = list("shots")
            .iter()
            .take(MAX_SHOTS)
            .filter_map(|s| {
                let id = text(s, "id", 24);
                (!id.is_empty()).then(|| Shot {
                    label: text(s, "label", 40),
                    title: text(s, "title", 200),
                    beat_no: s.get("beat_no").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()),
                    colour: s.get("colour").and_then(crate::chat::model::hex_colour).map(rgb),
                    made: s.get("made").and_then(Value::as_bool).unwrap_or(false),
                    cut: s.get("cut").and_then(Value::as_bool).unwrap_or(false),
                    look: text(s, "look", 24),
                    changed: s.get("changed").and_then(Value::as_bool).unwrap_or(false),
                    remaking: s.get("remaking").and_then(Value::as_bool).unwrap_or(false),
                    id,
                })
            })
            .collect();
        let using = Some(text(v, "using", 24)).filter(|u| !u.is_empty());
        Self { loaded: true, using, frames, shots, can_more: v.get("can_more").and_then(Value::as_bool).unwrap_or(false) }
    }

    pub fn frame(&self, id: &str) -> Option<&Frame> {
        self.frames.iter().find(|f| f.id == id)
    }

    /// The look in use.
    pub fn in_use(&self) -> Option<&Frame> {
        self.using.as_deref().and_then(|u| self.frame(u))
    }

    /// A frame's name for people ("Cold neon", else its label).
    pub fn name_of(&self, id: &str) -> String {
        self.frame(id).map_or_else(|| id.to_uppercase(), |f| if f.name.is_empty() { f.label.clone() } else { f.name.clone() })
    }
}

/// "Using another look remakes these shots": waits for Change it / Cancel.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Confirm {
    pub frame: String,
    pub from: Option<String>,
    pub remake: Vec<Shot>,
}

/// The Look stage's state, shared by the Look panel and the chat's looks card.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LookState {
    pub view: View,
    /// `music.theme.get`'s theme (doc 33's shape), or null.
    pub theme: Value,
    /// Picking another look waits on "This change makes some things again".
    pub confirm: Option<Confirm>,
    /// The last pick's outcome or a read error, in plain words.
    pub note: String,
    /// The Music theme page is open (Change…).
    pub theme_open: bool,
    #[serde(skip)]
    theme_draft: Option<MusicTheme>,
    #[serde(skip)]
    theme_ui: MusicUi,
    #[serde(skip)]
    theme_choice: String,
    #[serde(skip)]
    loaded_for: Option<String>,
    #[serde(skip)]
    fetch_soon: bool,
    /// `look.get` and `music.theme.get` on their way.
    #[serde(skip)]
    fetching: (Option<u64>, Option<u64>),
    /// (request, frame, confirmed)
    #[serde(skip)]
    using_req: Option<(u64, String, bool)>,
    #[serde(skip)]
    ops: Vec<(u64, String)>,
}

impl LookState {
    /// Events that change what the Look shows: read it again.
    pub fn on_message(&mut self, msg: &Value) {
        if matches!(msg.get("event").and_then(Value::as_str), Some("look.changed" | "node.changed" | "script.changed")) {
            self.fetch_soon = true;
        }
    }

    /// Ask for the looks again soon (the service came back).
    pub fn refresh(&mut self) {
        self.fetch_soon = true;
    }

    /// A pick is on its way.
    pub fn busy(&self) -> bool {
        self.using_req.is_some()
    }
}

/// Use another look (a click on a frame here or in the chat's looks card). With shots made in the
/// old look, the service answers with what would be remade and [`confirm_popup`] asks first.
pub fn pick(state: &mut MfxState, service: &mut Service, frame: &str) {
    let st = &mut state.look;
    if st.using_req.is_some() || st.view.using.as_deref() == Some(frame) {
        return;
    }
    let id = service.request("look.use", json!({"frame": frame}));
    st.using_req = Some((id, frame.to_string(), false));
    st.note = format!("Using {}…", st.view.name_of(frame));
}

/// Every frame (from [`crate::Mortiflix::poll`]): follow the open project, read the looks and the
/// music theme when they changed, take the replies. Never blocks.
pub fn tick(state: &mut MfxState, service: &mut Service) {
    let st = &mut state.look;
    if st.loaded_for != state.project_id {
        *st = LookState { loaded_for: state.project_id.clone(), fetch_soon: state.project_id.is_some(), ..LookState::default() };
    }
    if st.fetch_soon && st.fetching == (None, None) && st.loaded_for.is_some() && service.status().is_connected() {
        st.fetch_soon = false;
        st.fetching = (Some(service.request("look.get", json!({}))), Some(service.request("music.theme.get", json!({}))));
    }
    if let Some(r) = st.fetching.0.and_then(|id| service.take_reply(id)) {
        st.fetching.0 = None;
        if r.ok {
            st.view = View::parse(&r.result);
            // the looks asked for are in
            if st.note.starts_with("Making") && !st.view.frames.iter().any(|f| f.making) {
                st.note = "The new looks are in: click one to use it.".into();
            }
        } else if r.error.get("code").and_then(Value::as_str) != Some("no_project") {
            st.note = format!("Couldn't read the looks: {}", text(&r.error, "message", 200));
        }
    }
    if let Some(r) = st.fetching.1.and_then(|id| service.take_reply(id)) {
        st.fetching.1 = None;
        if r.ok {
            st.theme = r.result.get("theme").cloned().unwrap_or(Value::Null);
        }
    }
    if let Some((id, frame, confirmed)) = st.using_req.clone()
        && let Some(r) = service.take_reply(id)
    {
        st.using_req = None;
        let name = st.view.name_of(&frame);
        if !r.ok {
            st.note = format!("Couldn't use {name}: {}", text(&r.error, "message", 200));
        } else if r.result.get("needs_confirm").and_then(Value::as_bool) == Some(true) && !confirmed {
            let shots = View::parse(&json!({"shots": r.result.get("remake").cloned().unwrap_or(Value::Null)})).shots;
            st.confirm = Some(Confirm { frame, from: Some(text(&r.result, "from", 24)).filter(|f| !f.is_empty()), remake: shots });
            st.note.clear();
        } else {
            let n = r.result.get("remaking").and_then(Value::as_array).map_or(0, Vec::len);
            st.note = match n {
                0 => format!("Using {name}."),
                1 => format!("Using {name} · remaking 1 shot in it."),
                n => format!("Using {name} · remaking {n} shots in it."),
            };
            st.view.using = Some(frame);
        }
        st.fetch_soon = true;
    }
    let mut done = false;
    st.ops.retain(|(id, what)| match service.take_reply(*id) {
        Some(r) => {
            st.note = if r.ok { what.clone() } else { format!("That didn't work: {}", text(&r.error, "message", 200)) };
            done = true;
            false
        }
        None => true,
    });
    if done {
        st.fetch_soon = true;
    }
}

/// Stage 2's main panel (PanelKind::MfxStage on stage 2).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let full = ui.max_rect();
    let view = ctx.state.look.view.clone();
    let made = view.frames.iter().filter(|f| !f.making).count();
    ui.allocate_ui_with_layout(vec2(full.width(), 28.0), Layout::left_to_right(Align::Center), |ui| {
        ui.label(RichText::new("Look").size(15.0).color(t.text).strong());
        if made > 0 {
            let using = view.using.as_deref().map(|u| format!(" · using {}", view.name_of(u))).unwrap_or_default();
            ui.label(RichText::new(format!("{made} look{}{using}", if made == 1 { "" } else { "s" })).size(12.0).color(t.text_faint));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let making = view.frames.iter().any(|f| f.making);
            let enabled = view.can_more && !making && made > 0;
            let b = ui
                .add_enabled(enabled, egui::Button::new(RichText::new("Make more looks").size(12.0).color(t.text)).fill(t.hover).corner_radius(4.0))
                .on_hover_text("The AI makes two more looks to pick from; the ones here stay")
                .on_disabled_hover_text(if making { "The new looks are being made" } else { "No more looks can be added" });
            ctx.auto("mfx.look.more", &b, "Make more looks");
            if b.clicked() {
                let id = ctx.service.request("look.more", json!({}));
                ctx.state.look.ops.push((id, "Making 2 more looks: they appear here when they're ready.".into()));
                ctx.state.look.note = "Asking for more looks…".into();
            }
        });
    });
    // what these are (owner's first run: the four looks were taken for the shots)
    ui.add(
        egui::Label::new(
            RichText::new("These set the style: colours, characters, type. They are not the shots; the shots come next, one per line of the script.")
                .size(12.0)
                .color(t.text_dim),
        )
        .wrap(),
    );
    if crate::service_notice(ui, ctx) {
        return;
    }
    let top = ui.min_rect().max.y + 6.0;
    let body = Rect::from_min_max(pos2(full.min.x, top), pos2(full.max.x, full.max.y.max(top)));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
    egui::ScrollArea::vertical().id_salt("mfx-look-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
        ui.set_width(body.width() - 12.0);
        if view.frames.is_empty() {
            let words = if view.loaded { "The looks come with the first style frames." } else { "Reading the looks…" };
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(words).size(13.0).color(t.text_faint)));
        } else {
            // room for the outline of the look in use
            ui.add_space(4.0);
            frames(ui, ctx, &view);
            note(ui, ctx);
            ui.add_space(12.0);
            in_use(ui, ctx, &view);
        }
        ui.add_space(14.0);
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().min.y, Stroke::new(1.0, t.separator));
        ui.add_space(10.0);
        music_row(ui, ctx);
    });
    ui.allocate_rect(full, Sense::hover());
}

fn note(ui: &mut egui::Ui, ctx: &MfxCtx) {
    let st = &ctx.state.look;
    if !st.note.is_empty() {
        let col = if st.note.starts_with("Couldn't") || st.note.starts_with("That didn't") { ctx.theme.error } else { ctx.theme.text_dim };
        ui.add_space(6.0);
        ui.label(RichText::new(&st.note).size(12.0).color(col));
    }
}

/// The frames, large, in a grid (2 columns on a normal panel); the one in use ticked and outlined.
fn frames(ui: &mut egui::Ui, ctx: &mut MfxCtx, view: &View) {
    let t = ctx.theme;
    let w = ui.available_width();
    let cols = (((w + GAP) / (CARD_MIN_W + GAP)).floor() as usize).clamp(1, 3);
    let card_w = ((w - GAP * (cols as f32 - 1.0)) / cols as f32).max(80.0);
    let busy = ctx.state.look.busy();
    for row in view.frames.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for f in row {
                ui.vertical(|ui| {
                    ui.set_width(card_w);
                    let on = view.using.as_deref() == Some(f.id.as_str());
                    let name = if f.name.is_empty() { f.label.clone() } else { f.name.clone() };
                    let r = picture(ui, ctx, card_w, &f.path, on, !f.making);
                    let r = if f.making {
                        r.on_hover_text("Being made")
                    } else if on {
                        r.on_hover_text(format!("{name} is the look in use"))
                    } else {
                        r.on_hover_text(format!("Use {name}"))
                    };
                    if r.clicked() && !on && !f.making && !busy {
                        let MfxCtx { state, service, .. } = ctx;
                        pick(state, service, &f.id);
                    }
                    ctx.auto(&format!("mfx.look.frame.{}", f.id), &r, &name);
                    ui.add_space(2.0);
                    ui.allocate_ui_with_layout(vec2(card_w, 20.0), Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let shown = if on { format!("✓ {name}") } else { name.clone() };
                        ui.add(egui::Label::new(RichText::new(shown).size(13.0).color(if on { t.text } else { t.text_dim }).strong()).truncate());
                        if on {
                            ui.label(RichText::new("in use").size(11.0).color(t.text_faint));
                        }
                        if !f.making {
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let ask = ui
                                    .add(
                                        egui::Label::new(RichText::new("Ask for changes").size(11.5).color(crate::chat::LINK).underline())
                                            .sense(Sense::click()),
                                    )
                                    .on_hover_text(format!("Talk about {name} in the chat: what you type remakes this look"));
                                if ask.clicked() {
                                    ctx.state.selected_node = Some(f.id.clone());
                                    ctx.state.focus_chat = true;
                                }
                                ctx.auto(&format!("mfx.look.ask.{}", f.id), &ask, "Ask for changes");
                            });
                        }
                    });
                    let status = if f.making {
                        "being made…".to_string()
                    } else if f.pending {
                        "changing it…".to_string()
                    } else {
                        let mut s = format!("Style sample · {}", f.label);
                        if f.versions > 1 {
                            s.push_str(&format!(" · v{}", f.versions));
                        }
                        s
                    };
                    ui.label(RichText::new(status).size(11.0).color(if f.making || f.pending { t.accent } else { t.text_faint }));
                });
            }
        });
        ui.add_space(GAP);
    }
}

/// A fixed 16:9 frame with the picture cropped to fill it (nothing drawn over it); `on` = the look
/// in use (outlined), else hover outlines it.
fn picture(ui: &mut egui::Ui, ctx: &MfxCtx, w: f32, path: &str, on: bool, clickable: bool) -> egui::Response {
    let t = ctx.theme;
    let (r, resp) = ui.allocate_exact_size(vec2(w, w * 9.0 / 16.0), if clickable { Sense::click() } else { Sense::hover() });
    ui.painter().rect_filled(r, 4.0, t.field_bg);
    let p = std::path::Path::new(path);
    if !path.is_empty() && p.is_absolute() {
        let stills = crate::nodes::stills::Stills::large(ui.ctx());
        stills.poll(ui.ctx());
        match stills.get(p) {
            Some(tex) => {
                let [tw, th] = tex.size().map(|x| x.max(1) as f32);
                let (fw, fh) = (r.width().max(1.0), r.height().max(1.0));
                let (sx, sy) = if tw / th > fw / fh { ((fw / fh) / (tw / th), 1.0) } else { (1.0, (tw / th) / (fw / fh)) };
                let uv = Rect::from_min_max(pos2((1.0 - sx) / 2.0, (1.0 - sy) / 2.0), pos2((1.0 + sx) / 2.0, (1.0 + sy) / 2.0));
                ui.painter().image(tex.id(), r, uv, Color32::WHITE);
            }
            None => {
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Loading…", FontId::proportional(12.0), t.text_faint);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
            }
        }
    } else {
        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "Being made…", FontId::proportional(12.0), t.text_faint);
    }
    if on {
        ui.painter().rect_stroke(r.expand(3.0), 6.0, Stroke::new(2.0, t.text), StrokeKind::Outside);
    } else if resp.hovered() && clickable {
        ui.painter().rect_stroke(r.expand(3.0), 6.0, Stroke::new(1.0, t.text_faint), StrokeKind::Outside);
    }
    resp
}

/// The look in use: its words, palette swatches and fonts, and the shots made in it.
fn in_use(ui: &mut egui::Ui, ctx: &mut MfxCtx, view: &View) {
    let t = ctx.theme;
    let Some(f) = view.in_use() else { return };
    let name = if f.name.is_empty() { f.label.clone() } else { f.name.clone() };
    ui.horizontal(|ui| {
        ui.label(RichText::new("The look in use").size(12.0).color(t.text_faint));
        ui.label(RichText::new(&name).size(13.0).color(t.text).strong());
    });
    if !f.words.is_empty() {
        ui.add(egui::Label::new(RichText::new(&f.words).size(12.0).color(t.text_dim)).wrap());
    }
    // the service's palette, else the colours read from the picture itself
    let palette: Vec<Color32> = if f.palette.is_empty() {
        crate::nodes::stills::Stills::large(ui.ctx()).palette(std::path::Path::new(&f.path)).unwrap_or_default()
    } else {
        f.palette.iter().copied().map(colour).collect()
    };
    ui.add_space(6.0);
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(vec2(70.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new("Palette").size(12.0).color(t.text_dim));
        });
        if palette.is_empty() {
            ui.label(RichText::new("comes with the picture").size(12.0).color(t.text_faint));
        }
        ui.spacing_mut().item_spacing.x = 8.0;
        for (k, c) in palette.iter().enumerate() {
            ui.vertical(|ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(44.0, 28.0), Sense::hover());
                ui.painter().rect_filled(r, 3.0, *c);
                ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, t.separator.gamma_multiply(2.0)), StrokeKind::Inside);
                let hex = format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b());
                ui.label(RichText::new(&hex).size(10.5).color(t.text_faint).monospace());
                ctx.auto(&format!("mfx.look.swatch.{k}"), &resp, &hex);
            });
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.allocate_ui_with_layout(vec2(70.0, 20.0), Layout::left_to_right(Align::Center), |ui| {
            ui.label(RichText::new("Fonts").size(12.0).color(t.text_dim));
        });
        if f.fonts.is_empty() {
            ui.label(RichText::new("the AI picks them with the look").size(12.0).color(t.text_faint));
        }
        for (k, font) in f.fonts.iter().enumerate() {
            if k > 0 {
                ui.label(RichText::new("·").size(12.0).color(t.text_faint));
            }
            ui.label(RichText::new(font).size(13.0).color(t.text));
        }
    });
    let made: Vec<&Shot> = view.shots.iter().filter(|s| s.made && !s.cut).collect();
    let changing = made.iter().filter(|s| s.changed).count();
    if !made.is_empty() {
        ui.add_space(4.0);
        let words = if changing > 0 {
            format!("{changing} of {} shots are being remade in this look · the old versions are kept", made.len())
        } else {
            let here = made.iter().filter(|s| view.using.as_deref() == Some(s.look.as_str())).count();
            format!("{here} of {} shots are made in this look", made.len())
        };
        ui.label(RichText::new(words).size(12.0).color(if changing > 0 { t.accent } else { t.text_faint }));
    }
}

/// The music theme row (doc 33: part of the look): the theme in use, its samples (▶ plays one,
/// pick one to use it), Change… opens the Music theme page.
fn music_row(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let theme = ctx.state.look.theme.clone();
    let (name, summary) = summary(&theme);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Music theme").size(12.0).color(t.text_faint));
        ui.label(RichText::new(name).size(13.0).color(t.text).strong());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let b = ui.add(egui::Button::new(RichText::new("Change…").size(12.0).color(t.text)).fill(t.hover).corner_radius(4.0));
            ctx.auto("mfx.look.theme.change", &b, "Change…");
            if b.clicked() {
                let st = &mut ctx.state.look;
                st.theme_draft = Some(serde_json::from_value::<MusicTheme>(st.theme.clone()).unwrap_or_default());
                st.theme_ui = MusicUi::default();
                st.theme_open = true;
            }
        });
    });
    if !summary.is_empty() {
        ui.add(egui::Label::new(RichText::new(summary).size(12.0).color(t.text_dim)).wrap());
    }
    if !theme.is_object() {
        ui.label(RichText::new("The music theme comes after the look.").size(12.0).color(t.text_faint));
        return;
    }
    let using = text(&theme, "using", 40);
    let samples = theme.get("samples").and_then(Value::as_array).cloned().unwrap_or_default();
    if samples.is_empty() {
        ui.label(RichText::new("Its three 15 s samples come with the theme.").size(12.0).color(t.text_faint));
    }
    for s in samples.iter().take(6) {
        let id = text(s, "id", 40);
        if id.is_empty() {
            continue;
        }
        let name = Some(text(s, "name", 80)).filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone());
        let wav = text(s, "wav", 4096);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 24.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let play = ui
                .add_enabled(!wav.is_empty(), egui::Button::new(RichText::new("▶").size(11.0)).min_size(vec2(22.0, 20.0)).fill(t.hover).corner_radius(10.0))
                .on_hover_text("Hear it (15 s)")
                .on_disabled_hover_text("Not made yet");
            if play.clicked() {
                // the sample's own audio file, in the system player (as the chat's themes card does)
                ctx.command("mortiflix.reveal", json!({"path": wav, "open": true}));
            }
            ctx.auto(&format!("mfx.look.theme.sample.{id}.play"), &play, "Play");
            let here = id == using;
            let r = ui.radio(here, RichText::new(&name).size(12.5));
            ctx.auto(&format!("mfx.look.theme.sample.{id}"), &r, &name);
            if r.clicked() && !here {
                let rid = ctx.service.request("music.theme.use", json!({"sample": id}));
                ctx.state.look.ops.push((rid, format!("Music theme: using {name}.")));
            }
            if here {
                ui.label(RichText::new("Using this").size(11.5).color(t.text_dim));
            }
            let desc = text(s, "desc", 120);
            if !desc.is_empty() {
                ui.add(egui::Label::new(RichText::new(desc).size(11.5).color(t.text_faint)).truncate());
            }
        });
    }
}

/// The theme in words: (name, summary).
fn summary(theme: &Value) -> (String, String) {
    if !theme.is_object() {
        return ("Not picked yet".into(), String::new());
    }
    let mut tl = crate::voice_music::model::Timeline::default();
    crate::voice_music::model::apply_theme(&mut tl, &json!({ "theme": theme }));
    tl.theme.unwrap_or_else(|| ("The AI's pick".into(), String::new()))
}

/// The pop-ups of the Look stage, drawn wherever Mortiflix is on (from the stage bar): "This change
/// makes some things again" and the Music theme page (Change…).
pub fn popups(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    confirm_popup(ui, ctx);
    theme_window(ui, ctx);
}

/// Doc 30's "This change makes some things again": using another look remakes the shots made in
/// the old one. Lists them; nothing changes until Change it.
pub fn confirm_popup(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let Some(c) = ctx.state.look.confirm.clone() else { return };
    let t = ctx.theme;
    let to = ctx.state.look.view.name_of(&c.frame);
    let from = c.from.as_deref().map(|f| ctx.state.look.view.name_of(f));
    let modal = egui::Modal::new(egui::Id::new("mfx-look-confirm")).show(ui.ctx(), |ui| {
        ui.set_width(460.0);
        ui.heading("This change makes some things again");
        let n = c.remake.len();
        let shots = if n == 1 { "1 shot".to_string() } else { format!("{n} shots") };
        let lead = match &from {
            Some(f) => format!("Using {to} instead of {f} remakes {shots} in the new look:"),
            None => format!("Using {to} remakes {shots} in the new look:"),
        };
        ui.label(lead);
        ui.add_space(4.0);
        for s in c.remake.iter().take(12) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if let Some(no) = s.beat_no {
                    let (r, _) = ui.allocate_exact_size(vec2(9.0, 14.0), Sense::hover());
                    ui.painter().rect_filled(
                        Rect::from_center_size(pos2(r.center().x, r.center().y + 1.0), vec2(8.0, 8.0)),
                        1.5,
                        s.colour.map_or(t.text_faint, colour),
                    );
                    ui.label(RichText::new(format!("{no}")).size(12.0).color(t.text_dim));
                }
                ui.label(RichText::new(&s.label).size(12.0).color(t.text));
                ui.add(egui::Label::new(RichText::new(&s.title).size(12.0).color(t.text_faint)).truncate());
            });
        }
        if n > 12 {
            ui.label(RichText::new(format!("and {} more", n - 12)).size(12.0).color(t.text_faint));
        }
        ui.add_space(4.0);
        ui.label(RichText::new("The old versions are kept: you can go back to any of them.").size(12.0).color(t.text_dim));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let cancel = ui.button("Cancel");
            ctx.auto("mfx.look.confirm.cancel", &cancel, "Cancel");
            if cancel.clicked() {
                ctx.state.look.confirm = None;
            }
            let go = ui.add(egui::Button::new(RichText::new("Change it").color(Color32::WHITE)).fill(t.accent));
            ctx.auto("mfx.look.confirm.change", &go, "Change it");
            if go.clicked() {
                ctx.state.look.confirm = None;
                let id = ctx.service.request("look.use", json!({"frame": c.frame, "confirm": true}));
                ctx.state.look.using_req = Some((id, c.frame.clone(), true));
                ctx.state.look.note = format!("Using {to}…");
            }
        });
    });
    if modal.should_close() {
        ctx.state.look.confirm = None;
    }
}

/// Change…: the Music theme page (doc 33a-2) in its own small window, page 2 only. Done saves the
/// theme (`music.theme.set`); closing drops the changes.
fn theme_window(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    if !ctx.state.look.theme_open {
        return;
    }
    let mut open = true;
    let title = if ctx.state.project_title.is_empty() { "Music theme".to_string() } else { format!("Music theme · {}", ctx.state.project_title) };
    let mut done = false;
    egui::Window::new(title).id(egui::Id::new("mfx-look-theme")).open(&mut open).collapsible(false).resizable(false).default_width(640.0).show(
        ui.ctx(),
        |ui| {
            let st = &mut ctx.state.look;
            let mut draft = st.theme_draft.take().unwrap_or_default();
            let mut mu = std::mem::take(&mut st.theme_ui);
            let mut choice = std::mem::take(&mut st.theme_choice);
            theme_page::poll(ctx, &mut mu, &mut draft);
            done = theme_page::page(ui, ctx, &mut mu, &mut draft, &[], &[], &mut choice, PICK_THEME_REFERENCE);
            let st = &mut ctx.state.look;
            st.theme_draft = Some(draft);
            st.theme_ui = mu;
            st.theme_choice = choice;
        },
    );
    if done {
        let st = &mut ctx.state.look;
        if let Some(theme) = st.theme_draft.take() {
            let v = serde_json::to_value(&theme).unwrap_or(Value::Null);
            let id = ctx.service.request("music.theme.set", json!({ "theme": v }));
            ctx.state.look.ops.push((id, "Music theme saved.".into()));
        }
        ctx.state.look.theme_open = false;
    } else if !open {
        ctx.state.look.theme_open = false;
        ctx.state.look.theme_draft = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({"using": "sf02", "can_more": true, "frames": [
            {"id": "sf01", "label": "SF01 · Open", "name": "Ember night", "path": "/x/sf01.svg", "palette": ["#0f172a", "#F97316", "nope", 7], "fonts": ["sans-serif"]},
            {"id": "sf02", "label": "SF02 · Ideas", "name": "Cold neon", "path": "/x/sf02.svg", "versions": 2},
            {"id": "sf05", "label": "SF05", "making": true},
            {"label": "no id"}, "junk"
        ], "shots": [
            {"id": "s01", "label": "S01", "beat_no": 1, "colour": {"hex": "#380ea7"}, "made": true, "look": "sf02"},
            {"id": "s02", "label": "S02", "made": true, "look": "sf01", "changed": true, "remaking": true}
        ]})
    }

    #[test]
    fn parses_the_looks_and_skips_junk() {
        let v = View::parse(&sample());
        assert!(v.loaded && v.can_more);
        assert_eq!(v.frames.len(), 3);
        assert_eq!(v.frames[0].palette, vec![[0x0f, 0x17, 0x2a], [0xf9, 0x73, 0x16]]);
        assert_eq!(v.in_use().map(|f| f.name.as_str()), Some("Cold neon"));
        assert!(v.frames[2].making);
        assert_eq!(v.name_of("sf09"), "SF09");
        assert_eq!(v.shots[0].colour, Some([0x38, 0x0e, 0xa7]));
        assert!(v.shots[1].changed);
        assert_eq!(View::parse(&json!(7)).frames.len(), 0);
    }

    /// Owner: shots made from the old look are remade only after the person confirms, and the
    /// list of what gets remade comes first.
    #[test]
    fn picking_another_look_asks_first_then_confirms() {
        let (mut service, sent) = Service::fake();
        let mut state = MfxState { project_id: Some("p1".into()), ..MfxState::default() };
        tick(&mut state, &mut service);
        let get = sent.try_iter().find(|m| m["type"] == "look.get").unwrap();
        service.answer(get["id"].as_u64().unwrap(), true, sample());
        tick(&mut state, &mut service);
        assert_eq!(state.look.view.using.as_deref(), Some("sf02"));
        pick(&mut state, &mut service, "sf02");
        assert!(sent.try_iter().all(|m| m["type"] != "look.use"), "the look in use: nothing to send");
        pick(&mut state, &mut service, "sf01");
        let ask = sent.try_iter().find(|m| m["type"] == "look.use").unwrap();
        assert_eq!(ask["params"], json!({"frame": "sf01"}));
        service.answer(
            ask["id"].as_u64().unwrap(),
            true,
            json!({"needs_confirm": true, "from": "sf02", "to": "sf01", "remake": [{"id": "s01", "label": "S01", "beat_no": 1, "made": true}]}),
        );
        tick(&mut state, &mut service);
        let c = state.look.confirm.clone().unwrap();
        assert_eq!((c.frame.as_str(), c.from.as_deref(), c.remake.len()), ("sf01", Some("sf02"), 1));
        assert_eq!(state.look.view.using.as_deref(), Some("sf02"), "nothing changed before Change it");
        // Change it
        let id = service.request("look.use", json!({"frame": "sf01", "confirm": true}));
        state.look.confirm = None;
        state.look.using_req = Some((id, "sf01".into(), true));
        service.answer(id, true, json!({"using": "sf01", "changed": true, "remaking": [{"node": "s01", "edit": "E009"}]}));
        tick(&mut state, &mut service);
        assert_eq!(state.look.view.using.as_deref(), Some("sf01"));
        assert_eq!(state.look.note, "Using Ember night · remaking 1 shot in it.");
        assert!(state.look.confirm.is_none());
    }

    #[test]
    fn events_ask_for_the_looks_again() {
        let mut st = LookState::default();
        st.on_message(&json!({"event": "chat.message", "data": {}}));
        assert!(!st.fetch_soon);
        st.on_message(&json!({"event": "look.changed", "data": {}}));
        assert!(st.fetch_soon);
    }
}
