//! The loose preview and Play: FilmCraft's own monitor, no render, best media per shot (doc 36,
//! mockups `UI Layout/36a Loose preview.html`, `36b Loose preview - states.html`).
//!
//! One panel plays everything made so far as one video, in film order, at the planned timing: a
//! shot with motion plays its animatic piece, a shot with only a still holds it for its length, a
//! shot not made yet is a dark card in its beat's colour, cut shots are skipped, voice and music
//! play under it. The picture is FilmCraft's Program monitor playing a hidden sequence, "Loose
//! preview", that the host keeps in step with the graph ([`plan::Plan`] is its edit list), so
//! nothing is rendered to watch it.
//!
//! This module owns the plan (the service's `preview.plan` + `graph.get`), the panel's controls (one Play with All /
//! From here / This shot / This beat and Loop, the beats / shots / voice / music bar, the caption)
//! and [`PreviewState`]. The host (`filmcraft-ui-egui`, `mortiflix_preview.rs`) builds the
//! sequence, draws the picture in [`Layout::picture`], fills [`Player`] every frame and runs the
//! `mortiflix.preview.*` commands the controls push. FilmCraft's own keys drive the monitor
//! (Space, J/K/L, ←/→ one frame, ↑/↓ previous/next shot, Home/End, I/O); Ctrl+L / ⌘L is Loop.

pub mod plan;

use std::path::Path;

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use plan::{Media, Plan, PlanBeat, PlanShot, PlanSound};

use crate::{MfxCtx, MfxState, Service};

/// The FilmCraft sequence the preview plays (kept in the project bin, doc 36 "Out").
pub const SEQUENCE_NAME: &str = "Loose preview";
/// The graph is fetched again at least this often (seconds) while a project is open, and at once
/// after a service event.
pub const REFRESH_SECS: f64 = 10.0;
/// FilmCraft's playhead colour (the "under the playhead" outline and the bar's playhead).
pub const PLAYHEAD: Color32 = Color32::from_rgb(0x58, 0x95, 0xec);

const HEADER_H: f32 = 28.0;
const CAPTION_H: f32 = 22.0;
const TRANSPORT_H: f32 = 32.0;
const GAP: f32 = 8.0;
const GUTTER: f32 = 56.0;
const BEATS_H: f32 = 20.0;
const SHOTS_H: f32 = 40.0;
const SOUND_H: f32 = 12.0;
const ROW_GAP: f32 = 4.0;
const BAR_H: f32 = BEATS_H + ROW_GAP + SHOTS_H + ROW_GAP + SOUND_H + ROW_GAP + SOUND_H;

/// What Play plays (doc 36). Picking one doesn't start playback.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scope {
    All,
    /// From the playhead to the end (the default).
    #[default]
    FromHere,
    /// The selected node's shot (or the shot under the playhead).
    Shot,
    /// The selected node's beat (or the beat under the playhead).
    Beat,
}

impl Scope {
    pub const ALL: [Scope; 4] = [Scope::All, Scope::FromHere, Scope::Shot, Scope::Beat];
    pub fn key(self) -> &'static str {
        match self {
            Scope::All => "all",
            Scope::FromHere => "fromHere",
            Scope::Shot => "shot",
            Scope::Beat => "beat",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Scope::All => "All",
            Scope::FromHere => "From here",
            Scope::Shot => "This shot",
            Scope::Beat => "This beat",
        }
    }
    pub fn parse(s: &str) -> Option<Scope> {
        // the service says `from` for From here
        let s = if s.eq_ignore_ascii_case("from") { "fromHere" } else { s };
        Scope::ALL.into_iter().find(|x| x.key().eq_ignore_ascii_case(s) || x.label().eq_ignore_ascii_case(s))
    }
}

/// FilmCraft's playback as the host saw it this frame (read-only here).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Player {
    /// The "Loose preview" sequence is built and on the Program monitor.
    pub ready: bool,
    pub playing: bool,
    pub speed: f64,
    /// Playhead and the sequence's length, seconds (display only; the host edits in ticks).
    pub time: f64,
    pub duration: f64,
    pub fps: f64,
    /// FilmCraft's timecodes (`HH:MM:SS:FF`).
    pub timecode: String,
    pub total: String,
    /// Playback resolution ("1/2") and monitor zoom ("Fit").
    pub res: String,
    pub zoom: String,
    /// In and Out are marked on the sequence: Loop loops them instead of the scope.
    pub marked: bool,
}

/// What the preview remembers between frames (in [`MfxState::preview`], so `mortiflix.state` shows it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PreviewState {
    pub scope: Scope,
    pub looping: bool,
    /// The node area keeps the shot under the playhead in view (its header holds the toggle).
    pub follow_playhead: bool,
    pub plan: Plan,
    pub player: Player,
    /// The shot under the playhead (the node area outlines it in the playhead colour).
    pub shot_at_playhead: Option<String>,
    /// The last problem building the sequence (shown in the caption).
    pub problem: Option<String>,
    #[serde(skip)]
    fetch: Fetch,
}

impl Default for PreviewState {
    fn default() -> Self {
        Self {
            scope: Scope::default(),
            looping: false,
            follow_playhead: true,
            plan: Plan::default(),
            player: Player::default(),
            shot_at_playhead: None,
            problem: None,
            fetch: Fetch::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Fetch {
    plan: Option<u64>,
    graph: Option<u64>,
    /// The last `graph.get` result (kept when a later one fails).
    graph_value: Value,
    plan_value: Option<Value>,
    last: f64,
    project: Option<String>,
    event_seq: u64,
}

impl Default for Fetch {
    fn default() -> Self {
        Self { plan: None, graph: None, graph_value: Value::Null, plan_value: None, last: f64::NEG_INFINITY, project: None, event_seq: 0 }
    }
}

impl PreviewState {
    /// Fetch the graph again on the next poll.
    pub fn refetch(&mut self) {
        self.fetch.last = f64::NEG_INFINITY;
    }

    /// The (start, end) seconds Play plays for `scope`, from the playhead `at` and the selected node.
    pub fn range(&self, scope: Scope, at: f64, selected: Option<&str>) -> (f64, f64) {
        let total = self.plan.total.max(0.0);
        let under = self.plan.shot_at(at);
        match scope {
            Scope::All => (0.0, total),
            Scope::FromHere => (at.clamp(0.0, total), total),
            Scope::Shot => {
                let s = selected.and_then(|id| self.plan.shot(id).or_else(|| self.plan.span_of(id).and_then(|(t, _)| self.plan.shot_at(t)))).or(under);
                s.map_or((0.0, total), |s| (s.start, s.end()))
            }
            Scope::Beat => {
                let beat = selected
                    .and_then(|id| self.plan.beat(id).map(|b| b.id.clone()).or_else(|| self.plan.shot(id).and_then(|s| s.beat.clone())))
                    .or_else(|| under.and_then(|s| s.beat.clone()));
                beat.and_then(|b| self.plan.beat(&b)).map_or((0.0, total), |b| (b.start, b.end))
            }
        }
    }
}

/// Keep the plan fresh: ask the service for `preview.plan` (the edit list) and `graph.get` (beat
/// names, looks, shots being remade) when the project changes, soon after a `node.changed` or
/// `status` event, and every [`REFRESH_SECS`]. Called by the host every frame; cheap (nothing is
/// sent while replies are pending, and nothing at all while the service is down).
pub fn poll(state: &mut MfxState, service: &mut Service, now: f64) {
    let connected = service.status().is_connected();
    let pv = &mut state.preview;
    if !connected {
        pv.fetch.plan = None;
        pv.fetch.graph = None;
        return;
    }
    if pv.fetch.project != state.project_id {
        pv.fetch.project = state.project_id.clone();
        pv.fetch.last = f64::NEG_INFINITY;
        pv.fetch.graph_value = Value::Null;
        pv.fetch.plan_value = None;
        pv.plan = Plan::default();
    }
    let mut seq = pv.fetch.event_seq;
    let mut changed = false;
    for (s, e) in service.events_since(pv.fetch.event_seq) {
        seq = seq.max(*s);
        changed |= matches!(e.get("event").and_then(Value::as_str), Some("node.changed" | "status" | "project.changed"));
    }
    pv.fetch.event_seq = seq;
    if changed {
        // soon, not at once: a burst of events makes one fetch
        pv.fetch.last = pv.fetch.last.min(now - REFRESH_SECS + 0.3);
    }
    if let Some(id) = pv.fetch.graph
        && let Some(r) = service.take_reply(id)
    {
        pv.fetch.graph = None;
        if r.ok {
            pv.fetch.graph_value = r.result;
        }
    }
    if let Some(id) = pv.fetch.plan
        && let Some(r) = service.take_reply(id)
    {
        pv.fetch.plan = None;
        if r.ok {
            pv.fetch.plan_value = Some(r.result);
        } else {
            log::info!("loose preview: preview.plan: {}", r.error);
        }
    }
    if pv.fetch.plan.is_none()
        && pv.fetch.graph.is_none()
        && let Some(plan) = pv.fetch.plan_value.take()
    {
        let graph = Some(&pv.fetch.graph_value).filter(|g| g.is_object());
        let exists = |f: &str| Path::new(f).is_file();
        pv.plan = Plan::from_service(&plan, graph, &exists);
    }
    if state.project_id.is_some() && pv.fetch.plan.is_none() && pv.fetch.graph.is_none() && now - pv.fetch.last >= REFRESH_SECS {
        pv.fetch.last = now;
        pv.fetch.plan = Some(service.request("preview.plan", json!({"scope": "all"})));
        pv.fetch.graph = Some(service.request("graph.get", json!({})));
    }
}

/// Where the panel's parts go in `rect` (the panel's inner rect). The host draws the Program
/// monitor's picture in `picture` before the controls are drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub header: Rect,
    pub picture: Rect,
    pub caption: Rect,
    pub bar: Rect,
    pub transport: Rect,
}

pub fn layout(rect: Rect) -> Layout {
    // top-down for the header, bottom-up for the rest; a short panel squeezes the picture first,
    // and every part stays inside `rect` with a height of zero or more
    let (top, bottom) = (rect.min.y, rect.max.y.max(rect.min.y));
    let y = |v: f32| v.clamp(top, bottom);
    let row = |a: f32, b: f32| Rect::from_min_max(pos2(rect.min.x, y(a)), pos2(rect.max.x, y(b).max(y(a))));
    let header = row(top, top + HEADER_H);
    let floor = header.max.y;
    let below = |r: Rect| row(r.min.y.max(floor), r.max.y.max(floor));
    let transport = below(row(bottom - TRANSPORT_H, bottom));
    let bar = below(row(transport.min.y - GAP - BAR_H, transport.min.y - GAP));
    let caption = below(row(bar.min.y - GAP - CAPTION_H, bar.min.y - GAP));
    let picture = row(floor + 4.0, caption.min.y - GAP);
    Layout { header, picture, caption, bar, transport }
}

fn hex(c: &str) -> Color32 {
    plan::parse_hex(c).map_or(Color32::from_gray(58), |[r, g, b]| Color32::from_rgb(r, g, b))
}

/// The preview panel (PanelKind::MfxPreview), in the middle on stages 1-4.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let l = layout(ui.max_rect());
    header(ui, ctx, l.header);
    picture_notice(ui, ctx, l.picture);
    caption(ui, ctx, l.caption);
    bar(ui, ctx, l.bar);
    transport(ui, ctx, l.transport);
}

fn header(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect) {
    let t = ctx.theme;
    let p = ui.painter_at(r);
    let title = p.layout_no_wrap("Loose preview".into(), FontId::proportional(13.0), t.text);
    let tw = title.size().x;
    p.galley(pos2(r.min.x, r.center().y - title.size().y / 2.0), title, t.text);
    let tag = p.layout_no_wrap("everything made so far".into(), FontId::proportional(11.0), t.text_dim);
    let tag_r = Rect::from_min_size(pos2(r.min.x + tw + 10.0, r.center().y - 10.0), vec2(tag.size().x + 16.0, 20.0));
    p.rect_filled(tag_r, 10.0, t.hover);
    p.galley(pos2(tag_r.min.x + 8.0, tag_r.center().y - tag.size().y / 2.0), tag, t.text_dim);
    // right: Plays at ▾  Fit ▾ (FilmCraft's playback resolution and monitor zoom)
    let pl = &ctx.state.preview.player;
    let res_label = if pl.res.is_empty() { "1/2".to_string() } else { pl.res.clone() };
    let zoom_label = if pl.zoom.is_empty() { "Fit".to_string() } else { pl.zoom.clone() };
    let zoom_r = Rect::from_min_size(pos2(r.max.x - 64.0, r.center().y - 12.0), vec2(64.0, 24.0));
    let res_r = Rect::from_min_size(pos2(zoom_r.min.x - 72.0, zoom_r.min.y), vec2(64.0, 24.0));
    if res_r.min.x > tag_r.max.x + 70.0 {
        p.text(pos2(res_r.min.x - 6.0, r.center().y), Align2::RIGHT_CENTER, "Plays at:", FontId::proportional(12.0), t.text_faint);
    }
    let res = dropdown(ui, ctx, res_r, &res_label, "mfx.preview.res", "Plays at (playback resolution)");
    egui::Popup::menu(&res).show(|ui| {
        for (label, cmd) in [("Full", "view.playbackRes.full"), ("1/2 (Half)", "view.playbackRes.half"), ("1/4 (Quarter)", "view.playbackRes.quarter")] {
            if ui.button(label).clicked() {
                ctx.command(cmd, json!({}));
            }
        }
    });
    let zoom = dropdown(ui, ctx, zoom_r, &zoom_label, "mfx.preview.zoom", "Monitor zoom");
    egui::Popup::menu(&zoom).show(|ui| {
        for (label, cmd) in [("Fit", "view.programZoomFit"), ("100%", "view.programZoom100")] {
            if ui.button(label).clicked() {
                ctx.command(cmd, json!({}));
            }
        }
    });
}

fn dropdown(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect, label: &str, auto_id: &str, what: &str) -> egui::Response {
    let t = ctx.theme;
    let resp = ui.interact(r, egui::Id::new(auto_id), Sense::click()).on_hover_text(what);
    let p = ui.painter();
    p.rect_filled(r, 3.0, if resp.hovered() { t.hover } else { t.field_bg });
    p.rect_stroke(r, 3.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
    p.with_clip_rect(r.shrink(2.0)).text(pos2(r.min.x + 8.0, r.center().y), Align2::LEFT_CENTER, label, FontId::proportional(12.0), t.text_dim);
    let c = pos2(r.max.x - 10.0, r.center().y);
    p.add(Shape::convex_polygon(vec![pos2(c.x - 3.5, c.y - 2.0), pos2(c.x + 3.5, c.y - 2.0), pos2(c.x, c.y + 2.5)], t.text_dim, Stroke::NONE));
    ctx.auto(auto_id, &resp, what);
    resp
}

/// Over the picture: the empty state, "Opening the preview…" or the service notice. The host
/// draws the picture itself only once the sequence is ready.
fn picture_notice(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect) {
    if r.height() < 8.0 {
        return;
    }
    let t = ctx.theme;
    let empty = ctx.state.preview.plan.shots.is_empty();
    if ctx.state.preview.player.ready && !empty {
        return;
    }
    ui.painter().rect_filled(r, 0.0, Color32::BLACK);
    let line = if empty { "Nothing to play yet. The preview fills in as soon as the script has its shots." } else { "Opening the preview…" };
    ui.painter().with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, line, FontId::proportional(13.0), t.text_faint);
    if !ctx.service.status().is_connected() {
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_size(pos2(r.min.x + 12.0, r.max.y - 28.0), vec2(r.width() - 24.0, 24.0))));
        crate::service_notice(&mut child, ctx);
    }
}

/// The shot the caption talks about: under the playhead.
fn current_shot<'a>(ctx: &'a MfxCtx<'_>) -> Option<&'a PlanShot> {
    let pv = &ctx.state.preview;
    pv.plan.shot_at(pv.player.time)
}

fn caption(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect) {
    let t = ctx.theme;
    if r.height() < 4.0 {
        return;
    }
    let p = ui.painter_at(r);
    let y = r.center().y;
    if let Some(problem) = ctx.state.preview.problem.clone() {
        p.text(pos2(r.min.x, y), Align2::LEFT_CENTER, format!("⚠ {problem}"), FontId::proportional(12.0), t.warn);
        return;
    }
    let Some(shot) = current_shot(ctx).cloned() else {
        if !ctx.state.preview.plan.shots.is_empty() {
            p.text(pos2(r.min.x, y), Align2::LEFT_CENTER, "Opening the preview…", FontId::proportional(12.0), t.text_faint);
        }
        return;
    };
    let beat = shot.beat.as_deref().and_then(|b| ctx.state.preview.plan.beat(b)).cloned();
    if let Some(problem) = &shot.problem {
        p.text(pos2(r.min.x, y), Align2::LEFT_CENTER, format!("⚠ {problem}"), FontId::proportional(12.0), t.warn);
        let remake_label = format!("Remake {}", shot.label.to_lowercase());
        let reveal_r = Rect::from_min_size(pos2(r.max.x - 104.0, r.min.y), vec2(104.0, r.height()));
        let remake_r = Rect::from_min_size(pos2(reveal_r.min.x - 124.0, r.min.y), vec2(118.0, r.height()));
        if small_button(ui, ctx, remake_r, &remake_label, "mfx.preview.remake").clicked() {
            ctx.service.request("node.edit", json!({"node": shot.id, "action": "fix", "text": "Remake this shot: its motion won't play in the preview."}));
        }
        if small_button(ui, ctx, reveal_r, "Show the file", "mfx.preview.reveal").clicked() {
            ctx.command("mortiflix.preview.reveal", json!({"node": shot.id}));
        }
        return;
    }
    let mut x = r.min.x;
    if let Some(b) = &beat {
        let chip = Rect::from_center_size(pos2(x + 5.0, y), vec2(10.0, 10.0));
        p.rect_filled(chip, 2.0, hex(&b.chip));
        x += 16.0;
    }
    let mut head = String::new();
    if let Some(b) = &beat {
        head.push_str(&format!("{} {} · ", b.number, b.title));
    }
    head.push_str(&shot.label);
    if shot.version > 0 {
        head.push_str(&format!(" · v{}", shot.version));
    }
    if let Some(w) = ctx.state.work.label(&shot.id) {
        head.push_str(&format!(" · {w} the AI is on it"));
    } else if shot.making {
        head.push_str(&format!(" · v{} making · swaps in when done", shot.versions.saturating_add(1)));
    }
    let kind = shot.media.kind_text();
    let kind_g = p.layout_no_wrap(kind.into(), FontId::proportional(12.0), t.text_faint);
    let kind_w = kind_g.size().x;
    p.galley(pos2(r.max.x - kind_w, y - kind_g.size().y / 2.0), kind_g, t.text_faint);
    let words = p.with_clip_rect(Rect::from_min_max(r.min, pos2(r.max.x - kind_w - 12.0, r.max.y)));
    let g = words.layout_no_wrap(head, FontId::proportional(12.0), t.text_dim);
    let w = g.size().x;
    words.galley(pos2(x, y - g.size().y / 2.0), g, t.text_dim);
    if !shot.title.is_empty() {
        let quote: String = shot.title.chars().take(160).collect();
        words.text(pos2(x + w + 12.0, y), Align2::LEFT_CENTER, format!("“{quote}”"), FontId::proportional(12.0), t.text);
    }
}

fn small_button(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect, label: &str, auto_id: &str) -> egui::Response {
    let t = ctx.theme;
    let resp = ui.interact(r, egui::Id::new(auto_id), Sense::click());
    ui.painter().rect_filled(r, 3.0, if resp.hovered() { t.separator.gamma_multiply(2.0) } else { t.hover });
    ui.painter().with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.0), t.text);
    ctx.auto(auto_id, &resp, label);
    resp
}

/// Seconds → x on the bar.
fn x_of(area: Rect, total: f64, t: f64) -> f32 {
    if total <= 0.0 {
        return area.min.x;
    }
    area.min.x + ((t / total).clamp(0.0, 1.0) as f32) * area.width()
}

fn t_of(area: Rect, total: f64, x: f32) -> f64 {
    if area.width() <= 0.0 {
        return 0.0;
    }
    f64::from(((x - area.min.x) / area.width()).clamp(0.0, 1.0)) * total.max(0.0)
}

fn bar(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect) {
    let t = ctx.theme;
    let now = ui.input(|i| i.time);
    if r.height() < BAR_H - 1.0 {
        return;
    }
    let area = Rect::from_min_max(pos2(r.min.x + GUTTER, r.min.y), r.max);
    let beats_r = Rect::from_min_size(area.min, vec2(area.width(), BEATS_H));
    let shots_r = Rect::from_min_size(pos2(area.min.x, beats_r.max.y + ROW_GAP), vec2(area.width(), SHOTS_H));
    let voice_r = Rect::from_min_size(pos2(area.min.x, shots_r.max.y + ROW_GAP), vec2(area.width(), SOUND_H));
    let music_r = Rect::from_min_size(pos2(area.min.x, voice_r.max.y + ROW_GAP), vec2(area.width(), SOUND_H));
    let p = ui.painter_at(r);
    for (label, row) in [("Beats", beats_r), ("Shots", shots_r), ("Voice", voice_r), ("Music", music_r)] {
        p.text(pos2(r.min.x + GUTTER - 8.0, row.center().y), Align2::RIGHT_CENTER, label, FontId::proportional(11.0), t.text_faint);
    }
    let pv = &ctx.state.preview;
    let total = pv.plan.total;
    let selected = ctx.state.selected_node.clone();
    // beats: a colour line and "number name"
    for b in &pv.plan.beats {
        let (x0, x1) = (x_of(area, total, b.start), x_of(area, total, b.end) - 2.0);
        if x1 <= x0 {
            continue;
        }
        let br = Rect::from_min_max(pos2(x0, beats_r.min.y), pos2(x1, beats_r.max.y));
        p.rect_filled(Rect::from_min_size(br.min, vec2(br.width(), 3.0)), 0.0, hex(&b.chip));
        p.with_clip_rect(br).text(
            pos2(x0 + 2.0, br.min.y + 5.0),
            Align2::LEFT_TOP,
            format!("{} {}", b.number, b.title),
            FontId::proportional(12.0),
            t.text_dim,
        );
    }
    // shots: solid = moving, stripes = still, dashed = not made yet
    let under = if pv.player.playing { pv.shot_at_playhead.clone() } else { None };
    for s in &pv.plan.shots {
        let (x0, x1) = (x_of(area, total, s.start), (x_of(area, total, s.end()) - 2.0).max(x_of(area, total, s.start) + 1.0));
        let sr = Rect::from_min_max(pos2(x0, shots_r.min.y), pos2(x1, shots_r.max.y));
        let fill = hex(&s.colour);
        match s.media {
            Media::Motion { .. } => {
                p.rect_filled(sr, 2.0, fill);
            }
            Media::Still { .. } => {
                p.rect_filled(sr, 2.0, fill);
                let sp = p.with_clip_rect(sr.intersect(r));
                let mut x = sr.min.x - sr.height();
                while x < sr.max.x {
                    sp.line_segment([pos2(x, sr.max.y), pos2(x + sr.height(), sr.min.y)], Stroke::new(2.8, Color32::from_gray(0x15)));
                    x += 8.0;
                }
            }
            Media::Card => {
                p.rect_filled(sr, 2.0, t.field_bg);
                dashed_rect(&p, sr.shrink(0.5), t.separator.gamma_multiply(1.6));
            }
        }
        let number: String = s.label.chars().filter(char::is_ascii_digit).collect();
        let col = if matches!(s.media, Media::Card) { t.text_faint } else { Color32::from_gray(230) };
        if sr.width() > 14.0 {
            p.with_clip_rect(sr).text(pos2(sr.min.x + 4.0, sr.max.y - 3.0), Align2::LEFT_BOTTOM, format!("#{number}"), FontId::proportional(11.0), col);
        }
        if under.as_deref() == Some(s.id.as_str()) {
            p.rect_stroke(sr, 2.0, Stroke::new(2.0, PLAYHEAD), StrokeKind::Inside);
        }
        if selected.as_deref() == Some(s.id.as_str()) {
            p.rect_stroke(sr.shrink(if under.as_deref() == Some(s.id.as_str()) { 2.0 } else { 0.0 }), 2.0, Stroke::new(1.0, t.text), StrokeKind::Inside);
        }
        // the AI is making or remaking this shot: the working sign on its block
        if let Some(w) = ctx.state.work.label(&s.id) {
            crate::work::badge(&p, sr, &w, t.accent_hover, 11.0, now);
        } else if s.making {
            p.circle_filled(pos2(sr.max.x - 6.0, sr.min.y + 6.0), 3.0, t.ok);
        }
    }
    if ctx.state.work.animating() {
        ui.ctx().request_repaint_after(crate::work::FRAME);
    }
    // voice and music: only what exists; dotted = stand-in
    for (row, sounds) in [(voice_r, pv.plan.voice.iter().collect::<Vec<_>>()), (music_r, pv.plan.music.iter().collect())] {
        let track = Rect::from_center_size(row.center(), vec2(row.width(), 6.0));
        p.rect_filled(track, 1.0, t.field_bg);
        for snd in sounds.into_iter().filter(|s| s.file.is_some()) {
            let (x0, x1) = (x_of(area, total, snd.start), x_of(area, total, snd.start + snd.len).max(x_of(area, total, snd.start) + 1.0));
            let sr = Rect::from_min_max(pos2(x0, track.min.y), pos2(x1, track.max.y));
            if snd.stand_in {
                let mut x = sr.min.x;
                while x < sr.max.x {
                    p.rect_filled(Rect::from_min_max(pos2(x, sr.min.y), pos2((x + 3.0).min(sr.max.x), sr.max.y)), 1.0, Color32::from_gray(0x5a));
                    x += 5.0;
                }
            } else {
                p.rect_filled(sr, 1.0, Color32::from_gray(0x5a));
            }
        }
    }
    // the playhead, across every row
    if total > 0.0 {
        let x = x_of(area, total, pv.player.time);
        p.line_segment([pos2(x, area.min.y), pos2(x, area.max.y)], Stroke::new(2.0, PLAYHEAD));
    }
    // click = jump (and select), drag = scrub (picture follows, no sound), beats row = jump to the beat
    let resp = ui.interact(Rect::from_min_max(shots_r.min, music_r.max), egui::Id::new("mfx-preview-bar"), Sense::click_and_drag());
    ctx.auto("mfx.preview.bar", &resp, "Shots bar: click to jump, drag to scrub");
    let beats_resp = ui.interact(beats_r, egui::Id::new("mfx-preview-beats"), Sense::click());
    ctx.auto("mfx.preview.beats", &beats_resp, "Beats: click to jump to a beat");
    if total <= 0.0 {
        return;
    }
    if let Some(pos) = resp.interact_pointer_pos() {
        let at = t_of(area, total, pos.x);
        if resp.drag_started() {
            ctx.command("mortiflix.preview.pause", json!({}));
        }
        if resp.dragged() {
            ctx.command("mortiflix.preview.seek", json!({"seconds": at}));
        } else if resp.clicked()
            && let Some(id) = ctx.state.preview.plan.shot_at(at).map(|s| s.id.clone())
        {
            ctx.state.selected_node = Some(id.clone());
            ctx.command("mortiflix.preview.seek", json!({"node": id}));
        }
    }
    if beats_resp.clicked()
        && let Some(pos) = beats_resp.interact_pointer_pos()
    {
        let at = t_of(area, total, pos.x);
        let beat = ctx.state.preview.plan.beats.iter().find(|b| at >= b.start && at < b.end).cloned();
        if let Some(b) = beat {
            ctx.state.selected_node = b.first_shot.clone();
            ctx.command("mortiflix.preview.seek", json!({"node": b.id}));
        }
    }
}

fn dashed_rect(p: &egui::Painter, r: Rect, col: Color32) {
    let stroke = Stroke::new(1.0, col);
    let pts = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
    p.extend(Shape::dashed_line(&pts, stroke, 4.0, 3.0));
}

fn transport(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect) {
    let t = ctx.theme;
    if r.height() < 20.0 {
        return;
    }
    let pv = &ctx.state.preview;
    let can_play = !pv.plan.shots.is_empty() && pv.player.ready;
    let playing = pv.player.playing;
    let y = r.center().y;
    let h = 28.0;
    // ⏮ Play ⏭
    let prev_r = Rect::from_min_size(pos2(r.min.x, y - h / 2.0), vec2(28.0, h));
    let play_r = Rect::from_min_size(pos2(prev_r.max.x + 4.0, y - h / 2.0), vec2(84.0, h));
    let next_r = Rect::from_min_size(pos2(play_r.max.x + 4.0, y - h / 2.0), vec2(28.0, h));
    let prev = icon_button(ui, ctx, prev_r, Glyph::Prev, "mfx.preview.prev", "Previous shot (↑)");
    if prev.clicked() {
        ctx.command("mortiflix.preview.prevShot", json!({}));
    }
    let play = ui.interact(play_r, egui::Id::new("mfx.preview.play"), if can_play { Sense::click() } else { Sense::hover() });
    let fill = if !can_play {
        t.hover
    } else if play.hovered() {
        t.accent_hover
    } else {
        t.accent
    };
    ui.painter().rect_filled(play_r, 4.0, fill);
    let fg = if can_play { Color32::WHITE } else { t.text_faint };
    let ic = pos2(play_r.min.x + 20.0, y);
    paint_glyph(ui.painter(), ic, if playing { Glyph::Pause } else { Glyph::Play }, fg);
    let play_label = if playing { "Pause" } else { "Play" };
    ui.painter().text(pos2(play_r.min.x + 32.0, y), Align2::LEFT_CENTER, play_label, FontId::proportional(13.0), fg);
    let play = play.on_hover_text(format!("{play_label} (Space) · plays {}", ctx.state.preview.scope.label()));
    if play.clicked() {
        ctx.command("mortiflix.preview.toggle", json!({}));
    }
    ctx.auto("mfx.preview.play", &play, play_label);
    let next = icon_button(ui, ctx, next_r, Glyph::Next, "mfx.preview.next", "Next shot (↓)");
    if next.clicked() {
        ctx.command("mortiflix.preview.nextShot", json!({}));
    }
    // timecode / total
    let pv = &ctx.state.preview;
    let tc = if pv.player.timecode.is_empty() { "00:00:00:00".to_string() } else { pv.player.timecode.clone() };
    let total = if pv.player.total.is_empty() { "00:00:00:00".to_string() } else { pv.player.total.clone() };
    let tc_g = ui.painter().layout_no_wrap(format!("{tc} / {total}"), FontId::monospace(12.0), t.text_dim);
    let tc_w = tc_g.size().x;
    ui.painter().galley(pos2(next_r.max.x + 12.0, y - tc_g.size().y / 2.0), tc_g, t.text_dim);
    // All | From here | This shot | This beat, then Loop
    let mut x = next_r.max.x + 12.0 + tc_w + 16.0;
    let seg_h = 24.0;
    let current = ctx.state.preview.scope;
    for s in Scope::ALL {
        let g = ui.painter().layout_no_wrap(s.label().into(), FontId::proportional(12.0), t.text_dim);
        let w = g.size().x + 18.0;
        let sr = Rect::from_min_size(pos2(x, y - seg_h / 2.0), vec2(w, seg_h));
        x = sr.max.x;
        if sr.max.x > r.max.x {
            break;
        }
        let id = format!("mfx.preview.scope.{}", s.key());
        let resp = ui.interact(sr, egui::Id::new(&id), Sense::click()).on_hover_text(format!("Play plays: {}", s.label()));
        let on = s == current;
        ui.painter().rect_filled(
            sr,
            0.0,
            if on {
                t.hover
            } else if resp.hovered() {
                t.field_bg
            } else {
                t.panel_bg
            },
        );
        ui.painter().rect_stroke(sr, 0.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
        let col = if on { t.text } else { t.text_dim };
        ui.painter().galley(pos2(sr.min.x + 9.0, y - g.size().y / 2.0), g, col);
        if resp.clicked() {
            ctx.state.preview.scope = s;
        }
        ctx.auto(&id, &resp, s.label());
    }
    let loop_r = Rect::from_min_size(pos2(x + 8.0, y - seg_h / 2.0), vec2(64.0, seg_h));
    if loop_r.max.x <= r.max.x {
        let on = ctx.state.preview.looping;
        let marked = ctx.state.preview.player.marked;
        let tip = if marked { "Loop the In/Out range (Ctrl+L)" } else { "Loop the scope (Ctrl+L)" };
        let resp = ui.interact(loop_r, egui::Id::new("mfx.preview.loop"), Sense::click()).on_hover_text(tip);
        ui.painter().rect_filled(
            loop_r,
            3.0,
            if on {
                t.hover
            } else if resp.hovered() {
                t.field_bg
            } else {
                t.panel_bg
            },
        );
        let col = if on { t.text } else { t.text_dim };
        paint_glyph(ui.painter(), pos2(loop_r.min.x + 13.0, y), Glyph::Loop, col);
        ui.painter().text(pos2(loop_r.min.x + 24.0, y), Align2::LEFT_CENTER, "Loop", FontId::proportional(12.0), col);
        if resp.clicked() {
            ctx.command("mortiflix.preview.loop", json!({}));
        }
        ctx.auto("mfx.preview.loop", &resp, "Loop");
        x = loop_r.max.x;
    }
    // legend: Moving 7 · Still 5 · Not made 2
    let c = ctx.state.preview.plan.counts();
    if c.moving + c.still + c.not_made > 0 {
        let legend = format!("Moving {} · Still {} · Not made {}", c.moving, c.still, c.not_made);
        let g = ui.painter().layout_no_wrap(legend, FontId::proportional(11.0), t.text_faint);
        if r.max.x - g.size().x > x + 12.0 {
            ui.painter().galley(pos2(r.max.x - g.size().x, y - g.size().y / 2.0), g, t.text_faint);
        }
    }
}

#[derive(Clone, Copy)]
enum Glyph {
    Play,
    Pause,
    Prev,
    Next,
    Loop,
}

fn icon_button(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect, g: Glyph, auto_id: &str, tip: &str) -> egui::Response {
    let t = ctx.theme;
    let resp = ui.interact(r, egui::Id::new(auto_id), Sense::click()).on_hover_text(tip);
    if resp.hovered() {
        ui.painter().rect_filled(r, 4.0, t.hover);
    }
    paint_glyph(ui.painter(), r.center(), g, if resp.hovered() { t.text } else { t.text_dim });
    ctx.auto(auto_id, &resp, tip);
    resp
}

/// Transport glyphs drawn from scratch (no icon assets).
fn paint_glyph(p: &egui::Painter, c: Pos2, g: Glyph, col: Color32) {
    let tri = |x0: f32, x1: f32| Shape::convex_polygon(vec![pos2(x0, c.y - 5.0), pos2(x1, c.y), pos2(x0, c.y + 5.0)], col, Stroke::NONE);
    match g {
        Glyph::Play => {
            p.add(tri(c.x - 4.0, c.x + 5.0));
        }
        Glyph::Pause => {
            p.rect_filled(Rect::from_min_size(pos2(c.x - 4.5, c.y - 5.0), vec2(3.0, 10.0)), 0.0, col);
            p.rect_filled(Rect::from_min_size(pos2(c.x + 1.5, c.y - 5.0), vec2(3.0, 10.0)), 0.0, col);
        }
        Glyph::Prev => {
            p.rect_filled(Rect::from_min_size(pos2(c.x - 5.0, c.y - 5.0), vec2(2.0, 10.0)), 0.0, col);
            p.add(tri(c.x + 4.0, c.x - 3.0));
        }
        Glyph::Next => {
            p.add(tri(c.x - 4.0, c.x + 3.0));
            p.rect_filled(Rect::from_min_size(pos2(c.x + 3.0, c.y - 5.0), vec2(2.0, 10.0)), 0.0, col);
        }
        Glyph::Loop => {
            let r = 5.0;
            let pts: Vec<Pos2> = (0..=20)
                .map(|i| {
                    let a = 0.6 + (i as f32 / 20.0) * (std::f32::consts::TAU - 1.2);
                    pos2(c.x + r * a.cos(), c.y - r * a.sin())
                })
                .collect();
            let end = pts.last().copied().unwrap_or(c);
            p.add(Shape::line(pts, Stroke::new(1.4, col)));
            p.add(Shape::convex_polygon(
                vec![pos2(end.x - 3.0, end.y - 2.5), pos2(end.x + 2.5, end.y - 1.0), pos2(end.x - 0.5, end.y + 3.0)],
                col,
                Stroke::NONE,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> Plan {
        let shot = |id: &str, beat: &str, start: f64, len: f64| PlanShot { id: id.into(), beat: Some(beat.into()), start, len, ..PlanShot::default() };
        Plan {
            shots: vec![shot("s01", "b01", 0.0, 2.0), shot("s02", "b01", 2.0, 3.0), shot("s03", "b02", 5.0, 4.0)],
            beats: vec![
                PlanBeat { id: "b01".into(), start: 0.0, end: 5.0, first_shot: Some("s01".into()), ..PlanBeat::default() },
                PlanBeat { id: "b02".into(), start: 5.0, end: 9.0, first_shot: Some("s03".into()), ..PlanBeat::default() },
            ],
            total: 9.0,
            ..Plan::default()
        }
    }

    #[test]
    fn scope_ranges_follow_the_selection_then_the_playhead() {
        let pv = PreviewState { plan: plan(), ..PreviewState::default() };
        assert_eq!(pv.range(Scope::All, 3.0, None), (0.0, 9.0));
        assert_eq!(pv.range(Scope::FromHere, 3.0, None), (3.0, 9.0));
        assert_eq!(pv.range(Scope::FromHere, 99.0, None), (9.0, 9.0));
        assert_eq!(pv.range(Scope::Shot, 3.0, None), (2.0, 5.0));
        assert_eq!(pv.range(Scope::Shot, 3.0, Some("s03")), (5.0, 9.0));
        assert_eq!(pv.range(Scope::Beat, 6.0, None), (5.0, 9.0));
        assert_eq!(pv.range(Scope::Beat, 6.0, Some("s02")), (0.0, 5.0));
        assert_eq!(pv.range(Scope::Beat, 6.0, Some("b01")), (0.0, 5.0));
        assert_eq!(pv.range(Scope::Shot, 0.0, Some("nope")), (0.0, 2.0));
        assert_eq!(Scope::parse("This beat"), Some(Scope::Beat));
        assert_eq!(Scope::parse("fromhere"), Some(Scope::FromHere));
        assert_eq!(Scope::parse("x"), None);
    }

    #[test]
    fn state_round_trips_through_the_control_channel() {
        let pv = PreviewState { looping: true, plan: plan(), ..PreviewState::default() };
        let back: PreviewState = serde_json::from_value(serde_json::to_value(&pv).unwrap()).unwrap();
        assert_eq!(back.plan, pv.plan);
        assert!(back.follow_playhead && back.looping);
    }

    #[test]
    fn layout_keeps_every_part_inside_and_the_picture_never_negative() {
        for h in [10.0, 120.0, 260.0, 900.0] {
            let r = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, h));
            let l = layout(r);
            for part in [l.header, l.picture, l.caption, l.bar, l.transport] {
                assert!(part.height() >= 0.0 && part.min.y >= -0.1, "{h}: {part:?}");
            }
        }
        let l = layout(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 900.0)));
        assert!(l.picture.height() > 500.0);
        assert!(l.transport.max.y <= 900.0);
    }

    #[test]
    fn poll_never_waits_when_offline() {
        let mut state = MfxState { project_id: Some("p1".into()), ..MfxState::default() };
        let mut service = Service::default();
        // not connected: nothing sent
        poll(&mut state, &mut service, 0.0);
        assert!(state.preview.fetch.plan.is_none() && state.preview.fetch.graph.is_none());
    }

    #[test]
    fn the_panel_draws_in_every_state_without_panicking() {
        let ctx = egui::Context::default();
        let mut state = MfxState::default();
        let mut service = Service::default();
        for case in 0..3 {
            if case == 1 {
                state.preview.plan = plan();
            }
            if case == 2 {
                state.preview.player = Player { ready: true, playing: true, time: 4.0, duration: 9.0, ..Player::default() };
                state.preview.shot_at_playhead = Some("s02".into());
                state.preview.plan.shots[1].problem = Some("Shot 2's motion won't play. Showing its still frame.".into());
                state.selected_node = Some("s02".into());
            }
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut c = MfxCtx::new(&mut state, &mut service, crate::MfxTheme::default());
                show(ui, &mut c);
                assert!(c.auto.iter().any(|(id, _, _)| id == "mfx.preview.play"));
            });
            out.textures_delta.clear();
        }
    }
}
