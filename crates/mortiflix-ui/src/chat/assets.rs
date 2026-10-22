//! The Assets tab beside the chat (doc 32b): every asset of the project, yours and the ones
//! Mortiflix found, what uses each one, how to use it, where, a note, Replace… (old version kept),
//! drag onto a shot node (the node area reads [`AssetDrag`]) or onto the chat's message box,
//! Show in folder and Remove from project. The list is the service's (`assets.list`); every change
//! is a request (`assets.add`, `assets.update`, `assets.replace`, `assets.remove`), see protocol.md.

use egui::{Align, Align2, FontId, Layout, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde_json::{Value, json};

use super::model;
use crate::MfxCtx;

/// What the node area receives when an asset row is dropped on a shot (egui drag and drop
/// payload): the node area sends `assets.use {asset, node}`, which records it and asks for a Fix
/// that puts it in (doc 32b).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetDrag {
    pub id: String,
    pub name: String,
}

/// One shot (or line) that uses an asset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UsedIn {
    pub node: String,
    /// "background, slow push in" (the AI's words).
    pub how: String,
    pub at: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asset {
    pub id: String,
    pub name: String,
    /// logo, image, footage, audio, music, font, document, brand
    pub kind: String,
    /// must, fits or reference
    pub usage: String,
    /// Beat ids it should go in; empty = anywhere.
    pub places: Vec<String>,
    pub note: String,
    pub version: u32,
    pub path: String,
    /// "Image · 3024 × 2016 · 2.1 MB"
    pub facts: String,
    pub used_in: Vec<UsedIn>,
    /// Planned in shots not made yet.
    pub planned: Vec<String>,
    /// Found by Mortiflix (stock from your asset sites), with where from.
    pub found: bool,
    pub source: String,
    pub missing: bool,
    /// Still copying into the project: 0-1.
    pub copying: Option<f32>,
}

const USES: [(&str, &str); 3] = [("must", "Must use"), ("fits", "Use if it fits"), ("reference", "Just for reference")];

fn use_label(u: &str) -> &'static str {
    USES.iter().find(|(k, _)| *k == u).map_or("Use if it fits", |(_, l)| l)
}

fn str_of(v: &Value, key: &str, max: usize) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(max).collect()).unwrap_or_default()
}

/// "2.1 MB" from bytes.
fn size_text(v: Option<&Value>) -> String {
    match v.and_then(Value::as_f64).filter(|b| b.is_finite() && *b >= 0.0) {
        Some(b) if b >= 1e9 => format!("{:.1} GB", b / 1e9),
        Some(b) if b >= 1e6 => format!("{:.1} MB", b / 1e6),
        Some(b) if b >= 1e3 => format!("{:.0} KB", b / 1e3),
        Some(b) => format!("{b:.0} B"),
        None => String::new(),
    }
}

/// The assets from an `assets.list` result: part 08's `{yours, found, removed}`, or `{assets}` / a
/// bare list. Hostile input.
pub fn parse(v: &Value) -> Vec<Asset> {
    let mut out = Vec::new();
    if v.get("yours").is_some() || v.get("found").is_some() {
        out.extend(parse_list(v.get("yours"), false));
        out.extend(parse_list(v.get("found"), true));
        return out;
    }
    parse_list(v.get("assets").or(Some(v)), false)
}

fn parse_list(v: Option<&Value>, found_list: bool) -> Vec<Asset> {
    let Some(list) = v.and_then(Value::as_array) else { return Vec::new() };
    list.iter()
        .take(2000)
        .filter_map(|a| {
            let id = str_of(a, "id", 80);
            if id.is_empty() {
                return None;
            }
            let strings = |key: &str| -> Vec<String> {
                a.get(key)
                    .and_then(Value::as_array)
                    .map(|l| l.iter().take(100).filter_map(|x| x.as_str().map(|s| s.chars().take(80).collect())).collect())
                    .unwrap_or_default()
            };
            let used_in = a
                .get("used_in")
                .or_else(|| a.get("usedIn"))
                .and_then(Value::as_array)
                .map(|l| {
                    l.iter()
                        .take(200)
                        .filter_map(|u| {
                            let node = str_of(u, "node", 80);
                            (!node.is_empty()).then(|| UsedIn { node, how: str_of(u, "how", 200), at: str_of(u, "at", 20) })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let name = Some(str_of(a, "name", 200)).filter(|n| !n.is_empty()).unwrap_or_else(|| model::file_name(&str_of(a, "path", 4000)));
            let usage = str_of(a, "use", 20);
            let kind = str_of(a, "kind", 20);
            let mut facts = str_of(a, "facts", 200);
            if facts.is_empty() {
                let size = size_text(a.get("size"));
                let k = kind.get(..1).map(|f| f.to_uppercase() + kind.get(1..).unwrap_or("")).unwrap_or_default();
                facts = [k, size].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
            }
            let from = str_of(a, "from", 80);
            Some(Asset {
                name,
                kind,
                usage: if USES.iter().any(|(k, _)| *k == usage) { usage } else { "fits".into() },
                places: strings("where"),
                note: str_of(a, "note", 2000),
                version: a.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok()).unwrap_or(1).max(1),
                path: str_of(a, "path", 4000),
                facts,
                used_in,
                planned: strings("planned"),
                found: found_list || a.get("found").and_then(Value::as_bool).unwrap_or(false),
                source: Some(str_of(a, "source", 200)).filter(|s| !s.is_empty()).unwrap_or(if from == "mortiflix" {
                    "Found by Mortiflix".into()
                } else {
                    from
                }),
                missing: a.get("missing").and_then(Value::as_bool).unwrap_or(false),
                copying: a.get("copying").and_then(Value::as_f64).filter(|c| c.is_finite()).map(|c| c.clamp(0.0, 1.0) as f32),
                id,
            })
        })
        .collect()
}

/// What the row says happened to the asset (doc 32b "Row").
pub fn usage_line(graph: &Value, a: &Asset) -> (String, bool) {
    if a.missing {
        return ("File missing".into(), true);
    }
    if let Some(c) = a.copying {
        return (format!("Copying… {:.0}%", c * 100.0), false);
    }
    let shots = a.used_in.len();
    if shots > 0 {
        return (if shots == 1 { "1 shot".into() } else { format!("{shots} shots") }, false);
    }
    if let Some(p) = a.planned.first() {
        return (format!("Planned: {}", model::lower_name(&model::node(graph, p).name)), false);
    }
    if a.usage == "reference" {
        return (if a.kind == "music" { "Read for the music".into() } else { "Read".into() }, false);
    }
    ("Not used yet".into(), a.usage == "must")
}

fn kind_glyph(kind: &str) -> &'static str {
    match kind {
        "logo" => "Logo",
        "image" => "Img",
        "footage" => "Vid",
        "audio" | "music" => "♪",
        "font" => "Aa",
        "document" => "Doc",
        "brand" => "Brand",
        _ => "File",
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Filter {
    #[default]
    All,
    Unused,
    Must,
}

#[derive(Clone, Debug, Default)]
struct AssetsUi {
    list: Vec<Asset>,
    list_req: Option<u64>,
    loaded_for: Option<Option<String>>,
    event_seq: u64,
    selected: Option<String>,
    filter: Filter,
    /// The note being typed (asset id, words), sent when the box loses focus.
    note: Option<(String, String)>,
    /// After Replace…: (asset, new name, version, shots still on the old one).
    replaced: Option<(String, String, u32, Vec<String>)>,
    confirm_remove: Option<String>,
    pending: Vec<u64>,
    error: String,
}

fn ui_id() -> egui::Id {
    egui::Id::new("mfx-assets-ui")
}

fn mutate(ctx: &mut MfxCtx, st: &mut AssetsUi, ty: &str, params: Value) {
    let id = ctx.service.request(ty, params);
    st.pending.push(id);
}

fn refresh(ctx: &mut MfxCtx, st: &mut AssetsUi) {
    st.list_req = Some(ctx.service.request("assets.list", json!({})));
}

fn poll(ctx: &mut MfxCtx, st: &mut AssetsUi) {
    let key = Some(ctx.state.project_id.clone());
    if ctx.service.status().is_connected() && st.loaded_for != key && st.list_req.is_none() {
        st.loaded_for = key;
        refresh(ctx, st);
    }
    if let Some(id) = st.list_req
        && let Some(r) = ctx.service.take_reply(id)
    {
        st.list_req = None;
        if r.ok {
            st.list = parse(&r.result);
            st.error.clear();
        } else if r.error.get("code").and_then(Value::as_str) != Some("no_project") {
            st.error = r.error.get("message").and_then(Value::as_str).unwrap_or("").chars().take(200).collect();
        } else {
            st.list.clear();
        }
    }
    let mut changed = false;
    st.pending.retain(|id| match ctx.service.take_reply(*id) {
        Some(r) => {
            changed = true;
            if !r.ok {
                st.error = r.error.get("message").and_then(Value::as_str).unwrap_or("").chars().take(200).collect();
            }
            false
        }
        None => true,
    });
    let mut last = st.event_seq;
    for (seq, ev) in ctx.service.events_since(st.event_seq) {
        last = last.max(*seq);
        let data = ev.get("data").unwrap_or(&Value::Null);
        match ev.get("event").and_then(Value::as_str).unwrap_or("") {
            "assets.changed" | "assets.added" | "node.changed" => changed = true,
            "assets.failed" => {
                changed = true;
                st.error = format!("Couldn't add a file: {}", data.get("message").and_then(Value::as_str).unwrap_or("").chars().take(200).collect::<String>());
            }
            "assets.progress" => {
                let id = data.get("asset").and_then(Value::as_str).unwrap_or("");
                let done = data.get("done").and_then(Value::as_f64).unwrap_or(0.0);
                let total = data.get("total").and_then(Value::as_f64).filter(|t| *t > 0.0).unwrap_or(1.0);
                let frac = (done / total).clamp(0.0, 1.0) as f32;
                match st.list.iter_mut().find(|a| a.id == id) {
                    Some(a) => a.copying = (frac < 1.0).then_some(frac),
                    None if !id.is_empty() && frac < 1.0 => {
                        let name = data.get("name").and_then(Value::as_str).unwrap_or(id).chars().take(200).collect();
                        st.list.push(Asset {
                            id: id.chars().take(80).collect(),
                            name,
                            copying: Some(frac),
                            version: 1,
                            usage: "fits".into(),
                            ..Asset::default()
                        });
                    }
                    None => {}
                }
            }
            _ => {}
        }
    }
    st.event_seq = last;
    if changed && st.list_req.is_none() && ctx.service.status().is_connected() {
        refresh(ctx, st);
    }
    // files chosen with Add files… / Replace…, or dropped on the window while this tab shows
    for f in ctx.state.take_files("assets.add") {
        mutate(ctx, st, "assets.add", json!({"paths": f.paths}));
    }
    for f in ctx.state.take_files("assets.replace") {
        let id = f.purpose.split_once(':').map(|(_, id)| id.to_string()).unwrap_or_default();
        if let (Some(path), Some(a)) = (f.paths.first(), st.list.iter().find(|a| a.id == id)) {
            st.replaced = Some((a.id.clone(), model::file_name(path), a.version.saturating_add(1), a.used_in.iter().map(|u| u.node.clone()).collect()));
            mutate(ctx, st, "assets.replace", json!({"asset": id, "path": path}));
        }
    }
    if !ctx.state.show_new_project {
        for f in ctx.state.take_files("drop") {
            mutate(ctx, st, "assets.add", json!({"paths": f.paths}));
        }
    }
}

/// The Assets tab beside the chat (PanelKind::MfxAssets, doc 32b).
pub fn show_assets(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let mut st: AssetsUi = ui.data_mut(|d| d.get_temp(ui_id())).unwrap_or_default();
    poll(ctx, &mut st);
    let t = ctx.theme;
    let full = ui.max_rect();
    crate::service_notice(ui, ctx);

    // toolbar: Add files…, count, filter
    let unused = st.list.iter().filter(|a| a.used_in.is_empty() && a.usage != "reference").count();
    let must = st.list.iter().filter(|a| a.usage == "must").count();
    ui.horizontal(|ui| {
        let add = super::small_button(ui, ctx, "mfx.assets.add", "Add files…", false);
        if add.clicked() {
            ctx.command("mortiflix.chooseFiles", json!({"purpose": "assets.add", "multiple": true}));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = |f: Filter| match f {
                Filter::All => format!("All ({})", st.list.len()),
                Filter::Unused => format!("Not used yet ({unused})"),
                Filter::Must => format!("Must use ({must})"),
            };
            let mut filter = st.filter;
            let combo = egui::ComboBox::from_id_salt("mfx-assets-filter").selected_text(label(filter)).show_ui(ui, |ui| {
                for f in [Filter::All, Filter::Unused, Filter::Must] {
                    ui.selectable_value(&mut filter, f, label(f));
                }
            });
            ctx.auto("mfx.assets.filter", &combo.response, "Filter");
            st.filter = filter;
        });
    });
    let hovering = ui.input(|i| i.raw.hovered_files.len());
    if hovering > 0 {
        let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
        ui.painter().rect_filled(r, 4.0, t.accent);
        let text = if hovering == 1 { "Drop to add 1 file to Assets".to_string() } else { format!("Drop to add {hovering} files to Assets") };
        ui.painter().text(r.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.0), egui::Color32::WHITE);
    }
    if !st.error.is_empty() {
        ui.label(RichText::new(&st.error).size(12.0).color(t.error));
    }

    let graph = ctx.state.graph.clone();
    let shown: Vec<&Asset> = st
        .list
        .iter()
        .filter(|a| match st.filter {
            Filter::All => true,
            Filter::Unused => a.used_in.is_empty() && a.usage != "reference",
            Filter::Must => a.usage == "must",
        })
        .collect();
    let selected = st.selected.clone().and_then(|s| st.list.iter().find(|a| a.id == s).cloned());
    let rest = Rect::from_min_max(pos2(full.min.x, ui.cursor().min.y), full.max);
    let list_h = if selected.is_some() { (rest.height() * 0.45).max(120.0) } else { rest.height() };
    let list_r = Rect::from_min_size(rest.min, vec2(rest.width(), list_h.min(rest.height())));
    let detail_r = Rect::from_min_max(pos2(rest.min.x, list_r.max.y), rest.max);

    let mut select = None;
    super::region(ui, list_r, |ui| {
        if st.list.is_empty() {
            ui.add_space(16.0);
            ui.vertical_centered(|ui| {
                ui.label(RichText::new("No assets yet.").size(14.0).color(t.text));
                ui.add(
                    egui::Label::new(
                        RichText::new(crate::branded("Drop files here or Add files…. Mortiflix uses them in the shots you choose."))
                            .size(12.0)
                            .color(t.text_dim),
                    )
                    .wrap(),
                );
            });
            return;
        }
        if shown.is_empty() {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Every asset is used.").size(12.0).color(t.text_dim));
                let r = ui.add(egui::Label::new(RichText::new("Show all").size(12.0).color(super::LINK).underline()).sense(Sense::click()));
                if r.clicked() {
                    select = Some(None);
                }
                ctx.auto("mfx.assets.showAll", &r, "Show all");
            });
            return;
        }
        egui::ScrollArea::vertical().id_salt("mfx-assets-list").auto_shrink([false, false]).show(ui, |ui| {
            for found in [false, true] {
                let group: Vec<&&Asset> = shown.iter().filter(|a| a.found == found).collect();
                if group.is_empty() {
                    continue;
                }
                let head = if found { format!("{} · {}", crate::branded("Found by Mortiflix"), group.len()) } else { format!("Yours · {}", group.len()) };
                ui.label(RichText::new(head).size(11.0).color(t.text_faint));
                for a in group {
                    if let Some(id) = row(ui, ctx, &graph, a, selected.as_ref().is_some_and(|s| s.id == a.id)) {
                        select = Some(Some(id));
                    }
                }
                ui.add_space(6.0);
            }
        });
    });
    match select {
        Some(Some(id)) => st.selected = Some(id),
        Some(None) => st.filter = Filter::All,
        None => {}
    }
    if let Some(a) = selected {
        super::region(ui, detail_r, |ui| {
            ui.painter().line_segment([detail_r.left_top(), detail_r.right_top()], Stroke::new(1.0, t.separator));
            egui::ScrollArea::vertical().id_salt("mfx-assets-detail").auto_shrink([false, false]).show(ui, |ui| detail(ui, ctx, &mut st, &graph, &a));
        });
    }
    if ui.rect_contains_pointer(full) && ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ui.memory(|m| m.focused().is_some()) {
        st.selected = None;
    }
    confirm_remove(ui, ctx, &mut st);
    ui.data_mut(|d| d.insert_temp(ui_id(), st));
}

/// One asset row: thumbnail frame, name, how to use it, what happened to it. Drag it onto a shot.
fn row(ui: &mut egui::Ui, ctx: &mut MfxCtx, graph: &Value, a: &Asset, selected: bool) -> Option<String> {
    let t = ctx.theme;
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 40.0), if a.copying.is_some() { Sense::click() } else { Sense::click_and_drag() });
    if selected {
        ui.painter().rect_filled(r, 4.0, t.hover);
    } else if resp.hovered() {
        ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, t.separator.gamma_multiply(2.0)), egui::StrokeKind::Inside);
    }
    let thumb = Rect::from_min_size(pos2(r.min.x + 4.0, r.center().y - 13.5), vec2(48.0, 27.0));
    ui.painter().rect_filled(thumb, 3.0, t.field_bg);
    ui.painter().text(thumb.center(), Align2::CENTER_CENTER, kind_glyph(&a.kind), FontId::proportional(11.0), t.text_dim);
    let (usage, warn) = usage_line(graph, a);
    let right =
        ui.painter().layout_no_wrap(if warn { format!("! {usage}") } else { usage }, FontId::proportional(12.0), if warn { t.warn } else { t.text_dim });
    let rx = r.max.x - 6.0 - right.size().x;
    ui.painter().galley(pos2(rx, r.center().y - right.size().y / 2.0), right, t.text);
    let text_r = Rect::from_min_max(pos2(thumb.max.x + 8.0, r.min.y), pos2(rx - 8.0, r.max.y));
    let p = ui.painter().with_clip_rect(text_r);
    p.text(pos2(text_r.min.x, r.min.y + 13.0), Align2::LEFT_CENTER, &a.name, FontId::proportional(13.0), t.text);
    let sub = if a.found && !a.source.is_empty() { a.source.clone() } else { use_label(&a.usage).to_string() };
    p.text(pos2(text_r.min.x, r.min.y + 29.0), Align2::LEFT_CENTER, sub, FontId::proportional(11.0), t.text_faint);
    if let Some(c) = a.copying {
        let bar = Rect::from_min_size(pos2(text_r.min.x, r.max.y - 3.0), vec2(text_r.width(), 2.0));
        ui.painter().rect_filled(bar, 1.0, t.separator);
        ui.painter().rect_filled(Rect::from_min_size(bar.min, vec2(bar.width() * c, 2.0)), 1.0, t.accent);
    }
    if a.copying.is_none() {
        resp.dnd_set_drag_payload(AssetDrag { id: a.id.clone(), name: a.name.clone() });
    }
    let resp = resp.on_hover_text(&a.name);
    ctx.auto(&format!("mfx.assets.row.{}", a.id), &resp, &a.name);
    resp.clicked().then(|| a.id.clone())
}

fn detail(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut AssetsUi, graph: &Value, a: &Asset) {
    let t = ctx.theme;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.add(egui::Label::new(RichText::new(&a.name).size(14.0).color(t.text).strong()).truncate());
            let version = if a.version > 1 { format!("v{} (v{} kept)", a.version, a.version - 1) } else { "v1".into() };
            let facts = if a.facts.is_empty() { version } else { format!("{} · {version}", a.facts) };
            ui.label(RichText::new(facts).size(11.0).color(t.text_dim));
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            if super::small_button(ui, ctx, "mfx.assets.replace", "Replace…", false).clicked() {
                ctx.command("mortiflix.chooseFiles", json!({"purpose": format!("assets.replace:{}", a.id), "multiple": false}));
            }
        });
    });
    if a.found {
        ui.label(
            RichText::new(format!(
                "{} · used in {} shot{}",
                crate::branded(if a.source.is_empty() { "Found by Mortiflix" } else { &a.source }),
                a.used_in.len(),
                if a.used_in.len() == 1 { "" } else { "s" }
            ))
            .size(12.0)
            .color(t.text_dim),
        );
        used_in(ui, ctx, graph, a);
        reveal(ui, ctx, a);
        return;
    }
    if a.missing {
        ui.label(RichText::new("The project's copy of this file is gone. Replace it; shots keep their made versions.").size(12.0).color(t.warn));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("How to use it").size(12.0).color(t.text_faint));
        let mut usage = a.usage.clone();
        let c = egui::ComboBox::from_id_salt(("mfx-assets-use", &a.id)).selected_text(use_label(&usage)).show_ui(ui, |ui| {
            for (k, l) in USES {
                ui.selectable_value(&mut usage, k.to_string(), l);
            }
        });
        ctx.auto("mfx.assets.use", &c.response, "How to use it");
        if usage != a.usage {
            mutate(ctx, st, "assets.update", json!({"asset": a.id, "use": usage}));
            if let Some(x) = st.list.iter_mut().find(|x| x.id == a.id) {
                x.usage = usage;
            }
        }
    });
    // Where: Anywhere + one chip per beat
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new("Where").size(12.0).color(t.text_faint));
        let mut places = a.places.clone();
        let anywhere = places.is_empty();
        if chip(ui, ctx, "mfx.assets.where.anywhere", "Anywhere", anywhere).clicked() {
            places.clear();
        }
        for b in model::beats(graph) {
            let on = places.contains(&b.id);
            let label = format!("{} {}", b.beat_no.unwrap_or(0), model::short(&b.name, 22));
            if chip(ui, ctx, &format!("mfx.assets.where.{}", b.id), &label, on).clicked() {
                if on {
                    places.retain(|p| p != &b.id);
                } else {
                    places.push(b.id.clone());
                }
            }
        }
        if places != a.places {
            mutate(ctx, st, "assets.update", json!({"asset": a.id, "where": places}));
            if let Some(x) = st.list.iter_mut().find(|x| x.id == a.id) {
                x.places = places;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("Note").size(12.0).color(t.text_faint));
        if st.note.as_ref().is_none_or(|(id, _)| id != &a.id) {
            st.note = Some((a.id.clone(), a.note.clone()));
        }
        if let Some((_, words)) = st.note.as_mut() {
            let r = ui.add(egui::TextEdit::singleline(words).hint_text("Anywhere it fits").desired_width(ui.available_width()));
            ctx.auto("mfx.assets.note", &r, "Note");
            if r.lost_focus() && *words != a.note {
                let words = words.clone();
                mutate(ctx, st, "assets.update", json!({"asset": a.id, "note": words}));
                if let Some(x) = st.list.iter_mut().find(|x| x.id == a.id) {
                    x.note = words;
                }
            }
        }
    });
    if let Some((id, new_name, v, shots)) = st.replaced.clone().filter(|r| r.0 == a.id && !r.3.is_empty()) {
        let names: Vec<String> = shots.iter().map(|s| model::lower_name(&model::node(graph, s).name)).collect();
        ui.add_space(4.0);
        ui.add(
            egui::Label::new(
                RichText::new(format!("Replaced with {new_name} (v{v}). {} still show v{}.", join(&names), v.saturating_sub(1))).size(12.0).color(t.text),
            )
            .wrap(),
        );
        ui.horizontal(|ui| {
            let label = format!("Remake {}", join(&names));
            if super::small_button(ui, ctx, "mfx.assets.remake", &label, true).clicked() {
                for s in &shots {
                    mutate(ctx, st, "node.edit", json!({"node": s, "action": "fix", "text": format!("Use {new_name} (v{v}) here")}));
                }
                st.replaced = None;
            }
            if super::small_button(ui, ctx, "mfx.assets.keepOld", &format!("Keep v{} in those shots", v.saturating_sub(1)), false).clicked() {
                st.replaced = None;
            }
        });
        let _ = id;
    }
    used_in(ui, ctx, graph, a);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        reveal(ui, ctx, a);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let r = ui.add(egui::Button::new(RichText::new("Remove from project").size(12.0).color(t.text_dim)).frame(false));
            if r.clicked() {
                if a.used_in.is_empty() {
                    mutate(ctx, st, "assets.remove", json!({"asset": a.id}));
                    st.list.retain(|x| x.id != a.id);
                    st.selected = None;
                } else {
                    st.confirm_remove = Some(a.id.clone());
                }
            }
            ctx.auto("mfx.assets.remove", &r, "Remove from project");
        });
    });
}

fn used_in(ui: &mut egui::Ui, ctx: &mut MfxCtx, graph: &Value, a: &Asset) {
    let t = ctx.theme;
    ui.add_space(6.0);
    ui.label(RichText::new("Used in").size(12.0).color(t.text_faint));
    if a.used_in.is_empty() {
        ui.label(RichText::new("No shot uses it yet.").size(12.0).color(t.text_dim));
    }
    for (k, u) in a.used_in.iter().enumerate().take(100) {
        let info = model::node(graph, &u.node);
        ui.allocate_ui_with_layout(vec2(ui.available_width(), 22.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            super::beat_mark(ui, ctx, &info);
            let mut line = info.name.clone();
            if !u.at.is_empty() {
                line.push_str(&format!(" · {}", u.at));
            }
            if !u.how.is_empty() {
                line.push_str(&format!(" · {}", u.how));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let show = super::small_button(ui, ctx, &format!("mfx.assets.show.{k}"), "Show", false);
                if show.clicked() {
                    ctx.state.selected_node = Some(u.node.clone());
                }
                ui.add(egui::Label::new(RichText::new(line).size(12.0).color(t.text)).truncate());
            });
        });
    }
}

fn reveal(ui: &mut egui::Ui, ctx: &mut MfxCtx, a: &Asset) {
    if a.path.is_empty() {
        return;
    }
    let r = ui.add(egui::Label::new(RichText::new("Show in folder").size(12.0).color(super::LINK).underline()).sense(Sense::click()));
    if r.clicked() {
        ctx.command("mortiflix.reveal", json!({"path": a.path}));
    }
    ctx.auto("mfx.assets.reveal", &r, "Show in folder");
}

fn join(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [a] => a.clone(),
        [a @ .., b] => format!("{} and {b}", a.join(", ")),
    }
}

fn chip(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str, label: &str, on: bool) -> egui::Response {
    let t = ctx.theme;
    let r = ui.add(
        egui::Button::new(RichText::new(label).size(12.0).color(if on { t.text } else { t.text_dim }))
            .min_size(vec2(0.0, 22.0))
            .fill(if on { t.hover } else { t.panel_bg })
            .stroke(Stroke::new(1.0, if on { t.text_dim } else { t.separator.gamma_multiply(2.0) }))
            .corner_radius(11.0),
    );
    ctx.auto(id, &r, label);
    r
}

/// Remove an asset shots use: quiet on the page, loud in the confirm (doc 32b).
fn confirm_remove(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut AssetsUi) {
    let Some(id) = st.confirm_remove.clone() else { return };
    let Some(a) = st.list.iter().find(|a| a.id == id).cloned() else {
        st.confirm_remove = None;
        return;
    };
    let graph = ctx.state.graph.clone();
    let names: Vec<String> = a.used_in.iter().map(|u| model::lower_name(&model::node(&graph, &u.node).name)).collect();
    let n = names.len();
    let modal = egui::Modal::new(egui::Id::new("mfx-assets-remove")).show(ui.ctx(), |ui| {
        ui.set_width(380.0);
        ui.heading(format!("Remove {}?", a.name));
        ui.label(format!("{} use{} it and will be remade without it.", join(&names), if n == 1 { "s" } else { "" }));
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let go = ui.add(
                egui::Button::new(RichText::new(format!("Remove and remake {n} shot{}", if n == 1 { "" } else { "s" })).color(egui::Color32::WHITE))
                    .fill(ctx.theme.error),
            );
            ctx.auto("mfx.assets.remove.confirm", &go, "Remove and remake");
            if go.clicked() {
                mutate(ctx, st, "assets.remove", json!({"asset": a.id}));
                for u in &a.used_in {
                    mutate(
                        ctx,
                        st,
                        "node.edit",
                        json!({"node": u.node, "action": "fix", "text": format!("Don't use {} here any more (it was removed from the project).", a.name)}),
                    );
                }
                st.list.retain(|x| x.id != a.id);
                st.selected = None;
                st.confirm_remove = None;
            }
            let cancel = ui.button("Cancel");
            ctx.auto("mfx.assets.remove.cancel", &cancel, "Cancel");
            if cancel.clicked() {
                st.confirm_remove = None;
            }
        });
    });
    if modal.should_close() {
        st.confirm_remove = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_parse_and_say_what_happened() {
        let v = json!({"assets": [
            {"id": "a01", "name": "kottman-logo.svg", "kind": "logo", "use": "must", "planned": ["s14"]},
            {"id": "a02", "path": "/p/input/assets/dartmouth-1956.jpg", "use": "fits", "usedIn": [{"node": "s01", "how": "background", "at": "0:00"}, {"node": "s02"}, {"how": "no node"}], "version": 2},
            {"id": "a03", "kind": "music", "use": "reference"},
            {"id": "a04", "use": "must"},
            {"id": "a05", "use": "bogus", "copying": 0.64, "version": -3},
            {"name": "no id"}, 7
        ]});
        let list = parse(&v);
        assert_eq!(list.len(), 5);
        assert_eq!(list[1].name, "dartmouth-1956.jpg");
        assert_eq!(list[1].used_in.len(), 2);
        assert_eq!(list[4].usage, "fits");
        assert_eq!(list[4].version, 1);
        let g = Value::Null;
        assert_eq!(usage_line(&g, &list[0]), ("Planned: shot 14".into(), false));
        assert_eq!(usage_line(&g, &list[1]), ("2 shots".into(), false));
        assert_eq!(usage_line(&g, &list[2]), ("Read for the music".into(), false));
        assert_eq!(usage_line(&g, &list[3]), ("Not used yet".into(), true));
        assert_eq!(usage_line(&g, &list[4]), ("Copying… 64%".into(), false));
        assert!(parse(&json!("x")).is_empty());
        let p8 = parse(
            &json!({"yours": [{"id": "a01", "name": "logo.svg", "kind": "logo", "size": 14200, "use": "must", "where": null, "used_in": [{"node": "s14", "how": "corner"}]}],
            "found": [{"id": "found:x.jpg", "name": "x.jpg", "kind": "image", "from": "mortiflix", "path": "/p/x.jpg"}], "removed": []}),
        );
        assert_eq!(p8.len(), 2);
        assert_eq!((p8[0].facts.as_str(), p8[0].used_in.len(), p8[0].places.len()), ("Logo · 14 KB", 1, 0));
        assert!(p8[1].found);
        assert_eq!(p8[1].source, "Found by Mortiflix");
        assert_eq!(parse(&json!([{"id": "z"}])).len(), 1);
        assert_eq!(join(&["shot 1".into(), "shot 2".into(), "shot 3".into()]), "shot 1, shot 2 and shot 3");
    }

    #[test]
    fn the_tab_draws_and_takes_files() {
        let mut m = crate::Mortiflix::default();
        m.state.push_files("assets.add", vec!["/x/a.png".into()]);
        let ectx = egui::Context::default();
        let mut out = ectx.run_ui(egui::RawInput::default(), |ui| {
            let mut ctx = m.ctx(crate::MfxTheme::default());
            show_assets(ui, &mut ctx);
        });
        out.textures_delta.clear();
        assert!(m.state.files_in.is_empty());
    }
}
