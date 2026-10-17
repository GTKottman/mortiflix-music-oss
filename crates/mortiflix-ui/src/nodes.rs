//! The node area (doc 35 + owner answer 3): the whole film as a noodle graph you can really dig into.
//! Shot nodes chained by noodles with a transition node on every cut, voice lines under the shots
//! they're said over, the music under that, and (Show) the look and your assets as input nodes.
//!
//! **Free placement like Blender** (owner, 2026-10-16: "Drag them anywhere like Blender"): nodes
//! stay where you drop them (saved with `node.move`), the noodles follow, and the film order comes
//! from the chain, never from where nodes sit. Reorder = rewire the chain: pull a noodle from a
//! shot's socket to another shot, or drop a shot onto a noodle (`node.reorder`).
//!
//! The panel: header (counts, Maximise via FilmCraft's Maximize Frame), toolbar (Bin, Find, Show,
//! Highlight, + Add, zoom, Frame selected / all, Follow the playhead), the canvas, the inspector, the
//! overview strip. Keys are panel-scoped `nodes.*` commands (FilmCraft shortcut tables, scope
//! "Mortiflix Nodes"), so ←/→ walk the film here while the preview keeps its own ←/→. Every action
//! is a `nodes.*` command: the menus, keys, the control channel and the AI do the same.

pub mod area;
pub mod canvas;
pub mod model;
pub mod panel;
pub mod stills;

use egui::{Rect, pos2, vec2};
use serde_json::{Value, json};

pub use area::NodeArea;
use area::{Confirm, Frame, Highlight, PlayRequest, Wheel, ZOOM_STEP};
use model::{Kind, NodeState};

use crate::{MfxCtx, Mortiflix};

const HEADER_H: f32 = 30.0;
const TOOLBAR_H: f32 = 36.0;
const STRIP_H: f32 = 30.0;
const INSPECTOR_W: f32 = 340.0;
const BIN_W: f32 = 210.0;

/// The node area panel (PanelKind::MfxNodes), along the bottom on stages 1-4.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let now = ui.input(|i| i.time);
    let full = ui.max_rect();
    let t = ctx.theme;
    let connected = ctx.service.status().is_connected();
    {
        let project = ctx.state.project_id.clone();
        let area = &mut ctx.state.nodes;
        if let Some(raw) = area.tick(ctx.service, project.as_ref(), now) {
            ctx.state.graph = raw;
        }
    }
    // what the AI is working on: those nodes are drawn working (spinner, sweep, words)
    if ctx.state.nodes.work != ctx.state.work {
        ctx.state.nodes.work = ctx.state.work.clone();
    }
    // the loose preview's playhead: blue outline, and Follow the playhead keeps that shot in view
    let before = ctx.state.nodes.playhead_shot.clone();
    sync_preview(ctx.state);
    let area = &mut ctx.state.nodes;
    if area.follow_playhead
        && area.playing
        && area.playhead_shot != before
        && let Some(s) = area.playhead_shot.clone()
    {
        area.want_frame = Some(Frame::Reveal(s));
    }
    // the chat (or anyone) selected a node: select it here and bring it into view
    if ctx.state.selected_node != ctx.state.nodes.synced_selection {
        let sel = ctx.state.selected_node.clone();
        let area = &mut ctx.state.nodes;
        if let Some(id) = sel.as_ref().filter(|id| area.graph.get(id).is_some() || id.starts_with("cut:")) {
            area.select_only(id);
            area.want_frame = Some(Frame::Reveal(id.clone()));
        }
        area.synced_selection = sel;
    }
    if connected && ctx.state.project_id.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(1000));
    }

    let mut out = canvas::Out::default();
    let screen = ui.ctx().content_rect();
    let maximised = full.height() > screen.height() * 0.72;
    let header = Rect::from_min_size(full.min, vec2(full.width(), HEADER_H));
    panel::header(ui, &mut ctx.state.nodes, header, maximised, connected, &t, &mut out);

    if ctx.state.project_id.is_none() || (!connected && !ctx.state.nodes.loaded) {
        let body = Rect::from_min_max(pos2(full.min.x, header.max.y + 8.0), full.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body).layout(egui::Layout::top_down(egui::Align::Min)));
        flush(ctx, out);
        if !crate::service_notice(&mut child, ctx) {
            child.label(egui::RichText::new("No project open yet. The film's shots appear here as nodes.").color(t.text_dim));
            let r = child.button("New project…");
            ctx.auto("mfx.nodes.newProject", &r, "New project…");
            if r.clicked() {
                ctx.command("mortiflix.newProject", json!({}));
            }
        }
        return;
    }

    let area = &mut ctx.state.nodes;
    let toolbar = Rect::from_min_size(pos2(full.min.x, header.max.y), vec2(full.width(), TOOLBAR_H));
    panel::toolbar(ui, area, toolbar, &t, &mut out);
    let body = Rect::from_min_max(pos2(full.min.x, toolbar.max.y), full.max);
    let bin_w = if area.show.bin { BIN_W.min(body.width() * 0.25) } else { 0.0 };
    let insp_w = if body.width() - bin_w > INSPECTOR_W + 320.0 { INSPECTOR_W } else { 0.0 };
    let canvas_rect = Rect::from_min_max(pos2(body.min.x + bin_w, body.min.y), pos2(body.max.x - insp_w, body.max.y - STRIP_H));
    let strip_rect = Rect::from_min_max(pos2(canvas_rect.min.x, canvas_rect.max.y), pos2(canvas_rect.max.x, body.max.y));
    if !area.loaded {
        ui.painter().text(canvas_rect.center(), egui::Align2::CENTER_CENTER, "Loading the nodes…", egui::FontId::proportional(13.0), t.text_dim);
    }
    canvas::show(ui, area, &t, canvas_rect, &mut out);
    panel::strip(ui, area, strip_rect, &t, &mut out);
    if bin_w > 0.0 {
        panel::bin(ui, area, Rect::from_min_max(body.min, pos2(body.min.x + bin_w, body.max.y)), &t, &mut out);
    }
    if insp_w > 0.0 {
        panel::inspector(ui, area, Rect::from_min_max(pos2(body.max.x - insp_w, body.min.y), body.max), &t, &mut out);
    }
    panel::dialogs(ui, area, &t, &mut out);

    // what this area selected is what everyone talks about (the chat's "talking about" bar)
    let primary = area.primary().map(str::to_string);
    if primary != area.synced_selection {
        area.synced_selection.clone_from(&primary);
        ctx.state.selected_node = primary;
    }
    flush(ctx, out);
}

/// Copy the loose preview's playhead in (part 04 keeps it in `state.preview`): the shot under the
/// playhead, whether it plays (blue outline) and Follow the playhead.
fn sync_preview(state: &mut crate::MfxState) {
    state.nodes.playhead_shot.clone_from(&state.preview.shot_at_playhead);
    state.nodes.playing = state.preview.player.playing;
    state.nodes.follow_playhead = state.preview.follow_playhead;
}

/// An asset id as the service names them (`a01`, `a12`, …).
fn is_asset_id(id: &str) -> bool {
    id.len() <= 12 && id.strip_prefix('a').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

fn flush(ctx: &mut MfxCtx, out: canvas::Out) {
    for (id, r, label) in out.auto {
        ctx.auto(&id, &r, &label);
    }
    for (id, p) in out.cmds {
        ctx.command(&id, p);
    }
}

/// Stage 3 Shots' main panel (PanelKind::MfxStage on stage 3): the shots themselves are in the node
/// area; this slot lists what needs you, each a link that selects and frames the shot.
pub fn show_stage(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    if crate::service_notice(ui, ctx) && !ctx.state.nodes.loaded {
        return;
    }
    let c = ctx.state.nodes.graph.counts();
    ui.label(egui::RichText::new("Shots").strong().color(t.text));
    ui.label(egui::RichText::new(c.line()).color(t.text_dim));
    ui.add_space(6.0);
    ui.label(egui::RichText::new("Check each shot in the node area: OK, Fix or Cut it.").color(t.text_faint));
    ui.add_space(6.0);
    let need: Vec<(String, String)> = ctx
        .state
        .nodes
        .graph
        .film
        .iter()
        .filter_map(|id| ctx.state.nodes.graph.get(id))
        .filter(|n| matches!(n.state, NodeState::NeedsYou | NodeState::Ready | NodeState::OutOfDate))
        .map(|n| (n.id.clone(), format!("{} {} · {} {}", n.display_name(), n.title, n.state.glyph(), n.state.word())))
        .collect();
    egui::ScrollArea::vertical().id_salt("mfx-shots-stage").show(ui, |ui| {
        for (id, label) in need {
            let r = ui.link(label.as_str());
            ctx.auto(&format!("mfx.shots.{id}"), &r, &label);
            if r.clicked() {
                ctx.command("nodes.select", json!({"node": id, "frame": true}));
            }
        }
    });
}

/// The `nodes.*` commands (keys, menus, the control channel, the AI). `what` is the part after
/// `nodes.`. Every parameter is hostile input.
pub fn command(m: &mut Mortiflix, what: &str, params: &Value) -> Result<Value, String> {
    let Mortiflix { state, service } = m;
    let area = &mut state.nodes;
    let node_param = |p: &Value| -> Option<String> { p.get("node").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 200).map(str::to_string) };
    let force = params.get("force").and_then(Value::as_bool).unwrap_or(false);
    // the node(s) a command acts on: `node`, `nodes`, else the selection
    let targets = |area: &NodeArea, p: &Value| -> Vec<String> {
        if let Some(n) = node_param(p) {
            return vec![n];
        }
        if let Some(a) = p.get("nodes").and_then(Value::as_array) {
            return a.iter().filter_map(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 200).take(2000).map(str::to_string).collect();
        }
        area.selection.clone()
    };
    let r = match what {
        "state" => {
            json!({"zoom": area.zoom, "pan": area.pan, "selection": area.selection, "film": area.graph.film, "positions": area.positions, "loaded": area.loaded, "counts": format!("{:?}", area.graph.counts())})
        }
        "refresh" => {
            area.fetch_soon = true;
            area.last_fetch = f64::NEG_INFINITY;
            json!({"refreshing": true})
        }
        "select" => {
            let id = node_param(params).ok_or("nodes.select needs a `node`")?;
            if area.graph.get(&id).is_none() && !area.graph.cuts.iter().any(|c| c.id == id) {
                return Err(format!("no node `{id}`"));
            }
            if params.get("add").and_then(Value::as_bool).unwrap_or(false) {
                if !area.selection.contains(&id) {
                    area.selection.push(id.clone());
                }
            } else {
                area.select_only(&id);
            }
            area.want_frame =
                Some(if params.get("frame").and_then(Value::as_bool).unwrap_or(false) { Frame::Node(id.clone()) } else { Frame::Reveal(id.clone()) });
            json!({"selection": area.selection})
        }
        "selectAll" => {
            area.selection = area.graph.film.iter().filter(|id| area.graph.get(id).is_some_and(|n| n.state != NodeState::Cut)).cloned().collect();
            json!({"selection": area.selection})
        }
        "escape" => {
            if !area.find.is_empty() {
                area.find.clear();
            } else if area.fix_open || area.add_open || area.confirm.is_some() {
                area.fix_open = false;
                area.add_open = false;
                area.confirm = None;
            } else {
                area.selection.clear();
            }
            json!({"selection": area.selection})
        }
        "prev" | "next" => json!({"selected": area.step(if what == "prev" { -1 } else { 1 })}),
        "up" | "down" => json!({"selected": area.step_row(if what == "up" { -1 } else { 1 })}),
        "frameAll" => {
            area.want_frame = Some(Frame::All);
            json!({})
        }
        "frameSelected" => {
            area.want_frame = Some(if area.selection.is_empty() { Frame::All } else { Frame::Selected });
            json!({})
        }
        w if w.starts_with("frameBeat") => {
            let n =
                params.get("n").and_then(Value::as_u64).or_else(|| w.trim_start_matches("frameBeat").parse().ok()).ok_or("nodes.frameBeat needs `n` (1-9)")?;
            let beat = usize::try_from(n)
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|i| area.graph.beats.get(i))
                .cloned()
                .ok_or_else(|| format!("there is no beat {n}"))?;
            area.want_frame = Some(Frame::Beat(beat.clone()));
            json!({"beat": beat})
        }
        "zoomIn" | "zoomOut" | "zoomTo" => {
            let c = area.canvas_rect().center();
            let target = match what {
                "zoomIn" => area.zoom * ZOOM_STEP,
                "zoomOut" => area.zoom / ZOOM_STEP,
                _ => {
                    #[allow(clippy::cast_possible_truncation)]
                    let z = params.get("zoom").and_then(Value::as_f64).filter(|z| z.is_finite()).ok_or("nodes.zoomTo needs `zoom` (0.2-1.5)")? as f32;
                    z
                }
            };
            area.zoom_at(c, target / area.zoom.max(0.01));
            json!({"zoom": area.zoom})
        }
        "find" => {
            if let Some(text) = params.get("text").and_then(Value::as_str) {
                area.find = text.chars().take(200).collect();
                area.find_index = 0;
                let first = area.find_next();
                json!({"found": area.found(), "selected": first})
            } else {
                area.focus_find = true;
                json!({"focus": true})
            }
        }
        "show" => {
            for (k, slot) in [("voice", &mut area.show.voice), ("music", &mut area.show.music), ("inputs", &mut area.show.inputs), ("bin", &mut area.show.bin)]
            {
                if let Some(b) = params.get(k).and_then(Value::as_bool) {
                    *slot = b;
                }
            }
            json!(area.show)
        }
        "highlight" => {
            let key = params.get("what").and_then(Value::as_str).unwrap_or("all");
            area.highlight = Highlight::ALL
                .into_iter()
                .find(|h| h.key().eq_ignore_ascii_case(key))
                .ok_or_else(|| format!("highlight: one of all, notReviewed, needsYou, making, changed, notes (not `{key}`)"))?;
            json!({"highlight": area.highlight})
        }
        "wheel" => {
            area.wheel = match params.get("mode").and_then(Value::as_str) {
                Some("scroll") => Wheel::Scroll,
                Some("zoom") => Wheel::Zoom,
                _ => return Err("nodes.wheel needs `mode`: zoom or scroll".into()),
            };
            json!({"wheel": area.wheel})
        }
        "move" => {
            let list = params.get("moves").and_then(Value::as_array).ok_or("nodes.move needs `moves`: [{node, x, y} or {node, reset: true}]")?;
            let mut moves = Vec::new();
            for m in list.iter().take(5000) {
                let Some(id) = node_param(m) else { continue };
                if area.graph.get(&id).is_none() && !area.graph.cuts.iter().any(|c| c.id == id) {
                    continue;
                }
                if m.get("reset").and_then(Value::as_bool).unwrap_or(false) {
                    moves.push((id, None));
                    continue;
                }
                let (Some(x), Some(y)) = (m.get("x").and_then(Value::as_f64), m.get("y").and_then(Value::as_f64)) else { continue };
                if !(x.is_finite() && y.is_finite() && x.abs() < 1.0e6 && y.abs() < 1.0e6) {
                    continue;
                }
                #[allow(clippy::cast_possible_truncation)]
                moves.push((id, Some([x as f32, y as f32])));
            }
            if moves.is_empty() {
                return Err("nothing to move (unknown nodes or bad coordinates)".into());
            }
            area.move_nodes(service, &moves, true);
            json!({"moved": moves.len()})
        }
        "tidy" => {
            let mut names: Vec<String> =
                area.positions.keys().cloned().chain(area.graph.nodes.iter().filter(|n| n.pos.is_some()).map(|n| n.id.clone())).collect();
            names.sort();
            names.dedup();
            let ids: Vec<(String, Option<[f32; 2]>)> = names.into_iter().map(|i| (i, None)).collect();
            area.move_nodes(service, &ids, true);
            area.want_frame = Some(Frame::All);
            json!({"reset": ids.len()})
        }
        "reorder" | "moveInFilm" => {
            let id = node_param(params).ok_or("needs a `node` (a shot)")?;
            if area.graph.film_index(&id).is_none() {
                return Err(format!("`{id}` is not a shot in the film"));
            }
            let after: Option<String> = if what == "moveInFilm" {
                let by = params.get("by").and_then(Value::as_i64).unwrap_or(0).clamp(-1000, 1000);
                let mut film = area.graph.film.clone();
                let i = film.iter().position(|s| *s == id).unwrap_or(0);
                film.remove(i);
                let j = usize::try_from((i64::try_from(i).unwrap_or(0) + by).max(0)).unwrap_or(0).min(film.len());
                j.checked_sub(1).and_then(|k| film.get(k)).cloned()
            } else {
                match params.get("after") {
                    Some(Value::String(s)) if area.graph.film_index(s).is_some() => Some(s.clone()),
                    Some(Value::Null) | None => None,
                    Some(other) => return Err(format!("`after` must be a shot id or null, not {other}")),
                }
            };
            // a move into another beat asks first: its words come with it
            let from_beat = area.graph.get(&id).and_then(|n| n.beat.clone());
            let to_beat = after
                .as_deref()
                .and_then(|a| area.graph.get(a))
                .and_then(|n| n.beat.clone())
                .or_else(|| area.graph.film.first().and_then(|f| area.graph.get(f)).and_then(|n| n.beat.clone()));
            if !force && from_beat.is_some() && to_beat.is_some() && from_beat != to_beat {
                let name = area.graph.get(&id).map(model::Node::display_name).unwrap_or_default();
                let (n, b) = to_beat.as_deref().map(|b| area.graph.beat_title(b)).unwrap_or_default();
                area.confirm = Some(Confirm {
                    title: format!("Move shot {} into {n} {b}?", name.trim_start_matches('#')),
                    detail: "Its words come with it. The cuts around the old and new place get new transitions to pick.".into(),
                    yes: "Move it".into(),
                    what: "nodes.reorder".into(),
                    params: json!({"node": id, "after": after}),
                });
                return Ok(json!({"asked": true}));
            }
            let moved = area.reorder(service, &id, after.as_deref(), true);
            json!({"moved": moved, "film": area.graph.film})
        }
        "ok" | "cut" | "bringBack" | "delete" | "enter" => {
            let ids = targets(area, params);
            if ids.is_empty() {
                return Err("select a node first".into());
            }
            let action = match what {
                "ok" | "enter" => "ok",
                "bringBack" => "clear",
                _ => "cut",
            };
            // the ripple question: cutting something you OK'd asks first
            let okd: Vec<String> = ids.iter().filter(|i| area.graph.get(i).is_some_and(|n| n.state == NodeState::Ok)).cloned().collect();
            if action == "cut" && !force && !okd.is_empty() {
                let names: Vec<String> = okd.iter().filter_map(|i| area.graph.get(i)).map(model::Node::display_name).collect();
                area.confirm = Some(Confirm {
                    title: format!("Cut {}?", if ids.len() == 1 { names.first().cloned().unwrap_or_default() } else { format!("{} pieces", ids.len()) }),
                    detail: format!("You OK'd {}. Nothing is deleted: it stays in the Bin.", names.join(", ")),
                    yes: "Cut it".into(),
                    what: "nodes.cut".into(),
                    params: json!({"nodes": ids}),
                });
                return Ok(json!({"asked": true}));
            }
            // OK skips shots not made yet (several at once); one alone says why
            let unmade = |id: &String| area.graph.get(id).is_some_and(|n| n.kind != Kind::Beat && n.versions.is_empty());
            let ids: Vec<String> = if action == "ok" && ids.len() > 1 { ids.iter().filter(|i| !unmade(i)).cloned().collect() } else { ids };
            if ids.is_empty() {
                return Err("None of them is made yet: OK a shot once it exists".into());
            }
            let mut done = Vec::new();
            for id in &ids {
                if let Some(cut) = area.graph.cuts.iter().find(|c| &c.id == id).cloned() {
                    // OK on a cut = keep its transition; Cut it on a cut = a plain cut
                    if action == "cut" {
                        area.pick_transition(service, &cut.id, "Cut", 0.0)?;
                    } else if action == "ok" {
                        let (k, s, _) = area.cut_label(&cut);
                        let kind = if k.starts_with('!') { "Crossfade".to_string() } else { k };
                        area.pick_transition(service, &cut.id, &kind, s.unwrap_or(0.5))?;
                    }
                    done.push(id.clone());
                    continue;
                }
                if area.graph.get(id).is_some_and(|n| matches!(n.kind, Kind::Beat)) {
                    // a beat: act on its shots
                    let shots: Vec<String> = area
                        .graph
                        .beat_shots(id)
                        .iter()
                        .filter(|s| s.state != NodeState::Cut && (action != "ok" || !s.versions.is_empty()))
                        .map(|s| s.id.clone())
                        .collect();
                    for s in shots {
                        area.decide(service, &s, action, "", true)?;
                        done.push(s);
                    }
                    continue;
                }
                area.decide(service, id, action, "", true)?;
                done.push(id.clone());
            }
            // OK goes on to the next shot (Enter: OK, then on)
            if action == "ok" && ids.len() == 1 {
                let walk = area.walk();
                let next =
                    ids.first().and_then(|i| area.graph.film_index(i)).and_then(|i| area.graph.film.get(i + 1..)).and_then(|rest| {
                        rest.iter().find(|s| area.graph.get(s).is_some_and(|n| n.state != NodeState::Cut && n.state != NodeState::Ok)).cloned()
                    });
                if let Some(n) = next.filter(|n| walk.contains(n)) {
                    area.select_only(&n);
                    area.want_frame = Some(Frame::Reveal(n));
                }
            }
            if action == "cut" {
                area.selection.retain(|s| !done.contains(s));
            }
            json!({"done": done, "action": action})
        }
        "fixKey" => {
            if area.selection.is_empty() {
                return Err("select a node first".into());
            }
            area.fix_open = true;
            json!({"fixOpen": true})
        }
        "fix" => {
            let text: String = params.get("text").and_then(Value::as_str).unwrap_or("").trim().chars().take(4000).collect();
            if text.is_empty() {
                // no words yet: open the Fix box
                area.fix_open = true;
                return Ok(json!({"fixOpen": true}));
            }
            let ids = targets(area, params);
            if ids.is_empty() {
                return Err("select a node first".into());
            }
            let okd = ids.iter().any(|i| area.graph.get(i).is_some_and(|n| n.state == NodeState::Ok));
            if okd && !force {
                area.confirm = Some(Confirm {
                    title: "Fix something you OK'd?".into(),
                    detail: "Only what you fix is remade; the OK'd version stays until the new one is in.".into(),
                    yes: "Send it".into(),
                    what: "nodes.fix".into(),
                    params: json!({"nodes": ids, "text": text}),
                });
                return Ok(json!({"asked": true}));
            }
            let mut done = Vec::new();
            for id in &ids {
                let shots: Vec<String> = if area.graph.get(id).is_some_and(|n| n.kind == Kind::Beat) {
                    area.graph.beat_shots(id).iter().map(|s| s.id.clone()).collect()
                } else {
                    vec![id.clone()]
                };
                for s in shots {
                    area.decide(service, &s, "fix", &text, true)?;
                    done.push(s);
                }
            }
            area.fix_open = false;
            area.fix_text.clear();
            json!({"fixing": done})
        }
        "pickTransition" => {
            let cut = params.get("cut").and_then(Value::as_str).ok_or("nodes.pickTransition needs `cut`")?.to_string();
            let kind = params.get("kind").and_then(Value::as_str).ok_or("nodes.pickTransition needs `kind`")?.to_string();
            #[allow(clippy::cast_possible_truncation)]
            let sec = params.get("sec").and_then(Value::as_f64).unwrap_or(0.5) as f32;
            area.pick_transition(service, &cut, &kind, sec)?;
            json!({"cut": cut, "kind": kind, "sec": sec})
        }
        "duplicate" => {
            let id = node_param(params).or_else(|| area.primary().map(str::to_string)).ok_or("select a shot first")?;
            if area.graph.get(&id).is_none_or(|n| n.kind != Kind::Shot) {
                return Err(format!("`{id}` is not a shot"));
            }
            area.ask(service, "node.duplicate", json!({"node": id}));
            json!({"asked": "node.duplicate"})
        }
        "addShot" => {
            area.add_open = true;
            json!({"addOpen": true})
        }
        "add" => {
            let text: String = params.get("text").and_then(Value::as_str).unwrap_or("").trim().chars().take(4000).collect();
            if text.is_empty() {
                area.add_open = true;
                return Ok(json!({"addOpen": true}));
            }
            let after = node_param(&json!({"node": params.get("after")}))
                .or_else(|| area.primary().filter(|s| area.graph.film_index(s).is_some()).map(str::to_string))
                .or_else(|| area.graph.film.last().cloned())
                .ok_or("there is no shot to put it after yet")?;
            area.ask(service, "node.insert", json!({"after": after, "text": text}));
            json!({"asked": "node.insert", "after": after})
        }
        "useVersion" => {
            let id = node_param(params).ok_or("nodes.useVersion needs `node`")?;
            let v = params.get("v").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).ok_or("nodes.useVersion needs `v`")?;
            let n = area.graph.get(&id).ok_or_else(|| format!("no node `{id}`"))?;
            if !n.versions.iter().any(|x| x.v == v) {
                return Err(format!("{} has no v{v}", n.display_name()));
            }
            if n.state == NodeState::Ok {
                return Err("the OK'd version is locked: take back the OK first".into());
            }
            area.ask(service, "node.pick", json!({"node": id, "v": v}));
            if let Some(n) = area.graph.get_mut(&id) {
                n.current = Some(v);
            }
            area.peek_version = None;
            json!({"node": id, "v": v})
        }
        "note" => {
            let id = node_param(params).ok_or("nodes.note needs `node`")?;
            let text: String = params.get("text").and_then(Value::as_str).unwrap_or("").trim().chars().take(2000).collect();
            if text.is_empty() {
                return Err("a note needs words".into());
            }
            // a note doesn't start a remake: a grey line in the chat history, tied to the node
            let label = area.graph.get(&id).map(|n| n.label.clone()).unwrap_or_else(|| id.clone());
            area.ask(service, "chat.action", json!({"text": format!("Note on {label}: {text}"), "refs": [id]}));
            if let Some(n) = area.graph.get_mut(&id) {
                n.notes.push(text);
            }
            json!({"node": id})
        }
        "use" => {
            let id = node_param(params).ok_or("nodes.use needs `node` (a shot)")?;
            let input = params.get("input").and_then(Value::as_str).ok_or("nodes.use needs `input` (a look or asset)")?.to_string();
            // an asset dragged from the Assets tab need not be on the canvas (Show ▸ your assets off)
            let kind = area.graph.get(&input).map(|n| n.kind).or_else(|| is_asset_id(&input).then_some(Kind::Asset));
            if area.graph.get(&id).is_none_or(|n| n.kind != Kind::Shot) || !matches!(kind, Some(Kind::Frame | Kind::Asset)) {
                return Err("drop a look or one of your assets onto a shot".into());
            }
            if kind == Some(Kind::Asset) {
                area.ask(service, "assets.use", json!({"asset": input, "node": id}));
            } else {
                let look = area.graph.get(&input).map(|n| n.label.clone()).unwrap_or_else(|| input.clone());
                area.ask(service, "node.edit", json!({"node": id, "action": "fix", "text": format!("Use the look {look}")}));
            }
            if let Some(n) = area.graph.get_mut(&id) {
                if kind == Some(Kind::Frame) {
                    n.look = Some(input.clone());
                } else if !n.uses.contains(&input) {
                    n.uses.push(input.clone());
                }
            }
            json!({"node": id, "input": input})
        }
        "play" => {
            let id = node_param(params).or_else(|| area.primary().map(str::to_string)).ok_or("select a node first")?;
            let scope = params.get("scope").and_then(Value::as_str).filter(|s| ["shot", "from", "cut", "beat", "all"].contains(s)).unwrap_or("shot");
            area.play = Some(PlayRequest { node: id.clone(), scope: scope.to_string() });
            if !area.selection.contains(&id) {
                area.select_only(&id);
            }
            json!({"play": id, "scope": scope})
        }
        "undo" | "redo" => json!({"op": area.undo_redo(service, what == "undo")}),
        _ => return Err(format!("unknown command `nodes.{what}`")),
    };
    Ok(r)
}

/// Every `nodes.*` command id with its label and default key (panel scope "Mortiflix Nodes").
/// The host registers them (FilmCraft's command list and shortcut tables).
pub const COMMANDS: &[(&str, &str, Option<&str>)] = &[
    ("nodes.prev", "Previous Node", Some("Left")),
    ("nodes.next", "Next Node", Some("Right")),
    ("nodes.up", "Node Above", Some("Up")),
    ("nodes.down", "Node Below", Some("Down")),
    ("nodes.enter", "OK and On to the Next Shot", Some("Enter")),
    ("nodes.fixKey", "Fix the Selected Node", Some("Shift+Enter")),
    ("nodes.delete", "Cut It", Some("Delete")),
    ("nodes.frameSelected", "Frame Selected Nodes", Some("F")),
    ("nodes.frameAll", "Frame All Nodes", Some("\\")),
    ("nodes.zoomIn", "Zoom In on the Nodes", Some("=")),
    ("nodes.zoomOut", "Zoom Out of the Nodes", Some("-")),
    ("nodes.duplicate", "Duplicate Shot", Some("Cmd+D")),
    ("nodes.find", "Find a Shot", Some("Cmd+F")),
    ("nodes.selectAll", "Select All Shots", Some("Cmd+A")),
    ("nodes.escape", "Clear Search or Deselect", Some("Escape")),
    ("nodes.undo", "Undo in the Node Area", Some("Cmd+Z")),
    ("nodes.redo", "Redo in the Node Area", Some("Cmd+Shift+Z")),
    ("nodes.addShot", "Add a Shot", None),
    ("nodes.frameBeat1", "Frame Beat 1", Some("1")),
    ("nodes.frameBeat2", "Frame Beat 2", Some("2")),
    ("nodes.frameBeat3", "Frame Beat 3", Some("3")),
    ("nodes.frameBeat4", "Frame Beat 4", Some("4")),
    ("nodes.frameBeat5", "Frame Beat 5", Some("5")),
    ("nodes.frameBeat6", "Frame Beat 6", Some("6")),
    ("nodes.frameBeat7", "Frame Beat 7", Some("7")),
    ("nodes.frameBeat8", "Frame Beat 8", Some("8")),
    ("nodes.frameBeat9", "Frame Beat 9", Some("9")),
    ("nodes.select", "Select a Node", None),
    ("nodes.ok", "OK", None),
    ("nodes.fix", "Fix", None),
    ("nodes.cut", "Cut It", None),
    ("nodes.bringBack", "Bring It Back", None),
    ("nodes.move", "Move Nodes", None),
    ("nodes.reorder", "Move a Shot in the Film", None),
    ("nodes.moveInFilm", "Move a Shot Earlier or Later", None),
    ("nodes.pickTransition", "Pick a Transition", None),
    ("nodes.add", "New Shot", None),
    ("nodes.useVersion", "Use This Version", None),
    ("nodes.note", "Add a Note", None),
    ("nodes.use", "Use in Shot", None),
    ("nodes.play", "Play the Node", None),
    ("nodes.show", "Show in the Node Area", None),
    ("nodes.highlight", "Highlight Nodes", None),
    ("nodes.wheel", "Mouse Wheel in the Node Area", None),
    ("nodes.zoomTo", "Zoom the Nodes To", None),
    ("nodes.tidy", "Put Nodes Back in Film Order", None),
    ("nodes.refresh", "Reload the Nodes", None),
    ("nodes.state", "Node Area State", None),
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::model::tests::sample;

    fn loaded() -> Mortiflix {
        let mut m = Mortiflix::default();
        m.state.nodes.load(&sample());
        m.state.nodes.canvas = [0.0, 0.0, 1200.0, 500.0];
        m
    }

    #[test]
    fn ok_is_only_for_made_shots() {
        let mut m = loaded();
        // OK on several skips the plans; on a plan alone it says why
        let r = command(&mut m, "ok", &json!({"nodes": ["s02", "s03"]})).unwrap();
        assert_eq!(r["done"], json!(["s02"]));
        assert!(command(&mut m, "ok", &json!({"node": "s03"})).is_err());
        command(&mut m, "bringBack", &json!({"node": "s02"})).unwrap();
    }

    #[test]
    fn commands_walk_decide_and_ask_first() {
        let mut m = loaded();
        assert_eq!(command(&mut m, "next", &json!({})).unwrap()["selected"], "s01");
        assert_eq!(command(&mut m, "next", &json!({})).unwrap()["selected"], "t01");
        command(&mut m, "select", &json!({"node": "s02"})).unwrap();
        // Enter: OK, then on to the next shot that isn't OK yet
        let r = command(&mut m, "enter", &json!({})).unwrap();
        assert_eq!(r["done"], json!(["s02"]));
        assert_eq!(m.state.nodes.primary(), Some("s03"));
        // cutting an OK'd shot asks first
        let r = command(&mut m, "cut", &json!({"node": "s01"})).unwrap();
        assert_eq!(r["asked"], true);
        assert!(m.state.nodes.confirm.is_some());
        command(&mut m, "cut", &json!({"node": "s01", "force": true})).unwrap();
        assert_eq!(m.state.nodes.graph.get("s01").map(|n| n.state), Some(NodeState::Cut));
        command(&mut m, "bringBack", &json!({"node": "s01"})).unwrap();
        // a move into another beat asks first
        let r = command(&mut m, "reorder", &json!({"node": "s04", "after": "s01"})).unwrap();
        assert_eq!(r["asked"], true);
        let r = command(&mut m, "reorder", &json!({"node": "s04", "after": "s01", "force": true})).unwrap();
        assert_eq!(r["film"], json!(["s01", "s04", "s02", "s03", "x"]));
        command(&mut m, "undo", &json!({})).unwrap();
        assert_eq!(m.state.nodes.graph.film, ["s01", "s02", "s03", "s04", "x"]);
        let r = command(&mut m, "moveInFilm", &json!({"node": "s02", "by": 1, "force": true})).unwrap();
        assert_eq!(r["film"], json!(["s01", "s03", "s02", "s04", "x"]));
        assert!(command(&mut m, "fix", &json!({"node": "s03"})).unwrap()["fixOpen"] == true);
        command(&mut m, "fix", &json!({"node": "s03", "text": "warmer light"})).unwrap();
        assert_eq!(m.state.nodes.graph.get("s03").map(|n| n.state), Some(NodeState::Making));
        // the edits waited for the studio (not running in tests)
        assert!(!m.state.nodes.outbox.is_empty());
    }

    #[test]
    fn hostile_parameters_are_refused() {
        let mut m = loaded();
        for (what, p) in [
            ("select", json!({})),
            ("select", json!({"node": "ghost"})),
            ("select", json!({"node": 5})),
            ("move", json!({})),
            ("move", json!({"moves": [{"node": "s01", "x": "a", "y": 1}, {"node": "ghost", "x": 1, "y": 1}, {"node": "s01", "x": 1e300, "y": 0}]})),
            ("reorder", json!({"node": "b01"})),
            ("reorder", json!({"node": "s01", "after": 7})),
            ("highlight", json!({"what": "everything"})),
            ("wheel", json!({})),
            ("zoomTo", json!({"zoom": "big"})),
            ("frameBeat", json!({"n": 0})),
            ("frameBeat", json!({"n": 99})),
            ("pickTransition", json!({"cut": "nope", "kind": "Wipe"})),
            ("pickTransition", json!({"cut": "t01", "kind": "Wipe", "sec": -4})),
            ("useVersion", json!({"node": "s02", "v": 9})),
            ("note", json!({"node": "s02", "text": "  "})),
            ("use", json!({"node": "s02", "input": "s01"})),
            ("duplicate", json!({"node": "b01"})),
            ("ok", json!({"nodes": []})),
            ("bogus", json!({})),
        ] {
            assert!(command(&mut m, what, &p).is_err(), "nodes.{what} {p}");
        }
        assert_eq!(command(&mut m, "zoomTo", &json!({"zoom": 99})).unwrap()["zoom"], json!(area::ZOOM_MAX));
        assert!(command(&mut m, "frameBeat2", &json!({})).is_ok());
        assert!(command(&mut m, "use", &json!({"node": "s02", "input": "sf01"})).is_ok());
        assert_eq!(m.state.nodes.graph.get("s02").and_then(|n| n.look.clone()).as_deref(), Some("sf01"));
        let r = command(&mut m, "find", &json!({"text": "handshake"})).unwrap();
        assert_eq!(r["selected"], "s04");
        // the state is serde: the control channel reads it
        let v = serde_json::to_value(&m.state).unwrap();
        assert!(v["nodes"]["selection"].is_array());
    }
}
