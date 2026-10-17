//! The node area's state and logic, apart from drawing: the view (pan, zoom), where nodes sit, the
//! selection, Find and Highlight, the edits sent to the service (queued while it is away), undo,
//! and the `nodes.*` commands. Serde, so `mortiflix.state` shows it to agents.

use std::collections::BTreeMap;

use egui::{Pos2, Rect, Vec2, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::model::{Graph, Kind, Node, NodeState};
use crate::Service;

/// Zoom range (doc 35: 20-150 %).
pub const ZOOM_MIN: f32 = 0.2;
pub const ZOOM_MAX: f32 = 1.5;
/// One zoom step (the − / + buttons and keys).
pub const ZOOM_STEP: f32 = 1.2;
/// Shot node width at 100 % (doc 35).
pub const SHOT_W: f32 = 216.0;
/// The still is 16:9.
pub const STILL_H: f32 = SHOT_W * 9.0 / 16.0;
/// Default distance between shots in a fresh layout (room for the transition between).
pub const STEP_X: f32 = 400.0;
pub const LINE_W: f32 = 184.0;
pub const PILL_W: f32 = 104.0;
pub const PILL_H: f32 = 28.0;
pub const INPUT_W: f32 = 152.0;
pub const CUE_H: f32 = 30.0;
/// A cut shot shrinks to a sliver where it was.
pub const SLIVER_W: f32 = 28.0;
/// Most undo steps kept.
const MAX_UNDO: usize = 200;
/// Most edits kept while the studio is away.
const MAX_OUTBOX: usize = 500;
/// Coordinates beyond this are refused (hostile parameters).
const MAX_COORD: f32 = 1.0e6;

/// How much a node shows at this zoom (doc 35 "semantic zoom"): text never shrinks, the node shows
/// less instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lod {
    /// Under 40 %: `#4` + state.
    Far,
    /// 40-70 %: still + state.
    Middle,
    /// 70 % and up: still, words, OK / Fix / Cut it.
    Near,
}

impl Lod {
    pub fn of(zoom: f32) -> Lod {
        if zoom >= 0.7 {
            Lod::Near
        } else if zoom >= 0.4 {
            Lod::Middle
        } else {
            Lod::Far
        }
    }
}

/// Size of a node in world units at a level of detail.
pub fn node_size(n: &Node, lod: Lod) -> Vec2 {
    match n.kind {
        Kind::Shot => {
            let h = match lod {
                Lod::Near => 24.0 + STILL_H + 22.0 + 20.0 + 30.0,
                Lod::Middle => 24.0 + STILL_H + 22.0,
                Lod::Far => 24.0 + 22.0,
            };
            vec2(if n.state == NodeState::Cut { SLIVER_W } else { SHOT_W }, h)
        }
        Kind::Line => vec2(LINE_W, if lod == Lod::Far { 26.0 } else { 70.0 }),
        Kind::Transition => vec2(PILL_W, PILL_H),
        Kind::Frame | Kind::Asset => vec2(INPUT_W, if lod == Lod::Far { 26.0 } else { 22.0 + INPUT_W * 9.0 / 16.0 }),
        Kind::Cue => vec2(SHOT_W, CUE_H),
        _ => vec2(SHOT_W, 40.0),
    }
}

/// Which nodes dim (doc 35 "Highlight").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Highlight {
    #[default]
    All,
    NotReviewed,
    NeedsYou,
    Making,
    Changed,
    Notes,
}

impl Highlight {
    pub const ALL: [Highlight; 6] = [Highlight::All, Highlight::NotReviewed, Highlight::NeedsYou, Highlight::Making, Highlight::Changed, Highlight::Notes];
    pub fn label(self) -> &'static str {
        match self {
            Highlight::All => "All",
            Highlight::NotReviewed => "Not reviewed",
            Highlight::NeedsYou => "Needs you",
            Highlight::Making => "Making",
            Highlight::Changed => "Changed",
            Highlight::Notes => "Notes",
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Highlight::All => "all",
            Highlight::NotReviewed => "notReviewed",
            Highlight::NeedsYou => "needsYou",
            Highlight::Making => "making",
            Highlight::Changed => "changed",
            Highlight::Notes => "notes",
        }
    }
    pub fn matches(self, n: &Node) -> bool {
        match self {
            Highlight::All => true,
            Highlight::NotReviewed => matches!(n.state, NodeState::Ready | NodeState::OutOfDate),
            Highlight::NeedsYou => n.state == NodeState::NeedsYou,
            Highlight::Making => n.state.making(),
            Highlight::Changed => n.versions.len() > 1 || n.state == NodeState::OutOfDate,
            Highlight::Notes => !n.notes.is_empty(),
        }
    }
}

/// What the Show menu turns on (doc 35: simple by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Show {
    pub voice: bool,
    pub music: bool,
    /// The look and your assets as input nodes.
    pub inputs: bool,
    /// The Bin column.
    pub bin: bool,
}

impl Default for Show {
    fn default() -> Self {
        Self { voice: true, music: true, inputs: false, bin: false }
    }
}

/// Mouse wheel: zoom (node editors) or scroll (FilmCraft's timeline). Doc 35 open question 3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Wheel {
    #[default]
    Zoom,
    Scroll,
}

/// One step that Undo takes back.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Op {
    /// Nodes dragged: (id, where it was (None = its own place), where it went).
    Move { moves: Vec<(String, Option<[f32; 2]>, Option<[f32; 2]>)> },
    /// A shot moved in film order: it was after `from`, now after `to` (None = first).
    Reorder { node: String, from: Option<String>, to: Option<String> },
    /// OK / Cut it / taken back: the decision before and after (`ok`, `cut`, None).
    Decide { node: String, before: Option<String>, after: Option<String> },
}

/// What a request was, so its reply is handled.
#[derive(Clone, Debug, PartialEq)]
enum Sent {
    List,
    Hello,
    Move,
    Reorder,
    Edit(String),
    Other(String),
}

/// A question the node area asks before acting (doc 35 "the ripple question").
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Confirm {
    pub title: String,
    pub detail: String,
    pub yes: String,
    /// The command to run on yes (`nodes.<what>` + params with `force: true`).
    pub what: String,
    pub params: Value,
}

/// A play request for the loose preview (part 36 reads and takes it).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayRequest {
    pub node: String,
    /// `shot` (this shot), `from` (from here), `cut` (across this cut).
    pub scope: String,
}

/// The node area's state.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NodeArea {
    pub zoom: f32,
    /// The world point at the canvas's top-left corner.
    pub pan: [f32; 2],
    /// Where you dropped nodes (world units). Sent to the service (`node.move`), kept here too.
    pub positions: BTreeMap<String, [f32; 2]>,
    /// Selected node ids; the first is the one the inspector shows (and `state.selected_node`).
    pub selection: Vec<String>,
    pub find: String,
    pub find_index: usize,
    pub highlight: Highlight,
    pub show: Show,
    pub wheel: Wheel,
    pub follow_playhead: bool,
    /// The Fix box (inspector) and its words.
    pub fix_open: bool,
    pub fix_text: String,
    /// A note being written in the inspector.
    pub note_text: String,
    /// New shot box (+ Add › New shot).
    pub add_open: bool,
    pub add_text: String,
    /// The version chip being looked at in the inspector (None = the current one).
    pub peek_version: Option<u32>,
    /// Film order changed here and not yet confirmed by the service.
    pub film_override: Option<Vec<String>>,
    /// Transitions picked here: cut id → (kind, seconds).
    pub picks: BTreeMap<String, (String, f32)>,
    pub confirm: Option<Confirm>,
    pub play: Option<PlayRequest>,
    /// The studio can't save positions / order yet (it answered `unknown_type`): kept here only.
    pub positions_local: bool,
    pub order_local: bool,
    /// Last error from the studio, shown in the header.
    pub error: String,
    pub undo: Vec<Op>,
    pub redo: Vec<Op>,
    /// The canvas rectangle last frame (screen), for commands that frame things.
    pub canvas: [f32; 4],
    /// Edits waiting for the studio: (type, params).
    pub outbox: Vec<(String, Value)>,
    #[serde(skip)]
    pub graph: Graph,
    #[serde(skip)]
    pub loaded: bool,
    #[serde(skip)]
    pub project: Option<String>,
    #[serde(skip)]
    pub studio: String,
    #[serde(skip)]
    inflight: Vec<(u64, Sent)>,
    #[serde(skip)]
    pub last_fetch: f64,
    #[serde(skip)]
    pub fetch_soon: bool,
    #[serde(skip)]
    pub was_connected: bool,
    #[serde(skip)]
    pub event_seq: u64,
    /// Ask the frame to focus the Find box / frame the selection / frame all next time it draws.
    #[serde(skip)]
    pub focus_find: bool,
    #[serde(skip)]
    pub want_frame: Option<Frame>,
    /// The selection the area last wrote to `state.selected_node` (to see the chat change it).
    #[serde(skip)]
    pub synced_selection: Option<String>,
    /// The shot under the loose preview's playhead and whether it plays (part 36's `state.preview`).
    #[serde(skip)]
    pub playhead_shot: Option<String>,
    #[serde(skip)]
    pub playing: bool,
    /// Nodes being dragged right now and how far (world units); dropped = saved with `node.move`.
    #[serde(skip)]
    pub drag_offset: Option<(Vec<String>, [f32; 2])>,
}

/// What to frame next draw.
#[derive(Clone, Debug, PartialEq)]
pub enum Frame {
    All,
    /// Every shot of the film (and its cuts) as large as fits: what opening a project shows.
    Film,
    Selected,
    Node(String),
    Beat(String),
    /// Pan only, when the node is out of view.
    Reveal(String),
}

impl Default for NodeArea {
    fn default() -> Self {
        Self {
            zoom: 0.8,
            pan: [-40.0, -60.0],
            positions: BTreeMap::new(),
            selection: Vec::new(),
            find: String::new(),
            find_index: 0,
            highlight: Highlight::All,
            show: Show::default(),
            wheel: Wheel::Zoom,
            follow_playhead: false,
            fix_open: false,
            fix_text: String::new(),
            note_text: String::new(),
            add_open: false,
            add_text: String::new(),
            peek_version: None,
            film_override: None,
            picks: BTreeMap::new(),
            confirm: None,
            play: None,
            positions_local: false,
            order_local: false,
            error: String::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            canvas: [0.0, 0.0, 800.0, 400.0],
            outbox: Vec::new(),
            graph: Graph::default(),
            loaded: false,
            project: None,
            studio: String::new(),
            inflight: Vec::new(),
            last_fetch: f64::NEG_INFINITY,
            fetch_soon: true,
            was_connected: false,
            event_seq: 0,
            focus_find: false,
            want_frame: None,
            synced_selection: None,
            drag_offset: None,
            playhead_shot: None,
            playing: false,
        }
    }
}

fn finite(p: [f32; 2]) -> bool {
    p.iter().all(|v| v.is_finite() && v.abs() < MAX_COORD)
}

impl NodeArea {
    // ------------------------------------------------------------------ view

    pub fn canvas_rect(&self) -> Rect {
        let [x, y, w, h] = self.canvas;
        Rect::from_min_size(pos2(x, y), vec2(w.max(1.0), h.max(1.0)))
    }
    pub fn to_screen(&self, w: Pos2) -> Pos2 {
        let c = self.canvas_rect().min;
        pos2(c.x + (w.x - self.pan[0]) * self.zoom, c.y + (w.y - self.pan[1]) * self.zoom)
    }
    pub fn to_world(&self, s: Pos2) -> Pos2 {
        let c = self.canvas_rect().min;
        let z = self.zoom.max(ZOOM_MIN);
        pos2(self.pan[0] + (s.x - c.x) / z, self.pan[1] + (s.y - c.y) / z)
    }
    pub fn rect_to_screen(&self, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(r.min), self.to_screen(r.max))
    }
    /// The world rectangle in view.
    pub fn view_world(&self) -> Rect {
        let c = self.canvas_rect();
        Rect::from_min_max(self.to_world(c.min), self.to_world(c.max))
    }
    /// Zoom by `factor`, keeping the world point under `at` (screen) still.
    pub fn zoom_at(&mut self, at: Pos2, factor: f32) {
        if !factor.is_finite() || factor <= 0.0 {
            return;
        }
        let w = self.to_world(at);
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let c = self.canvas_rect().min;
        self.pan = [w.x - (at.x - c.x) / self.zoom, w.y - (at.y - c.y) / self.zoom];
    }
    /// Pan by a screen delta.
    pub fn pan_by(&mut self, d: Vec2) {
        if d.x.is_finite() && d.y.is_finite() {
            let z = self.zoom.max(ZOOM_MIN);
            self.pan = [(self.pan[0] - d.x / z).clamp(-MAX_COORD, MAX_COORD), (self.pan[1] - d.y / z).clamp(-MAX_COORD, MAX_COORD)];
        }
    }
    /// Fit a world rectangle in view (with a margin), within the zoom range.
    pub fn frame_rect(&mut self, r: Rect) {
        if !r.is_finite() || r.width() <= 0.0 && r.height() <= 0.0 {
            return;
        }
        let c = self.canvas_rect();
        let margin = 48.0;
        let zx = (c.width() - margin * 2.0).max(40.0) / r.width().max(1.0);
        let zy = (c.height() - margin * 2.0).max(40.0) / r.height().max(1.0);
        self.zoom = zx.min(zy).clamp(ZOOM_MIN, 1.0);
        self.pan = [r.center().x - c.width() / 2.0 / self.zoom, r.center().y - c.height() / 2.0 / self.zoom];
    }
    /// Pan (no zoom change) so the world rectangle is in view.
    pub fn reveal(&mut self, r: Rect) {
        let v = self.view_world().shrink(24.0 / self.zoom.max(ZOOM_MIN));
        if v.contains_rect(r) || !r.is_finite() {
            return;
        }
        let c = self.canvas_rect();
        self.pan = [r.center().x - c.width() / 2.0 / self.zoom, r.center().y - c.height() / 2.0 / self.zoom];
    }

    // ------------------------------------------------------------------ layout

    /// The node's world rectangle: where you dropped it, else where the service has it, else its
    /// own place in a fresh layout (shots in film order; lines and music under them; transitions
    /// half way along their noodle).
    pub fn node_rect(&self, id: &str) -> Option<Rect> {
        let r = self.node_rect_still(id)?;
        Some(r.translate(self.offset_of(id)))
    }

    /// How far a node is being dragged right now.
    pub fn offset_of(&self, id: &str) -> Vec2 {
        match &self.drag_offset {
            Some((ids, d)) if ids.iter().any(|i| i == id) => vec2(d[0], d[1]),
            _ => Vec2::ZERO,
        }
    }

    /// [`Self::node_rect`] without the drag in progress.
    pub fn node_rect_still(&self, id: &str) -> Option<Rect> {
        let n = self.graph.get(id)?;
        let lod = Lod::of(self.zoom);
        let size = node_size(n, lod);
        if let Some(p) = self.positions.get(id).copied().or(n.pos).filter(|p| finite(*p)) {
            let size = if n.kind == Kind::Cue { vec2(self.cue_width(n), CUE_H) } else { size };
            return Some(Rect::from_min_size(pos2(p[0], p[1]), size));
        }
        let max_shot_h = node_size(&Node { kind: Kind::Shot, ..Node::default() }, Lod::Near).y;
        match n.kind {
            Kind::Shot => {
                let i = self.graph.film_index(id)?;
                #[allow(clippy::cast_precision_loss)] // film index, small
                let x = i as f32 * STEP_X;
                Some(Rect::from_min_size(pos2(x, 0.0), size))
            }
            Kind::Transition => {
                let cut = self.graph.cuts.iter().find(|c| c.transition.as_deref() == Some(id))?;
                Some(self.cut_rect_auto(&cut.from, &cut.to))
            }
            Kind::Line => {
                let shot = self.graph.line_shot(n)?;
                let sr = self.node_rect(&shot)?;
                let mut lines: Vec<&Node> = self
                    .graph
                    .nodes
                    .iter()
                    .filter(|l| l.kind == Kind::Line && !l.missing && self.graph.line_shot(l).as_deref() == Some(shot.as_str()))
                    .collect();
                lines.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
                let k = lines.iter().position(|l| l.id == id).unwrap_or(0);
                #[allow(clippy::cast_precision_loss)]
                let y = sr.min.y + max_shot_h + 48.0 + k as f32 * 84.0;
                Some(Rect::from_min_size(pos2(sr.min.x + 16.0, y), size))
            }
            Kind::Cue => {
                let beat = n.beat.as_deref()?;
                let first = self.graph.beat_shots(beat).first().map(|s| s.id.clone())?;
                let fr = self.node_rect(&first)?;
                Some(Rect::from_min_size(pos2(fr.min.x, fr.min.y + max_shot_h + 48.0 + 2.0 * 84.0 + 12.0), vec2(self.cue_width(n), CUE_H)))
            }
            Kind::Frame | Kind::Asset => {
                let inputs: Vec<&Node> = self.graph.nodes.iter().filter(|m| matches!(m.kind, Kind::Frame | Kind::Asset) && !m.missing).collect();
                let i = inputs.iter().position(|m| m.id == id)?;
                #[allow(clippy::cast_precision_loss)]
                let x = i as f32 * (INPUT_W + 32.0);
                Some(Rect::from_min_size(pos2(x, -(22.0 + INPUT_W * 9.0 / 16.0) - 96.0), size))
            }
            _ => None,
        }
    }

    /// A cue spans its beat's shots in a fresh layout.
    fn cue_width(&self, n: &Node) -> f32 {
        let k = n.beat.as_deref().map_or(1, |b| self.graph.beat_shots(b).len().max(1));
        #[allow(clippy::cast_precision_loss)]
        let w = k as f32 * STEP_X - (STEP_X - SHOT_W);
        w
    }

    /// Where a cut's transition pill sits when nobody moved it: half way between the shots.
    pub fn cut_rect_auto(&self, from: &str, to: &str) -> Rect {
        let a = self.node_rect(from).unwrap_or(Rect::NOTHING);
        let b = self.node_rect(to).unwrap_or(Rect::NOTHING);
        let p = self.socket_out(a).lerp(self.socket_in(b), 0.5);
        Rect::from_center_size(p, vec2(PILL_W, PILL_H))
    }

    /// A cut's pill: its transition node's place, else a moved synthetic one, else half way.
    pub fn cut_rect(&self, cut: &super::model::Cut) -> Rect {
        if let Some(t) = &cut.transition
            && let Some(r) = self.node_rect(t)
        {
            return r;
        }
        if let Some(p) = self.positions.get(&cut.id).filter(|p| finite(**p)) {
            return Rect::from_min_size(pos2(p[0], p[1]), vec2(PILL_W, PILL_H)).translate(self.offset_of(&cut.id));
        }
        self.cut_rect_auto(&cut.from, &cut.to).translate(self.offset_of(&cut.id))
    }

    /// The film-in socket (left edge, level with the still) and film-out socket (right edge).
    pub fn socket_in(&self, r: Rect) -> Pos2 {
        pos2(r.min.x, r.min.y + (24.0 + STILL_H / 2.0).min(r.height() / 2.0))
    }
    pub fn socket_out(&self, r: Rect) -> Pos2 {
        pos2(r.max.x, r.min.y + (24.0 + STILL_H / 2.0).min(r.height() / 2.0))
    }

    /// Ids of the nodes drawn (shown kinds, not missing), back to front.
    pub fn drawn(&self) -> Vec<String> {
        let mut out = Vec::new();
        let kinds: &[Kind] = &[Kind::Frame, Kind::Asset, Kind::Cue, Kind::Line, Kind::Transition, Kind::Shot];
        for k in kinds {
            if !self.shows(*k) {
                continue;
            }
            for n in self.graph.nodes.iter().filter(|n| n.kind == *k && !n.missing) {
                if n.kind == Kind::Transition && !self.graph.cuts.iter().any(|c| c.transition.as_deref() == Some(n.id.as_str())) {
                    continue; // a transition on a cut that no longer exists (after a move)
                }
                out.push(n.id.clone());
            }
        }
        out
    }

    pub fn shows(&self, k: Kind) -> bool {
        match k {
            Kind::Shot | Kind::Transition => true,
            Kind::Line => self.show.voice,
            Kind::Cue => self.show.music,
            Kind::Frame | Kind::Asset => self.show.inputs,
            _ => false,
        }
    }

    /// The world rectangle around every drawn node.
    pub fn content_rect(&self) -> Rect {
        let mut r = Rect::NOTHING;
        for id in self.drawn() {
            if let Some(nr) = self.node_rect(&id) {
                r = r.union(nr);
            }
        }
        r
    }

    // ------------------------------------------------------------------ selection

    pub fn primary(&self) -> Option<&str> {
        self.selection.first().map(String::as_str)
    }
    pub fn select_only(&mut self, id: &str) {
        self.selection = vec![id.to_string()];
        self.fix_open = false;
        self.peek_version = None;
    }
    pub fn toggle_select(&mut self, id: &str) {
        if let Some(i) = self.selection.iter().position(|s| s == id) {
            self.selection.remove(i);
        } else {
            self.selection.push(id.to_string());
        }
    }

    /// The node ids in film order a ←/→ walk goes through: shots, with the transition between.
    pub fn walk(&self) -> Vec<String> {
        let mut v = Vec::new();
        let live: Vec<&String> = self.graph.film.iter().filter(|id| self.graph.get(id).is_some_and(|n| n.state != NodeState::Cut)).collect();
        for (i, id) in live.iter().enumerate() {
            if i > 0
                && let Some(c) = self.graph.cuts.get(i - 1)
            {
                v.push(c.id.clone());
            }
            v.push((*id).clone());
        }
        v
    }

    /// Next/previous node in film order (`dir` ±1); starts at the first shot.
    pub fn step(&mut self, dir: i32) -> Option<String> {
        let walk = self.walk();
        let cur = self.primary().and_then(|p| walk.iter().position(|w| w == p));
        let next = match (cur, dir) {
            (None, _) => walk.first().cloned(),
            (Some(i), d) if d < 0 => i.checked_sub(1).and_then(|j| walk.get(j)).cloned(),
            (Some(i), _) => walk.get(i + 1).cloned(),
        }?;
        self.select_only(&next);
        self.want_frame = Some(Frame::Reveal(next.clone()));
        Some(next)
    }

    /// ↑/↓: shot ↔ its voice line ↔ its music cue.
    pub fn step_row(&mut self, dir: i32) -> Option<String> {
        let cur = self.primary()?.to_string();
        let n = self.graph.get(&cur)?.clone();
        let line_of = |shot: &str| {
            self.graph.nodes.iter().find(|l| l.kind == Kind::Line && !l.missing && self.graph.line_shot(l).as_deref() == Some(shot)).map(|l| l.id.clone())
        };
        let cue_of = |beat: Option<&str>| {
            self.graph.nodes.iter().find(|c| c.kind == Kind::Cue && !c.missing && c.beat.as_deref() == beat && beat.is_some()).map(|c| c.id.clone())
        };
        let next = match (n.kind, dir > 0) {
            (Kind::Shot, true) => line_of(&cur).filter(|_| self.show.voice).or_else(|| cue_of(n.beat.as_deref()).filter(|_| self.show.music)),
            (Kind::Line, true) => cue_of(n.beat.as_deref()).filter(|_| self.show.music),
            (Kind::Line, false) => self.graph.line_shot(&n),
            (Kind::Cue, false) => {
                let beat = n.beat.as_deref().unwrap_or("");
                let first = self.graph.beat_shots(beat).first().map(|s| s.id.clone());
                first.as_deref().and_then(line_of).filter(|_| self.show.voice).or(first)
            }
            _ => None,
        }?;
        self.select_only(&next);
        self.want_frame = Some(Frame::Reveal(next.clone()));
        Some(next)
    }

    // ------------------------------------------------------------------ find

    /// Node ids Find matches (shot words, names `7` / `#7`, beat names, state words), film order.
    pub fn found(&self) -> Vec<String> {
        let q = self.find.trim().to_lowercase();
        if q.is_empty() {
            return Vec::new();
        }
        let q_num = q.trim_start_matches('#');
        let mut out = Vec::new();
        for id in self.walk().iter().chain(self.drawn().iter()) {
            if out.contains(id) {
                continue;
            }
            let Some(n) = self.graph.get(id) else { continue };
            let beat = n.beat.as_deref().map(|b| self.graph.beat_title(b).1.to_lowercase()).unwrap_or_default();
            let name = n.display_name().to_lowercase();
            let hit = n.title.to_lowercase().contains(&q)
                || name == q
                || name.trim_start_matches('#') == q_num
                || n.label.to_lowercase() == q
                || (!beat.is_empty() && beat.contains(&q))
                || n.state.word().to_lowercase().trim_end_matches('…') == q.trim_end_matches('…');
            if hit {
                out.push(id.clone());
            }
        }
        out
    }

    /// Enter in Find: frame the next match.
    pub fn find_next(&mut self) -> Option<String> {
        let found = self.found();
        if found.is_empty() {
            return None;
        }
        let i = self.find_index % found.len();
        self.find_index = i + 1;
        let id = found.get(i)?.clone();
        self.select_only(&id);
        self.want_frame = Some(Frame::Node(id.clone()));
        Some(id)
    }

    /// Dimmed by Find or Highlight.
    pub fn dimmed(&self, n: &Node, found: &[String]) -> bool {
        (!self.find.trim().is_empty() && !found.contains(&n.id)) || !self.highlight.matches(n)
    }

    // ------------------------------------------------------------------ service

    /// Send a request now, or keep an edit for later while the studio is away.
    fn send(&mut self, svc: &mut Service, ty: &str, params: Value, what: Sent) {
        if !svc.status().is_connected() {
            if !matches!(what, Sent::List | Sent::Hello) && self.outbox.len() < MAX_OUTBOX {
                self.outbox.push((ty.to_string(), params));
            }
            return;
        }
        let id = svc.request(ty, params);
        self.inflight.push((id, what));
    }

    fn what_of(ty: &str, params: &Value) -> Sent {
        match ty {
            "node.move" => Sent::Move,
            "node.reorder" => Sent::Reorder,
            "node.edit" => Sent::Edit(params.get("node").and_then(Value::as_str).unwrap_or("").to_string()),
            t => Sent::Other(t.to_string()),
        }
    }

    /// Every frame: send what waited, read replies, refresh the graph when it's due. Returns a fresh
    /// `node.list` result when one arrived (the host keeps it in `state.graph` for the other parts).
    pub fn tick(&mut self, svc: &mut Service, project: Option<&String>, now: f64) -> Option<Value> {
        let mut fresh = None;
        let connected = svc.status().is_connected();
        if connected && !self.was_connected {
            self.fetch_soon = true;
            let id = svc.request("hello", json!({"client": "mortiflix-filmcraft-nodes"}));
            self.inflight.push((id, Sent::Hello));
        }
        self.was_connected = connected;
        if project != self.project.as_ref() {
            self.project = project.cloned();
            self.graph = Graph::default();
            self.loaded = false;
            self.positions.clear();
            self.selection.clear();
            self.film_override = None;
            self.picks.clear();
            self.undo.clear();
            self.redo.clear();
            self.fetch_soon = true;
        }
        if connected && !self.outbox.is_empty() {
            for (ty, params) in std::mem::take(&mut self.outbox) {
                let what = Self::what_of(&ty, &params);
                self.send(svc, &ty, params, what);
            }
        }
        // refresh when something happened in the studio
        let mut newest = self.event_seq;
        for (seq, ev) in svc.events_since(self.event_seq) {
            newest = newest.max(*seq);
            let name = ev.get("event").and_then(Value::as_str).unwrap_or("");
            if ["status", "node", "graph", "canvas", "edit"].iter().any(|p| name.starts_with(p)) {
                self.fetch_soon = true;
            }
        }
        self.event_seq = newest;
        let making = self.graph.nodes.iter().any(|n| n.state.making());
        let every = if making { 2.0 } else { 6.0 };
        let listing = self.inflight.iter().any(|(_, w)| *w == Sent::List);
        if connected && self.project.is_some() && !listing && (self.fetch_soon && now - self.last_fetch > 0.3 || now - self.last_fetch > every) {
            self.fetch_soon = false;
            self.last_fetch = now;
            let params = self.project.as_ref().map_or(json!({}), |p| json!({"project": p}));
            self.send(svc, "graph.get", params, Sent::List);
        }
        let mut done = Vec::new();
        for (id, what) in &self.inflight {
            if let Some(r) = svc.take_reply(*id) {
                done.push((*id, what.clone(), r));
            }
        }
        for (id, what, r) in done {
            self.inflight.retain(|(i, _)| *i != id);
            let code = r.error.get("code").and_then(Value::as_str).unwrap_or("");
            let message: String = r.error.get("message").and_then(Value::as_str).unwrap_or("").chars().take(300).collect();
            match what {
                Sent::List if r.ok => {
                    self.load(&r.result);
                    fresh = Some(r.result);
                }
                Sent::List => {
                    if code != "no_project" {
                        self.error = format!("Couldn't load the nodes: {message}");
                    }
                    self.loaded = true;
                }
                Sent::Hello if r.ok => {
                    self.studio = r.result.get("studio").and_then(Value::as_str).unwrap_or("").chars().take(4000).collect();
                }
                Sent::Move if code == "unknown_type" => self.positions_local = true,
                Sent::Reorder if code == "unknown_type" => self.order_local = true,
                Sent::Reorder if r.ok => {
                    // the studio keeps the order now (graph.get `order.film`)
                    if !self.inflight.iter().any(|(_, w)| *w == Sent::Reorder) {
                        self.film_override = None;
                    }
                    self.fetch_soon = true;
                }
                Sent::Edit(_) | Sent::Other(_) if r.ok => self.fetch_soon = true,
                _ if !r.ok && code != "not_running" && code != "unknown_type" => self.error = format!("The studio said: {message}"),
                _ => {}
            }
        }
        fresh
    }

    /// A fresh `node.list` arrived.
    pub fn load(&mut self, result: &Value) {
        let mut g = Graph::from_json(result);
        // Frame all on its own when a project opens and when its shots first appear (owner's first run:
        // zoomed in, they thought shots were missing). Later loads keep the view the person set.
        let first = !self.loaded || self.graph.nodes.is_empty();
        let shots_appear = self.graph.film.is_empty() && !g.film.is_empty();
        if (first && !g.nodes.is_empty()) || shots_appear {
            self.want_frame = Some(if g.film.is_empty() { Frame::All } else { Frame::Film });
        }
        if let Some(order) = &self.film_override {
            // keep the order changed here; shots the studio added since go at the end
            let mut film: Vec<String> = order.iter().filter(|id| g.film.contains(id)).cloned().collect();
            for id in &g.film {
                if !film.contains(id) {
                    film.push(id.clone());
                }
            }
            g.set_film(film);
        }
        self.graph = g;
        self.loaded = true;
        self.error.clear();
        // forget selected nodes that are gone
        let graph = &self.graph;
        self.selection.retain(|id| graph.get(id).is_some() || id.starts_with("cut:"));
    }

    // ------------------------------------------------------------------ actions

    /// Drop nodes at new places (world, top-left); one undo step.
    pub fn move_nodes(&mut self, svc: &mut Service, moves: &[(String, Option<[f32; 2]>)], record: bool) {
        let mut op = Vec::new();
        let mut wire = serde_json::Map::new();
        for (id, to) in moves {
            if to.is_some_and(|p| !finite(p)) {
                continue;
            }
            let before = self.positions.get(id).copied().or_else(|| self.graph.get(id).and_then(|n| n.pos));
            match to {
                Some(p) => {
                    self.positions.insert(id.clone(), *p);
                    wire.insert(id.clone(), json!([p[0], p[1]]));
                }
                None => {
                    self.positions.remove(id);
                    if let Some(n) = self.graph.get_mut(id) {
                        n.pos = None;
                    }
                    wire.insert(id.clone(), Value::Null);
                }
            }
            op.push((id.clone(), before, *to));
        }
        if op.is_empty() {
            return;
        }
        if record {
            self.push_undo(Op::Move { moves: op });
        }
        // a pill on a cut with no transition node yet is placed here only (the studio has no id for it)
        wire.retain(|k, _| !k.starts_with("cut:"));
        if !wire.is_empty() {
            self.send(svc, "node.move", json!({"positions": wire}), Sent::Move);
        }
    }

    /// Move a shot in film order (rewire the chain).
    pub fn reorder(&mut self, svc: &mut Service, node: &str, after: Option<&str>, record: bool) -> bool {
        let from = self.graph.film_prev(node);
        if !self.graph.reorder(node, after) {
            return false;
        }
        self.film_override = Some(self.graph.film.clone());
        if record {
            self.push_undo(Op::Reorder { node: node.to_string(), from, to: after.map(str::to_string) });
        }
        let order = self.graph.film.clone();
        self.send(svc, "node.reorder", json!({"order": order}), Sent::Reorder);
        true
    }

    /// OK / Cut it / clear (take back) / fix on a node.
    pub fn decide(&mut self, svc: &mut Service, node: &str, action: &str, text: &str, record: bool) -> Result<(), String> {
        let Some(n) = self.graph.get(node) else { return Err(format!("no node `{node}`")) };
        let before = match n.state {
            NodeState::Ok => Some("ok".to_string()),
            NodeState::Cut => Some("cut".to_string()),
            _ => None,
        };
        let has_versions = !n.versions.is_empty();
        let mut params = json!({"node": node, "action": action});
        match action {
            "fix" => {
                let words = text.trim();
                if words.is_empty() {
                    return Err("Say what should change.".into());
                }
                params["text"] = json!(words.chars().take(4000).collect::<String>());
            }
            "ok" | "cut" | "clear" => {}
            _ => return Err(format!("unknown action `{action}`")),
        }
        if let Some(n) = self.graph.get_mut(node) {
            n.state = match action {
                "ok" => NodeState::Ok,
                "cut" => NodeState::Cut,
                "fix" if has_versions => NodeState::Remaking,
                "fix" => NodeState::Making,
                _ => {
                    if has_versions {
                        NodeState::Ready
                    } else {
                        NodeState::Planned
                    }
                }
            };
            if action == "fix" {
                n.pending = Some(text.trim().to_string());
            }
        }
        if matches!(action, "cut" | "clear") {
            let film = self.graph.film.clone();
            self.graph.set_film(film);
        }
        if record && action != "fix" {
            let after = match action {
                "ok" => Some("ok".to_string()),
                "cut" => Some("cut".to_string()),
                _ => None,
            };
            self.push_undo(Op::Decide { node: node.to_string(), before, after });
        }
        self.send(svc, "node.edit", params, Sent::Edit(node.to_string()));
        Ok(())
    }

    fn push_undo(&mut self, op: Op) {
        self.undo.push(op);
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// Undo (back = true) or redo the last step taken here.
    pub fn undo_redo(&mut self, svc: &mut Service, back: bool) -> Option<Op> {
        let op = if back { self.undo.pop()? } else { self.redo.pop()? };
        match &op {
            Op::Move { moves } => {
                let m: Vec<(String, Option<[f32; 2]>)> = moves.iter().map(|(id, b, a)| (id.clone(), if back { *b } else { *a })).collect();
                self.move_nodes(svc, &m, false);
            }
            Op::Reorder { node, from, to } => {
                let target = if back { from } else { to };
                self.reorder(svc, node, target.as_deref(), false);
            }
            Op::Decide { node, before, after } => {
                let target = if back { before } else { after };
                let action = target.clone().unwrap_or_else(|| "clear".to_string());
                let _ = self.decide(svc, node, &action, "", false);
            }
        }
        if back {
            self.redo.push(op.clone());
        } else {
            self.undo.push(op.clone());
        }
        Some(op)
    }

    /// Pick a transition on a cut.
    pub fn pick_transition(&mut self, svc: &mut Service, cut_id: &str, kind: &str, sec: f32) -> Result<(), String> {
        let cut = self.graph.cuts.iter().find(|c| c.id == cut_id).cloned().ok_or_else(|| format!("no cut `{cut_id}`"))?;
        let kind: String = kind.trim().chars().take(80).collect();
        if kind.is_empty() || !sec.is_finite() || !(0.0..=10.0).contains(&sec) {
            return Err("a transition needs a kind and a length of 0-10 s".into());
        }
        self.picks.insert(cut.id.clone(), (kind.clone(), sec));
        if let Some(c) = self.graph.cuts.iter_mut().find(|c| c.id == cut_id) {
            c.pick = false;
        }
        // The studio picks transitions through the transition node's Fix (doc 12 edit request); a cut
        // with no transition node yet goes into the chat history as a grey line the AI reads.
        let words = if kind == "Cut" { "Use a plain cut".to_string() } else { format!("Use {kind}, {sec:.1} s") };
        match &cut.transition {
            Some(t) => self.send(svc, "node.edit", json!({"node": t, "action": "fix", "text": words}), Sent::Edit(t.clone())),
            None => {
                let text = format!("Transition {} › {}: {words}", cut.from.to_uppercase(), cut.to.to_uppercase());
                self.send(svc, "chat.action", json!({"text": text, "refs": [cut.from, cut.to]}), Sent::Other("chat.action".into()));
            }
        }
        Ok(())
    }

    /// Ask the studio for something that has no local effect (add, duplicate, use version, note).
    pub fn ask(&mut self, svc: &mut Service, ty: &str, params: Value) {
        self.send(svc, ty, params, Sent::Other(ty.to_string()));
    }

    /// The kind and length shown on a cut's pill.
    pub fn cut_label(&self, cut: &super::model::Cut) -> (String, Option<f32>, bool) {
        if let Some((k, s)) = self.picks.get(&cut.id) {
            return (k.clone(), Some(*s), false);
        }
        if let Some(t) = cut.transition.as_deref().and_then(|t| self.graph.get(t)) {
            #[allow(clippy::cast_possible_truncation)]
            let sec = t.sec.map(|s| s as f32);
            let title = if t.title.is_empty() { "Cut".to_string() } else { capitalise(&t.title) };
            return (title, sec, false);
        }
        if cut.pick { ("! Pick one".into(), None, true) } else { ("Cut".into(), None, false) }
    }
}

/// "crossfade" → "Crossfade".
pub fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::model::tests::sample;

    fn area() -> NodeArea {
        NodeArea { graph: Graph::from_json(&sample()), loaded: true, canvas: [0.0, 0.0, 1000.0, 500.0], ..NodeArea::default() }
    }

    #[test]
    fn view_round_trips_and_zoom_keeps_the_point_under_the_pointer() {
        let mut a = area();
        let p = pos2(300.0, 200.0);
        let w = a.to_world(p);
        a.zoom_at(p, 1.5);
        let back = a.to_screen(w);
        assert!((back - p).length() < 0.01, "{back:?}");
        a.zoom_at(p, 1000.0);
        assert_eq!(a.zoom, ZOOM_MAX);
        a.zoom_at(p, 0.0);
        a.zoom_at(p, f32::NAN);
        assert_eq!(a.zoom, ZOOM_MAX);
        a.pan_by(vec2(f32::INFINITY, 0.0));
        assert!(a.pan.iter().all(|v| v.is_finite()));
        a.frame_rect(a.content_rect());
        let v = a.view_world();
        assert!(v.contains_rect(a.content_rect()), "{v:?} {:?}", a.content_rect());
    }

    #[test]
    fn default_layout_follows_the_film_and_drops_stay_put() {
        let mut a = area();
        let s1 = a.node_rect("s01").unwrap();
        let s2 = a.node_rect("s02").unwrap();
        assert!(s2.min.x > s1.max.x);
        let t = a.node_rect("t01").unwrap();
        assert!(t.center().x > s1.max.x && t.center().x < s2.min.x);
        let line = a.node_rect("b01-1").unwrap();
        assert!(line.min.y > s1.max.y);
        let mut svc = Service::default();
        a.move_nodes(&mut svc, &[("s01".into(), Some([5000.0, 900.0]))], true);
        assert_eq!(a.node_rect("s01").map(|r| r.min), Some(pos2(5000.0, 900.0)));
        // the transition follows its shots (it was never moved itself)
        let t2 = a.node_rect("t01").unwrap();
        assert!(t2.center().x > 2000.0);
        // offline: the move waits in the outbox
        assert_eq!(a.outbox.len(), 1);
        a.undo_redo(&mut svc, true);
        assert_eq!(a.node_rect("s01").map(|r| r.min), Some(s1.min));
        a.undo_redo(&mut svc, false);
        assert_eq!(a.node_rect("s01").map(|r| r.min), Some(pos2(5000.0, 900.0)));
        // hostile coordinates are refused
        a.move_nodes(&mut svc, &[("s02".into(), Some([f32::NAN, 0.0])), ("s03".into(), Some([1.0e9, 0.0]))], true);
        assert_eq!(a.node_rect("s02"), Some(s2));
    }

    #[test]
    fn walking_finding_and_deciding() {
        let mut a = area();
        assert_eq!(a.step(1).as_deref(), Some("s01"));
        assert_eq!(a.step(1).as_deref(), Some("t01"));
        assert_eq!(a.step(1).as_deref(), Some("s02"));
        assert_eq!(a.step(-1).as_deref(), Some("t01"));
        a.select_only("s01");
        assert_eq!(a.step(-1), None);
        assert_eq!(a.step_row(1).as_deref(), Some("b01-1"));
        assert_eq!(a.step_row(1).as_deref(), Some("m01"));
        assert_eq!(a.step_row(-1).as_deref(), Some("b01-1"));
        assert_eq!(a.step_row(-1).as_deref(), Some("s01"));
        a.find = "#4".into();
        assert_eq!(a.found(), ["s04"]);
        a.find = "table".into();
        assert_eq!(a.find_next().as_deref(), Some("s03"));
        a.find = "the bet".into();
        assert!(a.found().contains(&"s03".to_string()) && a.found().contains(&"s04".to_string()));
        a.find = "needs you".into();
        assert_eq!(a.found(), ["s04"]);
        let mut svc = Service::default();
        a.decide(&mut svc, "s02", "ok", "", true).unwrap();
        assert_eq!(a.graph.get("s02").map(|n| n.state), Some(NodeState::Ok));
        a.decide(&mut svc, "s03", "cut", "", true).unwrap();
        assert!(!a.walk().contains(&"s03".to_string()));
        assert!(a.decide(&mut svc, "s04", "fix", "  ", true).is_err());
        a.decide(&mut svc, "s04", "fix", "warmer", true).unwrap();
        assert_eq!(a.graph.get("s04").map(|n| n.state), Some(NodeState::Remaking));
        assert!(a.decide(&mut svc, "ghost", "ok", "", true).is_err());
        assert!(a.decide(&mut svc, "s04", "explode", "", true).is_err());
        a.undo_redo(&mut svc, true); // the cut
        assert_eq!(a.graph.get("s03").map(|n| n.state), Some(NodeState::Planned));
        a.undo_redo(&mut svc, true); // the OK
        assert_eq!(a.graph.get("s02").map(|n| n.state), Some(NodeState::Ready));
        assert!(a.reorder(&mut svc, "s04", Some("s01"), true));
        assert_eq!(a.film_override.as_deref().map(|f| f.get(1).cloned()), Some(Some("s04".to_string())));
        a.undo_redo(&mut svc, true);
        assert_eq!(a.graph.film.get(3).map(String::as_str), Some("s04"));
        assert!(a.pick_transition(&mut svc, "cut:s02:s03", "Dip to black", 0.5).is_ok());
        assert!(a.pick_transition(&mut svc, "cut:s02:s03", "Wipe", f32::NAN).is_err());
        assert!(a.pick_transition(&mut svc, "nope", "Wipe", 1.0).is_err());
        let c = a.graph.cuts.iter().find(|c| c.id == "cut:s02:s03").cloned().unwrap();
        assert_eq!(a.cut_label(&c).0, "Dip to black");
    }

    #[test]
    fn highlight_dims_and_lod_picks_detail() {
        let a = area();
        let n = a.graph.get("s04").cloned().unwrap();
        assert!(Highlight::NeedsYou.matches(&n));
        assert!(!Highlight::Making.matches(&n));
        assert_eq!(Lod::of(0.8), Lod::Near);
        assert_eq!(Lod::of(0.5), Lod::Middle);
        assert_eq!(Lod::of(0.25), Lod::Far);
        let near = node_size(&n, Lod::Near);
        let far = node_size(&n, Lod::Far);
        assert!(near.y > far.y && near.x == far.x);
    }

    #[test]
    fn frames_all_when_a_project_opens_and_when_its_shots_first_appear() {
        let mut a = NodeArea::default();
        a.load(&json!({"nodes": [{"id": "b01", "kind": "beat", "order": 0, "title": "One"}]}));
        assert!(matches!(a.want_frame.take(), Some(Frame::All)), "opened: frame all");
        a.load(&json!({"nodes": [{"id": "b01", "kind": "beat", "order": 0, "title": "One"}]}));
        assert!(a.want_frame.is_none(), "a later load keeps the person's view");
        a.load(&crate::nodes::model::tests::sample());
        assert!(matches!(a.want_frame.take(), Some(Frame::Film)), "the shots appeared: frame them all");
        a.load(&crate::nodes::model::tests::sample());
        assert!(a.want_frame.is_none());
    }
}
