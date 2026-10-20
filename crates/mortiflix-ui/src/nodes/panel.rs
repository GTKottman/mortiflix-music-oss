//! The parts of the node area around the canvas (doc 35): the header (counts, Maximise), the
//! toolbar (Bin, Find, Show, Highlight, + Add, zoom, framing, Follow the playhead), the Bin column,
//! the inspector on the right, the overview strip (minimap) under the canvas, and the questions the
//! area asks first (the ripple question).

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::json;

use super::area::{Frame, Highlight, NodeArea, Wheel};
use super::canvas::{Out, clock, line, starts, state_colour};
use super::model::{Kind, Node, NodeState};
use crate::MfxTheme;

/// What the Bin hands to the canvas when you drag one of its items onto a shot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinItem(pub String);

/// What the Bin's Transitions folder hands to the canvas when you drag one onto a cut: kind, length.
#[derive(Clone, Debug, PartialEq)]
pub struct TransitionItem(pub String, pub f32);

/// The transition library (today's `drawTransition` list): kind, default length, a picture.
pub const TRANSITIONS: [(&str, f32); 8] =
    [("Cut", 0.0), ("Crossfade", 0.5), ("Dip to black", 0.6), ("Dip to white", 0.6), ("Wipe", 0.5), ("Push", 0.5), ("Slide", 0.5), ("Zoom", 0.4)];

fn button(ui: &mut egui::Ui, out: &mut Out, id: &str, label: &str, primary: bool, t: &MfxTheme) -> egui::Response {
    let b = if primary { egui::Button::new(RichText::new(label).color(Color32::WHITE)).fill(t.accent) } else { egui::Button::new(label) };
    let r = ui.add(b);
    out.auto.push((format!("mfx.nodes.{id}"), r.clone(), label.to_string()));
    r
}

fn link(ui: &mut egui::Ui, out: &mut Out, id: &str, label: &str) -> egui::Response {
    let r = ui.link(label);
    out.auto.push((format!("mfx.nodes.{id}"), r.clone(), label.to_string()));
    r
}

/// The header row: `Nodes  14 shots · 3 OK · …` and Maximise / Restore.
pub fn header(ui: &mut egui::Ui, area: &mut NodeArea, rect: Rect, maximised: bool, connected: bool, t: &MfxTheme, out: &mut Out) {
    let p = ui.painter_at(rect);
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.separator));
    p.text(pos2(rect.min.x + 4.0, rect.center().y), Align2::LEFT_CENTER, "Nodes", FontId::proportional(13.0), t.text);
    let mut x = rect.min.x + 56.0;
    let c = area.graph.counts();
    // each count is a link to Highlight that group
    let parts: Vec<(String, Option<Highlight>)> = {
        // on Voice the count is of voice lines (the shots come later)
        let line = if area.voice_view() {
            let lines: Vec<_> = area.graph.nodes.iter().filter(|n| n.kind == super::model::Kind::Line && !n.missing).collect();
            let made = lines.iter().filter(|n| !n.versions.is_empty()).count();
            let ok = lines.iter().filter(|n| n.state == super::model::NodeState::Ok).count();
            format!("{} voice lines · made {made} of {} · OK'd {ok}", lines.len(), lines.len())
        } else {
            c.line()
        };
        line.split(" · ")
            .map(|s| {
                let h = if s.ends_with("OK") {
                    None
                } else if s.contains("need") {
                    Some(Highlight::NeedsYou)
                } else if s.contains("making") {
                    Some(Highlight::Making)
                } else {
                    Some(Highlight::All)
                };
                (s.to_string(), h)
            })
            .collect()
    };
    for (i, (text, h)) in parts.iter().enumerate() {
        if i > 0 {
            p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, "·", FontId::proportional(12.0), t.text_faint);
            x += 10.0;
        }
        let g = p.layout_no_wrap(text.clone(), FontId::proportional(12.0), t.text_dim);
        let r = Rect::from_min_size(pos2(x, rect.center().y - 9.0), vec2(g.size().x, 18.0));
        let resp = ui.interact(r, egui::Id::new(("mfx-nodes-count", i)), Sense::click());
        let col = if resp.hovered() { t.text } else { t.text_dim };
        p.text(pos2(x, rect.center().y), Align2::LEFT_CENTER, text, FontId::proportional(12.0), col);
        out.auto.push((format!("mfx.nodes.count.{i}"), resp.clone(), text.clone()));
        if resp.clicked()
            && let Some(h) = h
        {
            out.cmds.push(("nodes.highlight".into(), json!({"what": h.key()})));
        }
        x += g.size().x + 6.0;
    }
    // what else the header has to say: offline, saved here only, an error
    let note = if !connected && area.loaded {
        "Can't reach the studio. Your OKs, fixes and moves are kept and sent when it's back.".to_string()
    } else if !area.error.is_empty() {
        area.error.clone()
    } else if area.positions_local || area.order_local {
        "This studio can't keep node places yet: they stay here until you close the app.".to_string()
    } else if c.shots > 0 && c.ok == c.shots && !area.voice_view() {
        format!("All {} shots OK.", c.shots)
    } else {
        String::new()
    };
    let max_w = (rect.max.x - 130.0 - x - 16.0).max(0.0);
    if !note.is_empty() && max_w > 60.0 {
        let g = line(&p, &note, 12.0, if area.error.is_empty() { t.text_faint } else { t.warn }, max_w);
        p.galley(pos2(x + 12.0, rect.center().y - g.size().y / 2.0), g, t.text);
    }
    // ⤢ Maximise / ⤡ Restore in a fixed-width slot (Mortiflix Studio's Maximize Frame, `)
    let mr = Rect::from_min_size(pos2(rect.max.x - 118.0, rect.min.y + 3.0), vec2(114.0, rect.height() - 6.0));
    let label = if maximised { "Restore  `" } else { "Maximise  `" };
    let resp = ui.interact(mr, egui::Id::new("mfx-nodes-maximise"), Sense::click()).on_hover_text("Maximise or restore the node area (`)");
    if resp.hovered() {
        p.rect_filled(mr, 4.0, t.hover);
    }
    p.text(mr.center(), Align2::CENTER_CENTER, label, FontId::proportional(12.0), if resp.hovered() { t.text } else { t.text_dim });
    out.auto.push(("mfx.nodes.maximise".into(), resp.clone(), label.trim().to_string()));
    if resp.clicked() || ui.interact(Rect::from_min_max(rect.min, pos2(x, rect.max.y)), egui::Id::new("mfx-nodes-tabstrip"), Sense::click()).double_clicked() {
        out.cmds.push(("window.maximizeFrameUnderCursor".into(), json!({"panel": "MfxNodes"})));
    }
}

/// The toolbar row.
pub fn toolbar(ui: &mut egui::Ui, area: &mut NodeArea, rect: Rect, t: &MfxTheme, out: &mut Out) {
    let mut bar = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(2.0, 4.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let ui = &mut bar;
    ui.spacing_mut().item_spacing.x = 6.0;
    let cut_n = area.graph.film.iter().filter(|id| area.graph.get(id).is_some_and(|n| n.state == NodeState::Cut)).count();
    let inputs = area.graph.nodes.iter().filter(|n| matches!(n.kind, Kind::Frame | Kind::Asset) && !n.missing).count();
    let bin_label = format!("Bin · {}", cut_n + inputs);
    let r = ui.selectable_label(area.show.bin, bin_label.as_str());
    out.auto.push(("mfx.nodes.bin".into(), r.clone(), bin_label));
    if r.clicked() {
        out.cmds.push(("nodes.show".into(), json!({"bin": !area.show.bin})));
    }
    // Find (Ctrl/⌘ + F)
    let find_id = egui::Id::new("mfx-nodes-find");
    let fr = ui.add(egui::TextEdit::singleline(&mut area.find).id(find_id).hint_text("Find a shot, words, scene…  Ctrl+F").desired_width(230.0));
    out.auto.push(("mfx.nodes.find".into(), fr.clone(), "Find".into()));
    if area.focus_find {
        fr.request_focus();
        area.focus_find = false;
    }
    if fr.changed() {
        area.find_index = 0;
    }
    if fr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        area.find_next();
        fr.request_focus();
    }
    if fr.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        area.find.clear();
    }
    if !area.find.trim().is_empty() {
        let n = area.found().len();
        ui.label(RichText::new(if n == 1 { "1 found".to_string() } else { format!("{n} found") }).color(if n == 0 { t.warn } else { t.text_dim }));
        if n == 0 && link(ui, out, "clearFind", "Clear search").clicked() {
            area.find.clear();
        }
    }
    // Show ▾
    let show = ui.menu_button("Show", |ui| {
        let mut s = area.show;
        let a = ui.checkbox(&mut s.voice, "Voice lines");
        out.auto.push(("mfx.nodes.show.voice".into(), a, "Voice lines".into()));
        let a = ui.checkbox(&mut s.music, "Music lane");
        out.auto.push(("mfx.nodes.show.music".into(), a, "Music lane".into()));
        let a = ui.checkbox(&mut s.inputs, "Look and your assets");
        out.auto.push(("mfx.nodes.show.inputs".into(), a, "Look and your assets".into()));
        let a = ui.checkbox(&mut s.bin, "Bin");
        out.auto.push(("mfx.nodes.show.bin".into(), a, "Bin".into()));
        if s != area.show {
            out.cmds.push(("nodes.show".into(), json!({"voice": s.voice, "music": s.music, "inputs": s.inputs, "bin": s.bin})));
        }
        ui.separator();
        ui.label(RichText::new("Mouse wheel").color(t.text_dim));
        let mut w = area.wheel;
        let a = ui.radio_value(&mut w, Wheel::Zoom, "Zoom");
        out.auto.push(("mfx.nodes.wheel.zoom".into(), a, "Zoom".into()));
        let a = ui.radio_value(&mut w, Wheel::Scroll, "Scroll");
        out.auto.push(("mfx.nodes.wheel.scroll".into(), a, "Scroll".into()));
        if w != area.wheel {
            out.cmds.push(("nodes.wheel".into(), json!({"mode": if w == Wheel::Zoom { "zoom" } else { "scroll" }})));
        }
    });
    out.auto.push(("mfx.nodes.show".into(), show.response, "Show".into()));
    // Highlight ▾
    let hl = ui.menu_button(format!("Highlight: {}", area.highlight.label()), |ui| {
        for h in Highlight::ALL {
            let r = ui.selectable_label(area.highlight == h, h.label());
            out.auto.push((format!("mfx.nodes.highlight.{}", h.key()), r.clone(), h.label().into()));
            if r.clicked() {
                out.cmds.push(("nodes.highlight".into(), json!({"what": h.key()})));
                ui.close();
            }
        }
    });
    out.auto.push(("mfx.nodes.highlight".into(), hl.response, "Highlight".into()));
    // + Add ▾
    let add = ui.menu_button("+ Add", |ui| {
        if button(ui, out, "add.shot", "New shot…", false, t).clicked() {
            area.add_open = true;
            ui.close();
        }
        let has_shot = area.primary().and_then(|s| area.graph.get(s)).is_some_and(|n| n.kind == Kind::Shot);
        if ui.add_enabled_ui(has_shot, |ui| button(ui, out, "add.duplicate", "Duplicate the selected shot", false, t)).inner.clicked() {
            out.cmds.push(("nodes.duplicate".into(), json!({})));
            ui.close();
        }
        if button(ui, out, "add.line", "Voice line…", false, t).clicked() {
            area.add_open = true;
            area.add_text = "A voice line: ".into();
            ui.close();
        }
        if button(ui, out, "add.transition", "Transition on the selected cut", false, t).clicked() {
            let cut = area.primary().and_then(|s| {
                let i = area.graph.film_index(s)?;
                area.graph.cuts.iter().find(|c| area.graph.film_index(&c.to) == Some(i)).map(|c| c.id.clone())
            });
            if let Some(c) = cut {
                area.select_only(&c);
            }
            ui.close();
        }
        ui.label(RichText::new("Your own files: the Assets tab beside the chat").color(t.text_faint));
    });
    out.auto.push(("mfx.nodes.add".into(), add.response, "+ Add".into()));
    ui.separator();
    if button(ui, out, "zoomOut", "−", false, t).on_hover_text("Zoom out (-)").clicked() {
        out.cmds.push(("nodes.zoomOut".into(), json!({})));
    }
    #[allow(clippy::cast_possible_truncation)]
    let pct = (area.zoom * 100.0).round() as i32;
    ui.label(RichText::new(format!("{pct}%")).color(t.text_dim));
    if button(ui, out, "zoomIn", "+", false, t).on_hover_text("Zoom in (=)").clicked() {
        out.cmds.push(("nodes.zoomIn".into(), json!({})));
    }
    if button(ui, out, "frameSelected", "Frame selected  F", false, t).clicked() {
        out.cmds.push(("nodes.frameSelected".into(), json!({})));
    }
    if button(ui, out, "frameAll", "Frame all  \\", false, t).clicked() {
        out.cmds.push(("nodes.frameAll".into(), json!({})));
    }
    let f = ui.selectable_label(area.follow_playhead, "Follow the playhead");
    out.auto.push(("mfx.nodes.follow".into(), f.clone(), "Follow the playhead".into()));
    if f.clicked() {
        area.follow_playhead = !area.follow_playhead;
        out.cmds.push(("mortiflix.preview.follow".into(), json!({"on": area.follow_playhead})));
    }
}

/// The overview strip under the canvas (the minimap, part of the layout): the whole film by time,
/// one block per beat in its colour, a white bar under the shots in view. Click or drag to jump.
pub fn strip(ui: &mut egui::Ui, area: &mut NodeArea, rect: Rect, t: &MfxTheme, out: &mut Out) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.panel_bg);
    p.line_segment([rect.left_top(), rect.right_top()], Stroke::new(1.0, t.separator));
    let legend_w = 250.0_f32.min(rect.width() * 0.4);
    let bar = Rect::from_min_max(rect.min + vec2(4.0, 4.0), pos2(rect.max.x - legend_w - 8.0, rect.max.y - 8.0));
    let st = starts(area);
    let total: f64 = st.values().map(|(_, l)| *l).sum::<f64>().max(0.001);
    let x_of = |sec: f64| {
        #[allow(clippy::cast_possible_truncation)]
        let f = (sec / total) as f32;
        bar.min.x + f.clamp(0.0, 1.0) * bar.width()
    };
    for beat in &area.graph.beats {
        let shots = area.graph.beat_shots(beat);
        let spans: Vec<(f64, f64)> = shots.iter().filter_map(|s| st.get(&s.id).copied()).collect();
        let (Some(a), Some(b)) = (spans.iter().map(|s| s.0).reduce(f64::min), spans.iter().map(|s| s.0 + s.1).reduce(f64::max)) else { continue };
        let r = Rect::from_min_max(pos2(x_of(a), bar.min.y), pos2(x_of(b) - 1.0, bar.max.y));
        let colour = area.graph.get(beat).and_then(|n| n.colour).or_else(|| shots.first().and_then(|s| area.graph.colour_of(s)));
        p.rect_filled(r, 2.0, colour.map_or(t.hover, |c| c.fill));
        let (n, name) = area.graph.beat_title(beat);
        let g = line(&p, &format!("{n} {name}"), 11.0, Color32::WHITE, (r.width() - 6.0).max(1.0));
        p.galley(pos2(r.min.x + 4.0, r.center().y - g.size().y / 2.0), g, Color32::WHITE);
    }
    // the white bar: shots in view
    let view = area.view_world();
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for (id, (s, l)) in &st {
        if area.node_rect(id).is_some_and(|r| r.intersects(view)) {
            lo = lo.min(*s);
            hi = hi.max(s + l);
        }
    }
    if lo.is_finite() && hi.is_finite() {
        p.rect_filled(Rect::from_min_max(pos2(x_of(lo), bar.max.y + 2.0), pos2(x_of(hi).max(x_of(lo) + 2.0), bar.max.y + 4.0)), 1.0, Color32::WHITE);
    }
    let resp = ui.interact(bar, egui::Id::new("mfx-nodes-strip"), Sense::click_and_drag());
    out.auto.push(("mfx.nodes.strip".into(), resp.clone(), "Overview strip".into()));
    if (resp.clicked() || resp.dragged())
        && let Some(pp) = resp.interact_pointer_pos()
    {
        let f = f64::from(((pp.x - bar.min.x) / bar.width().max(1.0)).clamp(0.0, 1.0)) * total;
        if let Some((id, _)) = st.iter().find(|(_, (s, l))| f >= *s && f < s + l) {
            area.want_frame = Some(Frame::Reveal(id.clone()));
            if resp.clicked() {
                area.select_only(id);
            }
        }
    }
    // legend, always visible
    let lx = rect.max.x - legend_w;
    let y = rect.center().y;
    let mut x = lx;
    let items: [(&str, u8); 3] = [("film", 0), ("voice", 1), ("look / asset", 2)];
    for (label, kind) in items {
        let a = pos2(x, y);
        let b = pos2(x + 18.0, y);
        match kind {
            0 => {
                p.line_segment([a, b], Stroke::new(2.0, t.text_dim));
            }
            1 => {
                p.extend(egui::Shape::dashed_line(&[a, b], Stroke::new(1.5, t.text_dim), 4.0, 3.0));
            }
            _ => {
                p.extend(egui::Shape::dotted_line(&[a, b], t.text_dim, 5.0, 1.2));
            }
        }
        let g = p.layout_no_wrap(label.to_string(), FontId::proportional(11.0), t.text_faint);
        let w = g.size().x;
        p.galley(pos2(x + 22.0, y - g.size().y / 2.0), g, t.text_faint);
        x += 30.0 + w;
    }
}

/// The Bin column: cut pieces (bring them back) and the inputs (drag onto a shot).
pub fn bin(ui: &mut egui::Ui, area: &mut NodeArea, rect: Rect, t: &MfxTheme, out: &mut Out) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.panel_bg);
    p.line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, t.separator));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink(6.0)).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(rect);
    let ui = &mut child;
    egui::ScrollArea::vertical().id_salt("mfx-nodes-bin").show(ui, |ui| {
        ui.label(RichText::new("BIN").small().color(t.text_faint));
        let cut: Vec<Node> = area.graph.film.iter().filter_map(|id| area.graph.get(id)).filter(|n| n.state == NodeState::Cut).cloned().collect();
        ui.label(RichText::new(format!("Cut ({})", cut.len())).color(t.text_dim));
        for n in &cut {
            let r = ui.add(egui::Button::new(format!("{} {}", n.display_name(), n.title)).truncate()).on_hover_text("Bring it back");
            out.auto.push((format!("mfx.nodes.bin.{}", n.id), r.clone(), n.display_name()));
            if r.clicked() {
                out.cmds.push(("nodes.bringBack".into(), json!({"node": n.id})));
            }
        }
        for (kind, title) in [(Kind::Frame, "Looks"), (Kind::Asset, "Your assets")] {
            let items: Vec<Node> = area.graph.nodes.iter().filter(|n| n.kind == kind && !n.missing).cloned().collect();
            ui.add_space(6.0);
            ui.label(RichText::new(format!("{title} ({})", items.len())).color(t.text_dim));
            for n in &items {
                let name = if n.title.is_empty() { n.label.clone() } else { n.title.clone() };
                let r = ui.add(egui::Button::new(name.as_str()).truncate().sense(Sense::click_and_drag())).on_hover_text("Drag it onto a shot to use it there");
                r.dnd_set_drag_payload(BinItem(n.id.clone()));
                out.auto.push((format!("mfx.nodes.bin.{}", n.id), r.clone(), name.clone()));
                if r.clicked() {
                    area.select_only(&n.id);
                }
            }
        }
        ui.add_space(6.0);
        ui.label(RichText::new("(drag onto a shot)").small().color(t.text_faint));
        ui.add_space(6.0);
        transitions_folder(ui, area, t, out);
    });
}

/// The Transitions folder (owner, 2026-10-18): every transition with its picture. Drag one onto a cut,
/// or click it while a cut is selected; the cut's pill then shows it.
fn transitions_folder(ui: &mut egui::Ui, area: &mut NodeArea, t: &MfxTheme, out: &mut Out) {
    let cut = area.primary().filter(|s| area.graph.cuts.iter().any(|c| c.id == *s)).map(str::to_string);
    let head = egui::CollapsingHeader::new(RichText::new(format!("Transitions ({})", TRANSITIONS.len())).color(t.text_dim))
        .id_salt("mfx-nodes-bin-transitions")
        .default_open(true)
        .show(ui, |ui| {
            for (k, len) in TRANSITIONS {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click_and_drag());
                let resp = resp.on_hover_text(if cut.is_some() { "Click to use it on the selected cut, or drag it onto a cut" } else { "Drag it onto a cut" });
                let sec = if k == "Cut" { 0.0 } else { len };
                resp.dnd_set_drag_payload(TransitionItem(k.to_string(), sec));
                let p = ui.painter();
                if resp.hovered() {
                    p.rect_filled(r, 3.0, t.hover);
                }
                let pic = Rect::from_min_size(r.min + vec2(2.0, 4.0), vec2(32.0, 18.0));
                draw_transition(p, pic, k, t);
                p.text(pos2(pic.max.x + 6.0, r.center().y), Align2::LEFT_CENTER, k, FontId::proportional(12.5), t.text);
                out.auto.push((format!("mfx.nodes.bin.transition.{}", k.replace(' ', "")), resp.clone(), k.to_string()));
                if resp.clicked()
                    && let Some(c) = &cut
                {
                    out.cmds.push(("nodes.pickTransition".into(), json!({"cut": c, "kind": k, "sec": sec})));
                }
            }
            ui.label(RichText::new("(drag onto a cut)").small().color(t.text_faint));
        });
    out.auto.push(("mfx.nodes.bin.transitions".into(), head.header_response, "Transitions".into()));
}

/// The inspector (340 px, right): what is selected, with one filled button per view.
pub fn inspector(ui: &mut egui::Ui, area: &mut NodeArea, rect: Rect, t: &MfxTheme, out: &mut Out) {
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.panel_bg);
    p.line_segment([rect.left_top(), rect.left_bottom()], Stroke::new(1.0, t.separator));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(10.0, 8.0))).layout(egui::Layout::top_down(egui::Align::Min)));
    child.set_clip_rect(rect);
    let ui = &mut child;
    egui::ScrollArea::vertical().id_salt("mfx-nodes-inspector").auto_shrink([false, false]).show(ui, |ui| {
        ui.set_width(rect.width() - 24.0);
        let sel: Vec<String> = area.selection.clone();
        match sel.as_slice() {
            [] => nothing(ui, area, t),
            [one] => {
                if let Some(cut) = area.graph.cuts.iter().find(|c| &c.id == one).cloned() {
                    transition(ui, area, &cut, t, out);
                } else if let Some(n) = area.graph.get(one).cloned() {
                    match n.kind {
                        Kind::Beat => beat(ui, area, &n, t, out),
                        _ => piece(ui, area, &n, t, out),
                    }
                } else {
                    nothing(ui, area, t);
                }
            }
            many => several(ui, area, many, t, out),
        }
    });
}

fn breadcrumb(ui: &mut egui::Ui, area: &mut NodeArea, n: Option<&Node>, out: &mut Out) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        if link(ui, out, "crumb.film", "Whole film").clicked() {
            area.selection.clear();
            area.want_frame = Some(Frame::All);
        }
        if let Some(n) = n {
            if let Some(b) = n.beat.clone().filter(|_| n.kind != Kind::Beat) {
                ui.label("›");
                let (num, name) = area.graph.beat_title(&b);
                if link(ui, out, "crumb.beat", &format!("{num} {name}")).clicked() {
                    area.select_only(&b);
                    area.want_frame = Some(Frame::Beat(b.clone()));
                }
            }
            ui.label("›");
            ui.label(match n.kind {
                Kind::Shot => format!("Shot {}", n.display_name().trim_start_matches('#')),
                Kind::Beat => {
                    let (num, name) = area.graph.beat_title(&n.id);
                    format!("{num} {name}")
                }
                _ => n.display_name(),
            });
        }
    });
}

fn nothing(ui: &mut egui::Ui, area: &NodeArea, t: &MfxTheme) {
    if area.voice_view() {
        let lines: Vec<&Node> = area.graph.nodes.iter().filter(|n| n.kind == super::model::Kind::Line && !n.missing).collect();
        let secs: f64 = lines.iter().filter_map(|n| n.start_sec.zip(n.end_sec)).map(|(a, b)| (b - a).max(0.0)).filter(|s| s.is_finite()).sum();
        ui.label(RichText::new("The voice").strong().color(t.text));
        ui.label(RichText::new(format!("{} lines · {} spoken", lines.len(), clock(secs))).color(t.text_dim));
        ui.add_space(8.0);
        ui.label(RichText::new("Click a line to hear it and check it on the Voice stage. Double-click plays it.").color(t.text_faint));
        ui.add_space(8.0);
        ui.label(RichText::new("The shots join here once the style frames are drawn.").small().color(t.text_faint));
        return;
    }
    let c = area.graph.counts();
    let total: f64 = starts(area).values().map(|(_, l)| *l).sum();
    ui.label(RichText::new("Whole film").strong().color(t.text));
    ui.label(RichText::new(format!("{} · {}", c.line(), clock(total))).color(t.text_dim));
    ui.add_space(8.0);
    ui.label(RichText::new("Click a shot to see what goes into it.").color(t.text_faint));
    ui.add_space(8.0);
    ui.label(
        RichText::new("Drag nodes anywhere. Pull a noodle from a shot's socket to another shot, or drop a shot on a noodle, to change the film order.")
            .small()
            .color(t.text_faint),
    );
}

fn state_line(ui: &mut egui::Ui, n: &Node, t: &MfxTheme) {
    let mut word = format!("{} {}", n.state.glyph(), n.state.word());
    if let Some(v) = n.version_tag() {
        word = format!("{word} · {v}");
    }
    ui.label(RichText::new(word).color(state_colour(n.state, t)));
    if n.state == NodeState::NeedsYou && !n.reason.is_empty() {
        ui.label(RichText::new(&n.reason).color(t.warn));
    }
    if let Some(p) = n.pending.as_ref().filter(|_| n.state.making()) {
        ui.label(RichText::new(format!("Fix being made: {p}")).color(t.text_dim));
    }
}

/// OK · Fix · Cut it with the filled OK, the hint, and the Fix box.
fn decisions(ui: &mut egui::Ui, area: &mut NodeArea, n: &Node, t: &MfxTheme, out: &mut Out) {
    ui.horizontal(|ui| {
        if n.state == NodeState::Cut {
            if button(ui, out, "inspector.bringBack", "Bring it back", true, t).clicked() {
                out.cmds.push(("nodes.bringBack".into(), json!({"node": n.id})));
            }
            return;
        }
        let ok_label = if n.state == NodeState::Ok { "OK ✓" } else { "OK" };
        if n.versions.is_empty() && matches!(n.kind, Kind::Shot | Kind::Frame | Kind::Line | Kind::Cue | Kind::Transition) {
            // a plan: nothing to OK until it exists (Fix still changes the plan)
            let r = ui.add_enabled(false, egui::Button::new("OK")).on_disabled_hover_text("Not made yet: OK it once it exists");
            out.auto.push(("mfx.nodes.inspector.ok".into(), r, "OK (not made yet)".into()));
        } else if button(ui, out, "inspector.ok", ok_label, n.state != NodeState::Ok, t).clicked() {
            out.cmds.push((if n.state == NodeState::Ok { "nodes.bringBack" } else { "nodes.ok" }.into(), json!({"node": n.id})));
        }
        if button(ui, out, "inspector.fix", "Fix", false, t).clicked() {
            area.fix_open = !area.fix_open;
        }
        if button(ui, out, "inspector.cut", "Cut it", false, t).clicked() {
            out.cmds.push(("nodes.cut".into(), json!({"node": n.id})));
        }
    });
    if n.kind == Kind::Shot && n.state != NodeState::Cut {
        let next = area.graph.film_index(&n.id).and_then(|i| area.graph.film.get(i + 1)).and_then(|s| area.graph.get(s)).map(Node::display_name);
        let goes = next.map_or_else(|| "OK is the last shot".to_string(), |s| format!("OK goes on to shot {}", s.trim_start_matches('#')));
        ui.label(RichText::new(format!("{goes} · Enter OK · Shift+Enter Fix · Delete Cut it")).small().color(t.text_faint));
    }
    if area.fix_open && n.state != NodeState::Cut {
        ui.add_space(4.0);
        let q = match n.kind {
            Kind::Shot => format!("What should change in shot {}?", n.display_name().trim_start_matches('#')),
            k => format!("What should change in this {}?", k.word()),
        };
        ui.label(RichText::new(q).color(t.text));
        let r = ui.add(egui::TextEdit::multiline(&mut area.fix_text).id(egui::Id::new("mfx-nodes-fix")).desired_rows(3).desired_width(f32::INFINITY));
        out.auto.push(("mfx.nodes.inspector.fixText".into(), r.clone(), "Fix text".into()));
        if area.fix_text.is_empty() && !r.has_focus() {
            r.request_focus();
        }
        ui.label(RichText::new("Sent at once; only this one is remade while you carry on.").small().color(t.text_faint));
        ui.horizontal(|ui| {
            if button(ui, out, "inspector.fixSend", "Send", true, t).clicked() && !area.fix_text.trim().is_empty() {
                out.cmds.push(("nodes.fix".into(), json!({"node": n.id, "text": area.fix_text.trim()})));
            }
            if button(ui, out, "inspector.fixCancel", "Cancel", false, t).clicked() {
                area.fix_open = false;
            }
        });
    }
}

fn piece(ui: &mut egui::Ui, area: &mut NodeArea, n: &Node, t: &MfxTheme, out: &mut Out) {
    breadcrumb(ui, area, Some(n), out);
    ui.add_space(4.0);
    let colour = area.graph.colour_of(n);
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        ui.painter().rect_filled(r, 2.0, colour.map_or(t.hover, |c| c.chip));
        let title = match n.kind {
            Kind::Shot => format!("Shot {}", n.display_name().trim_start_matches('#')),
            k => format!("{} {}", super::area::capitalise(k.word()), n.display_name()),
        };
        ui.label(RichText::new(title).strong().color(t.text));
        if let Some((s, l)) = starts(area).get(&n.id) {
            ui.label(RichText::new(format!("{} · {l:.1} s", clock(*s))).color(t.text_dim));
        }
    });
    state_line(ui, n, t);
    if !matches!(n.kind, Kind::Frame | Kind::Asset) {
        ui.horizontal(|ui| {
            let scope = if n.kind == Kind::Shot { "from" } else { "shot" };
            if button(ui, out, "inspector.play", "▶ Play from here", false, t).clicked() {
                super::canvas::play(out, area, &n.id, scope);
            }
        });
        decisions(ui, area, n, t, out);
    }
    // Versions
    if !n.versions.is_empty() {
        ui.add_space(6.0);
        ui.label(RichText::new("Versions").color(t.text_dim));
        ui.horizontal_wrapped(|ui| {
            for v in &n.versions {
                let current = Some(v.v) == n.current;
                let peek = area.peek_version == Some(v.v);
                let mut label = format!("v{}", v.v);
                if current && n.state == NodeState::Ok {
                    label.push_str(" · OK");
                }
                let r = ui.selectable_label(current || peek, label.as_str());
                out.auto.push((format!("mfx.nodes.inspector.version.{}", v.v), r.clone(), label.clone()));
                if r.clicked() {
                    area.peek_version = if current { None } else { Some(v.v) };
                }
            }
            if n.state.making() {
                ui.label(RichText::new(format!("v{} · making", n.versions.len() + 1)).color(t.text_faint));
            }
        });
        if let Some(v) = area.peek_version.filter(|v| Some(*v) != n.current) {
            let locked = n.state == NodeState::Ok;
            let r = ui.add_enabled(!locked, egui::Button::new(format!("Use this version (v{v})")));
            out.auto.push(("mfx.nodes.inspector.useVersion".into(), r.clone(), "Use this version".into()));
            if locked {
                ui.label(RichText::new("The OK'd version is locked: take back the OK first.").small().color(t.text_faint));
            }
            if r.clicked() {
                out.cmds.push(("nodes.useVersion".into(), json!({"node": n.id, "v": v})));
            }
        }
    }
    // What goes into it
    if n.kind == Kind::Shot {
        ui.add_space(6.0);
        ui.label(RichText::new("What goes into it").color(t.text_dim));
        egui::Grid::new("mfx-nodes-inputs").num_columns(2).spacing([8.0, 3.0]).show(ui, |ui| {
            ui.label(RichText::new("Words").color(t.text_faint));
            ui.label(&n.title);
            ui.end_row();
            let lines: Vec<Node> =
                area.graph.nodes.iter().filter(|l| l.kind == Kind::Line && area.graph.line_shot(l).as_deref() == Some(n.id.as_str())).cloned().collect();
            ui.label(RichText::new("Voice").color(t.text_faint));
            ui.vertical(|ui| {
                if lines.is_empty() {
                    ui.label(RichText::new("none").color(t.text_faint));
                }
                for l in &lines {
                    if link(ui, out, &format!("inspector.go.{}", l.id), &l.display_name()).clicked() {
                        area.select_only(&l.id);
                        area.want_frame = Some(Frame::Reveal(l.id.clone()));
                    }
                }
            });
            ui.end_row();
            ui.label(RichText::new("Look").color(t.text_faint));
            match n.look.clone().and_then(|l| area.graph.get(&l).cloned()) {
                Some(look) => {
                    let name = if look.title.is_empty() { look.label.clone() } else { look.title.clone() };
                    if link(ui, out, "inspector.go.look", &name).clicked() {
                        area.show.inputs = true;
                        area.select_only(&look.id);
                        area.want_frame = Some(Frame::Reveal(look.id.clone()));
                    }
                }
                None => {
                    ui.label(RichText::new("the film's look").color(t.text_faint));
                }
            }
            ui.end_row();
            ui.label(RichText::new("Assets").color(t.text_faint));
            ui.label(if n.uses.is_empty() { "none".to_string() } else { n.uses.join(", ") });
            ui.end_row();
            ui.label(RichText::new("Music").color(t.text_faint));
            match area.graph.nodes.iter().find(|c| c.kind == Kind::Cue && c.beat.is_some() && c.beat == n.beat).cloned() {
                Some(c) => {
                    if link(ui, out, "inspector.go.cue", &format!("{} {}", c.label, c.title)).clicked() {
                        area.select_only(&c.id);
                        area.want_frame = Some(Frame::Reveal(c.id.clone()));
                    }
                }
                None => {
                    ui.label(RichText::new("none yet").color(t.text_faint));
                }
            }
            ui.end_row();
            for (label, cut) in
                [("Comes in", area.graph.cuts.iter().find(|c| c.to == n.id).cloned()), ("Goes out", area.graph.cuts.iter().find(|c| c.from == n.id).cloned())]
            {
                ui.label(RichText::new(label).color(t.text_faint));
                match cut {
                    Some(c) => {
                        let (k, s, _) = area.cut_label(&c);
                        let text = s.filter(|s| *s > 0.0).map_or(k.clone(), |s| format!("{k} {s:.1} s"));
                        if link(ui, out, &format!("inspector.go.{}", label.replace(' ', "")), &text).clicked() {
                            area.select_only(&c.id);
                        }
                    }
                    None => {
                        ui.label(RichText::new(if label == "Comes in" { "the start" } else { "the end" }).color(t.text_faint));
                    }
                }
                ui.end_row();
            }
        });
    }
    if n.kind == Kind::Line {
        ui.add_space(6.0);
        ui.label(RichText::new(&n.title).color(t.text));
        if let Some(s) = area.graph.line_shot(n) {
            let name = area.graph.get(&s).map(Node::display_name).unwrap_or_default();
            if link(ui, out, "inspector.go.shot", &format!("Said over shot {}", name.trim_start_matches('#'))).clicked() {
                area.select_only(&s);
                area.want_frame = Some(Frame::Reveal(s.clone()));
            }
        }
    }
    if matches!(n.kind, Kind::Frame | Kind::Asset) {
        let used: Vec<String> = area
            .graph
            .nodes
            .iter()
            .filter(|s| s.kind == Kind::Shot && (s.look.as_deref() == Some(n.id.as_str()) || s.uses.contains(&n.id)))
            .map(Node::display_name)
            .collect();
        ui.label(
            RichText::new(if used.is_empty() { "Not used yet. Drag it onto a shot.".to_string() } else { format!("Used in {}", used.join(", ")) })
                .color(t.text_dim),
        );
    }
    // folded: Notes, History
    ui.add_space(6.0);
    egui::CollapsingHeader::new(format!("Notes · {}", n.notes.len())).id_salt(("mfx-notes", &n.id)).show(ui, |ui| {
        for note in &n.notes {
            ui.label(note);
        }
        let r = ui.add(egui::TextEdit::singleline(&mut area.note_text).hint_text("Add a note (doesn't start a remake)"));
        out.auto.push(("mfx.nodes.inspector.noteText".into(), r.clone(), "Note".into()));
        if button(ui, out, "inspector.noteAdd", "Add note", false, t).clicked() && !area.note_text.trim().is_empty() {
            out.cmds.push(("nodes.note".into(), json!({"node": n.id, "text": area.note_text.trim()})));
            area.note_text.clear();
        }
    });
    egui::CollapsingHeader::new(if n.edits.len() == 1 { "History · 1 fix".to_string() } else { format!("History · {} fixes", n.edits.len()) })
        .id_salt(("mfx-history", &n.id))
        .show(ui, |ui| {
            for e in n.edits.iter().rev() {
                let v = e.version.map(|v| format!(" → v{v}")).unwrap_or_default();
                ui.label(RichText::new(format!("{} · {}{v}: {}", e.id, e.state, e.text)).small());
            }
            for v in n.versions.iter().rev() {
                ui.label(RichText::new(format!("v{} made {}", v.v, v.made_at.get(..16).unwrap_or(&v.made_at))).small().color(t.text_faint));
            }
        });
}

fn transition(ui: &mut egui::Ui, area: &mut NodeArea, cut: &super::model::Cut, t: &MfxTheme, out: &mut Out) {
    let from = area.graph.get(&cut.from).map(Node::display_name).unwrap_or_default();
    let to = area.graph.get(&cut.to).map(Node::display_name).unwrap_or_default();
    breadcrumb(ui, area, None, out);
    ui.label(RichText::new(format!("The cut from {from} to {to}")).strong().color(t.text));
    let (kind, sec, pick) = area.cut_label(cut);
    ui.label(
        RichText::new(if pick {
            "! Pick one: a new cut with no transition yet (the AI picks a crossfade if you don't)".to_string()
        } else if cut.transition.is_some() {
            format!("Now: {kind} (made by the AI)")
        } else {
            "Now: a straight cut".to_string()
        })
        .color(if pick { t.error } else { t.text_dim }),
    );
    if button(ui, out, "inspector.playCut", "▶ Play across the cut", false, t).clicked() {
        super::canvas::play(out, area, &cut.id, "cut");
    }
    ui.add_space(6.0);
    let chosen_id = egui::Id::new(("mfx-nodes-pick", &cut.id));
    let mut chosen: (String, f32) = ui.data(|d| d.get_temp(chosen_id)).unwrap_or_else(|| (kind.clone(), sec.unwrap_or(0.5)));
    for (k, len) in TRANSITIONS {
        let on = chosen.0 == k;
        let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
        let p = ui.painter();
        if on || resp.hovered() {
            p.rect_filled(r, 3.0, if on { t.accent.gamma_multiply(0.3) } else { t.hover });
        }
        let pic = Rect::from_min_size(r.min + vec2(4.0, 4.0), vec2(40.0, 22.0));
        draw_transition(p, pic, k, t);
        p.text(pos2(pic.max.x + 8.0, r.center().y), Align2::LEFT_CENTER, k, FontId::proportional(13.0), t.text);
        out.auto.push((format!("mfx.nodes.inspector.transition.{}", k.replace(' ', "")), resp.clone(), k.to_string()));
        if resp.clicked() {
            chosen = (k.to_string(), if k == "Cut" { 0.0 } else { len });
        }
    }
    if chosen.0 != "Cut" {
        ui.horizontal(|ui| {
            ui.label("Length");
            let r = ui.add(egui::DragValue::new(&mut chosen.1).range(0.1..=5.0).speed(0.05).suffix(" s").max_decimals(2));
            out.auto.push(("mfx.nodes.inspector.length".into(), r, "Length".into()));
        });
    }
    ui.data_mut(|d| d.insert_temp(chosen_id, chosen.clone()));
    if button(ui, out, "inspector.useTransition", "Use this transition", true, t).clicked() {
        out.cmds.push(("nodes.pickTransition".into(), json!({"cut": cut.id, "kind": chosen.0, "sec": chosen.1})));
    }
    if let Some(tid) = &cut.transition {
        ui.add_space(6.0);
        ui.label(RichText::new("Or say what you want in your own words").color(t.text_dim));
        let r = ui.add(egui::TextEdit::singleline(&mut area.fix_text).hint_text("a slow wipe to the left…"));
        out.auto.push(("mfx.nodes.inspector.transitionWords".into(), r, "Transition words".into()));
        if button(ui, out, "inspector.transitionSend", "Send", false, t).clicked() && !area.fix_text.trim().is_empty() {
            out.cmds.push(("nodes.fix".into(), json!({"node": tid, "text": area.fix_text.trim()})));
        }
    }
}

/// A small drawn picture of a transition (no glyphs needed).
pub fn draw_transition(p: &egui::Painter, r: Rect, kind: &str, t: &MfxTheme) {
    let a = Color32::from_rgb(70, 90, 140);
    let b = Color32::from_rgb(150, 110, 60);
    let half = Rect::from_min_max(r.min, pos2(r.center().x, r.max.y));
    let other = Rect::from_min_max(pos2(r.center().x, r.min.y), r.max);
    match kind {
        "Crossfade" => {
            p.rect_filled(r, 2.0, a);
            p.rect_filled(Rect::from_min_max(pos2(r.min.x + r.width() * 0.3, r.min.y), r.max), 2.0, b.gamma_multiply(0.7));
        }
        "Dip to black" | "Dip to white" => {
            let mid = if kind == "Dip to black" { Color32::BLACK } else { Color32::WHITE };
            p.rect_filled(half, 0.0, a);
            p.rect_filled(other, 0.0, b);
            p.rect_filled(Rect::from_center_size(r.center(), vec2(r.width() * 0.3, r.height())), 0.0, mid);
        }
        "Wipe" => {
            p.rect_filled(r, 2.0, a);
            p.add(egui::Shape::convex_polygon(
                vec![pos2(r.center().x + 4.0, r.min.y), r.right_top(), r.right_bottom(), pos2(r.center().x - 4.0, r.max.y)],
                b,
                Stroke::NONE,
            ));
        }
        "Push" | "Slide" => {
            p.rect_filled(half, 0.0, a);
            p.rect_filled(other, 0.0, b);
            let y = r.center().y;
            p.arrow(pos2(r.max.x - 4.0, y), vec2(-12.0, 0.0), Stroke::new(1.5, Color32::WHITE));
        }
        "Zoom" => {
            p.rect_filled(r, 2.0, a);
            p.rect_filled(Rect::from_center_size(r.center(), r.size() * 0.5), 1.0, b);
        }
        _ => {
            p.rect_filled(half, 0.0, a);
            p.rect_filled(other, 0.0, b);
        }
    }
    p.rect_stroke(r, 2.0, Stroke::new(1.0, t.separator), StrokeKind::Inside);
}

fn beat(ui: &mut egui::Ui, area: &mut NodeArea, n: &Node, t: &MfxTheme, out: &mut Out) {
    breadcrumb(ui, area, Some(n), out);
    let shots: Vec<Node> = area.graph.beat_shots(&n.id).into_iter().filter(|s| s.state != NodeState::Cut).cloned().collect();
    let ok = shots.iter().filter(|s| s.state == NodeState::Ok && !s.versions.is_empty()).count();
    let made = shots.iter().filter(|s| !s.versions.is_empty()).count();
    let (num, name) = area.graph.beat_title(&n.id);
    ui.label(RichText::new(format!("{num} {name}")).strong().color(t.text));
    ui.label(RichText::new(&n.title).color(t.text_dim));
    ui.label(RichText::new(format!("{} shots · made {made} of {} · OK'd {ok}", shots.len(), shots.len())).color(t.text_dim));
    let ids: Vec<String> = shots.iter().map(|s| s.id.clone()).collect();
    ui.horizontal(|ui| {
        let label = if made == shots.len() { format!("OK all {} shots", shots.len()) } else { format!("OK the {made} made") };
        if made > 0 && button(ui, out, "inspector.okAll", &label, true, t).clicked() {
            out.cmds.push(("nodes.ok".into(), json!({"nodes": ids})));
        }
        if button(ui, out, "inspector.fixBeat", "Fix the scene", false, t).clicked() {
            area.fix_open = true;
        }
    });
    if area.fix_open {
        let r = ui.add(egui::TextEdit::multiline(&mut area.fix_text).hint_text("What should change in this scene?").desired_rows(3));
        out.auto.push(("mfx.nodes.inspector.fixText".into(), r, "Fix text".into()));
        if button(ui, out, "inspector.fixSend", "Send", true, t).clicked() && !area.fix_text.trim().is_empty() {
            out.cmds.push(("nodes.fix".into(), json!({"node": n.id, "text": area.fix_text.trim()})));
        }
    }
}

fn several(ui: &mut egui::Ui, area: &mut NodeArea, ids: &[String], t: &MfxTheme, out: &mut Out) {
    breadcrumb(ui, area, None, out);
    let shots: Vec<String> = ids.iter().filter(|i| area.graph.get(i).is_some_and(|n| n.kind != Kind::Beat)).cloned().collect();
    ui.label(RichText::new(format!("{} selected", ids.len())).strong().color(t.text));
    ui.horizontal(|ui| {
        if button(ui, out, "inspector.okAll", &format!("OK all {}", shots.len()), true, t).clicked() {
            out.cmds.push(("nodes.ok".into(), json!({"nodes": shots})));
        }
        if button(ui, out, "inspector.fixAll", &format!("Fix all {}", shots.len()), false, t).clicked() {
            area.fix_open = true;
        }
        if button(ui, out, "inspector.cutAll", "Cut them", false, t).clicked() {
            out.cmds.push(("nodes.cut".into(), json!({"nodes": shots})));
        }
    });
    if area.fix_open {
        let r = ui.add(egui::TextEdit::multiline(&mut area.fix_text).hint_text("One note, sent to each").desired_rows(3));
        out.auto.push(("mfx.nodes.inspector.fixText".into(), r, "Fix text".into()));
        if button(ui, out, "inspector.fixSend", "Send", true, t).clicked() && !area.fix_text.trim().is_empty() {
            out.cmds.push(("nodes.fix".into(), json!({"nodes": shots, "text": area.fix_text.trim()})));
        }
    }
    ui.add_space(6.0);
    for id in ids {
        if let Some(n) = area.graph.get(id) {
            ui.label(RichText::new(format!("{} · {} {}", n.display_name(), n.state.glyph(), n.state.word())).color(t.text_dim));
        }
    }
}

/// The question asked first (the ripple question, a move into another beat, New shot).
pub fn dialogs(ui: &mut egui::Ui, area: &mut NodeArea, t: &MfxTheme, out: &mut Out) {
    if let Some(c) = area.confirm.clone() {
        let modal = egui::Modal::new(egui::Id::new("mfx-nodes-confirm")).show(ui.ctx(), |ui| {
            ui.set_width(400.0);
            ui.heading(&c.title);
            if !c.detail.is_empty() {
                ui.label(&c.detail);
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if button(ui, out, "confirm.cancel", "Cancel", false, t).clicked() {
                    area.confirm = None;
                }
                if button(ui, out, "confirm.yes", &c.yes, true, t).clicked() {
                    let mut params = c.params.clone();
                    if let Some(o) = params.as_object_mut() {
                        o.insert("force".into(), json!(true));
                    }
                    out.cmds.push((c.what.clone(), params));
                    area.confirm = None;
                }
            });
        });
        if modal.should_close() {
            area.confirm = None;
        }
    }
    if area.add_open {
        let after = area.primary().and_then(|s| area.graph.get(s)).filter(|n| n.kind == Kind::Shot).map(|n| (n.id.clone(), n.display_name()));
        let modal = egui::Modal::new(egui::Id::new("mfx-nodes-add")).show(ui.ctx(), |ui| {
            ui.set_width(420.0);
            ui.heading("New shot");
            ui.label(
                after
                    .as_ref()
                    .map_or("It goes at the end of the film.".to_string(), |(_, n)| format!("It goes right after shot {}.", n.trim_start_matches('#'))),
            );
            let r = ui.add(egui::TextEdit::multiline(&mut area.add_text).hint_text("Describe the shot").desired_rows(3).desired_width(f32::INFINITY));
            out.auto.push(("mfx.nodes.add.text".into(), r, "Describe the shot".into()));
            ui.label(RichText::new("It appears Planned, and the AI makes it.").small().color(t.text_faint));
            ui.horizontal(|ui| {
                if button(ui, out, "add.cancel", "Cancel", false, t).clicked() {
                    area.add_open = false;
                }
                if button(ui, out, "add.send", "Send", true, t).clicked() && !area.add_text.trim().is_empty() {
                    out.cmds.push(("nodes.add".into(), json!({"after": after.as_ref().map(|a| a.0.clone()), "text": area.add_text.trim()})));
                    area.add_open = false;
                    area.add_text.clear();
                }
            });
        });
        if modal.should_close() {
            area.add_open = false;
        }
    }
}
