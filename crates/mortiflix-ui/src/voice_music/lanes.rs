//! The lanes (doc 34 "Lanes"): Shots, Music and Voice on one time axis in seconds. Click a clip to
//! select it (the panel above fills, the chat scopes to it, the playhead moves), double-click to
//! play it, drag a line sideways to move its timing, drop a recording on a line. "Whole video" fits
//! the film in the width; Zoom lets the lanes scroll. (Lane M / S / faders need a lane mix message
//! in the service protocol; per-line and per-cue levels are in the panel above.)
//!
//! Clip marks are words or signs, never colour alone: ✓ OK, ↻ remaking, ● your take, vN, "making"
//! (striped), ! couldn't make it, cut. The playhead runs behind the clips (one layer).

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, pos2, vec2};

use super::model::{Kind, Piece, PieceState, Timeline, fmt_time};
use crate::MfxCtx;

pub const HEADER_W: f32 = 112.0;
const TOP_H: f32 = 24.0;
const RULER_H: f32 = 18.0;
const SHOT_H: f32 = 34.0;
const ROW_H: f32 = 42.0;
/// Clip text on a beat colour fill (doc 33: `#d9d9d9`).
const CLIP_TEXT: Color32 = Color32::from_rgb(0xd9, 0xd9, 0xd9);
/// Zoom steps: 1 = the whole video fits.
const ZOOMS: [f32; 4] = [1.0, 2.0, 4.0, 8.0];

/// What the lanes keep between frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneUi {
    /// Index into [`ZOOMS`].
    pub zoom: usize,
    /// A line being dragged: (id, seconds moved so far).
    pub drag: Option<(String, f64)>,
}

/// What happened on the lanes this frame (the stage acts on it).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaneOut {
    pub select: Option<(Kind, String)>,
    pub play: Option<(Kind, String)>,
    /// A line let go after a drag: (id, new start in seconds).
    pub moved: Option<(String, f64)>,
    pub seek: Option<f64>,
    /// Audio files dropped on a line: (line id, path).
    pub dropped: Option<(String, String)>,
}

/// The lanes' height for the current timeline.
pub fn height(tl: &Timeline) -> f32 {
    let mut h = TOP_H + RULER_H + SHOT_H + 6.0;
    if !tl.voice_only && (!tl.cues.is_empty() || tl.has_music || tl.has_view) {
        h += ROW_H;
    }
    if !tl.no_narration() {
        h += ROW_H;
    }
    h
}

/// Draw the lanes in the space left in `ui`.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx, tl: &Timeline, st: &mut LaneUi, selected: Option<&(Kind, String)>, cursor: f64) -> LaneOut {
    let t = ctx.theme;
    let mut out = LaneOut::default();

    // Top row: name, counts, legend, Whole video | Zoom.
    ui.horizontal(|ui| {
        ui.set_height(TOP_H);
        ui.label(egui::RichText::new(if tl.voice_only { "Voice" } else { "Voice & music" }).color(t.text).strong());
        let counts = if tl.voice_only {
            format!("{} lines · {}", tl.lines.len(), fmt_time(tl.duration))
        } else {
            format!("{} lines · {} cues · {}", tl.lines.len(), tl.cues.len(), fmt_time(tl.duration))
        };
        ui.colored_label(t.text_faint, counts);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let zin = ui.add_enabled(st.zoom + 1 < ZOOMS.len(), egui::Button::new("Zoom +").small());
            ctx.auto("mfx.vm.zoom.in", &zin, "Zoom");
            if zin.clicked() {
                st.zoom = (st.zoom + 1).min(ZOOMS.len() - 1);
            }
            let whole = ui.selectable_label(st.zoom == 0, "Whole video");
            ctx.auto("mfx.vm.zoom.whole", &whole, "Whole video");
            if whole.clicked() {
                st.zoom = 0;
            }
            // the legend only where it fits: on a narrow stage it was drawn over the counts (run 2)
            let legend = "✓ OK · ↻ remaking · ● your take";
            let w = ui.painter().layout_no_wrap(legend.to_string(), egui::TextStyle::Body.resolve(ui.style()), t.text_faint).size().x;
            if ui.available_width() > w + 12.0 {
                ui.colored_label(t.text_faint, legend);
            }
        });
    });

    let rows: Vec<(&str, f32)> = {
        let mut r = vec![("shots", SHOT_H)];
        if !tl.voice_only && (!tl.cues.is_empty() || tl.has_music || tl.has_view) {
            r.push(("music", ROW_H));
        }
        if !tl.no_narration() {
            r.push(("voice", ROW_H));
        }
        r
    };
    let body_h = RULER_H + rows.iter().map(|r| r.1).sum::<f32>();

    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        // Lane headers
        let (head, _) = ui.allocate_exact_size(vec2(HEADER_W, body_h), Sense::hover());
        let mut y = head.min.y + RULER_H;
        for (name, h) in &rows {
            let r = Rect::from_min_size(pos2(head.min.x, y), vec2(HEADER_W - 6.0, *h));
            y += h;
            lane_header(ui, ctx, tl, name, r);
        }
        // Clips
        let avail = ui.available_width().max(80.0);
        let zoom = ZOOMS.get(st.zoom).copied().unwrap_or(1.0);
        let dur = tl.duration.max(1.0);
        let content_w = (avail * zoom).min(200_000.0);
        let pps = f64::from(content_w - 8.0) / dur;
        egui::ScrollArea::horizontal().id_salt("mfx-vm-lanes").max_width(avail).auto_shrink([false, true]).show(ui, |ui| {
            let (area, _) = ui.allocate_exact_size(vec2(content_w, body_h), Sense::hover());
            let x_of = |s: f64| area.min.x + 4.0 + (s * pps) as f32;
            let p = ui.painter_at(area);
            // ruler (click to move the playhead)
            let ruler = Rect::from_min_size(area.min, vec2(area.width(), RULER_H));
            let rr = ui.interact(ruler, egui::Id::new("mfx-vm-ruler"), Sense::click());
            ctx.auto("mfx.vm.ruler", &rr, "Time ruler");
            if rr.clicked()
                && let Some(pos) = rr.interact_pointer_pos()
            {
                out.seek = Some((f64::from(pos.x - area.min.x - 4.0) / pps).clamp(0.0, dur));
            }
            let step = tick_step(dur / f64::from(zoom));
            let mut s = 0.0;
            while s <= dur + 1e-9 {
                let x = x_of(s);
                p.line_segment([pos2(x, ruler.max.y - 5.0), pos2(x, ruler.max.y)], Stroke::new(1.0, t.text_faint));
                p.text(
                    pos2(x + 3.0, ruler.min.y + 1.0),
                    Align2::LEFT_TOP,
                    fmt_time(s).trim_end_matches(".0").to_string(),
                    FontId::proportional(10.0),
                    t.text_faint,
                );
                s += step;
            }
            // the playhead, behind the clips
            let px = x_of(cursor.clamp(0.0, dur));
            p.line_segment([pos2(px, area.min.y), pos2(px, area.max.y)], Stroke::new(1.0, t.accent.gamma_multiply(0.7)));

            let mut y = area.min.y + RULER_H;
            for (name, h) in &rows {
                let row = Rect::from_min_size(pos2(area.min.x, y), vec2(area.width(), *h));
                y += h;
                p.line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, t.separator));
                let (list, kind) = match *name {
                    "shots" => (&tl.shots, Kind::Shot),
                    "music" => (&tl.cues, Kind::Cue),
                    _ => (&tl.lines, Kind::Line),
                };
                if list.is_empty() {
                    let text = match kind {
                        Kind::Cue => "No music cues yet",
                        Kind::Line => "No voice lines yet",
                        Kind::Shot => "No shots yet",
                    };
                    p.text(pos2(row.min.x + 8.0, row.center().y), Align2::LEFT_CENTER, text, FontId::proportional(11.0), t.text_faint);
                }
                for piece in list {
                    let shift = match (&st.drag, kind) {
                        (Some((id, d)), Kind::Line) if *id == piece.id => *d,
                        _ => 0.0,
                    };
                    let r = Rect::from_min_max(
                        pos2(x_of(piece.start + shift), row.min.y + 3.0),
                        pos2(x_of(piece.end + shift).max(x_of(piece.start + shift) + 3.0), row.max.y - 3.0),
                    );
                    let is_sel = selected.is_some_and(|(k, id)| *k == kind && *id == piece.id);
                    let sense = if kind == Kind::Line { Sense::click_and_drag() } else { Sense::click() };
                    let resp = ui.interact(r, egui::Id::new(("mfx-vm-clip", kind.key(), &piece.id)), sense).on_hover_text(hover_text(piece));
                    clip(&p, ctx, piece, r, is_sel, resp.hovered());
                    ctx.auto(&format!("mfx.vm.{}.{}", kind.key(), piece.id), &resp, &format!("{} {}", kind.key(), piece.label));
                    if resp.double_clicked() {
                        out.play = Some((kind, piece.id.clone()));
                    } else if resp.clicked() {
                        out.select = Some((kind, piece.id.clone()));
                    }
                    if kind == Kind::Line {
                        if resp.dragged() {
                            let d = st.drag.as_ref().filter(|(id, _)| *id == piece.id).map_or(0.0, |x| x.1);
                            st.drag = Some((piece.id.clone(), d + f64::from(resp.drag_delta().x) / pps));
                        }
                        if resp.drag_stopped() {
                            let moved = (piece.start + shift).max(0.0);
                            out.moved = Some((piece.id.clone(), (moved * 10.0).round() / 10.0));
                            out.select = Some((kind, piece.id.clone()));
                            st.drag = None;
                        }
                        // A recording dragged in from the desktop, let go on this line (doc 34).
                        let dropped: Vec<String> = ui.ctx().input(|i| {
                            if i.raw.dropped_files.is_empty() || !i.pointer.latest_pos().is_some_and(|pos| r.contains(pos)) {
                                return Vec::new();
                            }
                            i.raw.dropped_files.iter().map(|f| f.path().to_string_lossy().into_owned()).filter(|p| !p.is_empty()).collect()
                        });
                        if let Some(path) = dropped.into_iter().next() {
                            out.dropped = Some((piece.id.clone(), path));
                        }
                    }
                }
            }
        });
    });
    out
}

fn lane_header(ui: &egui::Ui, ctx: &MfxCtx, tl: &Timeline, name: &'static str, r: Rect) {
    let t = ctx.theme;
    let p = ui.painter_at(r);
    p.line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, t.separator));
    let title = match name {
        "shots" if tl.voice_only => format!("Scenes {}", tl.shots.len()),
        "shots" => format!("Shots {}", tl.shots.len()),
        "music" => "Music".into(),
        _ => "Voice".into(),
    };
    p.text(pos2(r.min.x + 4.0, r.min.y + 9.0), Align2::LEFT_CENTER, title, FontId::proportional(12.0), t.text);
    if name == "shots" {
        return;
    }
    let (count, what) = if name == "music" { (tl.cues.len(), "cues") } else { (tl.lines.len(), "lines") };
    p.text(pos2(r.min.x + 4.0, r.min.y + 26.0), Align2::LEFT_CENTER, format!("{count} {what}"), FontId::proportional(11.0), t.text_faint);
}

fn hover_text(p: &Piece) -> String {
    let mut s = format!("{} · {} – {}", p.label, fmt_time(p.start), fmt_time(p.end));
    if let Some(l) = &p.line {
        s.push_str(&format!("\n\"{}\"", l.words.chars().take(120).collect::<String>()));
    } else if !p.title.is_empty() {
        s.push_str(&format!("\n{}", p.title));
    }
    s.push_str(&format!("\n{}", p.state.words()));
    if !p.error.is_empty() {
        s.push_str(&format!(": {}", p.error));
    }
    s
}

fn clip(p: &egui::Painter, ctx: &MfxCtx, piece: &Piece, r: Rect, selected: bool, hovered: bool) {
    let t = ctx.theme;
    let base = match piece.kind {
        Kind::Cue => piece.colour.unwrap_or(t.hover),
        Kind::Line => t.hover.lerp_to_gamma(t.text_faint, 0.35),
        Kind::Shot => t.hover,
    };
    let fill = if piece.state == PieceState::Cut { base.gamma_multiply(0.35) } else { base };
    p.rect_filled(r, 3.0, fill);
    if piece.kind != Kind::Cue
        && let Some(c) = piece.colour
    {
        // the beat's colour as a strip (lines and shots), the number in the text
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width(), 3.0)), 0.0, c);
    }
    if matches!(piece.state, PieceState::Making | PieceState::Planned) {
        stripes(p, r, t.text_faint.gamma_multiply(0.35));
    }
    if piece.state == PieceState::Failed {
        p.rect_stroke(r, 3.0, Stroke::new(1.5, t.error), StrokeKind::Inside);
    }
    if selected {
        p.rect_stroke(r, 3.0, Stroke::new(2.0, t.accent), StrokeKind::Inside);
    } else if hovered {
        p.rect_stroke(r, 3.0, Stroke::new(1.0, t.text_dim), StrokeKind::Inside);
    }
    let text = if piece.state == PieceState::Failed { format!("! couldn't make it {}", piece.label) } else { piece.clip_text() };
    let col = if piece.kind == Kind::Cue { CLIP_TEXT } else { t.text };
    let tp = p.with_clip_rect(r.shrink(2.0));
    tp.text(pos2(r.min.x + 5.0, r.center().y), Align2::LEFT_CENTER, text, FontId::proportional(11.0), col);
}

/// Diagonal stripes: a piece not made yet.
fn stripes(p: &egui::Painter, r: Rect, col: Color32) {
    let sp = p.with_clip_rect(r);
    let mut x = r.min.x - r.height();
    let mut shapes = Vec::new();
    while x < r.max.x {
        shapes.push(Shape::line_segment([Pos2::new(x, r.max.y), Pos2::new(x + r.height(), r.min.y)], Stroke::new(1.0, col)));
        x += 7.0;
        if shapes.len() > 4000 {
            break;
        }
    }
    sp.extend(shapes);
}

/// Seconds between ruler marks for a visible span.
fn tick_step(visible: f64) -> f64 {
    [1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0].into_iter().find(|s| visible / s <= 12.0).unwrap_or(1200.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ruler_steps_stay_readable() {
        assert_eq!(tick_step(9.0), 1.0);
        assert_eq!(tick_step(90.0), 10.0);
        assert_eq!(tick_step(3600.0), 300.0);
        assert_eq!(tick_step(1e9), 1200.0);
    }
}
