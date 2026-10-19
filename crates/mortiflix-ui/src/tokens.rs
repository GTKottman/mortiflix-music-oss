//! Token usage (PanelKind::MfxTokens, "Mortiflix Tokens"; owner's note, 2026-10-18): every Claude run of the
//! project as a row, newest on top. Click one: its calls as bars by what they did, sorted by raw tokens, each
//! split into input, cache and output (so a cheap, cache-heavy step never looks like the most expensive one),
//! with why it ran (the AI's own words) and where it pulled its information from. Under the bars, what was
//! pulled into the context and how many later calls re-read it: the place to look when token-optimizing a
//! pipeline.
//!
//! The service reads the transcripts (`tokens.runs`, `tokens.run`, see the service protocol doc); this panel
//! keeps the last answers, asks again only while it is on screen (every 5 s while a run is going, else every
//! minute), and draws rows inside a scroll area that lays out only the visible ones.

use egui::{Color32, Rect, RichText, Sense, Stroke, pos2, vec2};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{MfxCtx, MfxTheme};

/// While a run is going.
const POLL_RUNNING: f64 = 5.0;
/// Nothing running: a finished run doesn't change.
const POLL_IDLE: f64 = 60.0;
const ROW_H: f32 = 46.0;
const BAR_H: f32 = 10.0;

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Run {
    pub id: String,
    pub started: Option<String>,
    pub ended: Option<String>,
    pub running: bool,
    pub backend: String,
    pub model: String,
    pub calls: u64,
    pub cost_usd: Option<f64>,
    pub why: String,
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
    pub total: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Example {
    pub label: String,
    pub times: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Bar {
    pub key: String,
    pub calls: u64,
    pub input: u64,
    pub cache_write: u64,
    pub cache_read: u64,
    pub output: u64,
    pub total: u64,
    pub examples: Vec<Example>,
    pub why: String,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Pulled {
    pub label: String,
    pub group: String,
    pub from: String,
    pub tokens: u64,
    pub calls_after: u64,
    pub carried: u64,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct Detail {
    pub run: Run,
    pub bars: Vec<Bar>,
    pub pulled: Vec<Pulled>,
}

/// The panel's state (egui memory: nothing here is saved).
#[derive(Clone, Debug, Default)]
struct Tokens {
    project: Option<String>,
    runs: Vec<Run>,
    loaded: bool,
    error: String,
    selected: Option<String>,
    detail: Option<Detail>,
    open_bar: Option<String>,
    runs_req: Option<u64>,
    detail_req: Option<u64>,
    runs_at: f64,
    detail_at: f64,
}

fn key() -> egui::Id {
    egui::Id::new("mfx-tokens")
}

/// 35296544 → "35.3 M", 12869 → "12.9 k", 812 → "812".
pub fn short(n: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let f = n as f64;
    if n >= 1_000_000 {
        format!("{:.1} M", f / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{:.0} k", f / 1_000.0)
    } else if n >= 1_000 {
        format!("{:.1} k", f / 1_000.0)
    } else {
        n.to_string()
    }
}

/// "Oct 17, 4:39 pm" from the run's ISO start.
fn when(iso: Option<&str>) -> String {
    let Some(at) = iso else { return String::new() };
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let md = crate::chat::model::local_month_day(at);
    let month = md.and_then(|(m, _)| usize::try_from(m).ok()).and_then(|m| m.checked_sub(1)).and_then(|m| MONTHS.get(m));
    let day = md.map(|(_, d)| d);
    let clock = crate::chat::model::clock(at);
    match (month, day) {
        (Some(m), Some(d)) => format!("{m} {d}, {clock}"),
        _ => clock,
    }
}

/// The three colours: input (what was new to Claude), cache (the context re-read or stored), output.
fn colours(t: &MfxTheme) -> [Color32; 3] {
    [t.accent, t.text_faint.gamma_multiply(0.8), t.warn]
}

/// A bar split into input · cache · output, `full` = the width of the largest one.
#[allow(clippy::cast_precision_loss)]
fn split_bar(p: &egui::Painter, r: Rect, input: u64, cache: u64, output: u64, full: u64, t: &MfxTheme) {
    p.rect_filled(r, 2.0, t.field_bg);
    let full = full.max(1) as f32;
    let mut x = r.min.x;
    for (n, c) in [input, cache, output].into_iter().zip(colours(t)) {
        let w = r.width() * (n as f32 / full);
        if w > 0.0 {
            let seg = Rect::from_min_max(pos2(x, r.min.y), pos2((x + w.max(1.0)).min(r.max.x), r.max.y));
            p.rect_filled(seg, 0.0, c);
            x = seg.max.x;
        }
    }
}

fn split_text(input: u64, cache_write: u64, cache_read: u64, output: u64) -> String {
    format!(
        "Input {} · cache {} (written {}, re-read {}) · output {}",
        short(input),
        short(cache_write.saturating_add(cache_read)),
        short(cache_write),
        short(cache_read),
        short(output)
    )
}

/// Ask for the runs, and the selected run, when they are due.
fn poll(ctx: &mut MfxCtx, st: &mut Tokens, now: f64) {
    let project = ctx.state.project_id.clone();
    if project != st.project {
        *st = Tokens { project: project.clone(), ..Tokens::default() };
    }
    let Some(project) = project else { return };
    if let Some(id) = st.runs_req
        && let Some(r) = ctx.service.take_reply(id)
    {
        st.runs_req = None;
        if r.ok {
            st.runs = r.result.get("runs").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
            st.loaded = true;
            st.error.clear();
            // the newest run is shown until you pick another
            if st.selected.as_ref().is_none_or(|s| !st.runs.iter().any(|r| &r.id == s)) {
                st.selected = st.runs.first().map(|r| r.id.clone());
                st.detail = None;
                st.detail_at = f64::NEG_INFINITY;
            }
        } else {
            st.error = r.error.get("message").and_then(Value::as_str).unwrap_or("The studio didn't answer.").chars().take(300).collect();
        }
    }
    if let Some(id) = st.detail_req
        && let Some(r) = ctx.service.take_reply(id)
    {
        st.detail_req = None;
        if r.ok
            && let Ok(d) = serde_json::from_value::<Detail>(r.result)
            && st.selected.as_deref() == Some(d.run.id.as_str())
        {
            st.detail = Some(d);
        }
    }
    let running = st.runs.iter().any(|r| r.running);
    let every = if running { POLL_RUNNING } else { POLL_IDLE };
    if st.runs_req.is_none() && now - st.runs_at >= every {
        st.runs_at = now;
        st.runs_req = Some(ctx.service.request("tokens.runs", json!({"project": project})));
    }
    if let Some(sel) = st.selected.clone() {
        let sel_running = st.runs.iter().any(|r| r.id == sel && r.running);
        let due = st.detail.as_ref().is_none_or(|d| d.run.id != sel) || (sel_running && now - st.detail_at >= POLL_RUNNING);
        if st.detail_req.is_none() && due {
            st.detail_at = now;
            st.detail_req = Some(ctx.service.request("tokens.run", json!({"project": project, "run": sel})));
        }
    }
}

/// Draw the panel.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let now = ui.input(|i| i.time);
    let mut st: Tokens = ui.ctx().data_mut(|d| d.get_temp::<Tokens>(key())).unwrap_or_default();
    if st.runs_at == 0.0 {
        st.runs_at = f64::NEG_INFINITY;
        st.detail_at = f64::NEG_INFINITY;
    }
    poll(ctx, &mut st, now);
    // only while this panel is drawn: a wake-up when the next answer is due (the reply itself wakes it too)
    let running = st.runs.iter().any(|r| r.running);
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(if running { POLL_RUNNING } else { POLL_IDLE }));

    ui.label(RichText::new("Token usage").strong().color(t.text));
    legend(ui, &t);
    if ctx.state.project_id.is_none() {
        ui.label(RichText::new("Open a project to see what its Claude runs used.").color(t.text_dim));
    } else if !st.error.is_empty() {
        ui.label(RichText::new(&st.error).color(t.error));
    } else if !st.loaded {
        ui.label(RichText::new("Reading the runs…").color(t.text_dim));
    } else if st.runs.is_empty() {
        ui.label(RichText::new("No Claude runs yet. Each run shows here as it starts.").color(t.text_dim));
    } else {
        runs_list(ui, ctx, &mut st, &t);
        ui.add_space(6.0);
        ui.separator();
        detail(ui, ctx, &mut st, &t);
    }
    ui.ctx().data_mut(|d| d.insert_temp(key(), st));
}

fn legend(ui: &mut egui::Ui, t: &MfxTheme) {
    ui.horizontal(|ui| {
        for (name, c) in ["Input", "Cache", "Output"].into_iter().zip(colours(t)) {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, c);
            ui.label(RichText::new(name).small().color(t.text_dim));
            ui.add_space(6.0);
        }
    })
    .response
    .on_hover_text("Raw tokens. Input: new to Claude on that call. Cache: the context written to or re-read from Claude's cache (cheap to re-read, but it adds up on every call). Output: what Claude wrote.");
}

/// Every run, newest on top (a third of the panel, scrolls).
fn runs_list(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut Tokens, t: &MfxTheme) {
    let biggest = st.runs.iter().map(|r| r.total).max().unwrap_or(1);
    let height = (ui.available_height() * 0.36).clamp(ROW_H * 2.0, ROW_H * 8.0);
    let mut pick: Option<String> = None;
    egui::ScrollArea::vertical().id_salt("mfx-tokens-runs").max_height(height).auto_shrink([false, true]).show_rows(ui, ROW_H, st.runs.len(), |ui, range| {
        for run in st.runs.get(range).unwrap_or_default() {
            let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
            let sel = st.selected.as_deref() == Some(run.id.as_str());
            let p = ui.painter();
            if sel || resp.hovered() {
                p.rect_filled(r, 3.0, if sel { t.accent.gamma_multiply(0.22) } else { t.hover });
            }
            let inner = r.shrink2(vec2(6.0, 4.0));
            let head = if run.running { format!("● {} · running", when(run.started.as_deref())) } else { when(run.started.as_deref()) };
            let head_col = if run.running { t.ok } else { t.text };
            p.text(inner.left_top(), egui::Align2::LEFT_TOP, head, egui::FontId::proportional(12.5), head_col);
            p.text(inner.right_top(), egui::Align2::RIGHT_TOP, short(run.total), egui::FontId::proportional(12.5), t.text);
            let bar = Rect::from_min_size(pos2(inner.min.x, inner.min.y + 17.0), vec2(inner.width(), BAR_H - 2.0));
            split_bar(p, bar, run.input, run.cache_write.saturating_add(run.cache_read), run.output, biggest, t);
            let why = if run.why.is_empty() { format!("{} calls", run.calls) } else { format!("{} calls · {}", run.calls, run.why) };
            let g = crate::nodes::canvas::line(p, &why, 11.0, t.text_dim, inner.width());
            p.galley(pos2(inner.min.x, bar.max.y + 3.0), g, t.text_dim);
            let cost = run.cost_usd.map(|c| format!(" · list price ${c:.2}")).unwrap_or_default();
            let resp = resp.on_hover_text(format!(
                "{}\n{} · {}{}\n{}",
                when(run.started.as_deref()),
                if run.model.is_empty() { "Claude" } else { run.model.as_str() },
                run.backend,
                cost,
                split_text(run.input, run.cache_write, run.cache_read, run.output)
            ));
            ctx.auto(&format!("mfx.tokens.run.{}", run.id), &resp, &when(run.started.as_deref()));
            if resp.clicked() {
                pick = Some(run.id.clone());
            }
        }
    });
    if let Some(id) = pick
        && st.selected.as_ref() != Some(&id)
    {
        st.selected = Some(id);
        st.detail = None;
        st.open_bar = None;
    }
}

/// The selected run: bars sorted by raw tokens, then what was pulled into the context.
fn detail(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut Tokens, t: &MfxTheme) {
    let Some(d) = st.detail.clone() else {
        ui.label(RichText::new("Reading the run…").color(t.text_dim));
        return;
    };
    let r = &d.run;
    ui.label(RichText::new(format!("{} · {} calls · {}", when(r.started.as_deref()), r.calls, short(r.total))).strong().color(t.text));
    ui.label(RichText::new(split_text(r.input, r.cache_write, r.cache_read, r.output)).small().color(t.text_dim));
    let biggest = d.bars.first().map_or(1, |b| b.total);
    egui::ScrollArea::vertical().id_salt("mfx-tokens-detail").auto_shrink([false, false]).show(ui, |ui| {
        ui.add_space(4.0);
        ui.label(RichText::new("What used the tokens").color(t.text_dim));
        for b in &d.bars {
            let open = st.open_bar.as_deref() == Some(b.key.as_str());
            let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::click());
            let p = ui.painter();
            if open || resp.hovered() {
                p.rect_filled(row, 3.0, t.hover);
            }
            let inner = row.shrink2(vec2(6.0, 3.0));
            // a drawn ▸ / ▾ (the UI font has no arrow glyphs), then the name on its own line
            let c = pos2(inner.min.x + 4.0, inner.min.y + 8.0);
            let tri = if open {
                vec![c + vec2(-4.0, -2.5), c + vec2(4.0, -2.5), c + vec2(0.0, 3.0)]
            } else {
                vec![c + vec2(-2.5, -4.0), c + vec2(3.0, 0.0), c + vec2(-2.5, 4.0)]
            };
            p.add(egui::Shape::convex_polygon(tri, t.text_dim, Stroke::NONE));
            let g = crate::nodes::canvas::line(p, &b.key, 12.0, t.text, inner.width() - 14.0);
            p.galley(pos2(inner.min.x + 12.0, inner.min.y), g, t.text);
            let numbers = p.layout_no_wrap(format!("{} · {}×", short(b.total), b.calls), egui::FontId::proportional(11.5), t.text_dim);
            let bar_w = (inner.width() - numbers.size().x - 8.0).max(20.0);
            let bar = Rect::from_min_size(pos2(inner.min.x, inner.max.y - BAR_H - 2.0), vec2(bar_w, BAR_H));
            split_bar(p, bar, b.input, b.cache_write.saturating_add(b.cache_read), b.output, biggest, t);
            p.galley(pos2(inner.max.x - numbers.size().x, bar.center().y - numbers.size().y / 2.0), numbers, t.text_dim);
            let resp = resp.on_hover_text(split_text(b.input, b.cache_write, b.cache_read, b.output));
            ctx.auto(&format!("mfx.tokens.bar.{}", b.key), &resp, &b.key);
            if resp.clicked() {
                st.open_bar = if open { None } else { Some(b.key.clone()) };
            }
            if open {
                ui.indent("mfx-tokens-bar-open", |ui| {
                    if !b.why.is_empty() {
                        ui.label(RichText::new("Why (Claude's words)").small().color(t.text_faint));
                        ui.label(RichText::new(&b.why).color(t.text_dim));
                    }
                    if !b.examples.is_empty() {
                        ui.label(RichText::new("What it did").small().color(t.text_faint));
                        for e in &b.examples {
                            let times = if e.times > 1 { format!("  ×{}", e.times) } else { String::new() };
                            ui.add(egui::Label::new(RichText::new(format!("{}{times}", e.label)).color(t.text_dim)).truncate()).on_hover_text(&e.label);
                        }
                    }
                });
                ui.add_space(4.0);
            }
        }
        if !d.pulled.is_empty() {
            ui.add_space(8.0);
            ui.label(RichText::new("Pulled into the context").color(t.text_dim)).on_hover_text(
                "Each thing read into Claude's context stays there: every later call re-reads it from the cache. Big, early reads cost the most.",
            );
            let most = d.pulled.first().map_or(1, |p| p.carried.max(1));
            for p in &d.pulled {
                let (row, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                let painter = ui.painter();
                let inner = row.shrink2(vec2(6.0, 2.0));
                let g = crate::nodes::canvas::line(painter, &p.label, 11.5, t.text, inner.width() - 120.0);
                painter.galley(inner.left_top(), g, t.text);
                painter.text(
                    inner.right_top(),
                    egui::Align2::RIGHT_TOP,
                    format!("{} × {}", short(p.tokens), p.calls_after),
                    egui::FontId::proportional(11.5),
                    t.text_dim,
                );
                #[allow(clippy::cast_precision_loss)]
                let w = inner.width() * (p.carried as f32 / most as f32);
                let bar = Rect::from_min_size(pos2(inner.min.x, inner.min.y + 16.0), vec2(w.max(1.0), 6.0));
                painter.rect_filled(Rect::from_min_size(bar.min, vec2(inner.width(), 6.0)), 2.0, t.field_bg);
                painter.rect_filled(bar, 2.0, colours(t)[1]);
                painter.line_segment([row.left_bottom(), row.right_bottom()], Stroke::new(1.0, t.separator));
                let from = if p.from.is_empty() { String::new() } else { format!("\nFrom: {}", p.from) };
                resp.on_hover_text(format!(
                    "{}{from}\n{} tokens, re-read by {} later calls: {} tokens in all ({})",
                    p.label,
                    short(p.tokens),
                    p.calls_after,
                    short(p.carried),
                    p.group
                ));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_numbers_and_times() {
        assert_eq!(short(812), "812");
        assert_eq!(short(1_234), "1.2 k");
        assert_eq!(short(12_869), "13 k");
        assert_eq!(short(35_296_544), "35.3 M");
        assert!(when(Some("2026-10-17T21:39:08.975Z")).starts_with("Oct 17, "));
        assert_eq!(when(None), "");
        assert_eq!(when(Some("junk")), "");
    }

    /// Hostile replies: missing or wrong fields fall back to defaults, never a crash.
    #[test]
    fn replies_parse_with_defaults() {
        let d: Detail = serde_json::from_value(json!({"run": {"id": "x", "total": 5}, "bars": [{"key": "Read files"}], "pulled": [{}]})).unwrap();
        assert_eq!(d.run.total, 5);
        assert_eq!(d.bars.len(), 1);
        assert_eq!(d.pulled.first().map(|p| p.carried), Some(0));
        assert!(serde_json::from_value::<Detail>(json!({"run": {"total": -3}})).is_err());
    }
}
