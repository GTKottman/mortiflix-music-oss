//! The look menu (owner, 2026-10-18: "before generating the style frames, it should ask for a look.
//! And there should be a drop down for style packs the user has, if they have any. If they don't,
//! the user can pick a font, generate a color scheme, a vibe, a look"). Shown in the Style frames
//! centre before any frame exists (and from Look menu… after): a style pack, or a font, a colour
//! scheme (Generate makes one, every swatch can be changed), a vibe and a look, plus any words.
//! Make the style frames saves it (`look.brief.set`); the service holds the style frames until then.
//!
//! Data: `look.packs`, `look.fonts`, `look.brief.get`, `look.brief.set` (protocol: `docs/mortiflix/protocol.md`).

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, vec2};
use serde_json::{Value, json};

use crate::MfxCtx;

const VIBES: [&str; 10] = ["Calm", "Bold", "Playful", "Serious", "Warm", "Tense", "Dreamy", "Retro", "Urgent", "Hopeful"];
const LOOKS: [&str; 10] = ["Flat vector", "Paper cut-out", "Hand-drawn", "Photo collage", "3D clay", "Neon", "Editorial", "Minimal", "Risograph", "Blueprint"];
const SCHEMES: [&str; 4] = ["Analogous", "Complementary", "Triad", "One colour"];
/// Most fonts listed in the font box at once (the filter narrows the rest).
const FONT_ROWS: usize = 200;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pack {
    pub id: String,
    pub name: String,
    pub fonts: Vec<String>,
    pub palette: Vec<Color32>,
    pub vibe: String,
    pub look: String,
    pub words: String,
    pub pictures: usize,
}

/// The menu's state between frames (egui temporary data: the saved look is the service's).
#[derive(Clone, Debug, Default)]
pub struct Menu {
    pub packs: Vec<Pack>,
    pub fonts: Vec<String>,
    /// The chosen pack's id ("" = none: pick below).
    pub pack: String,
    pub font: String,
    pub font_filter: String,
    pub palette: Vec<Color32>,
    pub scheme: usize,
    pub vibe: String,
    pub look: String,
    pub words: String,
    /// A look is saved for this project (`look.brief.get` answered with one).
    pub saved: bool,
    pub loaded: bool,
    project: Option<String>,
    reqs: [Option<u64>; 3],
    save: Option<u64>,
    pub note: String,
    seed: u64,
}

fn key() -> egui::Id {
    egui::Id::new("mfx-look-menu")
}

fn text(v: &Value, k: &str, n: usize) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("").chars().take(n).collect()
}

fn hex(c: Color32) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r(), c.g(), c.b())
}

fn parse_hex(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    let [_, r, g, b] = u32::from_str_radix(h, 16).ok()?.to_be_bytes();
    Some(Color32::from_rgb(r, g, b))
}

fn colours(v: &Value) -> Vec<Color32> {
    v.as_array().into_iter().flatten().take(8).filter_map(Value::as_str).filter_map(parse_hex).collect()
}

pub fn parse_packs(v: &Value) -> Vec<Pack> {
    v.as_array()
        .into_iter()
        .flatten()
        .take(200)
        .filter_map(|p| {
            let id = text(p, "id", 120);
            (!id.is_empty()).then(|| Pack {
                name: text(p, "name", 80),
                fonts: p
                    .get("fonts")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .take(4)
                    .filter_map(Value::as_str)
                    .map(|s| s.chars().take(60).collect())
                    .collect(),
                palette: p.get("palette").map(colours).unwrap_or_default(),
                vibe: text(p, "vibe", 200),
                look: text(p, "look", 200),
                words: text(p, "words", 2000),
                pictures: p.get("pictures").and_then(Value::as_array).map_or(0, Vec::len),
                id,
            })
        })
        .collect()
}

/// HSL (h in turns, s and l 0-1) to a colour.
fn hsl(h: f32, s: f32, l: f32) -> Color32 {
    let h = h.rem_euclid(1.0) * 6.0;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u8 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to = |v: f32| ((v + m).clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(to(r), to(g), to(b))
}

/// A five-colour scheme from a seed: a dark ground, a light ground, the main colour and two that go
/// with it by the scheme's rule.
pub fn generate(seed: u64, scheme: usize) -> Vec<Color32> {
    // splitmix64: plenty for picking a hue
    let mut z = seed.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut next = || {
        z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut x = z;
        x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        ((x ^ (x >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    };
    let base = next();
    let sat = 0.55 + next() * 0.35;
    let (a, b) = match scheme {
        0 => (base + 0.08, base - 0.08),
        1 => (base + 0.5, base + 0.45),
        2 => (base + 1.0 / 3.0, base + 2.0 / 3.0),
        _ => (base, base),
    };
    let one = scheme >= 3;
    vec![
        hsl(base, sat * 0.35, 0.09),
        hsl(base, sat * 0.25, 0.93),
        hsl(base, sat, 0.52),
        hsl(a, if one { sat * 0.6 } else { sat * 0.85 }, if one { 0.72 } else { 0.6 }),
        hsl(b, if one { sat * 0.4 } else { sat * 0.75 }, if one { 0.32 } else { 0.45 }),
    ]
}

/// Keep the menu's data fresh; true once it has read the project's saved look (or found none).
fn sync(ui: &egui::Ui, ctx: &mut MfxCtx, m: &mut Menu) {
    if m.project != ctx.state.project_id {
        *m = Menu { project: ctx.state.project_id.clone(), seed: (ui.input(|i| i.time) * 1000.0) as u64, ..Menu::default() };
        m.palette = generate(m.seed, 0);
    }
    for (slot, req) in m.reqs.iter_mut().enumerate() {
        let Some(r) = req.and_then(|id| ctx.service.take_reply(id)) else { continue };
        *req = None;
        if !r.ok {
            if slot == 2 {
                m.loaded = true;
            }
            continue;
        }
        match slot {
            0 => m.packs = parse_packs(&r.result),
            1 => m.fonts = r.result.as_array().into_iter().flatten().take(4000).filter_map(Value::as_str).map(|s| s.chars().take(80).collect()).collect(),
            _ => {
                m.loaded = true;
                let b = &r.result;
                if b.is_object() {
                    m.saved = true;
                    m.pack = text(b, "pack", 120);
                    m.font = text(b, "font", 80);
                    let p = b.get("palette").map(colours).unwrap_or_default();
                    if !p.is_empty() {
                        m.palette = p;
                    }
                    m.vibe = text(b, "vibe", 200);
                    m.look = text(b, "look", 200);
                    m.words = text(b, "words", 2000);
                }
            }
        }
    }
    if let Some(id) = m.save
        && let Some(r) = ctx.service.take_reply(id)
    {
        m.save = None;
        if r.ok {
            m.saved = true;
            m.note = "Saved: the style frames are made in this look now.".into();
            ctx.state.look.show_menu = false;
        } else {
            m.note = format!("Couldn't save the look: {}", r.error.get("message").and_then(Value::as_str).unwrap_or("the studio said no"));
        }
    }
    if m.reqs.iter().all(Option::is_none) && !m.loaded && ctx.service.status().is_connected() && ctx.state.project_id.is_some() {
        m.reqs = [
            Some(ctx.service.request("look.packs", json!({}))),
            Some(ctx.service.request("look.fonts", json!({}))),
            Some(ctx.service.request("look.brief.get", json!({}))),
        ];
    }
}

/// Whether the Style frames centre shows the menu: asked for (Look menu…), or no frame made yet and
/// no look saved.
pub fn wanted(ui: &egui::Ui, ctx: &MfxCtx) -> bool {
    if ctx.state.look.show_menu {
        return true;
    }
    let m: Menu = ui.ctx().data(|d| d.get_temp::<Menu>(key())).unwrap_or_default();
    let none_made = ctx.state.look.view.loaded && ctx.state.look.view.frames.is_empty();
    none_made && !(m.loaded && m.saved && m.project == ctx.state.project_id)
}

/// The menu, in the Style frames centre.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let mut m: Menu = ui.ctx().data_mut(|d| d.get_temp::<Menu>(key())).unwrap_or_default();
    sync(ui, ctx, &mut m);
    let t = ctx.theme;
    ui.set_max_width(ui.available_width());
    ui.horizontal(|ui| {
        ui.label(RichText::new("Pick a look").size(15.0).color(t.text).strong());
        if ctx.state.look.show_menu && !ctx.state.look.view.frames.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let back = ui.button("Back to the style frames");
                ctx.auto("mfx.lookMenu.back", &back, "Back to the style frames");
                if back.clicked() {
                    ctx.state.look.show_menu = false;
                }
            });
        }
    });
    ui.add(
        egui::Label::new(
            RichText::new("The style frames are made from this. Nothing is drawn until you press Make the style frames.").size(12.0).color(t.text_dim),
        )
        .wrap(),
    );
    ui.add_space(10.0);
    if crate::service_notice(ui, ctx) {
        store(ui, m);
        return;
    }
    egui::ScrollArea::vertical().id_salt("mfx-look-menu").auto_shrink([false, false]).show(ui, |ui| {
        body(ui, ctx, &mut m);
    });
    store(ui, m);
}

fn store(ui: &egui::Ui, m: Menu) {
    ui.ctx().data_mut(|d| d.insert_temp(key(), m));
}

fn row_label(ui: &mut egui::Ui, ctx: &MfxCtx, words: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(words).size(13.0).color(ctx.theme.text).strong());
    ui.add_space(2.0);
}

fn body(ui: &mut egui::Ui, ctx: &mut MfxCtx, m: &mut Menu) {
    let t = ctx.theme;
    // Style pack
    row_label(ui, ctx, "Style pack");
    if m.packs.is_empty() {
        ui.label(RichText::new("You have no style packs yet: pick the look below.").size(12.0).color(t.text_faint));
    } else {
        let shown = m.packs.iter().find(|p| p.id == m.pack).map_or("None: I'll pick below".to_string(), |p| p.name.clone());
        let r = egui::ComboBox::from_id_salt("mfx-look-pack").width(320.0).selected_text(shown).show_ui(ui, |ui| {
            ui.selectable_value(&mut m.pack, String::new(), "None: I'll pick below");
            for p in &m.packs {
                ui.selectable_value(&mut m.pack, p.id.clone(), &p.name);
            }
        });
        ctx.auto("mfx.lookMenu.pack", &r.response, "Style pack");
        if let Some(p) = m.packs.iter().find(|p| p.id == m.pack).cloned() {
            ui.add_space(4.0);
            swatches(ui, &p.palette, t.separator);
            let mut bits = Vec::new();
            if !p.fonts.is_empty() {
                bits.push(p.fonts.join(", "));
            }
            bits.extend([p.vibe.clone(), p.look.clone()].into_iter().filter(|s| !s.is_empty()));
            if p.pictures > 0 {
                bits.push(format!("{} pictures", p.pictures));
            }
            ui.label(RichText::new(bits.join(" · ")).size(12.0).color(t.text_dim));
            if !p.words.is_empty() {
                ui.add(egui::Label::new(RichText::new(&p.words).size(12.0).color(t.text_faint)).wrap());
            }
            ui.label(RichText::new("Anything you pick below changes the pack for this video only.").size(11.5).color(t.text_faint));
        }
    }

    // Font
    row_label(ui, ctx, "Font");
    let r = egui::ComboBox::from_id_salt("mfx-look-font")
        .width(320.0)
        .height(360.0)
        .selected_text(if m.font.is_empty() { "The AI picks one".to_string() } else { m.font.clone() })
        .show_ui(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut m.font_filter).hint_text("Type to find a font").desired_width(280.0));
            ui.selectable_value(&mut m.font, String::new(), "The AI picks one");
            let needle = m.font_filter.to_lowercase();
            for name in m.fonts.iter().filter(|n| needle.is_empty() || n.to_lowercase().contains(&needle)).take(FONT_ROWS) {
                ui.selectable_value(&mut m.font, name.clone(), name);
            }
        });
    ctx.auto("mfx.lookMenu.font", &r.response, "Font");

    // Colours
    row_label(ui, ctx, "Colours");
    ui.horizontal(|ui| {
        let r = egui::ComboBox::from_id_salt("mfx-look-scheme").width(140.0).selected_text(SCHEMES.get(m.scheme).copied().unwrap_or("Analogous")).show_ui(
            ui,
            |ui| {
                for (i, s) in SCHEMES.iter().enumerate() {
                    ui.selectable_value(&mut m.scheme, i, *s);
                }
            },
        );
        ctx.auto("mfx.lookMenu.scheme", &r.response, "Colour scheme");
        let g = ui.button("Generate").on_hover_text("A new colour scheme of this kind (click a swatch to change one colour)");
        ctx.auto("mfx.lookMenu.generate", &g, "Generate");
        if g.clicked() {
            m.seed = m.seed.wrapping_add(1);
            m.palette = generate(m.seed, m.scheme);
        }
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        for (i, c) in m.palette.iter_mut().enumerate() {
            ui.vertical(|ui| {
                let mut rgb = [c.r(), c.g(), c.b()];
                let r = ui.color_edit_button_srgb(&mut rgb);
                ctx.auto(&format!("mfx.lookMenu.colour.{i}"), &r, "Colour");
                *c = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
                ui.label(RichText::new(hex(*c)).size(10.5).monospace().color(t.text_faint));
            });
        }
    });

    // Vibe and look
    for (label, list, value, id) in [("Vibe", &VIBES[..], &mut m.vibe, "vibe"), ("Look", &LOOKS[..], &mut m.look, "look")] {
        row_label(ui, ctx, label);
        ui.horizontal_wrapped(|ui| {
            for name in list {
                let on = value.eq_ignore_ascii_case(name);
                let r = ui.selectable_label(on, *name);
                ctx.auto(&format!("mfx.lookMenu.{id}.{name}"), &r, name);
                if r.clicked() {
                    *value = if on { String::new() } else { (*name).to_string() };
                }
            }
        });
        let r = ui.add(egui::TextEdit::singleline(value).hint_text("or say it in your own words").desired_width(320.0));
        ctx.auto(&format!("mfx.lookMenu.{id}.words"), &r, label);
    }

    row_label(ui, ctx, "Anything else");
    let r = ui.add(egui::TextEdit::multiline(&mut m.words).hint_text("Characters, textures, references…").desired_rows(2).desired_width(f32::INFINITY));
    ctx.auto("mfx.lookMenu.words", &r, "Anything else");

    ui.add_space(14.0);
    let label = if m.saved { "Save the look" } else { "Make the style frames" };
    let b = ui.add_enabled(
        m.save.is_none(),
        egui::Button::new(RichText::new(label).size(13.0).color(Color32::WHITE)).fill(t.accent).min_size(vec2(0.0, 30.0)).corner_radius(4.0),
    );
    ctx.auto("mfx.lookMenu.make", &b, label);
    if b.clicked() {
        let params = json!({
            "pack": if m.pack.is_empty() { Value::Null } else { json!(m.pack) },
            "font": m.font, "palette": m.palette.iter().map(|c| hex(*c)).collect::<Vec<_>>(),
            "vibe": m.vibe, "look": m.look, "words": m.words,
        });
        m.save = Some(ctx.service.request("look.brief.set", params));
        m.note = "Saving the look…".into();
    }
    if !m.note.is_empty() {
        ui.add_space(4.0);
        let col = if m.note.starts_with("Couldn't") { t.error } else { t.text_dim };
        ui.label(RichText::new(&m.note).size(12.0).color(col));
    }
}

fn swatches(ui: &mut egui::Ui, palette: &[Color32], edge: Color32) {
    ui.horizontal(|ui| {
        for c in palette {
            let (r, _) = ui.allocate_exact_size(vec2(36.0, 22.0), Sense::hover());
            ui.painter().rect_filled(r, 3.0, *c);
            ui.painter().rect_stroke(r, 3.0, Stroke::new(1.0, edge.gamma_multiply(2.0)), StrokeKind::Inside);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_generated_scheme_has_five_colours_a_dark_and_a_light_ground_and_changes_with_the_seed() {
        for scheme in 0..SCHEMES.len() {
            let p = generate(42, scheme);
            assert_eq!(p.len(), 5);
            let lum = |c: Color32| u32::from(c.r()) + u32::from(c.g()) + u32::from(c.b());
            assert!(lum(p[0]) < 150 && lum(p[1]) > 600, "{scheme}: {p:?}");
        }
        assert_ne!(generate(1, 0), generate(2, 0));
        assert_eq!(generate(7, 2), generate(7, 2));
    }

    #[test]
    fn packs_are_read_safely() {
        let p = parse_packs(
            &json!([{"id": "noir", "name": "Noir", "palette": ["#101010", "red"], "fonts": ["Bebas Neue", 3], "pictures": ["/a.png"]}, {"name": "no id"}, 5]),
        );
        assert_eq!(p.len(), 1);
        assert_eq!((p[0].palette.len(), p[0].fonts.len(), p[0].pictures), (1, 1, 1));
        assert!(parse_packs(&json!(null)).is_empty());
    }
}
