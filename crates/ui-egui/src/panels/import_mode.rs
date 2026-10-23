//! Import mode (header "Import", also Home), Mortiflix's home (owner, 2026-10-19): New video, Open
//! project, Import files and Get packs; the Mortiflix community links; every installed pack by
//! category; the recent projects down the right.

use egui::{Align2, Color32, Rect, Sense, pos2, vec2};
use serde_json::json;

use crate::StudioApp;
use crate::theme::Tokens;

/// Home's Recent column, on the right edge (owner, 2026-10-19: "a long list down the screen").
const RECENT_W: f32 = 420.0;

pub fn show(app: &mut StudioApp, ui: &mut egui::Ui, rect: Rect) {
    let t = app.tokens;
    ui.painter().rect_filled(rect, t.radius, t.panel_bg);
    // Recent down the right; the cards, community and packs on the left
    let recent_w = RECENT_W.min((rect.width() * 0.35).max(220.0));
    let left = Rect::from_min_max(rect.min, pos2((rect.max.x - recent_w - 24.0).max(rect.min.x + 200.0), rect.max.y));
    let right = Rect::from_min_max(pos2(left.max.x + 8.0, rect.min.y), rect.max);
    if mortiflix_ui::staged() {
        recent_apps(app, ui, right);
    } else {
        recent(app, ui, right);
    }
    let title = if mortiflix_ui::staged() { mortiflix_ui::app_name() } else { tl!("Mortiflix Studio") };
    ui.painter().text(left.min + vec2(24.0, 30.0), Align2::LEFT_CENTER, title, Tokens::semibold(22.0), t.text);
    ui.painter().text(
        left.min + vec2(24.0, 58.0),
        Align2::LEFT_CENTER,
        // the app studio (mortiflix_ui::apps): English only for now
        if mortiflix_ui::midi() {
            "Make a song from an idea, stage by stage, and take it home as MIDI."
        } else if mortiflix_ui::staged() {
            "Plan an app from an idea, step by step, then the AI builds it."
        } else {
            tl!("Make a video from an idea, open one you made, or start from a pack.")
        },
        Tokens::ui(13.0),
        t.text_dim,
    );
    let ctx = ui.ctx().clone();
    let cards = [
        (
            match mortiflix_ui::product() {
                mortiflix_ui::Product::Apps => "New app",
                mortiflix_ui::Product::Midi => "New song",
                mortiflix_ui::Product::Video => tl!("New video"),
            },
            "mortiflix.newProject",
            tl!("Start from an idea"),
            if mortiflix_ui::staged() { crate::icons::Icon::NewItem } else { crate::icons::Icon::Film },
        ),
        (tl!("Open project"), "file.open", tl!("Pick a saved .mortiflix"), crate::icons::Icon::Folder),
        (tl!("Import files"), "file.import", tl!("Movies, audio and stills"), crate::icons::Icon::Sequence),
        (tl!("Get packs"), "", tl!("Browse what's installed below"), crate::icons::Icon::Grid),
    ];
    // the app studio opens no video project, imports no media and has no packs: New app only
    let cards = if mortiflix_ui::staged() { &cards[..1] } else { &cards[..] };
    let cw = ((left.width() - 48.0 - 3.0 * 16.0) / 4.0).clamp(150.0, 260.0);
    for (i, (title, cmd, sub, icon)) in cards.iter().enumerate() {
        let r = Rect::from_min_size(left.min + vec2(24.0 + i as f32 * (cw + 16.0), 90.0), vec2(cw, 150.0));
        let key = if cmd.is_empty() { "packs" } else { *cmd };
        let resp = ui.interact(r, egui::Id::new(("imp", key)), Sense::click());
        app.auto.add(&format!("import.{key}"), r, title);
        ui.painter().rect_filled(r, 8.0, if resp.hovered() { t.hover } else { t.tl_header_bg });
        let p = ui.painter().with_clip_rect(r);
        p.text(r.min + vec2(16.0, 110.0), Align2::LEFT_CENTER, *title, Tokens::semibold(14.0), t.text);
        p.text(r.min + vec2(16.0, 132.0), Align2::LEFT_CENTER, *sub, Tokens::ui(11.5), t.text_dim);
        crate::icons::paint(ui.painter(), Rect::from_min_size(r.min + vec2(16.0, 18.0), vec2(56.0, 56.0)), *icon, t.accent);
        if resp.clicked() {
            if cmd.is_empty() {
                // Get packs: every installed pack, from the top
                app.mfx.core.state.home.category = 0;
                app.mfx.core.state.home.open = None;
            } else {
                let _ = crate::menus::invoke(app, &ctx, cmd, json!({}));
                if *cmd != "mortiflix.newProject" {
                    app.ui.mode = crate::state::Mode::Edit;
                }
            }
        }
    }
    // Community: the Mortiflix Discord and mortiflix.com
    let y = 90.0 + 150.0 + 30.0;
    ui.painter().text(left.min + vec2(24.0, y), Align2::LEFT_CENTER, tl!("Community"), Tokens::semibold(15.0), t.text);
    let mut x = left.min.x + 24.0;
    for (id, label, url) in crate::links::ALL.iter().filter(|(id, _, _)| matches!(*id, "help.discord" | "help.appPage")) {
        let primary = *id == "help.discord";
        let label = if primary { tl!("Join us on Discord") } else { crate::i18n::t(label) };
        let g = ui.painter().layout_no_wrap(label.to_string(), Tokens::ui(13.0), t.text);
        let r = Rect::from_min_size(pos2(x, left.min.y + y + 18.0), vec2(g.size().x + 44.0, 34.0));
        let resp = ui.interact(r, egui::Id::new(("imp-link", *id)), Sense::click()).on_hover_text(*url);
        app.auto.add(&format!("import.link.{}", id.trim_start_matches("help.")), r, label);
        let (bg, fg) = if primary {
            (if resp.hovered() { t.accent_hover } else { t.accent }, Color32::WHITE)
        } else {
            (if resp.hovered() { t.hover } else { t.tl_header_bg }, t.text)
        };
        ui.painter().rect_filled(r, 8.0, bg);
        let icon = if primary { crate::icons::Icon::Chat } else { crate::icons::Icon::Globe };
        crate::icons::paint(ui.painter(), Rect::from_center_size(pos2(r.min.x + 20.0, r.center().y), vec2(16.0, 16.0)), icon, fg);
        ui.painter().galley(pos2(r.min.x + 34.0, r.center().y - g.size().y / 2.0), g, fg);
        if resp.clicked() {
            crate::links::open(app, &ctx, url);
        }
        x = r.max.x + 12.0;
    }
    if mortiflix_ui::staged() {
        return;
    }
    // Packs: every pack installed in the studio, in its category
    let ty = y + 18.0 + 34.0 + 34.0;
    ui.painter().text(left.min + vec2(24.0, ty), Align2::LEFT_CENTER, tl!("Packs"), Tokens::semibold(15.0), t.text);
    let area = Rect::from_min_max(left.min + vec2(24.0, ty + 18.0), pos2(left.max.x - 8.0, left.max.y - 16.0));
    if area.height() > 60.0 && area.width() > 100.0 {
        crate::mortiflix::home_packs(app, ui, area);
    }
}

const RECENT_ROW_H: f32 = 28.0;

/// The app studio's Recent column: the user's app projects (the chat's list), newest first; a click opens one.
fn recent_apps(app: &mut StudioApp, ui: &mut egui::Ui, col: Rect) {
    let t = app.tokens;
    ui.painter().text(col.min + vec2(16.0, 30.0), Align2::LEFT_CENTER, "Recent", Tokens::semibold(15.0), t.text);
    let row_h = RECENT_ROW_H + 14.0;
    let fit = (((col.height() - 64.0) / row_h).floor().max(0.0)) as usize;
    let projects: Vec<(String, String, String)> = mortiflix_ui::chat::listed_projects(ui.ctx()).into_iter().take(fit).collect();
    if projects.is_empty() {
        ui.painter().text(col.min + vec2(16.0, 58.0), Align2::LEFT_CENTER, if mortiflix_ui::midi() { "Songs you start show here." } else { "Apps you start show here." }, Tokens::ui(12.0), t.text_faint);
        return;
    }
    let mut open = None;
    for (i, (id, title, at)) in projects.iter().enumerate() {
        let row = Rect::from_min_size(col.min + vec2(8.0, 48.0 + i as f32 * row_h), vec2((col.width() - 24.0).max(0.0), row_h));
        let resp = ui.interact(row, egui::Id::new(("imp-recent-app", i)), Sense::click());
        app.auto.add(&format!("import.recent.{i}"), row, title);
        if resp.hovered() {
            ui.painter().rect_filled(row, 4.0, t.hover);
        }
        let p = ui.painter().with_clip_rect(row);
        crate::icons::paint(&p, Rect::from_center_size(pos2(row.min.x + 16.0, row.min.y + 14.0), vec2(14.0, 14.0)), crate::icons::Icon::Folder, t.text_dim);
        p.text(pos2(row.min.x + 32.0, row.min.y + 14.0), Align2::LEFT_CENTER, title, Tokens::ui(13.0), t.text);
        p.text(pos2(row.min.x + 32.0, row.min.y + 30.0), Align2::LEFT_CENTER, at.get(..10).unwrap_or(""), Tokens::ui(11.0), t.text_faint);
        if resp.clicked() {
            open = Some(id.clone());
        }
    }
    if let Some(id) = open {
        let ctx = ui.ctx().clone();
        let _ = crate::menus::invoke(app, &ctx, "mortiflix.request", json!({"type": "project.open", "params": {"id": id}}));
        app.ui.mode = crate::state::Mode::Edit;
    }
}

/// Home's Recent column in `col`: the recent projects (newest first, file name with its folder under
/// it), as many as fit down the screen; a click opens one.
fn recent(app: &mut StudioApp, ui: &mut egui::Ui, col: Rect) {
    let t = app.tokens;
    ui.painter().text(col.min + vec2(16.0, 30.0), Align2::LEFT_CENTER, tl!("Recent"), Tokens::semibold(15.0), t.text);
    let row_h = RECENT_ROW_H + 14.0;
    let fit = (((col.height() - 64.0) / row_h).floor().max(0.0)) as usize;
    let paths: Vec<String> = app.session.prefs.general.recent_projects.iter().take(fit).cloned().collect();
    if paths.is_empty() {
        ui.painter().text(col.min + vec2(16.0, 58.0), Align2::LEFT_CENTER, tl!("Projects you open show here."), Tokens::ui(12.0), t.text_faint);
        return;
    }
    let mut open = None;
    for (i, path) in paths.iter().enumerate() {
        let row = Rect::from_min_size(col.min + vec2(8.0, 48.0 + i as f32 * row_h), vec2((col.width() - 24.0).max(0.0), row_h));
        let name = crate::menus::recent_name(path);
        let resp = ui.interact(row, egui::Id::new(("imp-recent", i)), Sense::click()).on_hover_text(path.as_str());
        app.auto.add(&format!("import.recent.{i}"), row, &name);
        if resp.hovered() {
            ui.painter().rect_filled(row, 4.0, t.hover);
        }
        let p = ui.painter().with_clip_rect(row);
        crate::icons::paint(&p, Rect::from_center_size(pos2(row.min.x + 16.0, row.min.y + 14.0), vec2(14.0, 14.0)), crate::icons::Icon::Sequence, t.text_dim);
        p.text(pos2(row.min.x + 32.0, row.min.y + 14.0), Align2::LEFT_CENTER, &name, Tokens::ui(13.0), t.text);
        if let Some(folder) = std::path::Path::new(path).parent() {
            p.text(pos2(row.min.x + 32.0, row.min.y + 30.0), Align2::LEFT_CENTER, folder.to_string_lossy(), Tokens::ui(11.0), t.text_faint);
        }
        if resp.clicked() {
            open = Some(path.clone());
        }
    }
    if let Some(path) = open {
        let ctx = ui.ctx().clone();
        if crate::menus::invoke(app, &ctx, "file.open", json!({"path": path})).is_ok() {
            app.ui.mode = crate::state::Mode::Edit;
        }
    }
}
