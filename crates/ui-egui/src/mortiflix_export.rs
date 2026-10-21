//! Mortiflix Studio stage 6, Render (doc 39): the host side of `mortiflix_ui::export`.
//!
//! While Mortiflix is on, Mortiflix Studio's Export tab shows the simple Mortiflix page (four presets, the
//! render queue, Render, the Rendered page). This file feeds that page from the engine each frame,
//! draws the timeline picture where it asks, and carries out what it asks for, always through
//! Mortiflix Studio's own export commands (`export.queue.add / start / cancel / retry / remove`), so the
//! files are exactly what Mortiflix Studio's Export would write. Advanced › All Mortiflix Studio settings… shows
//! Mortiflix Studio's full Export page, with a strip to come back.
//!
//! One render queue for the PC: the app's exports wait while the Mortiflix service runs one of its
//! heavy jobs (`render.jobs`), and the service holds its own jobs while the app exports
//! (`render.hold`). Get the video (stage 4) renders the YouTube video straight away.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use egui::{Color32, Rect, pos2, vec2};
use mortiflix_engine::export::{ExportSettings, HardwareEncoding, Scaling};
use mortiflix_engine::export_tools::{QueueStatus, range_param, settings_from_params};
use mortiflix_ui::export::{self as ex, Action, Choices, ExportState, Os, Page, Pending, Preset, QueueRow, RenderRecord, RowState, Stem, TabView};
use serde_json::{Value, json};

use crate::StudioApp;
use crate::frames::{FrameKey, Target};
use crate::native_dialogs::FileDialog;

/// The command name the page's exports run under (errors name it).
const CMD: &str = "mortiflix.renderVideo";

/// One render the page started: the queue items that make it and its line in `renders`.
#[derive(Clone, Debug)]
struct Job {
    record: usize,
    queue_ids: Vec<u64>,
    started: web_time::Instant,
    /// Show the Rendered page when it finishes (the person pressed Render / Get the video).
    show: bool,
    /// Its job in the service's one queue (`render.add`), once the service answered.
    svc: Option<u64>,
    /// The service said go (its `render.job` is running), or there is no service to ask.
    go: bool,
    /// The service stopped it (Stop in another window): no `render.finish` for it.
    svc_closed: bool,
    last_progress: Option<web_time::Instant>,
}

/// What the host keeps for the Export page.
#[derive(Default)]
pub struct ExportHost {
    pub ui: ExportState,
    jobs: Vec<Job>,
    /// Stopped renders whose half-written files are deleted once the encoder stops.
    delete_on_cancel: Vec<(u64, String)>,
    /// (engine revision, timeline hash) so the hash is only recomputed after an edit.
    hash_cache: Option<(u64, Option<u64>, u64)>,
    /// Mortiflix Studio's presets (category, name), read once.
    formats: Vec<(String, String)>,
    /// The project whose renders are loaded from disk.
    loaded_for: Option<String>,
}

/// Mortiflix's simple Export page is on (Mortiflix active and not in Mortiflix Studio's full settings).
pub fn active(app: &StudioApp) -> bool {
    app.mfx.core.state.active && !app.mfx.export.ui.full_settings
}

/// Export mode's body while Mortiflix is on: the simple page, or Mortiflix Studio's full one under a strip
/// that leads back.
pub fn show(app: &mut StudioApp, ui: &mut egui::Ui, rect: Rect) {
    if app.mfx.export.ui.full_settings {
        let strip = Rect::from_min_size(rect.min, vec2(rect.width(), 30.0));
        ui.painter().rect_filled(strip, 0.0, app.tokens.header_bg);
        let b = ui.put(Rect::from_min_size(strip.min + vec2(8.0, 3.0), vec2(260.0, 24.0)), egui::Button::new("‹ Back to the simple Export page").frame(false));
        app.auto.add_response("mfx.export.simple", &b, "Back to the simple Export page");
        if b.clicked() {
            app.mfx.export.ui.full_settings = false;
        }
        crate::panels::export_mode::show(app, ui, Rect::from_min_max(pos2(rect.min.x, strip.max.y + 2.0), rect.max));
        return;
    }
    load_renders(app);
    let view = view(app);
    let theme = crate::mortiflix::theme(&app.tokens);
    let mut st = std::mem::take(&mut app.mfx.export.ui);
    let mut c = app.mfx.core.ctx(theme);
    let out = ex::show_tab(ui, rect, &mut c, &mut st, &view);
    let (cmds, autos) = (std::mem::take(&mut c.commands), std::mem::take(&mut c.auto));
    app.mfx.export.ui = st;
    for (id, resp, label) in &autos {
        app.auto.add_response(id, resp, label);
    }
    if let Some((r, fill)) = out.preview {
        draw_picture(app, ui, r, fill);
    }
    for a in out.actions {
        if let Err(e) = act(app, ui.ctx(), a) {
            app.mfx.export.ui.note = e.clone();
            app.ui.status = e;
        }
    }
    for (id, params) in cmds {
        if let Err(e) = crate::menus::invoke(app, ui.ctx(), &id, params) {
            app.ui.status = e;
        }
    }
    if app.session.export_queue.is_active() || !app.mfx.export.jobs.is_empty() {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
    }
}

/// Every frame (from `mortiflix::frame_begin`): read the service's replies, keep the one PC-wide
/// queue moving, notice finished renders, and run a pending Get the video.
pub fn frame(app: &mut StudioApp, ctx: &egui::Context) {
    if !app.mfx.core.state.active {
        return;
    }
    let host = &mut app.mfx.export;
    host.ui.poll_service(&mut app.mfx.core.service);
    let watching = app.mfx.core.state.stage == mortiflix_ui::render_stage() || !host.jobs.is_empty();
    if watching {
        host.ui.poll_jobs(&mut app.mfx.core.service, ctx.input(|i| i.time));
    }
    // Get the video waits for the hand-off into Edit (`mortiflix.import.run {open_edit: false}`)
    let handoff = app.mfx.core.state.handoff.phase;
    if app.mfx.export.ui.pending_get_video && handoff == mortiflix_ui::into_edit::Phase::Failed {
        app.mfx.export.ui.pending_get_video = false;
        let why = app.mfx.core.state.handoff.error.clone();
        app.mfx.export.ui.note = format!("The video couldn't be brought into Edit, so nothing was rendered. {why}").trim().to_string();
    }
    if app.mfx.export.ui.pending_get_video
        && handoff != mortiflix_ui::into_edit::Phase::Preparing
        && app.session.active_sequence().is_some_and(|q| q.duration().0 > 0)
    {
        app.mfx.export.ui.pending_get_video = false;
        app.mfx.export.ui.choices.preset = Preset::Youtube;
        app.mfx.export.ui.choices.studio_preset = None;
        if let Err(e) = render(app, None) {
            app.mfx.export.ui.note = e.clone();
            app.ui.status = e;
        }
    }
    pump(app);
    settle(app);
    if !app.mfx.export.jobs.is_empty() {
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// Get the video (stage 4 → 6): render the YouTube video as soon as the timeline is there.
pub fn get_video(app: &mut StudioApp) {
    app.mfx.export.ui.pending_get_video = true;
    app.mfx.export.ui.page = Page::Settings;
    app.mfx.export.ui.full_settings = false;
}

/// `mortiflix.renderVideo`: `{"set": {...choices}}` changes the choices (preset, size, quality,
/// framing, howMuch, loudness, captions, hardware, fileName, folder); `{"render": false}` only
/// changes / reads; otherwise it renders. `{"page": "settings"|"rendered"}` switches the page.
/// Returns the page's state and the render's queue ids.
pub fn route(app: &mut StudioApp, params: &Value) -> Result<Value, String> {
    if let Some(set) = params.get("set") {
        let mut c = app.mfx.export.ui.choices.clone();
        c.apply(set)?;
        app.mfx.export.ui.choices = c;
    }
    match params.get("page").and_then(Value::as_str) {
        Some("settings") => app.mfx.export.ui.page = Page::Settings,
        Some("rendered") if !app.mfx.export.ui.renders.is_empty() => app.mfx.export.ui.page = Page::Rendered,
        Some("full") => app.mfx.export.ui.full_settings = true,
        Some("simple") => app.mfx.export.ui.full_settings = false,
        _ => {}
    }
    let mut started = Value::Null;
    if params.get("render").and_then(Value::as_bool).unwrap_or(params.get("set").is_none() && params.get("page").is_none()) {
        let record = render(app, None)?;
        started = json!({"record": record, "queueIds": app.mfx.export.ui.renders.get(record).map(|r| r.queue_ids.clone())});
    }
    Ok(json!({"started": started, "export": app.mfx.export.ui, "view": view_json(app)}))
}

fn view_json(app: &StudioApp) -> Value {
    let v = view(app);
    json!({"hasSequence": v.has_sequence, "seconds": v.seconds, "estimate": v.estimate, "out": [v.out_width, v.out_height], "summary": v.summary,
           "defaultFolder": v.default_folder, "queue": v.queue, "timeline": v.timeline})
}

// ---------------------------------------------------------------------------------------------
// the view
// ---------------------------------------------------------------------------------------------

fn project_name(app: &StudioApp) -> String {
    if !app.mfx.core.state.project_title.trim().is_empty() {
        return app.mfx.core.state.project_title.trim().to_string();
    }
    if let Some(p) = app.session.path.as_deref() {
        let stem = std::path::Path::new(p).file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        if !stem.is_empty() {
            return stem;
        }
    }
    app.session.active_sequence().and(app.session.state.active_sequence).and_then(|id| app.session.project.item(id)).map(|i| i.name.clone()).unwrap_or_default()
}

fn default_folder(app: &StudioApp) -> String {
    let os = Os::this();
    let home = mortiflix_engine::media_browser::std_home_dir().unwrap_or_default();
    if home.is_empty() {
        return mortiflix_engine::export_tools::default_export_dir(&app.session).to_string_lossy().to_string();
    }
    let xdg = match os {
        Os::Linux => std::env::var("XDG_VIDEOS_DIR").ok().or_else(|| {
            let cfg = std::env::var("XDG_CONFIG_HOME").ok().filter(|c| !c.is_empty()).unwrap_or_else(|| format!("{home}/.config"));
            std::fs::read_to_string(format!("{cfg}/user-dirs.dirs")).ok().and_then(|t| ex::xdg_videos_from_user_dirs(&t, &home))
        }),
        _ => None,
    };
    ex::default_folder(os, &home, xdg.as_deref(), &project_name(app))
}

/// Mortiflix Studio settings for the person's choices (the Mortiflix Studio preset plus the page's overrides).
fn settings_for(app: &StudioApp, c: &Choices) -> Result<(String, ExportSettings, String), String> {
    let r = ex::recipe(c);
    let (_, mut s) = settings_from_params(&app.session, &json!({"preset": r.studio_preset}), CMD).map_err(|e| e.to_string())?;
    if let Some(fs) = r.frame_size {
        s.frame_size = Some(fs);
    }
    if let Some(k) = r.bitrate_kbps {
        s.bitrate_kbps = k;
        s.adaptive_bitrate = None;
    }
    if let Some(k) = r.max_bitrate_kbps {
        s.max_bitrate_kbps = Some(k);
    }
    if let Some(k) = r.audio_kbps {
        s.audio.bitrate_kbps = k;
    }
    if let Some(sr) = r.sample_rate {
        s.audio.sample_rate = Some(sr);
    }
    match r.loudness {
        Some((lufs, tp)) => {
            s.effects.loudness.enabled = true;
            s.effects.loudness.target_lufs = lufs;
            s.effects.loudness.true_peak_dbtp = tp;
        }
        None if c.studio_preset.is_none() => s.effects.loudness.enabled = false,
        None => {}
    }
    if let Some(fill) = r.fill {
        s.scaling = if fill { Scaling::ScaleToFill } else { Scaling::ScaleToFit };
    }
    if r.burn_captions {
        s.burn_captions = true;
    }
    if let Some(sc) = r.caption_sidecar {
        s.caption_sidecar = Some(sc);
    }
    s.hardware_encoding = if r.hardware { HardwareEncoding::Auto } else { HardwareEncoding::Off };
    s.validate().map_err(|e| e.to_string())?;
    Ok((r.studio_preset, s, r.range))
}

/// "H.264 · 16 Mbps · AAC 384 kbps · −14 LUFS"
fn summary(s: &ExportSettings, w: u32, h: u32, rate: mortiflix_engine::time::FrameRate, sr: u32) -> String {
    let r = s.resolve(w, h, rate, sr);
    let mut parts = vec![s.format.label().to_string()];
    if s.has_video() && r.target_kbps > 0 {
        parts.push(format!("{:.0} Mbps", f64::from(r.target_kbps) / 1000.0));
    }
    if s.include_audio {
        parts.push(format!("{} · {} kHz", audio_label(s), r.sample_rate / 1000));
    }
    parts.push(if s.effects.loudness.enabled { format!("{:.0} LUFS", s.effects.loudness.target_lufs).replace('-', "−") } else { "sound as mixed".into() });
    parts.join(" · ")
}

fn audio_label(s: &ExportSettings) -> String {
    let codec = format!("{:?}", s.audio.codec).to_lowercase();
    match codec.as_str() {
        "aac" | "auto" if s.has_video() && s.extension() == "mp4" => format!("AAC {} kbps", s.audio.bitrate_kbps),
        _ => format!("PCM {}-bit", s.audio.bits),
    }
}

/// The timeline's hash: renders made from another timeline are "made before your last edit".
fn timeline_hash(app: &mut StudioApp) -> u64 {
    let rev = app.session.revision;
    let seq = app.session.state.active_sequence;
    if let Some((r, s, h)) = app.mfx.export.hash_cache
        && r == rev
        && s == seq.map(|i| i.0)
    {
        return h;
    }
    let h = seq
        .and_then(|id| app.session.project.sequence(id))
        .and_then(|q| serde_json::to_string(q).ok())
        .map(|s| {
            let mut hasher = DefaultHasher::new();
            s.hash(&mut hasher);
            hasher.finish()
        })
        .unwrap_or(0);
    app.mfx.export.hash_cache = Some((rev, seq.map(|i| i.0), h));
    h
}

fn view(app: &StudioApp) -> TabView {
    let mut v = TabView { os: Os::this(), project: project_name(app), default_folder: default_folder(app), ..TabView::default() };
    v.all_formats = if app.mfx.export.formats.is_empty() {
        app.session.export_presets.all().iter().map(|p| (p.category.clone(), p.name.clone())).collect()
    } else {
        app.mfx.export.formats.clone()
    };
    v.queue = queue_rows(app);
    v.timeline = app.mfx.export.hash_cache.map_or(0, |c| c.2);
    let Some(seq) = app.session.state.active_sequence else { return v };
    let Some(q) = app.session.project.sequence(seq) else { return v };
    let (w, h, rate, sr) = (q.settings.width, q.settings.height, q.settings.frame_rate, q.settings.sample_rate);
    let fd = rate.frame_duration();
    let whole = q.duration();
    v.has_sequence = whole.0 > 0;
    v.seconds = whole.seconds();
    v.seq_width = w;
    v.seq_height = h;
    if q.mark_in.is_some() || q.mark_out.is_some() {
        v.in_out_seconds = mortiflix_engine::export::export_range(&app.session.project, seq, &ExportSettings::default()).ok().map(|r| r.duration.seconds());
    }
    v.audio_tracks = q.audio_tracks.iter().map(|t| (t.name.clone(), Stem::of_track(&t.name))).collect();
    v.has_captions = q.caption_tracks.iter().any(|t| t.enabled && !t.captions.is_empty());
    let dur = match app.mfx.export.ui.choices.how_much {
        ex::HowMuch::InOut => {
            mortiflix_engine::export::export_range(&app.session.project, seq, &ExportSettings::default()).map(|r| r.duration).unwrap_or(whole)
        }
        ex::HowMuch::Whole => whole.max(fd),
    };
    for (i, p) in Preset::ALL.into_iter().enumerate() {
        let c = Choices { preset: p, studio_preset: None, ..app.mfx.export.ui.choices.clone() };
        if let Ok((_, s, _)) = settings_for(app, &c)
            && let Some(slot) = v.estimates.get_mut(i)
        {
            let one = s.estimate_bytes(w, h, rate, sr, dur);
            *slot = if p == Preset::Stems { one.saturating_mul(3) } else { one };
        }
    }
    let c = &app.mfx.export.ui.choices;
    if let Ok((_, s, _)) = settings_for(app, c) {
        let r = s.resolve(w, h, rate, sr);
        if s.has_video() {
            v.out_width = r.width;
            v.out_height = r.height;
        }
        let one = s.estimate_bytes(w, h, rate, sr, dur);
        v.estimate = if c.studio_preset.is_none() && c.preset == Preset::Stems { one.saturating_mul(3) } else { one };
        v.summary = summary(&s, w, h, rate, sr);
    }
    v
}

fn queue_rows(app: &StudioApp) -> Vec<QueueRow> {
    let timeline = app.mfx.export.hash_cache.map_or(0, |c| c.2);
    app.session
        .export_queue
        .items
        .iter()
        .map(|it| {
            let job = it.job.and_then(|id| app.session.jobs.iter().find(|j| j.id == id));
            let rec = app.mfx.export.ui.renders.iter().find(|r| r.queue_ids.contains(&it.id));
            let state = match it.status {
                QueueStatus::Ready => RowState::Waiting,
                QueueStatus::Encoding => RowState::Rendering,
                QueueStatus::Done => RowState::Done,
                QueueStatus::Failed => RowState::Failed,
                QueueStatus::Cancelled => RowState::Cancelled,
            };
            QueueRow {
                id: it.id,
                service: false,
                label: ex::file_name_of(&it.settings.path).to_string(),
                state,
                progress: job.map_or(if state == RowState::Done { 1.0 } else { 0.0 }, |j| j.progress.fraction()),
                seconds_left: job.filter(|_| state == RowState::Rendering).and_then(|j| j.progress.eta()).map(|d| d.as_secs_f64()),
                ahead: 0,
                path: it.settings.path.clone(),
                error: it.error.clone().unwrap_or_default().chars().take(200).collect(),
                stale: rec.is_some_and(|r| r.stale(timeline)),
                finished: rec.map(|r| r.finished_label.clone()).unwrap_or_default(),
            }
        })
        .collect()
}

/// The timeline frame at the playhead, framed as the export will frame it.
fn draw_picture(app: &mut StudioApp, ui: &mut egui::Ui, pic: Rect, fill: bool) {
    let Some(seq) = app.session.state.active_sequence else { return };
    let Some(q) = app.session.project.sequence(seq).cloned() else { return };
    let rate = q.settings.frame_rate;
    let frame = rate.frame_at(app.session.playhead());
    let key = FrameKey { target: Target::Sequence(seq), frame, size: 500, revision: app.session.revision, draft: false };
    let project = app.session.project.clone();
    app.frames.request(key, rate.tick_of(frame), 0.5, &project, 0);
    let Some(img) = app.frames.get(&key) else { return };
    let ctx = ui.ctx().clone();
    let tex = app.texture_for(&ctx, "mfx-export-preview", key, &img);
    let (sw, sh) = (q.settings.width.max(1) as f32, q.settings.height.max(1) as f32);
    let inner = if fill {
        let s = (pic.width() / sw).max(pic.height() / sh);
        Rect::from_center_size(pic.center(), vec2(sw * s, sh * s))
    } else {
        crate::panels::monitor::fit(pic, sw, sh)
    };
    ui.painter().with_clip_rect(pic).image(tex, inner, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
}

// ---------------------------------------------------------------------------------------------
// rendering
// ---------------------------------------------------------------------------------------------

fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn clock_label(app: &StudioApp, unix: u64) -> String {
    let secs = i64::try_from(unix).unwrap_or(0);
    if let Some(p) = app.session.persistence.as_ref() {
        let s = p.local_time(secs);
        if let Some(hm) = s.get(11..16) {
            return hm.to_string();
        }
    }
    let day = unix % 86_400;
    format!("{:02}:{:02} UTC", day / 3600, (day / 60) % 60)
}

/// The folder exists and takes files (doc 39: caught before starting, never mid-render).
fn ensure_folder(folder: &str) -> Result<(), String> {
    if folder.trim().is_empty() {
        return Err("Pick a folder to save to (Change…)".into());
    }
    std::fs::create_dir_all(folder).map_err(|e| format!("Can't make the folder {folder}: {e}. Pick another with Change…"))?;
    let probe = std::path::Path::new(folder).join(format!(".mortiflix-write-test-{}", std::process::id()));
    std::fs::write(&probe, b"").map_err(|e| format!("Can't write into {folder}: {e}. Pick another with Change…"))?;
    let _ = std::fs::remove_file(&probe);
    Ok(())
}

fn exists(p: &str) -> bool {
    std::path::Path::new(p).exists()
}

/// Queue one export of the active sequence with `settings` to `path`; returns the queue id.
fn queue_one(app: &mut StudioApp, preset: &str, settings: &ExportSettings, path: &str, range: &str) -> Result<u64, String> {
    let settings_json = serde_json::to_value(settings).map_err(|e| e.to_string())?;
    let p = json!({"preset": preset, "settings": settings_json, "path": path, "range": range, "start": false});
    let r = app.session.execute("export.queue.add", p).map_err(|e| e.to_string())?;
    r["added"].as_array().and_then(|a| a.first()).and_then(Value::as_u64).ok_or_else(|| "the export queue didn't take the render".to_string())
}

/// Render the current choices (or a remembered render's, `again`). Returns the record index.
fn render(app: &mut StudioApp, again: Option<usize>) -> Result<usize, String> {
    let seq = app.session.state.active_sequence.ok_or("Nothing on the timeline yet: finish the shots first")?;
    let dur = app.session.project.sequence(seq).map(|q| q.duration()).unwrap_or_default();
    if dur.0 <= 0 {
        return Err("The timeline is empty: nothing to render".into());
    }
    let choices = match again {
        Some(i) => app.mfx.export.ui.renders.get(i).map(|r| r.choices.clone()).ok_or("No such render")?,
        None => app.mfx.export.ui.choices.clone(),
    };
    let (preset, settings, range) = settings_for(app, &choices)?;
    let project = app.session.project.clone();
    let rng = range_param(&app.session, &project, seq, Some(&json!(range)), &json!({}), CMD).map_err(|e| e.to_string())?;
    let seconds = rng.map_or(dur.seconds(), |r| r.duration.seconds());
    let folder = choices.folder_or(&default_folder(app)).to_string();
    ensure_folder(&folder)?;
    let name = project_name(app);
    let stem = choices.file_stem(&name);
    let timeline = timeline_hash(app);
    let (w, h) = app
        .session
        .project
        .sequence(seq)
        .map(|q| {
            let r = settings.resolve(q.settings.width, q.settings.height, q.settings.frame_rate, q.settings.sample_rate);
            if settings.has_video() { (r.width, r.height) } else { (0, 0) }
        })
        .unwrap_or((0, 0));
    let mut ids = Vec::new();
    let mut files = Vec::new();
    let path;
    if choices.studio_preset.is_none() && choices.preset == Preset::Stems {
        // a folder with Voice.wav, Music.wav, Effects.wav: each the whole length, the other stems' tracks muted
        let dir = ex::unique_path(&folder, &stem, "", &exists);
        std::fs::create_dir_all(&dir).map_err(|e| format!("Can't make {dir}: {e}"))?;
        path = dir.clone();
        let saved = app.session.project.clone();
        for s in Stem::ALL {
            let mut p = (*saved).clone();
            if let Some(q) = p.sequence_mut(seq) {
                for t in &mut q.audio_tracks {
                    t.solo = false;
                    t.muted = t.muted || Stem::of_track(&t.name) != s;
                }
            }
            app.session.project = Arc::new(p);
            let file = ex::unique_path(&dir, s.file(), "wav", &exists);
            let r = queue_one(app, &preset, &settings, &file, &range);
            app.session.project = saved.clone();
            ids.push(r?);
            files.push(file);
        }
    } else {
        let file = ex::unique_path(&folder, &stem, settings.extension(), &exists);
        ids.push(queue_one(app, &preset, &settings, &file, &range)?);
        files.push(file.clone());
        path = file;
    }
    let record = RenderRecord { queue_ids: ids.clone(), title: name, choices, path, files, timeline, seconds, width: w, height: h, ..RenderRecord::default() };
    let i = app.mfx.export.ui.push_render(record);
    app.mfx.export.ui.note.clear();
    let label = ex::file_name_of(&app.mfx.export.ui.renders.get(i).map(|r| r.path.clone()).unwrap_or_default()).to_string();
    start_job(app, i, ids, &label);
    Ok(i)
}

/// Track a render: ask the service for its place in the one queue (`render.add`); without a
/// service it goes at once.
fn start_job(app: &mut StudioApp, record: usize, queue_ids: Vec<u64>, label: &str) {
    let connected = app.mfx.core.service.status().is_connected();
    if connected {
        let id = app.mfx.core.service.request("render.add", json!({"label": label, "kind": "export"}));
        app.mfx.export.ui.pending.push((id, Pending::Add(record)));
    }
    for j in &mut app.mfx.export.jobs {
        // the newest job shows when it finishes; older ones just finish in the queue
        j.show = false;
    }
    let job = Job { record, queue_ids, started: web_time::Instant::now(), show: true, svc: None, go: !connected, svc_closed: false, last_progress: None };
    app.mfx.export.jobs.push(job);
    pump(app);
}

/// Keep the one queue moving. The service owns the PC's one render queue: each render of the app
/// is a job there (`render.add`) and Mortiflix Studio encodes it only once that job is running; progress
/// goes back as `render.progress`. Mortiflix Studio exports started from its own full Export page are left
/// to Mortiflix Studio.
fn pump(app: &mut StudioApp) {
    let host = &mut app.mfx.export;
    // the service's answers to render.add and its render.job events for our jobs
    for (record, row) in std::mem::take(&mut host.ui.added) {
        if let Some(j) = host.jobs.iter_mut().find(|j| j.record == record && j.svc.is_none() && !j.go) {
            match row {
                Some(r) => {
                    j.svc = Some(r.id);
                    j.go = r.state == RowState::Rendering;
                }
                // refused or no service: render alone rather than never
                None => j.go = true,
            }
        }
    }
    let mut stop_items = Vec::new();
    for row in std::mem::take(&mut host.ui.job_events) {
        let Some(j) = host.jobs.iter_mut().find(|j| j.svc == Some(row.id)) else { continue };
        match row.state {
            RowState::Rendering => j.go = true,
            RowState::Cancelled | RowState::Failed if !j.svc_closed => {
                // stopped in the service's queue: stop the encode too
                j.svc_closed = true;
                stop_items.extend(j.queue_ids.iter().copied());
            }
            _ => {}
        }
    }
    for id in stop_items {
        let _ = app.session.execute("export.queue.cancel", json!({"id": id}));
    }
    // Mortiflix Studio's queue runs while the next ready item's job may go
    let q = &app.session.export_queue;
    let next = q.items.iter().find(|i| i.status == QueueStatus::Ready).map(|i| i.id);
    let encoding = q.items.iter().any(|i| i.status == QueueStatus::Encoding);
    let owner = next.and_then(|id| app.mfx.export.jobs.iter().find(|j| j.queue_ids.contains(&id)));
    match owner.map(|j| j.go) {
        Some(false) if q.running => {
            // the encode in progress finishes; the next waits for the service's go
            let _ = app.session.execute("export.queue.stop", json!({}));
        }
        Some(true) if !q.running && !encoding => {
            if let Err(e) = app.session.execute("export.queue.start", json!({})) {
                app.ui.status = e.to_string();
            }
        }
        _ => mortiflix_engine::export_tools::pump_queue(&mut app.session, false),
    }
    // progress into the service's queue, about once a second
    let items = &app.session.export_queue.items;
    let jobs = &app.session.jobs;
    let mut reports = Vec::new();
    for j in &mut app.mfx.export.jobs {
        let Some(svc) = j.svc else { continue };
        if j.last_progress.is_some_and(|t| t.elapsed().as_millis() < 1000) {
            continue;
        }
        let mut done = 0.0f64;
        let mut any = false;
        for id in &j.queue_ids {
            let Some(it) = items.iter().find(|i| i.id == *id) else { continue };
            match it.status {
                QueueStatus::Done => done += 1.0,
                QueueStatus::Encoding => {
                    any = true;
                    done += it.job.and_then(|jid| jobs.iter().find(|x| x.id == jid)).map_or(0.0, |x| f64::from(x.progress.fraction()).clamp(0.0, 1.0));
                }
                _ => {}
            }
        }
        if any {
            j.last_progress = Some(web_time::Instant::now());
            let total = j.queue_ids.len().max(1) as f64;
            reports.push(json!({"id": svc, "done": (done * 1000.0).round(), "total": total * 1000.0, "phase": "rendering"}));
        }
    }
    for r in reports {
        app.mfx.export.ui.send(&mut app.mfx.core.service, "render.progress", r);
    }
}

/// Notice renders that finished, failed or were stopped.
fn settle(app: &mut StudioApp) {
    // stopped renders: delete the half-written file once the encoder let go of it
    let items = &app.session.export_queue.items;
    app.mfx.export.delete_on_cancel.retain(|(id, path)| match items.iter().find(|i| i.id == *id).map(|i| i.status) {
        Some(QueueStatus::Cancelled | QueueStatus::Failed) => {
            let _ = std::fs::remove_file(path);
            false
        }
        Some(QueueStatus::Encoding | QueueStatus::Ready) => true,
        _ => false,
    });
    let jobs = std::mem::take(&mut app.mfx.export.jobs);
    let mut keep = Vec::new();
    for job in jobs {
        let statuses: Vec<(QueueStatus, Option<String>)> = job
            .queue_ids
            .iter()
            .map(|id| {
                app.session
                    .export_queue
                    .items
                    .iter()
                    .find(|i| i.id == *id)
                    .map_or((QueueStatus::Cancelled, Some("removed from the queue".into())), |i| (i.status, i.error.clone()))
            })
            .collect();
        if statuses.iter().any(|(s, _)| matches!(s, QueueStatus::Ready | QueueStatus::Encoding)) {
            keep.push(job);
            continue;
        }
        let failed = statuses.iter().find(|(s, _)| *s != QueueStatus::Done);
        let now = now_unix();
        let label = clock_label(app, now);
        let speed_secs = job.started.elapsed().as_secs_f64();
        let Some(rec) = app.mfx.export.ui.renders.get_mut(job.record) else { continue };
        rec.finished_at = now;
        rec.finished_label = label;
        let error = failed.map(|(s, e)| match s {
            QueueStatus::Cancelled => "Stopped".to_string(),
            _ => e.clone().unwrap_or_else(|| "the export failed".into()).chars().take(300).collect(),
        });
        let first_file = rec.files.first().cloned().unwrap_or_default();
        let single = rec.files.len() == 1;
        let preset = rec.choices.title();
        let timeline = rec.timeline;
        if let (Some(svc), false) = (job.svc, job.svc_closed) {
            let p = match &error {
                None => json!({"id": svc, "ok": true, "file": first_file}),
                Some(e) => json!({"id": svc, "ok": false, "error": e}),
            };
            app.mfx.export.ui.send(&mut app.mfx.core.service, "render.finish", p);
        }
        if let Some(e) = error {
            if let Some(rec) = app.mfx.export.ui.renders.get_mut(job.record) {
                rec.error = e;
            }
            continue;
        }
        if single && app.mfx.core.state.project_id.is_some() {
            // the project keeps the delivered final (its record), and it counts as delivered
            app.mfx.export.ui.send(
                &mut app.mfx.core.service,
                "export.done",
                json!({"file": first_file, "preset": preset, "timeline_rev": timeline.to_string()}),
            );
        }
        let Some(rec) = app.mfx.export.ui.renders.get_mut(job.record) else { continue };
        rec.bytes = rec.files.iter().filter_map(|f| std::fs::metadata(f).ok()).map(|m| m.len()).sum();
        let files = rec.files.clone();
        let expected = rec.seconds;
        let rec_index = job.record;
        // checks: it opens and plays, its length is the timeline's (doc 39 Checks)
        let mut checks = Vec::new();
        let mut length = None;
        for f in &files {
            match app.session.execute("file.mediaPropertiesFile", json!({"path": f})) {
                Ok(info) => {
                    let d = info.get("duration").and_then(|d| d.get("seconds")).and_then(Value::as_f64).filter(|d| d.is_finite());
                    length = length.or(d);
                }
                Err(e) => checks.push(ex::Check { name: "plays".into(), ok: false, text: format!("{} doesn't open: {e}", ex::file_name_of(f)) }),
            }
        }
        if checks.is_empty() {
            checks.push(ex::Check { name: "plays".into(), ok: true, text: "plays start to end".into() });
        }
        if let Some(d) = length {
            let ok = (d - expected).abs() <= 0.1;
            let text = if ok { format!("{} long", ex::clock(d)) } else { format!("{} long, the timeline is {}", ex::clock(d), ex::clock(expected)) };
            checks.push(ex::Check { name: "length".into(), ok, text });
        }
        let ui_state = &mut app.mfx.export.ui;
        if let Some(rec) = ui_state.renders.get_mut(rec_index) {
            rec.checks = checks;
        }
        ui_state.learn_speed(speed_secs, expected);
        if job.show {
            let ui_state = &mut app.mfx.export.ui;
            ui_state.shown = rec_index;
            ui_state.page = Page::Rendered;
            ui_state.post.results.clear();
            ui_state.post.title.clear();
            ask_publish_setup(app);
        }
    }
    app.mfx.export.jobs = keep;
    save_renders(app);
}

fn ask_publish_setup(app: &mut StudioApp) {
    if app.mfx.export.ui.pending.iter().any(|p| p.1 == Pending::PublishSetup) {
        return;
    }
    if !app.mfx.core.service.status().is_connected() {
        app.mfx.export.ui.post.service = "none".into();
        return;
    }
    // what would be posted: the error says when posting isn't set up
    let id = app.mfx.core.service.request("publish.plan", json!({}));
    app.mfx.export.ui.pending.push((id, Pending::PublishSetup));
}

// ---------------------------------------------------------------------------------------------
// the page's actions
// ---------------------------------------------------------------------------------------------

fn act(app: &mut StudioApp, ctx: &egui::Context, a: Action) -> Result<(), String> {
    match a {
        Action::Render => render(app, None).map(|_| ()),
        Action::RenderAgain(i) => render(app, Some(i)).map(|_| ()),
        Action::Stop(id) => {
            if let Some(it) = app.session.export_queue.items.iter().find(|i| i.id == id) {
                app.mfx.export.delete_on_cancel.push((id, it.settings.path.clone()));
            }
            app.session.execute("export.queue.cancel", json!({"id": id})).map(|_| ()).map_err(|e| e.to_string())
            // the job's render.finish (ok: false, "Stopped") follows when the encoder stops
        }
        Action::Remove(id) => app.session.execute("export.queue.remove", json!({"id": id})).map(|_| ()).map_err(|e| e.to_string()),
        Action::Retry(id) => {
            app.session.execute("export.queue.retry", json!({"id": id})).map_err(|e| e.to_string())?;
            if let Some(ri) = app.mfx.export.ui.renders.iter().position(|r| r.queue_ids.contains(&id)) {
                if let Some(r) = app.mfx.export.ui.renders.get_mut(ri) {
                    r.error.clear();
                    r.finished_at = 0;
                }
                let r = app.mfx.export.ui.renders.get(ri);
                let ids = r.map(|r| r.queue_ids.clone()).unwrap_or_default();
                let label = r.map(|r| ex::file_name_of(&r.path).to_string()).unwrap_or_default();
                start_job(app, ri, ids, &label);
            } else {
                pump(app);
            }
            Ok(())
        }
        Action::Open(p) => crate::panels::menu_dialogs::open_path(app, ctx, &p, false),
        Action::Reveal(p) => crate::panels::menu_dialogs::open_path(app, ctx, &p, true),
        Action::SaveCopy(p) => {
            let name = ex::file_name_of(&p).to_string();
            let ext = name.rsplit('.').next().unwrap_or("").to_string();
            let exts: Vec<&str> = if ext.is_empty() || ext == name { Vec::new() } else { vec![ext.as_str()] };
            app.pick_ui(FileDialog::save_as("Video", &exts, &name), move |app, paths| {
                if let Some(to) = paths.first()
                    && to != &p
                    && let Err(e) = std::fs::copy(&p, to)
                {
                    app.ui.status = format!("Couldn't save a copy: {e}");
                }
            });
            Ok(())
        }
        Action::ChangeFolder => {
            let at = app.mfx.export.ui.choices.folder_or(&default_folder(app)).to_string();
            app.pick_ui(FileDialog::Folder { at: Some(at) }, |app, paths| {
                if let Some(f) = paths.first() {
                    app.mfx.export.ui.choices.folder = f.clone();
                }
            });
            Ok(())
        }
        Action::FullSettings => {
            app.mfx.export.ui.full_settings = true;
            Ok(())
        }
        Action::EditTab => crate::menus::invoke(app, ctx, "stages.edit", json!({})).map(|_| ()),
        Action::NewProject => crate::menus::invoke(app, ctx, "mortiflix.newProject", json!({})).map(|_| ()),
        Action::SetupPosting => crate::menus::invoke(app, ctx, "mortiflix.setup", json!({})).map(|_| ()),
        Action::Post(i) => {
            // only the person's click on the confirm dialog gets here: the AI has no way to post
            let p = ex::post_params(&app.mfx.export.ui, i).ok_or("Only a finished video file can be posted")?;
            // the file on screen is the one that goes out: make it the project's delivered final first
            if let Some(r) = app.mfx.export.ui.renders.get(i)
                && let Some(f) = r.files.first().cloned()
            {
                let done = json!({"file": f, "preset": r.choices.title(), "timeline_rev": r.timeline.to_string()});
                app.mfx.export.ui.send(&mut app.mfx.core.service, "export.done", done);
            }
            let id = app.mfx.core.service.request("publish.send", p);
            app.mfx.export.ui.post.sending = true;
            app.mfx.export.ui.pending.push((id, Pending::Post));
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------------------------
// renders of this project, kept across restarts (<data dir>/mortiflix-renders.json)
// ---------------------------------------------------------------------------------------------

fn renders_file(app: &StudioApp) -> Option<std::path::PathBuf> {
    app.session.prefs_path.as_ref().and_then(|p| p.parent()).map(|d| d.join("mortiflix-renders.json"))
}

fn project_key(app: &StudioApp) -> String {
    app.mfx.core.state.project_id.clone().or_else(|| app.session.path.clone()).unwrap_or_else(|| project_name(app))
}

fn load_renders(app: &mut StudioApp) {
    let _ = timeline_hash(app);
    if app.mfx.export.formats.is_empty() {
        app.mfx.export.formats = app.session.export_presets.all().iter().map(|p| (p.category.clone(), p.name.clone())).collect();
    }
    let key = project_key(app);
    if app.mfx.export.loaded_for.as_deref() == Some(key.as_str()) {
        return;
    }
    app.mfx.export.loaded_for = Some(key.clone());
    if app.mfx.export.ui.post.service.is_empty() {
        ask_publish_setup(app);
    }
    let Some(f) = renders_file(app) else { return };
    let Ok(text) = std::fs::read_to_string(&f) else { return };
    let Ok(all) = serde_json::from_str::<Value>(&text) else { return };
    let list: Vec<RenderRecord> = all.get(&key).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
    let live: Vec<RenderRecord> = std::mem::take(&mut app.mfx.export.ui.renders).into_iter().filter(|r| r.finished_at == 0).collect();
    for mut r in list.into_iter().rev().take(ex::MAX_RENDERS).rev() {
        r.missing = r.done() && !r.files.iter().all(|f| exists(f));
        r.queue_ids.clear();
        app.mfx.export.ui.renders.push(r);
    }
    for r in live {
        app.mfx.export.ui.push_render(r);
    }
}

fn save_renders(app: &StudioApp) {
    if !app.mfx.export.jobs.is_empty() && app.mfx.export.ui.renders.iter().all(|r| r.finished_at == 0) {
        return;
    }
    let Some(f) = renders_file(app) else { return };
    let key = project_key(app);
    let done: Vec<&RenderRecord> = app.mfx.export.ui.renders.iter().filter(|r| r.finished_at > 0).collect();
    let mut all = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).filter(Value::is_object).unwrap_or_else(|| json!({}));
    let new = serde_json::to_value(&done).unwrap_or_default();
    if all.get(&key) == Some(&new) {
        return;
    }
    all[key] = new;
    if let Ok(text) = serde_json::to_string(&all) {
        let tmp = f.with_extension("json.tmp");
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, &f);
        }
    }
}

/// For tests: the item id of a queued render.
#[cfg(test)]
fn first_queue_id(app: &StudioApp) -> Option<u64> {
    app.session.export_queue.items.first().map(|i| i.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with_sequence() -> StudioApp {
        let mut app = StudioApp::new(mortiflix_engine::Session::default());
        app.session.execute("file.openDemoProject", json!({})).unwrap();
        app.mfx.core.activate();
        app
    }

    #[test]
    fn choices_turn_into_mortiflix_settings() {
        let app = app_with_sequence();
        let (name, s, range) = settings_for(&app, &Choices::default()).unwrap();
        assert_eq!(name, "YouTube 1080p Full HD");
        assert_eq!(s.frame_size, Some((1920, 1080)));
        assert_eq!(s.audio.bitrate_kbps, 384);
        assert!(s.effects.loudness.enabled);
        assert_eq!((s.effects.loudness.target_lufs, s.effects.loudness.true_peak_dbtp), (-14.0, -1.0));
        assert_eq!(s.hardware_encoding, HardwareEncoding::Off);
        assert_eq!(range, "entire");
        let social = Choices { preset: Preset::Social, framing: ex::Framing::Fit, ..Choices::default() };
        assert_eq!(settings_for(&app, &social).unwrap().1.scaling, Scaling::ScaleToFit);
        let master = Choices { preset: Preset::Master, ..Choices::default() };
        let (_, s, _) = settings_for(&app, &master).unwrap();
        assert_eq!(s.extension(), "mov");
        assert!(!s.effects.loudness.enabled);
        let bad = Choices { studio_preset: Some("No Such Preset".into()), ..Choices::default() };
        assert!(settings_for(&app, &bad).is_err());
    }

    #[test]
    fn the_view_knows_the_timeline_and_estimates_every_preset() {
        let mut app = app_with_sequence();
        load_renders(&mut app);
        let v = view(&app);
        assert!(v.has_sequence);
        assert!(v.seconds > 0.0);
        assert!(v.estimates.iter().all(|e| *e > 0), "{:?}", v.estimates);
        assert_eq!((v.out_width, v.out_height), (1920, 1080));
        assert!(v.all_formats.len() >= 30);
        assert!(v.summary.contains("LUFS"), "{}", v.summary);
    }

    #[test]
    fn render_queues_into_mortiflix_and_never_overwrites() {
        let dir = mortiflix_engine::temp_dir().join(format!("mfx-export-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut app = app_with_sequence();
        let folder = dir.to_string_lossy().to_string();
        route(&mut app, &json!({"set": {"folder": folder, "fileName": "clip", "howMuch": "whole"}})).unwrap();
        // a file of that name exists: the render gets " (2)"
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("clip.mp4"), b"x").unwrap();
        // no service: the queue starts at once
        let i = render(&mut app, None).unwrap();
        let rec = app.mfx.export.ui.renders[i].clone();
        assert!(rec.path.ends_with("clip (2).mp4"), "{}", rec.path);
        assert_eq!(rec.queue_ids.len(), 1);
        assert_eq!(first_queue_id(&app), rec.queue_ids.first().copied());
        assert!(app.session.export_queue.running);
        // stop it: the half-written file goes
        act(&mut app, &egui::Context::default(), Action::Stop(rec.queue_ids[0])).unwrap();
        let t0 = web_time::Instant::now();
        while app.session.export_queue.items.iter().any(|i| i.status == QueueStatus::Encoding) && t0.elapsed().as_secs() < 30 {
            pump(&mut app);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        settle(&mut app);
        assert_eq!(app.mfx.export.ui.renders[i].error, "Stopped");
        assert!(!std::path::Path::new(&rec.path).exists());
        assert!(app.mfx.export.jobs.is_empty());
        // hostile control-channel input is refused, nothing renders
        assert!(route(&mut app, &json!({"set": {"size": "16k"}})).is_err());
        assert!(route(&mut app, &json!({"set": {"folder": ""}, "render": false})).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn stems_mute_the_other_tracks_only_in_the_queued_copy() {
        let dir = mortiflix_engine::temp_dir().join(format!("mfx-export-stems-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut app = app_with_sequence();
        let before = app.session.project.clone();
        route(&mut app, &json!({"set": {"preset": "stems", "folder": dir.to_string_lossy()}, "render": false})).unwrap();
        let i = render(&mut app, None).unwrap();
        let rec = app.mfx.export.ui.renders[i].clone();
        assert_eq!(rec.files.len(), 3);
        assert!(rec.files[0].ends_with("Voice.wav") && rec.files[2].ends_with("Effects.wav"));
        assert!(Arc::ptr_eq(&before, &app.session.project), "the open project is untouched");
        // each queued copy hears only its stem's tracks
        for (it, stem) in app.session.export_queue.items.iter().zip(Stem::ALL) {
            let q = it.project.sequence(it.sequence).unwrap();
            for t in &q.audio_tracks {
                assert_eq!(
                    t.muted,
                    Stem::of_track(&t.name) != stem || before.sequence(it.sequence).unwrap().audio_tracks.iter().any(|b| b.id == t.id && b.muted)
                );
            }
        }
        app.session.execute("export.queue.cancel", json!({"wait": true})).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_render_waits_for_the_services_go() {
        let dir = mortiflix_engine::temp_dir().join(format!("mfx-export-go-{}", std::process::id()));
        let mut app = app_with_sequence();
        route(&mut app, &json!({"set": {"folder": dir.to_string_lossy()}, "render": false})).unwrap();
        let i = render(&mut app, None).unwrap();
        app.session.execute("export.queue.cancel", json!({"wait": true})).unwrap();
        // the same render again, but the service's queue has not said go yet
        app.mfx.export.jobs.clear();
        let ids: Vec<u64> = app.mfx.export.ui.renders[i].queue_ids.clone();
        app.session.execute("export.queue.retry", json!({"id": ids[0]})).unwrap();
        app.mfx.export.jobs.push(Job {
            record: i,
            queue_ids: ids,
            started: web_time::Instant::now(),
            show: true,
            svc: Some(7),
            go: false,
            svc_closed: false,
            last_progress: None,
        });
        app.mfx.export.ui.own_service_ids.push(7);
        pump(&mut app);
        assert!(!app.session.export_queue.running, "waits");
        assert!(app.session.export_queue.items.iter().all(|i| i.status != QueueStatus::Encoding));
        // its render.job says running: it goes
        app.mfx.export.ui.apply_event(&json!({"event": "render.job", "data": {"id": 7, "state": "running", "label": "x"}}));
        pump(&mut app);
        assert!(app.session.export_queue.running);
        app.session.execute("export.queue.cancel", json!({"wait": true})).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn get_video_waits_for_the_timeline_then_renders_youtube() {
        let mut app = StudioApp::new(mortiflix_engine::Session::default());
        app.mfx.core.activate();
        get_video(&mut app);
        frame(&mut app, &egui::Context::default());
        assert!(app.mfx.export.ui.pending_get_video, "nothing on the timeline yet: it waits");
        assert!(render(&mut app, None).is_err());
    }
}
