//! The stage bar (doc 30, mockup `UI Layout/30b Stage bar.html`): one 44 px row under Mortiflix Studio's
//! header on every project screen. `‹ Back: <stage>` · six stages with their status · one hint ·
//! `Next: <stage> ›`. On 4 Voice & music the Next slot holds Edit the video + Get the video (owner
//! answer 1). Every click is a command (`stages.*`), so menus, shortcuts and agents do the same.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, pos2, vec2};
use serde_json::json;

use crate::state::{StageStatus, StatusKind};
use crate::{MfxCtx, STAGE_RENDER, STAGE_VOICE_MUSIC, STAGES, stage_name};

const BACK_W: f32 = 176.0;
const STAGE_W: f32 = 168.0;
const NEXT_W: f32 = 232.0;
const PAIR_W: f32 = 300.0;
const MIN_HINT_W: f32 = 140.0;

/// The hint sentence for a stage (doc 30: what to do here).
pub fn hint(ctx: &MfxCtx) -> String {
    if ctx.state.looking_back() {
        return "You're looking back. A change here only remakes what it touches.".into();
    }
    match ctx.state.stage {
        0 => "Click any line to change it, Save, then press Next.",
        1 => "Pick the look you like, then press Next.",
        2 => "Check each shot: OK, Fix or Cut it.",
        3 => "Listen with the shots. Get the video, or edit it in Edit first.",
        4 => "Make any final edits, then press Next.",
        _ => "Pick where the video goes and render it.",
    }
    .into()
}

/// Draw the bar in `rect` (the host reserves [`crate::STAGE_BAR_HEIGHT`] under its header).
pub fn show(ui: &mut egui::Ui, rect: Rect, ctx: &mut MfxCtx) {
    // a project just opened: go to the stage it was last looked at
    if let Some(stage) = ctx.state.open_stage.take()
        && stage != ctx.state.stage
    {
        ctx.command("stages.set", serde_json::json!({"stage": stage, "force": true}));
    }
    let t = ctx.theme;
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0.0, t.header_bg);
    p.line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, Color32::BLACK));
    let stage = ctx.state.stage;
    let pair = stage == STAGE_VOICE_MUSIC;
    // Widths: stages never change width with their status (nothing shifts); narrow windows scale
    // everything down together and drop the hint first.
    let right_w = if pair { PAIR_W } else { NEXT_W };
    let full = BACK_W + STAGE_W * 6.0 + right_w;
    let scale = if rect.width() < full { (rect.width() / full).max(0.3) } else { 1.0 };
    let (back_w, stage_w, right_w) = (BACK_W * scale, STAGE_W * scale, right_w * scale);

    // ‹ Back
    let back = Rect::from_min_size(rect.min, vec2(back_w, rect.height()));
    let back_label = if stage == 0 { "Back: New project".to_string() } else { format!("Back: {}", stage_name(stage.saturating_sub(1))) };
    let r = ui.interact(back, egui::Id::new("mfx-stagebar-back"), Sense::click()).on_hover_text("Back (Ctrl+Alt+Page Up)");
    if r.hovered() {
        p.rect_filled(back.shrink(4.0), 4.0, t.hover);
    }
    chevron(&p, pos2(back.min.x + 16.0, back.center().y), -1.0, t.text_dim);
    p.text(
        pos2(back.min.x + 26.0, back.center().y),
        Align2::LEFT_CENTER,
        &back_label,
        FontId::proportional(13.0),
        if r.hovered() { t.text } else { t.text_dim },
    );
    if r.clicked() {
        ctx.command("stages.back", json!({}));
    }
    ctx.auto("mfx.stagebar.back", &r, &back_label);

    // The six stages
    let mut x = back.max.x;
    for (i, (name, key)) in STAGES.iter().enumerate() {
        let Ok(i) = u8::try_from(i) else { break };
        let tab = Rect::from_min_size(pos2(x, rect.min.y), vec2(stage_w, rect.height()));
        x = tab.max.x;
        let here = i == stage;
        let resp = ui.interact(tab, egui::Id::new(("mfx-stagebar-stage", i)), Sense::click()).on_hover_text(format!("{} {name} (Ctrl+Alt+{})", i + 1, i + 1));
        if resp.hovered() && !here {
            p.rect_filled(tab.shrink2(vec2(2.0, 4.0)), 4.0, t.hover);
        }
        let tp = p.with_clip_rect(tab.shrink2(vec2(4.0, 0.0)));
        if i > 0 {
            chevron(&p, pos2(tab.min.x + 6.0, tab.min.y + 14.0), 1.0, t.text_faint);
        }
        let name_col = if here {
            t.tab_text_active
        } else if resp.hovered() {
            t.text
        } else {
            t.tab_text
        };
        let title = format!("{} {name}", i + 1);
        let g = tp.layout_no_wrap(title, FontId::proportional(13.0), name_col);
        let name_pos = pos2(tab.min.x + 16.0, tab.min.y + 6.0);
        let name_w = g.size().x;
        tp.galley(name_pos, g, name_col);
        if here {
            tp.line_segment([pos2(name_pos.x, tab.max.y - 3.0), pos2(name_pos.x + name_w.max(40.0), tab.max.y - 3.0)], Stroke::new(2.0, t.tab_text_active));
        }
        let st = ctx.state.status(i);
        let status_y = tab.min.y + 30.0;
        // the stage the AI works in now: a turning spinner (never colour alone: its words beside it)
        if ctx.state.work.stage() == Some(i) {
            crate::work::spinner(&tp, pos2(tab.min.x + 21.0, status_y), 4.0, t.accent_hover, ui.input(|inp| inp.time));
            ui.ctx().request_repaint_after(crate::work::FRAME);
        } else {
            status_icon(&tp, pos2(tab.min.x + 21.0, status_y), &st, &t);
        }
        tp.text(pos2(tab.min.x + 30.0, status_y), Align2::LEFT_CENTER, st.line(), FontId::proportional(11.0), if here { t.text_dim } else { t.text_faint });
        if resp.clicked() {
            ctx.command(&format!("stages.{key}"), json!({}));
        }
        ctx.auto(&format!("mfx.stagebar.{key}"), &resp, name);
    }

    // Next (or Edit the video + Get the video on Voice & music)
    let right = Rect::from_min_max(pos2((rect.max.x - right_w).max(x), rect.min.y), rect.max);
    let ready = ctx.state.status(stage).ready();
    if pair {
        let half = (right.width() - 12.0) / 2.0;
        let edit = Rect::from_min_size(pos2(right.min.x + 4.0, rect.min.y + 7.0), vec2(half - 4.0, rect.height() - 14.0));
        let get = Rect::from_min_size(pos2(edit.max.x + 6.0, rect.min.y + 7.0), vec2(half, rect.height() - 14.0));
        if button(ui, ctx, edit, "Edit the video", false, "mfx.stagebar.editVideo").clicked() {
            ctx.command("stages.editVideo", json!({}));
        }
        if button(ui, ctx, get, "Get the video", true, "mfx.stagebar.getVideo").clicked() {
            ctx.command("stages.getVideo", json!({}));
        }
    } else {
        let next = Rect::from_min_size(pos2(right.min.x + 4.0, rect.min.y + 7.0), vec2(right.width() - 12.0, rect.height() - 14.0));
        let label = if stage >= STAGE_RENDER { "Render the video".to_string() } else { format!("Next: {}", stage_name(stage.saturating_add(1))) };
        let resp = button(ui, ctx, next, &label, ready, "mfx.stagebar.next");
        if stage < STAGE_RENDER {
            chevron(&p, pos2(next.max.x - 14.0, next.center().y), 1.0, if ready { Color32::WHITE } else { t.text_dim });
        }
        if resp.clicked() {
            ctx.command(if stage >= STAGE_RENDER { "mortiflix.renderVideo" } else { "stages.next" }, json!({}));
        }
    }

    // Hint between the stages and Next, when there is room; the AI stopped: Resume (or Link Claude
    // Code) at its end
    let mut hint_rect = Rect::from_min_max(pos2(x + 12.0, rect.min.y), pos2(right.min.x - 8.0, rect.max.y));
    // Resume first: the hint gives way (on Voice & music, with two buttons on the right, a 1600 px window had no
    // Resume in the bar at all; run 2)
    let w = if ctx.state.stop.as_ref().is_some_and(|s| s.login) { 230.0 } else { 72.0 };
    if ctx.state.stop.is_some() && hint_rect.width() >= w + 4.0 {
        let br = Rect::from_min_max(pos2(hint_rect.max.x - w, rect.min.y + 7.0), pos2(hint_rect.max.x, rect.max.y - 7.0));
        hint_rect.max.x = br.min.x - 8.0;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(br).layout(egui::Layout::left_to_right(egui::Align::Center)));
        crate::resume::bar(&mut child, ctx, "stagebar", true);
    }
    if hint_rect.width() >= MIN_HINT_W {
        let text = if let Some(stop) = &ctx.state.stop {
            // stopped: short here, the whole story in the chat
            if stop.login {
                "The AI stopped: Claude Code is signed out (its login expired).".to_string()
            } else {
                "The AI stopped and needs you: press Resume.".to_string()
            }
        } else if ctx.state.ai_line.is_empty() || ctx.state.looking_back() {
            hint(ctx)
        } else {
            format!("{} · {}", hint(ctx), ctx.state.ai_line)
        };
        let hp = p.with_clip_rect(hint_rect);
        // at most two lines in the 44 px bar, never cut off at the top
        let mut job = egui::text::LayoutJob::simple(text, FontId::proportional(12.0), t.text_dim, hint_rect.width());
        job.wrap.max_rows = 2;
        job.wrap.overflow_character = Some('…');
        let g = hp.layout_job(job);
        let y = hint_rect.center().y - g.size().y / 2.0;
        hp.galley(pos2(hint_rect.min.x, y), g, t.text_dim);
    }

    confirm_popup(ui, ctx);
    crate::script::leave_popup(ui, ctx);
    crate::look::popups(ui, ctx);
}

/// A stage-bar button: blue (the one accent) when `primary`, else grey. It always works.
fn button(ui: &mut egui::Ui, ctx: &mut MfxCtx, r: Rect, label: &str, primary: bool, auto_id: &str) -> egui::Response {
    let t = ctx.theme;
    let resp = ui.interact(r, egui::Id::new(auto_id), Sense::click());
    let fill = match (primary, resp.hovered()) {
        (true, false) => t.accent,
        (true, true) => t.accent_hover,
        (false, false) => t.hover,
        (false, true) => t.separator.gamma_multiply(2.0),
    };
    ui.painter().rect_filled(r, 4.0, fill);
    let col = if primary { Color32::WHITE } else { t.text };
    ui.painter().with_clip_rect(r).text(r.center(), Align2::CENTER_CENTER, label, FontId::proportional(13.0), col);
    ctx.auto(auto_id, &resp, label);
    resp
}

/// A small `‹` (dir -1) or `›` (dir 1), drawn (no glyph needed).
fn chevron(p: &egui::Painter, c: Pos2, dir: f32, col: Color32) {
    let s = 3.5;
    p.line_segment([pos2(c.x - s * dir * 0.6, c.y - s), pos2(c.x + s * dir * 0.6, c.y)], Stroke::new(1.5, col));
    p.line_segment([pos2(c.x + s * dir * 0.6, c.y), pos2(c.x - s * dir * 0.6, c.y + s)], Stroke::new(1.5, col));
}

/// The status icon: never colour alone (doc 30), each kind has its own shape.
fn status_icon(p: &egui::Painter, c: Pos2, st: &StageStatus, t: &crate::MfxTheme) {
    match st.kind {
        StatusKind::NotStarted => {
            p.circle_stroke(c, 3.5, Stroke::new(1.2, t.text_faint));
        }
        StatusKind::Making => {
            p.circle_filled(c, 3.5, t.ok);
        }
        StatusKind::Ready => {
            p.add(egui::Shape::convex_polygon(vec![pos2(c.x - 3.0, c.y - 4.0), pos2(c.x + 4.0, c.y), pos2(c.x - 3.0, c.y + 4.0)], t.accent, Stroke::NONE));
        }
        StatusKind::Done => {
            let s = Stroke::new(1.6, t.ok);
            p.line_segment([pos2(c.x - 4.0, c.y), pos2(c.x - 1.0, c.y + 3.0)], s);
            p.line_segment([pos2(c.x - 1.0, c.y + 3.0), pos2(c.x + 4.5, c.y - 3.5)], s);
        }
        StatusKind::NeedsYou | StatusKind::Stopped => {
            let col = if st.kind == StatusKind::Stopped { t.error } else { t.warn };
            p.line_segment([pos2(c.x, c.y - 4.5), pos2(c.x, c.y + 1.5)], Stroke::new(1.8, col));
            p.circle_filled(pos2(c.x, c.y + 4.0), 1.0, col);
        }
    }
}

/// "2 still need you" (doc 30): Next asked first. Show them closes; Go on anyway moves on.
fn confirm_popup(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let Some(mut c) = ctx.state.confirm_next.clone() else { return };
    let now = ui.input(|i| i.time);
    if c.opened_at.is_none() {
        c.opened_at = Some(now);
        if let Some(open) = ctx.state.confirm_next.as_mut() {
            open.opened_at = Some(now);
        }
    }
    // the click that opened it (or one landing with it) never closes it: only a later click outside does
    let settled = c.opened_at.is_some_and(|t| now - t > 0.35);
    if !settled {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(120));
    }
    let what = if c.need == 1 { "1 piece still needs you".to_string() } else { format!("{} pieces still need you", c.need) };
    let modal = egui::Modal::new(egui::Id::new("mfx-confirm-next")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading(&what);
        ui.label("Going on keeps every piece as it is now.");
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let show = ui.button("Show them");
            ctx.auto("mfx.stagebar.confirm.show", &show, "Show them");
            if show.clicked() {
                ctx.state.confirm_next = None;
                ctx.command("mortiflix.showNeedsYou", json!({"stage": c.stage}));
            }
            let go = ui.add(egui::Button::new(egui::RichText::new("Go on anyway").color(Color32::WHITE)).fill(ctx.theme.accent));
            ctx.auto("mfx.stagebar.confirm.goOn", &go, "Go on anyway");
            if go.clicked() {
                ctx.state.confirm_next = None;
                ctx.command(if c.to_render { "stages.getVideo" } else { "stages.next" }, json!({"force": true}));
            }
        });
    });
    if modal.should_close() && settled {
        ctx.state.confirm_next = None;
    }
}
