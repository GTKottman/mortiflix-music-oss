//! The node canvas: free placement like Blender (owner, 2026-10-16). Nodes stay where you drop
//! them and the noodles follow; the film order is the chain, not where nodes sit. Pan (drag empty
//! space, middle-drag anywhere, trackpad), zoom at the pointer (wheel, pinch, Ctrl + scroll), box
//! select (Shift + drag), drag a node anywhere, drop a shot on a noodle to put it there, or pull a
//! noodle from a shot's socket to another shot to rewire the chain.

use std::sync::Arc;

use egui::text::{LayoutJob, TextWrapping};
use egui::{Align2, Color32, CornerRadius, FontId, Galley, Painter, PointerButton, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, TextFormat, Vec2, pos2, vec2};
use serde_json::{Value, json};

use super::area::{Frame, Lod, NodeArea, STILL_H, Wheel, ZOOM_MAX, ZOOM_MIN};
use super::model::{Cut, Kind, Node, NodeState};
use super::stills::{Stills, resolve};
use crate::MfxTheme;

/// FilmCraft's selected outline (`#ebebeb`).
pub const SELECTED: Color32 = Color32::from_rgb(0xeb, 0xeb, 0xeb);
/// The shot under the playhead (`#5895ec`, part 36).
pub const PLAYHEAD: Color32 = Color32::from_rgb(0x58, 0x95, 0xec);
/// Distance (screen px) at which a dragged shot "lands on" a noodle.
const NOODLE_HIT: f32 = 14.0;

/// What a canvas pass asks the host for: automation ids and commands.
#[derive(Default)]
pub struct Out {
    pub auto: Vec<(String, egui::Response, String)>,
    pub cmds: Vec<(String, Value)>,
}

impl Out {
    fn auto(&mut self, id: String, r: &egui::Response, label: &str) {
        self.auto.push((id, r.clone(), label.to_string()));
    }
    fn cmd(&mut self, id: &str, params: Value) {
        self.cmds.push((id.to_string(), params));
    }
}

/// A drag in progress on the canvas (kept in egui memory between frames).
#[derive(Clone, Debug, Default, PartialEq)]
enum Drag {
    #[default]
    None,
    /// Moving nodes: grabbed at this world point.
    Nodes {
        ids: Vec<String>,
        grab: Pos2,
    },
    /// Pulling a noodle from a shot's socket (`out` = the film-out socket).
    Link {
        from: String,
        out: bool,
    },
    /// Box select from this world point.
    Box {
        start: Pos2,
    },
    Pan,
}

fn drag_id() -> egui::Id {
    egui::Id::new("mfx-nodes-drag")
}

/// Text size at this zoom: grows with zoom, never under 11 px (doc 35).
pub fn text_size(z: f32, base: f32) -> f32 {
    (base * z).clamp(11.0, base * 1.4)
}

/// One line of text, cut with `…` to fit.
pub fn line(p: &Painter, text: &str, size: f32, col: Color32, max_w: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(text.to_owned(), TextFormat::simple(FontId::proportional(size), col));
    job.wrap = TextWrapping { max_width: max_w.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    p.layout_job(job)
}

fn fade(c: Color32, dim: bool) -> Color32 {
    if dim { c.gamma_multiply(0.35) } else { c }
}

/// A cubic noodle from `a` to `b` leaving and entering horizontally (Blender's look).
pub fn noodle(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let dx = ((b.x - a.x).abs() * 0.5).clamp(40.0, 240.0);
    let (c1, c2) = (pos2(a.x + dx, a.y), pos2(b.x - dx, b.y));
    (0..=24)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            pos2(w[0] * a.x + w[1] * c1.x + w[2] * c2.x + w[3] * b.x, w[0] * a.y + w[1] * c1.y + w[2] * c2.y + w[3] * b.y)
        })
        .collect()
}

/// A vertical noodle (voice line up to its shot, look down to a shot).
fn noodle_v(a: Pos2, b: Pos2) -> Vec<Pos2> {
    let dy = ((b.y - a.y).abs() * 0.5).max(20.0);
    let s = if b.y < a.y { -1.0 } else { 1.0 };
    let (c1, c2) = (pos2(a.x, a.y + dy * s), pos2(b.x, b.y - dy * s));
    (0..=20)
        .map(|i| {
            #[allow(clippy::cast_precision_loss)]
            let t = i as f32 / 20.0;
            let u = 1.0 - t;
            let w = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            pos2(w[0] * a.x + w[1] * c1.x + w[2] * c2.x + w[3] * b.x, w[0] * a.y + w[1] * c1.y + w[2] * c2.y + w[3] * b.y)
        })
        .collect()
}

/// Distance from a point to a polyline.
pub fn distance_to(points: &[Pos2], p: Pos2) -> f32 {
    let mut best = f32::INFINITY;
    for w in points.windows(2) {
        let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
        let ab = *b - *a;
        let len2 = ab.length_sq();
        let t = if len2 > 0.0 { ((p - *a).dot(ab) / len2).clamp(0.0, 1.0) } else { 0.0 };
        best = best.min((*a + ab * t - p).length());
    }
    best
}

/// Seconds as `0:21`.
pub fn clock(sec: f64) -> String {
    if !sec.is_finite() || sec < 0.0 {
        return "0:00".into();
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let s = sec.round().min(1.0e7) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

/// The colour a state's glyph and word use (never colour alone: the glyph and word carry it).
pub fn state_colour(s: NodeState, t: &MfxTheme) -> Color32 {
    match s {
        NodeState::Ok => t.ok,
        NodeState::NeedsYou => t.warn,
        NodeState::Making | NodeState::Remaking => t.accent_hover,
        NodeState::OutOfDate => t.warn.gamma_multiply(0.8),
        NodeState::Cut => t.text_faint,
        NodeState::Planned => t.text_faint,
        NodeState::Ready => t.text_dim,
    }
}

/// Where each shot starts in the film (seconds), shots of unknown length counted as 4 s.
pub fn starts(area: &NodeArea) -> std::collections::HashMap<String, (f64, f64)> {
    let mut at = 0.0;
    let mut m = std::collections::HashMap::new();
    for id in &area.graph.film {
        let Some(n) = area.graph.get(id) else { continue };
        if n.state == NodeState::Cut {
            continue;
        }
        let len = n.seconds().unwrap_or(4.0);
        m.insert(id.clone(), (at, len));
        at += len;
    }
    m
}

/// Draw the canvas in `rect` and handle its input.
#[allow(clippy::too_many_lines)]
pub fn show(parent: &mut egui::Ui, area: &mut NodeArea, t: &MfxTheme, rect: Rect, out: &mut Out) {
    // everything on the canvas is clipped to it: a node half under the toolbar can't take its clicks
    let mut child = parent.new_child(egui::UiBuilder::new().max_rect(rect).id_salt("mfx-nodes-canvas-ui"));
    child.set_clip_rect(rect.intersect(parent.clip_rect()));
    let ui = &mut child;
    area.canvas = [rect.min.x, rect.min.y, rect.width(), rect.height()];
    let ctx = ui.ctx().clone();
    let was_following = area.follow_playhead;
    let stills = Stills::of(&ctx);
    stills.poll(&ctx);

    // ---- framing asked by a command or a key
    if let Some(f) = area.want_frame.take() {
        let r = match &f {
            Frame::All => Some(area.content_rect()),
            Frame::Selected => {
                let mut r = Rect::NOTHING;
                for id in area.selection.clone() {
                    if let Some(nr) = area_rect(area, &id) {
                        r = r.union(nr.expand2(vec2(nr.width() * 0.8, nr.height() * 0.2)));
                    }
                }
                (r != Rect::NOTHING).then_some(r)
            }
            Frame::Node(id) | Frame::Reveal(id) => area_rect(area, id),
            Frame::Beat(b) => {
                let mut r = Rect::NOTHING;
                for s in area.graph.beat_shots(b) {
                    if let Some(nr) = area.node_rect(&s.id) {
                        r = r.union(nr);
                    }
                }
                (r != Rect::NOTHING).then_some(r)
            }
        };
        if let Some(r) = r.filter(|r| r.is_finite() && *r != Rect::NOTHING) {
            if matches!(f, Frame::Reveal(_)) { area.reveal(r) } else { area.frame_rect(r) }
        }
    }

    // ---- background: pan, box select, deselect, wheel / pinch zoom
    let bg = ui.interact(rect, egui::Id::new("mfx-nodes-canvas"), Sense::click_and_drag());
    out.auto("mfx.nodes.canvas".into(), &bg, "Node canvas");
    let hover = ctx.pointer_hover_pos().filter(|p| rect.contains(*p) && ui.is_rect_visible(rect));
    let over_popup = hover.is_some_and(|p| ctx.layer_id_at(p).is_some_and(|l| l != ui.layer_id()));
    if let Some(p) = hover.filter(|_| !over_popup) {
        wheel(ui, area, p);
    }

    let mut drag: Drag = ctx.data(|d| d.get_temp(drag_id())).unwrap_or_default();
    let mods = ctx.input(|i| i.modifiers);
    let pointer = ctx.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
    let pointer_world = pointer.map(|p| area.to_world(p));
    // a drag starts where the button went down (egui reports it a few pixels later)
    let press_world = ctx.input(|i| i.pointer.press_origin()).map(|p| area.to_world(p)).or(pointer_world);

    if bg.drag_started_by(PointerButton::Primary) && mods.shift {
        if let Some(p) = press_world {
            drag = Drag::Box { start: p };
        }
    } else if bg.drag_started() {
        drag = Drag::Pan;
    }
    if bg.dragged() && drag == Drag::Pan {
        area.pan_by(bg.drag_delta());
        area.follow_playhead = false;
    }
    if bg.clicked() && !mods.command && !mods.shift {
        area.selection.clear();
        area.fix_open = false;
    }
    bg.context_menu(|ui| background_menu(ui, area, out, pointer_world));

    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.field_bg);
    grid(&p, area, rect, t);

    let z = area.zoom;
    let lod = Lod::of(z);
    let view = area.view_world().expand(400.0);
    let found = area.found();
    let starts = starts(area);
    let selected: std::collections::HashSet<String> = area.selection.iter().cloned().collect();

    // ---- beat bands (behind everything): a 3 px marker-colour line over the beat's shots
    for beat in area.graph.beats.clone() {
        let mut r = Rect::NOTHING;
        for s in area.graph.beat_shots(&beat) {
            if s.state != NodeState::Cut
                && let Some(nr) = area.node_rect(&s.id)
            {
                r = r.union(nr);
            }
        }
        if r == Rect::NOTHING || !view.intersects(r) {
            continue;
        }
        let sr = area.rect_to_screen(r);
        let (num, name) = area.graph.beat_title(&beat);
        let colour = area.graph.get(&beat).and_then(|b| b.colour).or_else(|| area.graph.beat_shots(&beat).first().and_then(|s| area.graph.colour_of(s)));
        let chip = colour.map_or(t.text_dim, |c| c.chip);
        let y = sr.min.y - 10.0;
        let band = Rect::from_min_max(pos2(sr.min.x, y - 20.0), pos2(sr.max.x, y + 3.0));
        let resp = ui.interact(band, egui::Id::new(("mfx-beat", &beat)), Sense::click_and_drag());
        let sel = selected.contains(&beat);
        p.line_segment([pos2(sr.min.x, y), pos2(sr.max.x, y)], Stroke::new(3.0, if sel { SELECTED } else { chip }));
        let size = text_size(z, 12.0);
        let chip_r = Rect::from_min_size(pos2(sr.min.x, y - 6.0 - size - 2.0), vec2(size + 6.0, size + 2.0));
        p.rect_filled(chip_r, 2.0, chip);
        p.text(chip_r.center(), Align2::CENTER_CENTER, num.to_string(), FontId::proportional(size - 1.0), Color32::BLACK);
        let g = line(&p, &name, size, if resp.hovered() || sel { t.text } else { t.text_dim }, (sr.width() - chip_r.width() - 8.0).max(60.0));
        p.galley(pos2(chip_r.max.x + 6.0, chip_r.center().y - g.size().y / 2.0), g, t.text);
        let label = format!("{num} {name}");
        out.auto(format!("mfx.nodes.beat.{beat}"), &resp, &label);
        if resp.clicked() {
            area.selection = vec![beat.clone()];
        }
        if resp.double_clicked() {
            area.want_frame = Some(Frame::Beat(beat.clone()));
        }
        if resp.drag_started_by(PointerButton::Primary)
            && let Some(w) = press_world
        {
            let mut ids: Vec<String> = area.graph.beat_shots(&beat).iter().map(|s| s.id.clone()).collect();
            ids.extend(
                area.graph.nodes.iter().filter(|n| matches!(n.kind, Kind::Line | Kind::Cue) && n.beat.as_deref() == Some(beat.as_str())).map(|n| n.id.clone()),
            );
            drag = Drag::Nodes { ids, grab: w };
        }
        follow_drag(area, &resp, &mut drag, pointer_world, out, mods.alt);
    }

    // ---- noodles: film order (shot → transition → shot), voice (dashed), look and assets (dotted)
    let mut film_noodles: Vec<(String, Vec<Pos2>)> = Vec::new();
    let cuts = area.graph.cuts.clone();
    for cut in &cuts {
        let (Some(a), Some(b)) = (area.node_rect(&cut.from), area.node_rect(&cut.to)) else { continue };
        let pill = area.cut_rect(cut);
        let (sa, sb) = (area.to_screen(area.socket_out(a)), area.to_screen(area.socket_in(b)));
        let (pl, pr) = (area.to_screen(pos2(pill.min.x, pill.center().y)), area.to_screen(pos2(pill.max.x, pill.center().y)));
        let ok = area.graph.get(&cut.from).is_some_and(|n| n.state == NodeState::Ok);
        let col = if ok { t.text } else { t.text_dim.gamma_multiply(0.8) };
        let w = (2.0 * z).clamp(1.5, 3.0);
        let first = noodle(sa, pl);
        let second = noodle(pr, sb);
        p.add(Shape::line(first.clone(), Stroke::new(w, col)));
        p.add(Shape::line(second.clone(), Stroke::new(w, col)));
        let mut all = first;
        all.extend(second);
        film_noodles.push((cut.id.clone(), all));
    }
    if area.show.voice {
        for n in area.graph.nodes.iter().filter(|n| n.kind == Kind::Line && !n.missing) {
            let (Some(shot), Some(lr)) = (area.graph.line_shot(n), area.node_rect(&n.id)) else { continue };
            let Some(sr) = area.node_rect(&shot) else { continue };
            let a = area.to_screen(pos2(lr.center().x, lr.min.y));
            let b = area.to_screen(pos2(sr.center().x, sr.max.y + 6.0));
            let pts = noodle_v(a, b);
            let dim = area.dimmed(n, &found);
            p.extend(Shape::dashed_line(&pts, Stroke::new((1.5 * z).clamp(1.0, 2.0), fade(t.text_faint, dim)), 6.0, 4.0));
        }
    }
    if area.show.inputs
        && let Some(sel) = area.primary().and_then(|s| area.graph.get(s)).filter(|n| n.kind == Kind::Shot)
    {
        let mut inputs: Vec<String> = sel.look.iter().cloned().collect();
        inputs.extend(sel.uses.iter().cloned());
        for i in inputs {
            let (Some(ir), Some(sr)) = (area.node_rect(&i), area.node_rect(&sel.id)) else { continue };
            let a = area.to_screen(pos2(ir.center().x, ir.max.y));
            let b = area.to_screen(pos2(sr.center().x, sr.min.y - 6.0));
            p.extend(Shape::dotted_line(&noodle_v(a, b), t.accent_hover, 6.0, 1.5));
        }
    }

    // ---- what a drag would do: drop a shot on a noodle, or a pulled noodle onto a shot
    let mut insert_on: Option<String> = None;
    let mut hint: Option<(Pos2, String)> = None;
    if let (Drag::Nodes { ids, .. }, Some(pp)) = (&drag, pointer)
        && ids.len() == 1
        && !mods.alt
        && let Some(id) = ids.first()
        && area.graph.get(id).is_some_and(|n| n.kind == Kind::Shot && n.state != NodeState::Cut)
    {
        for (cid, pts) in &film_noodles {
            let Some(c) = cuts.iter().find(|c| &c.id == cid) else { continue };
            if c.from == *id || c.to == *id {
                continue;
            }
            if distance_to(pts, pp) < NOODLE_HIT {
                insert_on = Some(cid.clone());
                p.add(Shape::line(pts.clone(), Stroke::new(4.0, t.accent)));
                hint = Some((pp, "Drop to put it here".into()));
                break;
            }
        }
    }
    let mut link_target: Option<String> = None;
    if let (Drag::Link { from, out: from_out }, Some(pp)) = (&drag, pointer)
        && let Some(fr) = area.node_rect(from)
    {
        let sock = area.to_screen(if *from_out { area.socket_out(fr) } else { area.socket_in(fr) });
        let pts = if *from_out { noodle(sock, pp) } else { noodle(pp, sock) };
        link_target = shot_at(area, pp, from);
        p.add(Shape::line(pts, Stroke::new(2.5, if link_target.is_some() { t.accent_hover } else { t.text })));
        if let Some(tg) = &link_target
            && let Some(tr) = area.node_rect(tg)
        {
            p.rect_stroke(area.rect_to_screen(tr).expand(3.0), 6.0, Stroke::new(2.0, t.accent_hover), StrokeKind::Outside);
            let name = area.graph.get(tg).map(Node::display_name).unwrap_or_default();
            let from_name = area.graph.get(from).map(Node::display_name).unwrap_or_default();
            let text = if *from_out { format!("{name} comes after {from_name}") } else { format!("{from_name} comes after {name}") };
            hint = Some((pp, text));
        }
    }

    // ---- transitions: a pill on every cut (`! Pick one` on a new cut)
    for cut in &cuts {
        let wr = area.cut_rect(cut);
        if !view.intersects(wr) {
            continue;
        }
        let r = area.rect_to_screen(wr);
        let sel = selected.contains(&cut.id);
        let (kind, sec, pick) = area.cut_label(cut);
        let dim = !area.find.trim().is_empty() && !found.contains(&cut.id) || area.highlight != super::area::Highlight::All;
        let fill = fade(if pick { t.error.gamma_multiply(0.18) } else { t.header_bg }, dim);
        let radius = CornerRadius::same(u8::try_from(((r.height() / 2.0) as i32).clamp(2, 40)).unwrap_or(8));
        p.rect_filled(r, radius, fill);
        if pick {
            let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
            p.extend(Shape::dashed_line(&pts, Stroke::new(1.2, fade(t.error, dim)), 5.0, 3.0));
        } else {
            p.rect_stroke(r, radius, Stroke::new(1.0, fade(t.separator.gamma_multiply(2.0), dim)), StrokeKind::Inside);
        }
        if sel {
            p.rect_stroke(r.expand(2.0), radius, Stroke::new(2.0, SELECTED), StrokeKind::Outside);
        }
        let text = match sec {
            Some(s) if !pick && kind != "Cut" => format!("{kind} {s:.1} s"),
            _ => kind.clone(),
        };
        // far out the pill is too small for words: its kind shows on hover and in the inspector
        if r.width() >= 44.0 && r.height() >= 13.0 {
            let size = text_size(z, 11.5);
            let g = line(&p, &text, size, fade(if pick { t.error } else { t.text_dim }, dim), r.width() - 8.0);
            p.galley(r.center() - g.size() / 2.0, g, t.text);
        }
        let resp = ui.interact(r, egui::Id::new(("mfx-cut", &cut.id)), Sense::click_and_drag());
        out.auto(format!("mfx.nodes.cut.{}", cut.id), &resp, &text);
        if resp.clicked() {
            if mods.command {
                area.toggle_select(&cut.id);
            } else {
                area.select_only(&cut.id);
            }
        }
        if resp.double_clicked() {
            play(out, area, &cut.id, "cut");
        }
        if resp.drag_started_by(PointerButton::Primary)
            && let Some(w) = press_world
        {
            if !area.selection.contains(&cut.id) {
                area.select_only(&cut.id);
            }
            drag = Drag::Nodes { ids: area.selection.clone(), grab: w };
        }
        follow_drag(area, &resp, &mut drag, pointer_world, out, mods.alt);
        resp.context_menu(|ui| cut_menu(ui, area, cut, out));
    }

    // ---- nodes, back to front
    // selected nodes on top (the one you drag is never under another)
    let mut order = area.drawn();
    order.sort_by_key(|id| selected.contains(id));
    for id in order {
        let Some(n) = area.graph.get(&id).cloned() else { continue };
        if n.kind == Kind::Transition {
            continue; // drawn with the cuts below
        }
        let Some(wr) = area.node_rect(&id) else { continue };
        if !view.intersects(wr) {
            continue;
        }
        let r = area.rect_to_screen(wr);
        let dim = area.dimmed(&n, &found);
        let sel = selected.contains(&id);
        let resp = ui.interact(r, egui::Id::new(("mfx-node", &id)), Sense::click_and_drag());
        let start = starts.get(&id).copied();
        match n.kind {
            Kind::Shot => shot_node(ui, &p, area, &n, r, lod, dim, sel, start, &stills, t, out, &mut drag),
            Kind::Line => line_node(ui, &p, area, &n, r, lod, dim, sel, t, out),
            Kind::Cue => cue_node(&p, area, &n, r, dim, sel, t),
            _ => input_node(&p, area, &n, r, lod, dim, sel, &stills, t),
        }
        let label = format!("{} {}", n.display_name(), n.state.word());
        out.auto(format!("mfx.nodes.node.{id}"), &resp, &label);
        node_input(area, &n, &resp, &mut drag, press_world, mods, out);
        if n.kind == Kind::Shot {
            // a look or one of your assets dragged from the Bin: "Use it in shot 4"
            if hovering_input(&resp) {
                p.rect_stroke(r.expand(3.0), 6.0, Stroke::new(2.0, t.accent_hover), StrokeKind::Outside);
                let name = n.display_name();
                p.text(
                    r.center_top() - vec2(0.0, 8.0),
                    Align2::CENTER_BOTTOM,
                    format!("Use it in shot {}", name.trim_start_matches('#')),
                    FontId::proportional(12.0),
                    t.text,
                );
            }
            if let Some(input) = dropped_input(&resp) {
                out.cmd("nodes.use", json!({"node": n.id, "input": input}));
            }
        }
        follow_drag(area, &resp, &mut drag, pointer_world, out, mods.alt);
        resp.context_menu(|ui| node_menu(ui, area, &n, out));
    }

    // ---- what the drop will do, over everything
    if let Some((pp, text)) = &hint {
        let g = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), t.text);
        let r = Rect::from_min_size(*pp + vec2(14.0, -18.0 - g.size().y), g.size()).expand(4.0);
        p.rect_filled(r, 3.0, t.header_bg);
        p.galley(r.min + vec2(4.0, 4.0), g, t.text);
    }

    // ---- box select
    if let (Drag::Box { start }, Some(pw)) = (&drag, pointer_world) {
        let wr = Rect::from_two_pos(*start, pw);
        let sr = area.rect_to_screen(wr);
        p.rect_filled(sr, 2.0, t.accent.gamma_multiply(0.12));
        p.rect_stroke(sr, 2.0, Stroke::new(1.0, t.accent), StrokeKind::Inside);
        if bg.drag_stopped() {
            let mut sel: Vec<String> = if mods.command { area.selection.clone() } else { Vec::new() };
            for id in area.drawn() {
                if area.node_rect(&id).is_some_and(|r| r.intersects(wr)) && !sel.contains(&id) {
                    sel.push(id);
                }
            }
            for c in &cuts {
                if area.cut_rect(c).intersects(wr) && !sel.contains(&c.id) {
                    sel.push(c.id.clone());
                }
            }
            area.selection = sel;
            drag = Drag::None;
        }
    }

    // ---- drops
    let released = ctx.input(|i| i.pointer.any_released());
    if released {
        match &drag {
            Drag::Nodes { ids, .. } => {
                if let Some((moved, d)) = area.drag_offset.take()
                    && (d[0].abs() > 0.5 || d[1].abs() > 0.5)
                {
                    let moves: Vec<Value> = moved
                        .iter()
                        .filter_map(|id| {
                            let r = area_rect_still(area, id)?;
                            Some(json!({"node": id, "x": r.min.x + d[0], "y": r.min.y + d[1]}))
                        })
                        .collect();
                    // the command runs right after this pass, before the next frame: nothing jumps
                    out.cmd("nodes.move", json!({"moves": moves}));
                }
                if let (Some(cid), Some(id)) = (&insert_on, ids.first())
                    && let Some(c) = cuts.iter().find(|c| &c.id == cid)
                {
                    out.cmd("nodes.reorder", json!({"node": id, "after": c.from}));
                }
            }
            Drag::Link { from, out: from_out } => {
                if let Some(tg) = &link_target {
                    if *from_out {
                        out.cmd("nodes.reorder", json!({"node": tg, "after": from}));
                    } else {
                        out.cmd("nodes.reorder", json!({"node": from, "after": tg}));
                    }
                }
            }
            _ => {}
        }
        if !matches!(drag, Drag::Box { .. }) || !ctx.input(|i| i.pointer.any_down()) {
            drag = Drag::None;
        }
        area.drag_offset = None;
    }
    ctx.data_mut(|d| d.insert_temp(drag_id(), drag));
    // panning by hand turns Follow the playhead off (part 36 owns the switch)
    if was_following && !area.follow_playhead {
        out.cmd("mortiflix.preview.follow", json!({"on": false}));
    }

    // ---- empty states
    if area.loaded && area.graph.film.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, "Shots appear here as their first frames are made.", FontId::proportional(13.0), t.text_dim);
    } else if !found.is_empty() || area.find.trim().is_empty() {
    } else {
        let msg = format!("No shots match \u{201c}{}\u{201d}.", area.find.trim());
        p.text(rect.center_top() + vec2(0.0, 28.0), Align2::CENTER_CENTER, msg, FontId::proportional(13.0), t.text);
    }
}

/// Play in the loose preview (part 36's `mortiflix.preview.play {node, scope}`, scopes all / from /
/// shot / beat). A cut plays from the shot before it; a voice line plays its shot; a cue its beat.
pub fn play(out: &mut Out, area: &NodeArea, id: &str, scope: &str) {
    let (node, scope) = if let Some(c) = area.graph.cuts.iter().find(|c| c.id == id) {
        (c.from.clone(), "from")
    } else {
        match area.graph.get(id) {
            Some(n) if n.kind == Kind::Line => (area.graph.line_shot(n).unwrap_or_else(|| id.to_string()), "shot"),
            Some(n) if n.kind == Kind::Cue || n.kind == Kind::Beat => {
                let beat = if n.kind == Kind::Beat { Some(n.id.as_str()) } else { n.beat.as_deref() };
                (beat.and_then(|b| area.graph.beat_shots(b).first().map(|s| s.id.clone())).unwrap_or_else(|| id.to_string()), "beat")
            }
            _ => (id.to_string(), if scope == "cut" { "from" } else { scope }),
        }
    };
    out.cmd("mortiflix.preview.play", json!({"node": node, "scope": scope}));
}

/// A look or asset being dragged over a node: from the Bin here, or from part 02's Assets tab.
/// MERGE NOTE (parts 02 + 03): with `crate::chat::AssetDrag` in the tree, add
/// `|| resp.dnd_hover_payload::<crate::chat::AssetDrag>().is_some()`.
fn hovering_input(resp: &egui::Response) -> bool {
    resp.dnd_hover_payload::<super::panel::BinItem>().is_some()
}

/// The look or asset id dropped on a node (`nodes.use` sends `assets.use {asset, node}` for assets).
/// MERGE NOTE (parts 02 + 03): with `crate::chat::AssetDrag` in the tree, add
/// `.or_else(|| resp.dnd_release_payload::<crate::chat::AssetDrag>().map(|a| a.id.clone()))`.
fn dropped_input(resp: &egui::Response) -> Option<String> {
    resp.dnd_release_payload::<super::panel::BinItem>().map(|i| i.0.clone())
}

/// The rectangle of a node or a cut (by id).
pub fn area_rect(area: &NodeArea, id: &str) -> Option<Rect> {
    area.node_rect(id).or_else(|| area.graph.cuts.iter().find(|c| c.id == id).map(|c| area.cut_rect(c)))
}

fn area_rect_still(area: &NodeArea, id: &str) -> Option<Rect> {
    let d = area.offset_of(id);
    area_rect(area, id).map(|r| r.translate(-d))
}

/// The shot under a screen point (its body or film sockets), not `except`.
fn shot_at(area: &NodeArea, p: Pos2, except: &str) -> Option<String> {
    area.graph.film.iter().rev().find_map(|id| {
        if id == except {
            return None;
        }
        let n = area.graph.get(id)?;
        if n.state == NodeState::Cut {
            return None;
        }
        let r = area.rect_to_screen(area.node_rect(id)?).expand(14.0);
        r.contains(p).then(|| id.clone())
    })
}

/// Wheel, trackpad and pinch over the canvas (doc 35: a mouse notch zooms, a trackpad pans, pinch
/// and Ctrl/⌘ + scroll zoom, Shift + wheel pans sideways; Settings can make the wheel scroll).
fn wheel(ui: &egui::Ui, area: &mut NodeArea, at: Pos2) {
    let mut pan = Vec2::ZERO;
    let mut zoom = 1.0f32;
    ui.input(|i| {
        for e in &i.events {
            match e {
                egui::Event::MouseWheel { unit, delta, modifiers, .. } => {
                    if !delta.x.is_finite() || !delta.y.is_finite() {
                        continue;
                    }
                    let command = modifiers.command || modifiers.ctrl || modifiers.mac_cmd;
                    let notch = matches!(unit, egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page);
                    let points = match unit {
                        egui::MouseWheelUnit::Point => 1.0,
                        egui::MouseWheelUnit::Line => 40.0,
                        egui::MouseWheelUnit::Page => 400.0,
                    };
                    let d = *delta * points;
                    if command || (notch && area.wheel == Wheel::Zoom && !modifiers.shift) {
                        let amount = d.y + d.x;
                        zoom *= (amount * 0.0025).exp();
                    } else if modifiers.shift && d.x.abs() < f32::EPSILON {
                        pan += vec2(d.y, 0.0);
                    } else {
                        pan += d;
                    }
                }
                egui::Event::Zoom(f) if f.is_finite() && *f > 0.0 => zoom *= *f,
                _ => {}
            }
        }
    });
    if pan != Vec2::ZERO {
        area.pan_by(pan);
        area.follow_playhead = false;
    }
    if (zoom - 1.0).abs() > f32::EPSILON {
        let z = zoom.clamp(0.2, 5.0);
        area.zoom_at(at, z);
        area.zoom = area.zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

/// A faint dot grid that pans and zooms with the canvas (like Blender's).
fn grid(p: &Painter, area: &NodeArea, rect: Rect, t: &MfxTheme) {
    let step = if area.zoom < 0.45 { 160.0 } else { 40.0 };
    let v = area.view_world();
    let col = t.separator.gamma_multiply(1.4);
    let x0 = (v.min.x / step).floor() * step;
    let y0 = (v.min.y / step).floor() * step;
    let mut n = 0;
    let mut y = y0;
    while y <= v.max.y && n < 6000 {
        let mut x = x0;
        while x <= v.max.x && n < 6000 {
            let s = area.to_screen(pos2(x, y));
            if rect.contains(s) {
                p.circle_filled(s, 1.0, col);
            }
            x += step;
            n += 1;
        }
        y += step;
    }
}

/// Click, Ctrl/⌘ + click, double-click, and the start of a node drag.
fn node_input(area: &mut NodeArea, n: &Node, resp: &egui::Response, drag: &mut Drag, pointer_world: Option<Pos2>, mods: egui::Modifiers, out: &mut Out) {
    if resp.clicked() {
        if n.kind == Kind::Shot && n.state == NodeState::Cut {
            out.cmd("nodes.bringBack", json!({"node": n.id}));
        }
        if mods.command {
            area.toggle_select(&n.id);
        } else {
            area.select_only(&n.id);
        }
    }
    if resp.double_clicked() {
        play(out, area, &n.id, "shot");
    }
    if resp.drag_started_by(PointerButton::Middle) {
        *drag = Drag::Pan;
    }
    if resp.drag_started_by(PointerButton::Primary)
        && let Some(w) = pointer_world
    {
        if !area.selection.contains(&n.id) {
            if mods.command {
                area.selection.push(n.id.clone());
            } else {
                area.select_only(&n.id);
            }
        }
        *drag = Drag::Nodes { ids: area.selection.clone(), grab: w };
    }
}

/// While a widget is dragged: move nodes (all selected together) or pan.
fn follow_drag(area: &mut NodeArea, resp: &egui::Response, drag: &mut Drag, pointer_world: Option<Pos2>, _out: &mut Out, _alt: bool) {
    if !resp.dragged() {
        return;
    }
    match drag {
        Drag::Nodes { ids, grab } => {
            if let Some(w) = pointer_world {
                // pointer_world is read with last frame's offset applied nowhere: it is the world point
                let d = w - *grab;
                if d.x.is_finite() && d.y.is_finite() {
                    area.drag_offset = Some((ids.clone(), [d.x, d.y]));
                }
            }
        }
        Drag::Pan => {
            area.pan_by(resp.drag_delta());
            area.follow_playhead = false;
        }
        _ => {}
    }
}

/// Draw one shot node (doc 35 "The parts of a node").
#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn shot_node(
    ui: &mut egui::Ui,
    p: &Painter,
    area: &mut NodeArea,
    n: &Node,
    r: Rect,
    lod: Lod,
    dim: bool,
    sel: bool,
    start: Option<(f64, f64)>,
    stills: &Stills,
    t: &MfxTheme,
    out: &mut Out,
    drag: &mut Drag,
) {
    let z = area.zoom;
    let colour = area.graph.colour_of(n);
    let fill = colour.map_or(t.hover, |c| c.fill);
    let chip = colour.map_or(t.text_dim, |c| c.chip);
    let body = t.header_bg;
    if n.state == NodeState::Cut {
        // a sliver where it was: click it to bring it back
        p.rect_filled(r, 3.0, fade(body, true));
        let pts = vec![r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom(), r.left_top()];
        p.extend(Shape::dashed_line(&pts, Stroke::new(1.0, t.text_faint), 4.0, 3.0));
        p.text(r.center_top() + vec2(0.0, 14.0), Align2::CENTER_CENTER, "×", FontId::proportional(13.0), t.text_faint);
        let g = line(p, &n.display_name(), 11.0, t.text_faint, r.height());
        p.galley(r.center() - vec2(g.size().x / 2.0, 0.0), g, t.text_faint);
        if sel {
            p.rect_stroke(r.expand(2.0), 4.0, Stroke::new(2.0, SELECTED), StrokeKind::Outside);
        }
        return;
    }
    let radius = 5.0;
    p.rect_filled(r, radius, fade(body, dim));
    let head_h = (24.0 * z).max(text_size(z, 12.0) + 6.0).min(r.height());
    let head = Rect::from_min_size(r.min, vec2(r.width(), head_h));
    p.rect_filled(head, CornerRadius { nw: 5, ne: 5, sw: 0, se: 0 }, fade(fill, dim));
    // beat number on its marker colour, always beside the colour (doc 11 §6)
    let size = text_size(z, 12.0);
    let beat_n = n.beat.as_deref().map_or(0, |b| area.graph.beat_title(b).0);
    let chip_r = Rect::from_min_size(head.min + vec2(4.0, (head_h - size - 2.0) / 2.0), vec2(size + 4.0, size + 2.0));
    if beat_n > 0 {
        p.rect_filled(chip_r, 2.0, fade(chip, dim));
        p.text(chip_r.center(), Align2::CENTER_CENTER, beat_n.to_string(), FontId::proportional(size - 1.0), Color32::BLACK);
    }
    let name_x = if beat_n > 0 { chip_r.max.x + 5.0 } else { head.min.x + 6.0 };
    let name = line(p, &n.display_name(), size, fade(Color32::WHITE, dim), (head.max.x - name_x - 4.0).max(10.0));
    let name_w = name.size().x;
    p.galley(pos2(name_x, head.center().y - name.size().y / 2.0), name, Color32::WHITE);
    if lod == Lod::Far {
        // far: `#4` + state on one line
        let g = line(p, n.state.glyph(), size, fade(state_colour(n.state, t), dim), 20.0);
        p.galley(pos2(name_x + name_w + 6.0, head.center().y - g.size().y / 2.0), g, t.text);
    } else if let Some((s, len)) = start {
        let when = format!("{} · {len:.1} s", clock(s));
        let g = line(p, &when, (size - 1.0).max(11.0), fade(Color32::from_white_alpha(200), dim), (head.max.x - name_x - name_w - 12.0).max(1.0));
        p.galley(pos2(head.max.x - 6.0 - g.size().x, head.center().y - g.size().y / 2.0), g, t.text);
    }
    let mut y = head.max.y;
    if lod >= Lod::Middle {
        let sh = (STILL_H * z).min(r.max.y - y);
        let still = Rect::from_min_size(pos2(r.min.x, y), vec2(r.width(), sh));
        let tex =
            n.still().and_then(|f| resolve(f, &area.graph.base, &area.studio, area.project.as_deref())).and_then(|path| stills.get(&path)).or_else(|| {
                // a planned or video shot shows its look's still until it has its own
                let look = n.look.as_deref().and_then(|l| area.graph.get(l))?;
                let f = look.still()?;
                stills.get(&resolve(f, &area.graph.base, &area.studio, area.project.as_deref())?)
            });
        match tex {
            Some(tex) => {
                let tint = if dim {
                    Color32::from_white_alpha(90)
                } else if n.still().is_none() {
                    Color32::from_white_alpha(150)
                } else {
                    Color32::WHITE
                };
                p.image(tex.id(), still, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
            }
            None => {
                // no picture yet: an empty 16:9 frame, nothing written on it
                p.rect_filled(still, 0.0, fade(Color32::from_gray(30), dim));
                p.rect_stroke(still.shrink(6.0 * z), 2.0, Stroke::new(1.0, fade(Color32::from_gray(48), dim)), StrokeKind::Inside);
            }
        }
        if n.state == NodeState::Planned {
            let pts = vec![still.left_top(), still.right_top(), still.right_bottom(), still.left_bottom(), still.left_top()];
            p.extend(Shape::dashed_line(&pts, Stroke::new(1.0, t.text_faint), 5.0, 4.0));
        }
        y = still.max.y;
        if lod == Lod::Near {
            let words = Rect::from_min_size(pos2(r.min.x + 6.0, y + 3.0), vec2(r.width() - 12.0, 22.0 * z));
            let g = line(p, &n.title, text_size(z, 12.0), fade(t.text, dim), words.width());
            p.galley(pos2(words.min.x, words.center().y - g.size().y / 2.0), g, t.text);
            y = words.max.y;
        }
        let st = Rect::from_min_size(pos2(r.min.x + 6.0, y), vec2(r.width() - 12.0, (20.0 * z).max(14.0)));
        let mut word = format!("{} {}", n.state.glyph(), n.state.word());
        if n.state == NodeState::Remaking {
            word = format!("» Remaking v{}…", n.versions.len() + 1);
        }
        if let Some(v) = n.version_tag() {
            word = format!("{word}  {v}");
        }
        let g = line(p, &word, text_size(z, 11.5), fade(state_colour(n.state, t), dim), st.width());
        p.galley(pos2(st.min.x, st.center().y - g.size().y / 2.0), g, t.text);
        y = st.max.y;
    }
    // OK · Fix · Cut it (near only: small and quiet; the inspector carries the filled OK)
    if lod == Lod::Near && r.max.y - y > 12.0 {
        let bh = (r.max.y - y - 6.0).min(22.0 * z).max(10.0);
        let bw = (r.width() - 16.0) / 3.0;
        for (i, (key, label)) in [("ok", "OK"), ("fix", "Fix"), ("cut", "Cut it")].iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let br = Rect::from_min_size(pos2(r.min.x + 4.0 + i as f32 * (bw + 4.0), y + 2.0), vec2(bw, bh));
            let resp = ui.interact(br, egui::Id::new(("mfx-node-btn", &n.id, *key)), Sense::click());
            let on = *key == "ok" && n.state == NodeState::Ok;
            let bg = if resp.hovered() {
                t.hover.gamma_multiply(1.6)
            } else if on {
                t.ok.gamma_multiply(0.35)
            } else {
                t.hover
            };
            p.rect_filled(br, 3.0, fade(bg, dim));
            let text = if on { "OK ✓" } else { label };
            let g = line(p, text, text_size(z, 11.0), fade(t.text, dim), br.width() - 4.0);
            p.galley(br.center() - g.size() / 2.0, g, t.text);
            out.auto(format!("mfx.nodes.node.{}.{key}", n.id), &resp, label);
            if resp.clicked() {
                match *key {
                    "ok" => out.cmd("nodes.ok", json!({"node": n.id})),
                    "fix" => {
                        area.select_only(&n.id);
                        area.fix_open = true;
                    }
                    _ => out.cmd("nodes.cut", json!({"node": n.id})),
                }
            }
        }
    }
    // under the playhead: FilmCraft blue (part 36); selected: white 2 px
    if area.playhead_shot.as_deref() == Some(n.id.as_str()) {
        p.rect_stroke(r.expand(if sel { 5.0 } else { 2.0 }), radius + 2.0, Stroke::new(2.0, PLAYHEAD), StrokeKind::Outside);
    }
    if sel {
        p.rect_stroke(r.expand(2.0), radius + 1.0, Stroke::new(2.0, SELECTED), StrokeKind::Outside);
    } else {
        p.rect_stroke(r, radius, Stroke::new(1.0, Color32::BLACK), StrokeKind::Outside);
    }
    // sockets just outside the edge: left film in, right film out, bottom voice, top look/assets
    let wr = area.node_rect(&n.id).unwrap_or(Rect::NOTHING);
    let sr = (5.0 * z).clamp(3.5, 6.0);
    let ok = n.state == NodeState::Ok;
    for (key, w, filled) in [("in", area.socket_in(wr), false), ("out", area.socket_out(wr), ok)] {
        let c = area.to_screen(w) + vec2(if key == "in" { -sr - 2.0 } else { sr + 2.0 }, 0.0);
        let hit = Rect::from_center_size(c, Vec2::splat(sr * 3.4));
        let resp = ui.interact(hit, egui::Id::new(("mfx-sock", &n.id, key)), Sense::drag());
        let col = if resp.hovered() || resp.dragged() {
            t.accent_hover
        } else if filled {
            t.text
        } else {
            t.text_dim
        };
        p.circle_filled(c, sr, fade(t.field_bg, dim));
        p.circle_stroke(c, sr, Stroke::new(1.5, fade(col, dim)));
        if filled {
            p.circle_filled(c, sr * 0.5, col);
        }
        out.auto(format!("mfx.nodes.node.{}.{key}", n.id), &resp, if key == "in" { "Film in" } else { "Film out" });
        if resp.drag_started_by(PointerButton::Primary) {
            *drag = Drag::Link { from: n.id.clone(), out: key == "out" };
        }
    }
    if area.show.voice {
        let c = pos2(r.center().x, r.max.y + sr + 2.0);
        p.circle_stroke(c, sr * 0.8, Stroke::new(1.2, fade(t.text_faint, dim)));
    }
    if area.show.inputs {
        let c = pos2(r.center().x, r.min.y - sr - 2.0);
        p.circle_stroke(c, sr * 0.8, Stroke::new(1.2, fade(t.text_faint, dim)));
    }
}

#[allow(clippy::too_many_arguments)]
fn line_node(ui: &mut egui::Ui, p: &Painter, area: &mut NodeArea, n: &Node, r: Rect, lod: Lod, dim: bool, sel: bool, t: &MfxTheme, out: &mut Out) {
    let z = area.zoom;
    let colour = area.graph.colour_of(n);
    p.rect_filled(r, 4.0, fade(t.header_bg, dim));
    let size = text_size(z, 11.5);
    let head_h = (20.0 * z).max(size + 4.0).min(r.height());
    let head = Rect::from_min_size(r.min, vec2(r.width(), head_h));
    p.rect_filled(head, CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, fade(colour.map_or(t.hover, |c| c.fill), dim));
    let len = match (n.start_sec, n.end_sec) {
        (Some(a), Some(b)) if b > a => format!(" · {:.1} s", b - a),
        _ => String::new(),
    };
    let g = line(p, &format!("{}{len}", n.display_name()), size, fade(Color32::WHITE, dim), head.width() - 30.0);
    p.galley(pos2(head.min.x + 6.0, head.center().y - g.size().y / 2.0), g, Color32::WHITE);
    // ▶ play the line
    let play_r = Rect::from_center_size(pos2(head.max.x - 12.0, head.center().y), Vec2::splat(head_h.min(20.0)));
    let resp = ui.interact(play_r, egui::Id::new(("mfx-line-play", &n.id)), Sense::click());
    let c = play_r.center();
    let s = play_r.height() * 0.28;
    p.add(Shape::convex_polygon(
        vec![pos2(c.x - s * 0.7, c.y - s), pos2(c.x + s, c.y), pos2(c.x - s * 0.7, c.y + s)],
        if resp.hovered() { Color32::WHITE } else { Color32::from_white_alpha(190) },
        Stroke::NONE,
    ));
    out.auto(format!("mfx.nodes.node.{}.play", n.id), &resp, "Play the line");
    if resp.clicked() {
        play(out, area, &n.id, "shot");
    }
    if lod > Lod::Far {
        let g = line(p, &n.title, text_size(z, 11.5), fade(t.text, dim), r.width() - 12.0);
        p.galley(pos2(r.min.x + 6.0, head.max.y + 4.0), g, t.text);
        let g = line(p, &format!("{} {}", n.state.glyph(), n.state.word()), text_size(z, 11.0), fade(state_colour(n.state, t), dim), r.width() - 12.0);
        p.galley(pos2(r.min.x + 6.0, r.max.y - g.size().y - 4.0), g, t.text);
    }
    if sel {
        p.rect_stroke(r.expand(2.0), 5.0, Stroke::new(2.0, SELECTED), StrokeKind::Outside);
    }
}

fn cue_node(p: &Painter, area: &NodeArea, n: &Node, r: Rect, dim: bool, sel: bool, t: &MfxTheme) {
    let colour = area.graph.colour_of(n);
    p.rect_filled(r, 3.0, fade(colour.map_or(t.hover, |c| c.fill), dim));
    let beat = n.beat.as_deref().map(|b| area.graph.beat_title(b).1).unwrap_or_default();
    let text = [n.label.clone(), beat, n.title.clone()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
    let g = line(p, &format!("♪ {text}"), text_size(area.zoom, 11.5), fade(Color32::WHITE, dim), r.width() - 12.0);
    p.galley(pos2(r.min.x + 6.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
    if sel {
        p.rect_stroke(r.expand(2.0), 4.0, Stroke::new(2.0, SELECTED), StrokeKind::Outside);
    }
}

#[allow(clippy::too_many_arguments)]
fn input_node(p: &Painter, area: &NodeArea, n: &Node, r: Rect, lod: Lod, dim: bool, sel: bool, stills: &Stills, t: &MfxTheme) {
    let z = area.zoom;
    p.rect_filled(r, 4.0, fade(t.header_bg, dim));
    let size = text_size(z, 11.5);
    let head = Rect::from_min_size(r.min, vec2(r.width(), (22.0 * z).max(size + 4.0).min(r.height())));
    let what = if n.kind == Kind::Frame { "Look" } else { "Your asset" };
    let title = if n.title.is_empty() { n.label.clone() } else { n.title.clone() };
    let g = line(p, &format!("{title} · {what}"), size, fade(t.text, dim), r.width() - 12.0);
    p.galley(pos2(head.min.x + 6.0, head.center().y - g.size().y / 2.0), g, t.text);
    if lod > Lod::Far {
        let still = Rect::from_min_max(pos2(r.min.x, head.max.y), r.max);
        match n.still().and_then(|f| resolve(f, &area.graph.base, &area.studio, area.project.as_deref())).and_then(|path| stills.get(&path)) {
            Some(tex) => {
                p.image(tex.id(), still, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), if dim { Color32::from_white_alpha(90) } else { Color32::WHITE });
            }
            None => {
                p.rect_filled(still, 0.0, fade(Color32::from_gray(30), dim));
                p.rect_stroke(still.shrink(5.0 * z), 2.0, Stroke::new(1.0, fade(Color32::from_gray(48), dim)), StrokeKind::Inside);
            }
        }
    }
    let c = pos2(r.center().x, r.max.y + 6.0);
    p.circle_stroke(c, 4.0, Stroke::new(1.2, fade(t.text_faint, dim)));
    if sel {
        p.rect_stroke(r.expand(2.0), 5.0, Stroke::new(2.0, SELECTED), StrokeKind::Outside);
    }
}

fn menu_item(ui: &mut egui::Ui, out: &mut Out, id: &str, label: &str) -> bool {
    let r = ui.button(label);
    out.auto(format!("mfx.nodes.menu.{id}"), &r, label);
    let c = r.clicked();
    if c {
        ui.close();
    }
    c
}

fn node_menu(ui: &mut egui::Ui, area: &mut NodeArea, n: &Node, out: &mut Out) {
    if !area.selection.contains(&n.id) {
        area.select_only(&n.id);
    }
    let id = json!({"node": n.id});
    if n.kind == Kind::Shot {
        if n.state == NodeState::Cut {
            if menu_item(ui, out, "bringBack", "Bring it back") {
                out.cmd("nodes.bringBack", id.clone());
            }
        } else {
            if menu_item(ui, out, "ok", if n.state == NodeState::Ok { "Take back the OK" } else { "OK" }) {
                out.cmd(if n.state == NodeState::Ok { "nodes.bringBack" } else { "nodes.ok" }, id.clone());
            }
            if menu_item(ui, out, "fix", "Fix…") {
                area.fix_open = true;
            }
            if menu_item(ui, out, "cut", "Cut it") {
                out.cmd("nodes.cut", id.clone());
            }
        }
        ui.separator();
        if menu_item(ui, out, "play", "Play from here") {
            play(out, area, &n.id, "from");
        }
        if menu_item(ui, out, "addShot", "Add a shot after this one") {
            area.add_open = true;
        }
        if menu_item(ui, out, "duplicate", "Duplicate") {
            out.cmd("nodes.duplicate", id.clone());
        }
        if menu_item(ui, out, "earlier", "Move earlier in the film") {
            out.cmd("nodes.moveInFilm", json!({"node": n.id, "by": -1}));
        }
        if menu_item(ui, out, "later", "Move later in the film") {
            out.cmd("nodes.moveInFilm", json!({"node": n.id, "by": 1}));
        }
    } else if n.kind != Kind::Cue && n.kind != Kind::Frame && n.kind != Kind::Asset {
        if menu_item(ui, out, "ok", "OK") {
            out.cmd("nodes.ok", id.clone());
        }
        if menu_item(ui, out, "fix", "Fix…") {
            area.fix_open = true;
        }
        if menu_item(ui, out, "cut", "Cut it") {
            out.cmd("nodes.cut", id.clone());
        }
    }
    ui.separator();
    if menu_item(ui, out, "frame", "Frame it") {
        area.want_frame = Some(Frame::Node(n.id.clone()));
    }
    if menu_item(ui, out, "resetPlace", "Put it back in its own place") {
        out.cmd("nodes.move", json!({"moves": area.selection.iter().map(|s| json!({"node": s, "reset": true})).collect::<Vec<_>>()}));
    }
}

fn cut_menu(ui: &mut egui::Ui, area: &mut NodeArea, cut: &Cut, out: &mut Out) {
    if !area.selection.contains(&cut.id) {
        area.select_only(&cut.id);
    }
    if menu_item(ui, out, "playCut", "Play across the cut") {
        play(out, area, &cut.id, "cut");
    }
    for (kind, sec) in [("Cut", 0.0), ("Crossfade", 0.5), ("Dip to black", 0.5)] {
        if menu_item(ui, out, &format!("pick.{kind}"), &format!("Use {kind}")) {
            out.cmd("nodes.pickTransition", json!({"cut": cut.id, "kind": kind, "sec": sec}));
        }
    }
    if menu_item(ui, out, "resetPlace", "Put it back half way") {
        out.cmd("nodes.move", json!({"moves": [{"node": cut.id, "reset": true}]}));
    }
}

fn background_menu(ui: &mut egui::Ui, area: &mut NodeArea, out: &mut Out, at: Option<Pos2>) {
    if menu_item(ui, out, "frameAll", "Frame all") {
        area.want_frame = Some(Frame::All);
    }
    if menu_item(ui, out, "addShot", "Add a shot…") {
        area.add_open = true;
    }
    if menu_item(ui, out, "tidy", "Put every node back in film order") {
        out.cmd("nodes.tidy", json!({}));
    }
    let _ = at;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noodles_start_and_end_on_their_sockets_and_distances_work() {
        let a = pos2(0.0, 0.0);
        let b = pos2(300.0, 100.0);
        let pts = noodle(a, b);
        assert_eq!(pts.first(), Some(&a));
        assert!((*pts.last().unwrap() - b).length() < 0.01);
        assert!(distance_to(&pts, a) < 0.01);
        assert!(distance_to(&pts, pos2(150.0, 50.0)) < 5.0);
        assert!(distance_to(&pts, pos2(150.0, 400.0)) > 100.0);
        assert_eq!(distance_to(&[], a), f32::INFINITY);
        assert_eq!(clock(81.4), "1:21");
        assert_eq!(clock(f64::NAN), "0:00");
        assert_eq!(clock(-3.0), "0:00");
        assert_eq!(text_size(0.2, 12.0), 11.0);
        assert!(text_size(1.5, 12.0) <= 12.0 * 1.4);
    }
}
