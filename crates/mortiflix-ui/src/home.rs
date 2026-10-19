//! Home's packs (owner, 2026-10-19: "call them packs"): every pack installed in the studio's `packs/`
//! folder, in the owner's categories, with their pictures. The folder is read again whenever Home
//! is showing (every [`REFRESH_SECS`]), so a pack added on disk appears without a restart.
//!
//! Data: `packs.list {}` (every kind; protocol: `docs/mortiflix/protocol.md`).

use std::path::PathBuf;

use egui::{Color32, FontId, Rect, RichText, Sense, Stroke, StrokeKind, pos2, vec2};
use serde_json::{Value, json};

use crate::MfxCtx;
use crate::nodes::stills::Stills;

/// The owner's categories, in their order. "All" shows every pack.
pub const CATEGORIES: [&str; 10] = ["All", "Transitions", "Motion", "Design", "Overlays", "Textures", "Backgrounds", "Captions", "Data", "Sound"];
/// How often Home reads the packs folder again while it is showing.
pub const REFRESH_SECS: f64 = 10.0;
const MAX_PACKS: usize = 2000;
const CARD_W: f32 = 220.0;
const CARD_PIC_H: f32 = 124.0;
const CARD_H: f32 = CARD_PIC_H + 46.0;
const GAP: f32 = 14.0;

/// One pack (a pack) as `packs.list` sends it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pack {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub description: String,
    pub tags: Vec<String>,
    pub pictures: Vec<PathBuf>,
}

impl Pack {
    /// The owner's category for this pack: from its kind (the folder it is in under `packs/`).
    pub fn category(&self) -> &'static str {
        category_of(&self.kind)
    }
}

/// A pack kind's category; "" for a kind none of them fits (it shows under All only).
pub fn category_of(kind: &str) -> &'static str {
    match kind.to_ascii_lowercase().trim_end_matches('s') {
        "transition" => "Transitions",
        "motion" | "rule" => "Motion",
        "style" | "design" | "look" => "Design",
        "overlay" => "Overlays",
        "texture" => "Textures",
        "background" => "Backgrounds",
        "caption" | "title" => "Captions",
        "data" | "chart" => "Data",
        "sound" | "music" | "voice" | "sfx" => "Sound",
        _ => "",
    }
}

/// Home's state: the packs read last, the category picked and the pack opened.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HomeState {
    pub packs: Vec<Pack>,
    /// Index into [`CATEGORIES`].
    pub category: usize,
    /// The pack whose pictures are shown large.
    pub open: Option<String>,
    pub loaded: bool,
    pub error: String,
    req: Option<u64>,
    asked_at: Option<f64>,
}

impl HomeState {
    /// The packs in the picked category, in the order the folder gave them.
    pub fn shown(&self) -> Vec<&Pack> {
        let cat = CATEGORIES.get(self.category).copied().unwrap_or("All");
        self.packs.iter().filter(|t| cat == "All" || t.category() == cat).collect()
    }
    /// How many packs a category has.
    pub fn count(&self, cat: &str) -> usize {
        if cat == "All" { self.packs.len() } else { self.packs.iter().filter(|t| t.category() == cat).count() }
    }
}

fn text(v: &Value, k: &str, n: usize) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").chars().take(n).collect()
}

pub fn parse(v: &Value) -> Vec<Pack> {
    v.as_array()
        .into_iter()
        .flatten()
        .take(MAX_PACKS)
        .filter_map(|p| {
            let id = text(p, "id", 120);
            if id.is_empty() {
                return None;
            }
            let strs = |k: &str, n: usize| -> Vec<String> {
                p.get(k)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().filter_map(Value::as_str).take(n).map(|s| s.chars().take(400).collect()).collect())
                    .unwrap_or_default()
            };
            let name = text(p, "name", 120);
            Some(Pack {
                name: if name.is_empty() { id.clone() } else { name },
                id,
                kind: text(p, "kind", 40),
                description: text(p, "description", 600),
                tags: strs("tags", 12),
                pictures: strs("pictures", 12).into_iter().map(PathBuf::from).filter(|p| p.is_absolute()).collect(),
            })
        })
        .collect()
}

/// Ask for the packs (first time, then every [`REFRESH_SECS`] while Home shows) and take the reply.
pub fn poll(ctx: &mut MfxCtx, now: f64) {
    let h = &mut ctx.state.home;
    if let Some(id) = h.req
        && let Some(r) = ctx.service.take_reply(id)
    {
        h.req = None;
        if r.ok {
            h.packs = parse(&r.result);
            h.error.clear();
        } else {
            h.error = r.error.get("message").and_then(Value::as_str).unwrap_or("the studio said no").chars().take(300).collect();
        }
        h.loaded = true;
    }
    let due = h.asked_at.is_none_or(|t| now - t >= REFRESH_SECS);
    if h.req.is_none() && due && ctx.service.status().is_connected() {
        h.asked_at = Some(now);
        h.req = Some(ctx.service.request("packs.list", json!({})));
    }
}

/// The packs area in `rect`: the category chips, the opened pack's pictures, then every
/// pack of the category as a card with its first picture.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx, rect: Rect) {
    let now = ui.input(|i| i.time);
    poll(ctx, now);
    if ctx.state.home.req.is_some() || ctx.state.home.asked_at.is_some() {
        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(if ctx.state.home.loaded { REFRESH_SECS } else { 0.2 }));
    }
    let t = ctx.theme;
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
    let ui = &mut child;
    ui.set_clip_rect(rect.intersect(ui.clip_rect()));
    // the chips
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
        for (i, cat) in CATEGORIES.iter().enumerate() {
            let on = ctx.state.home.category == i;
            let n = ctx.state.home.count(cat);
            let label = if ctx.state.home.loaded { format!("{cat} {n}") } else { (*cat).to_string() };
            let r = ui.add(
                egui::Button::new(RichText::new(&label).size(13.0).color(if on { t.text } else { t.text_dim }))
                    .min_size(vec2(0.0, 28.0))
                    .fill(if on { t.hover } else { t.panel_bg })
                    .stroke(Stroke::new(1.0, if on { t.text_dim } else { t.separator.gamma_multiply(2.0) }))
                    .corner_radius(14.0),
            );
            if r.clicked() {
                ctx.state.home.category = i;
                ctx.state.home.open = None;
            }
            ctx.auto(&format!("mfx.home.category.{}", cat.to_ascii_lowercase()), &r, cat);
        }
    });
    ui.add_space(12.0);
    let h = &ctx.state.home;
    if !h.loaded {
        ui.label(
            RichText::new(if ctx.service.status().is_connected() { "Reading the packs…" } else { "The packs show once Mortiflix is running." })
                .size(13.0)
                .color(t.text_faint),
        );
        return;
    }
    if !h.error.is_empty() {
        ui.label(RichText::new(format!("Couldn't read the packs: {}", h.error)).size(13.0).color(t.text_dim));
        return;
    }
    let shown: Vec<Pack> = h.shown().into_iter().cloned().collect();
    let open = h.open.clone().and_then(|id| shown.iter().find(|x| x.id == id).cloned());
    if shown.is_empty() {
        let cat = CATEGORIES.get(h.category).copied().unwrap_or("All");
        let words = if cat == "All" { "No packs installed yet.".to_string() } else { format!("No {} packs installed yet.", cat.to_lowercase()) };
        ui.label(RichText::new(words).size(13.0).color(t.text_faint));
        return;
    }
    egui::ScrollArea::vertical().id_salt("mfx-home-packs").auto_shrink([false, false]).show(ui, |ui| {
        if let Some(tpl) = &open {
            opened(ui, ctx, tpl);
            ui.add_space(16.0);
        }
        let cols = (((ui.available_width() + GAP) / (CARD_W + GAP)).floor() as usize).max(1);
        for row in shown.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
                for tpl in row {
                    card(ui, ctx, tpl);
                }
            });
            ui.add_space(GAP);
        }
    });
}

fn crop_into(ui: &egui::Ui, r: Rect, path: Option<&PathBuf>, empty: Color32) {
    let p = ui.painter();
    p.rect_filled(r, 4.0, empty);
    let Some(path) = path else {
        // a pack whose folder has no preview picture
        p.text(r.center(), egui::Align2::CENTER_CENTER, "No picture", FontId::proportional(12.0), Color32::from_gray(110));
        return;
    };
    let stills = Stills::large(ui.ctx());
    stills.poll(ui.ctx());
    match stills.get(path) {
        Some(tex) => {
            let [tw, th] = tex.size().map(|x| x.max(1) as f32);
            let (fw, fh) = (r.width().max(1.0), r.height().max(1.0));
            let (sx, sy) = if tw / th > fw / fh { ((fw / fh) / (tw / th), 1.0) } else { (1.0, (tw / th) / (fw / fh)) };
            let uv = Rect::from_min_max(pos2((1.0 - sx) / 2.0, (1.0 - sy) / 2.0), pos2((1.0 + sx) / 2.0, (1.0 + sy) / 2.0));
            p.image(tex.id(), r, uv, Color32::WHITE);
        }
        None => ui.ctx().request_repaint_after(std::time::Duration::from_millis(200)),
    }
}

/// One pack: its first picture, its name and category; a click shows all its pictures large.
fn card(ui: &mut egui::Ui, ctx: &mut MfxCtx, tpl: &Pack) {
    let t = ctx.theme;
    let (r, resp) = ui.allocate_exact_size(vec2(CARD_W, CARD_H), Sense::click());
    let resp = resp.on_hover_text(if tpl.description.is_empty() { tpl.name.clone() } else { tpl.description.clone() });
    let pic = Rect::from_min_size(r.min, vec2(CARD_W, CARD_PIC_H));
    crop_into(ui, pic, tpl.pictures.first(), t.field_bg);
    let on = ctx.state.home.open.as_deref() == Some(tpl.id.as_str());
    if on {
        ui.painter().rect_stroke(pic.expand(2.0), 6.0, Stroke::new(2.0, t.text), StrokeKind::Outside);
    } else if resp.hovered() {
        ui.painter().rect_stroke(pic.expand(2.0), 6.0, Stroke::new(1.0, t.text_faint), StrokeKind::Outside);
    }
    let p = ui.painter().with_clip_rect(r);
    p.text(pos2(r.min.x, pic.max.y + 14.0), egui::Align2::LEFT_CENTER, &tpl.name, FontId::proportional(13.0), t.text);
    let sub = match (tpl.category(), tpl.tags.first()) {
        ("", Some(tag)) => tag.clone(),
        (c, Some(tag)) => format!("{c} · {tag}"),
        (c, None) => c.to_string(),
    };
    p.text(pos2(r.min.x, pic.max.y + 32.0), egui::Align2::LEFT_CENTER, sub, FontId::proportional(11.5), t.text_faint);
    if resp.clicked() {
        ctx.state.home.open = if on { None } else { Some(tpl.id.clone()) };
    }
    ctx.auto(&format!("mfx.home.pack.{}", tpl.id), &resp, &tpl.name);
}

/// The opened pack: its name, words and tags, and every picture large.
fn opened(ui: &mut egui::Ui, ctx: &mut MfxCtx, tpl: &Pack) {
    let t = ctx.theme;
    ui.horizontal(|ui| {
        ui.label(RichText::new(&tpl.name).size(15.0).color(t.text).strong());
        let c = tpl.category();
        if !c.is_empty() {
            ui.label(RichText::new(c).size(12.0).color(t.text_faint));
        }
        let r = ui.button(RichText::new("Close").size(12.0));
        if r.clicked() {
            ctx.state.home.open = None;
        }
        ctx.auto("mfx.home.pack.close", &r, "Close");
    });
    if !tpl.description.is_empty() {
        ui.add(egui::Label::new(RichText::new(&tpl.description).size(12.0).color(t.text_dim)).wrap());
    }
    if !tpl.tags.is_empty() {
        ui.label(RichText::new(tpl.tags.join(" · ")).size(11.5).color(t.text_faint));
    }
    ui.add_space(8.0);
    let n = tpl.pictures.len().clamp(1, 3);
    let w = ((ui.available_width() - GAP * (n as f32 - 1.0)) / n as f32).clamp(120.0, 384.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = vec2(GAP, 0.0);
        for i in 0..n {
            let (r, _) = ui.allocate_exact_size(vec2(w, w * 10.0 / 16.0), Sense::hover());
            crop_into(ui, r, tpl.pictures.get(i), t.field_bg);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_land_in_the_owners_categories() {
        let v = json!([
            {"id": "agenko", "kind": "style", "name": "Agenko", "tags": ["agency", "dark"], "pictures": ["/p/agenko/preview-1.jpg", "relative.jpg"]},
            {"id": "swipes", "kind": "transition", "name": "Swipes"},
            {"id": "grain", "kind": "textures"},
            {"id": "odd", "kind": "something-new", "name": "Odd"},
            {"kind": "style", "name": "no id"},
        ]);
        let list = parse(&v);
        assert_eq!(list.len(), 4);
        assert_eq!(list[0].pictures, vec![PathBuf::from("/p/agenko/preview-1.jpg")], "only absolute paths");
        assert_eq!(list[2].name, "grain", "no name: the id");
        let mut h = HomeState { packs: list, loaded: true, ..HomeState::default() };
        assert_eq!(h.count("All"), 4);
        assert_eq!(h.count("Design"), 1);
        assert_eq!(h.count("Transitions"), 1);
        assert_eq!(h.count("Textures"), 1);
        assert_eq!(h.count("Sound"), 0);
        h.category = CATEGORIES.iter().position(|c| *c == "Design").unwrap_or(0);
        assert_eq!(h.shown().iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["agenko"]);
        h.category = 0;
        assert!(h.shown().iter().any(|x| x.id == "odd"), "an unknown kind still shows under All");
        h.category = 99;
        assert_eq!(h.shown().len(), 4, "a bad index is All");
        assert!(parse(&json!({"not": "a list"})).is_empty());
    }

    #[test]
    fn kinds_map_to_categories() {
        for (k, c) in [
            ("style", "Design"),
            ("transition", "Transitions"),
            ("motion", "Motion"),
            ("rule", "Motion"),
            ("music", "Sound"),
            ("voice", "Sound"),
            ("Overlays", "Overlays"),
            ("background", "Backgrounds"),
            ("captions", "Captions"),
            ("data", "Data"),
            ("xyz", ""),
        ] {
            assert_eq!(category_of(k), c, "{k}");
        }
    }
}
