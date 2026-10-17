//! The Voice & music stage: hear it all before FilmCraft, per line / per cue (docs 34, 33b).
//!
//! Stage 4's main panel (PanelKind::MfxStage on stage 4), mockup `UI Layout/34a Voice and music
//! stage.html`:
//! - a header: the AI's live line, the tally ("7 of 13 lines OK · 2 of 5 cues OK") and the shared
//!   transport (FilmCraft's own player: `playhead.set`, `playback.*`, the same one the loose preview
//!   drives; doc 40 clash 3);
//! - the selected line or cue (doc 34 "Right panel"): Play, words + Remake line, voice (all lines /
//!   this line), takes + New take, Record it yourself (booth), Import a recording, timing ±0.1 s
//!   with the fit, level, OK / Fix / Cut it; for a cue: Play, versions + Swap, the theme, Shorter /
//!   Longer by a bar with the landing check, level, Quieter under the voice, OK / Fix / Cut it; a
//!   planned cue (before the score): its feel and energy (doc 33b);
//! - the lanes (Shots, Music, Voice) on one time axis, with a drag bar to resize them.
//!
//! The move-on buttons (Edit the video / Get the video) are the stage bar's (owner answer 1).
//! Data (part 08's protocol, `docs/mortiflix/protocol.md`): `graph.get` (every piece, its state,
//! verdict and versions), `voicemusic.view` (seconds, mix, your takes, the cue plan), `voice.list`
//! and `music.theme.get`; refreshed on `node.changed` / `status`. Changes: `node.edit` (OK / Fix /
//! Cut it), `node.pick` (which version plays), `mix.set` (level, timing, voice, ducking, length),
//! `voice.line.take`, `voice.line.words`, `voice.booth.import` / `voice.booth.keep`,
//! `music.cue.set`. The theme card and the per-beat cue plan for the chat during Look and Shots
//! (doc 33b) are [`show_theme_card`] and [`show_cue_plan`].

pub mod lanes;
pub mod model;
pub mod theme;

use egui::{Color32, RichText, Sense, Stroke, vec2};
use serde_json::{Value, json};

use crate::state::{StageStatus, StatusKind};
use crate::{MfxCtx, STAGE_VOICE_MUSIC};
use model::{Fit, Kind, Piece, PieceState, Timeline, fmt_db, fmt_secs, fmt_time};

pub use theme::{show_cue_plan, show_theme_card};

/// Re-read the pieces this often while something is being made (seconds).
const REFRESH_BUSY_SEC: f64 = 3.0;
/// Otherwise this often (an AI session may add a take at any time).
const REFRESH_IDLE_SEC: f64 = 15.0;
/// Lanes panel height bounds (doc 34: 200-640 px, here never more than the panel allows).
const LANES_MIN: f32 = 150.0;
const LANES_MAX: f32 = 640.0;
/// Audio files the booth / Import a recording take (doc 34).
const AUDIO_EXTS: [&str; 8] = ["wav", "aif", "aiff", "flac", "mp3", "m4a", "ogg", "opus"];

/// What the stage keeps between frames (an egui temporary: nothing here is saved).
#[derive(Clone, Default)]
struct Vm {
    tl: Timeline,
    graph: Value,
    view: Option<Value>,
    voices: Value,
    theme: Value,
    /// The four reads, in flight: graph.get, voicemusic.view, voice.list, music.theme.get.
    reqs: [Option<u64>; 4],
    /// Requests sent by a button: (id, what to say when it's done).
    ops: Vec<(u64, String)>,
    seq: u64,
    project: Option<String>,
    loaded: bool,
    /// graph.get failed (no project, service down): its words.
    problem: String,
    fetched_at: Option<f64>,
    sel: Option<(Kind, String)>,
    /// The words being edited: (line id, text).
    words: Option<(String, String)>,
    /// The inline Fix note: (piece id, text).
    fix: Option<(String, String)>,
    /// The answer to the last change ("Line 3·3: a new take is being made.") or an error.
    msg: Option<(String, bool)>,
    /// Where this stage last put the playhead.
    cursor: f64,
    lanes: lanes::LaneUi,
    lanes_h: f32,
    /// The booth is open on this line.
    booth: Option<String>,
    /// Voice applies to: all lines (true) or this line only.
    voice_all: bool,
    /// A slider being dragged: (piece id, which, dB).
    slider: Option<(String, &'static str, f32)>,
    /// A planned cue's feel / energy being edited: (cue id, feel, energy).
    plan: Option<(String, String, u8)>,
    /// The stage-bar status this stage last filled in (it never overrides the service's own).
    status_set: Option<StageStatus>,
}

fn key() -> egui::Id {
    egui::Id::new("mfx-voice-music")
}

/// Stage 4's main panel (PanelKind::MfxStage on stage 4).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let mut vm: Vm = ui.ctx().data_mut(|d| d.get_temp::<Vm>(key())).unwrap_or_default();
    sync(ui, ctx, &mut vm);
    if crate::service_notice(ui, ctx) {
        ui.add_space(4.0);
    }
    header(ui, ctx, &mut vm);
    ui.separator();

    if !vm.loaded {
        let text = if vm.problem.is_empty() { "Reading the voice lines and music cues…".to_string() } else { vm.problem.clone() };
        ui.colored_label(ctx.theme.text_faint, text);
        store(ui, vm);
        return;
    }

    // The detail panel above, the lanes below with a drag bar between them.
    let lanes_need = lanes::height(&vm.tl) + 8.0;
    let total = ui.available_height();
    if vm.lanes_h <= 0.0 {
        vm.lanes_h = lanes_need.max(LANES_MIN);
    }
    let lanes_h = vm.lanes_h.clamp(LANES_MIN.min(total * 0.5), LANES_MAX.min((total - 120.0).max(LANES_MIN)));
    let detail_h = (total - lanes_h - 8.0).max(80.0);
    egui::ScrollArea::vertical().id_salt("mfx-vm-detail").max_height(detail_h).auto_shrink([false, false]).show(ui, |ui| {
        detail(ui, ctx, &mut vm);
    });
    // drag bar
    let (bar, bar_resp) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::drag());
    let bar_resp = bar_resp.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    ui.painter().line_segment([bar.left_center(), bar.right_center()], Stroke::new(if bar_resp.hovered() { 2.0 } else { 1.0 }, ctx.theme.separator));
    ctx.auto("mfx.vm.lanes.resize", &bar_resp, "Resize the lanes");
    if bar_resp.dragged() {
        vm.lanes_h = (lanes_h - bar_resp.drag_delta().y).clamp(LANES_MIN, LANES_MAX);
    }
    let sel = vm.sel.clone();
    let out = lanes::show(ui, ctx, &vm.tl, &mut vm.lanes, sel.as_ref(), vm.cursor);
    lane_actions(ctx, &mut vm, out);

    booth(ui, ctx, &mut vm);
    keys(ui, ctx, &mut vm);
    stage_status(ctx, &mut vm);
    store(ui, vm);
}

fn store(ui: &egui::Ui, vm: Vm) {
    let busy = vm.tl.busy() > 0 || vm.reqs.iter().any(Option::is_some) || !vm.ops.is_empty();
    ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(if busy { 1.0 } else { REFRESH_IDLE_SEC }));
    ui.ctx().data_mut(|d| d.insert_temp(key(), vm));
}

fn err_text(e: &Value) -> String {
    e.get("message").and_then(Value::as_str).filter(|m| !m.is_empty()).unwrap_or("That didn't work.").chars().take(300).collect()
}

/// Take replies, follow events and the shared selection, and read the pieces again when needed.
fn sync(ui: &egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm) {
    let now = ui.input(|i| i.time);
    let mut rebuild = false;
    for (slot, req) in vm.reqs.iter_mut().enumerate() {
        let Some(r) = req.and_then(|id| ctx.service.take_reply(id)) else { continue };
        *req = None;
        rebuild = true;
        match slot {
            0 => {
                vm.fetched_at = Some(now);
                if r.ok {
                    vm.graph = r.result;
                    vm.loaded = true;
                    vm.problem.clear();
                } else {
                    let code = r.error.get("code").and_then(Value::as_str).unwrap_or("");
                    vm.problem = match code {
                        "no_project" => "Open or make a project first (Back: New project). Its voice lines and music cues show here.".into(),
                        "not_running" => String::new(),
                        _ => format!("Can't read the pieces: {}", err_text(&r.error)),
                    };
                }
            }
            1 => vm.view = r.ok.then_some(r.result),
            2 => vm.voices = if r.ok { r.result } else { Value::Null },
            _ => vm.theme = if r.ok { r.result } else { Value::Null },
        }
    }
    let mut changed = false;
    let mut done = Vec::new();
    vm.ops.retain(|(id, what)| match ctx.service.take_reply(*id) {
        Some(r) => {
            done.push(if r.ok { (what.clone(), false) } else { (err_text(&r.error), true) });
            changed = true;
            false
        }
        None => true,
    });
    if let Some(m) = done.pop() {
        vm.msg = Some(m);
    }
    for (seq, ev) in ctx.service.events_since(vm.seq) {
        vm.seq = vm.seq.max(*seq);
        if matches!(ev.get("event").and_then(Value::as_str), Some("node.changed" | "status")) {
            changed = true;
        }
    }
    if vm.project != ctx.state.project_id {
        vm.project = ctx.state.project_id.clone();
        changed = true;
        vm.sel = None;
    }
    if rebuild {
        vm.tl = model::build(&vm.graph, vm.view.as_ref());
        model::apply_voices(&mut vm.tl, &vm.voices);
        model::apply_theme(&mut vm.tl, &vm.theme);
        if let Some((k, id)) = &vm.sel
            && vm.tl.find(*k, id).is_none()
        {
            vm.sel = None;
        }
    }
    let every = if vm.tl.busy() > 0 { REFRESH_BUSY_SEC } else { REFRESH_IDLE_SEC };
    let stale = vm.fetched_at.is_none_or(|t| now - t > every);
    if vm.reqs.iter().all(Option::is_none) && (changed || stale) && ctx.service.status().is_connected() {
        vm.reqs = ["graph.get", "voicemusic.view", "voice.list", "music.theme.get"].map(|ty| Some(ctx.service.request(ty, json!({}))));
    }
    // The chat's "talking about" and the node area select a line or cue too (shared selection).
    if let Some(id) = ctx.state.selected_node.clone()
        && vm.sel.as_ref().is_none_or(|s| s.1 != id)
        && let Some(p) = vm.tl.find_any(&id)
    {
        vm.sel = Some((p.kind, p.id.clone()));
    }
}

fn send(ctx: &mut MfxCtx, vm: &mut Vm, ty: &str, params: Value, done: String) {
    let id = ctx.service.request(ty, params);
    vm.ops.push((id, done));
}

fn select(ctx: &mut MfxCtx, vm: &mut Vm, kind: Kind, id: &str, seek: bool) {
    vm.sel = Some((kind, id.to_string()));
    ctx.state.selected_node = Some(id.to_string());
    if seek && let Some(p) = vm.tl.find(kind, id) {
        let at = p.start;
        seek_to(ctx, vm, at);
    }
}

fn seek_to(ctx: &mut MfxCtx, vm: &mut Vm, at: f64) {
    vm.cursor = at.max(0.0);
    ctx.command("playhead.set", json!({"seconds": vm.cursor}));
}

/// Play from `at` in FilmCraft's player (the transport the loose preview shares).
fn play_from(ctx: &mut MfxCtx, vm: &mut Vm, at: f64) {
    ctx.command("playback.stop", json!({}));
    seek_to(ctx, vm, at);
    ctx.command("playback.forward", json!({}));
}

fn header(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm) {
    let t = ctx.theme;
    ui.horizontal(|ui| {
        let busy = vm.tl.busy();
        if !ctx.state.ai_line.is_empty() {
            ui.colored_label(t.ok, "●");
            ui.colored_label(t.text_dim, ctx.state.ai_line.chars().take(120).collect::<String>());
        } else if busy > 0 {
            ui.colored_label(t.ok, "●");
            ui.colored_label(t.text_dim, format!("Making {busy} {}", if busy == 1 { "piece" } else { "pieces" }));
        }
        let tally = vm.tl.tally_text();
        if !tally.is_empty() {
            ui.colored_label(t.text, tally);
        }
    });
    // The shared transport: ⏮ ⏭ previous / next line, Play all, Play this beat.
    ui.horizontal(|ui| {
        let prev = ui.button("⏮").on_hover_text("Previous line");
        ctx.auto("mfx.vm.prevLine", &prev, "Previous line");
        let all = ui.button("▶ Play all").on_hover_text("The whole video with voice and music, from the playhead (Space)");
        ctx.auto("mfx.vm.playAll", &all, "Play all");
        let sel_beat = vm
            .sel
            .as_ref()
            .and_then(|(k, id)| vm.tl.find(*k, id))
            .and_then(|p| p.beat.clone())
            .or_else(|| vm.tl.shot_at(vm.cursor).and_then(|s| s.beat.clone()));
        let beat_label = sel_beat.as_deref().and_then(model::beat_no).map_or("▶ Play beat".to_string(), |n| format!("▶ Play beat {n}"));
        let beat = ui.add_enabled(sel_beat.is_some(), egui::Button::new(&beat_label));
        ctx.auto("mfx.vm.playBeat", &beat, &beat_label);
        let next = ui.button("⏭").on_hover_text("Next line");
        ctx.auto("mfx.vm.nextLine", &next, "Next line");
        let stop = ui.button("■").on_hover_text("Stop");
        ctx.auto("mfx.vm.stop", &stop, "Stop");
        ui.colored_label(t.text_faint, format!("{} / {}", fmt_time(vm.cursor), fmt_time(vm.tl.duration)));
        if all.clicked() {
            ctx.command("playback.toggle", json!({}));
        }
        if stop.clicked() {
            ctx.command("playback.stop", json!({}));
        }
        if beat.clicked()
            && let Some((a, _)) = sel_beat.as_deref().and_then(|b| vm.tl.beat_span(b))
        {
            play_from(ctx, vm, a);
        }
        for (r, dir) in [(&prev, -1), (&next, 1)] {
            if r.clicked() {
                let cur = vm.sel.as_ref().filter(|s| s.0 == Kind::Line).map(|s| s.1.clone());
                if let Some(id) = vm.tl.step_line(cur.as_deref(), dir).map(|l| l.id.clone()) {
                    select(ctx, vm, Kind::Line, &id, true);
                }
            }
        }
    });
    if let Some((m, bad)) = &vm.msg {
        ui.colored_label(if *bad { t.error } else { t.text_dim }, m);
    }
}

/// The selected line or cue, or the whole video's summary.
fn detail(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm) {
    let piece = vm.sel.as_ref().and_then(|(k, id)| vm.tl.find(*k, id)).cloned();
    match piece {
        Some(p) if p.kind == Kind::Line => line_panel(ui, ctx, vm, &p),
        Some(p) if p.kind == Kind::Cue => cue_panel(ui, ctx, vm, &p),
        Some(p) => shot_panel(ui, ctx, vm, &p),
        None => summary(ui, ctx, vm),
    }
}

/// "▌3 Winter · line 3·3" with the beat's colour bar, and Back to the whole video.
fn title_row(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm, p: &Piece, what: &str) {
    let t = ctx.theme;
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(4.0, 18.0), Sense::hover());
        ui.painter().rect_filled(r, 1.0, p.colour.unwrap_or(t.separator));
        let beat = vm.tl.beat(p.beat.as_deref());
        let head = match beat {
            Some(b) => format!("{} {} · {what} {}", b.no, b.title.chars().take(40).collect::<String>(), p.label),
            None => format!("{what} {}", p.label),
        };
        ui.label(RichText::new(head).color(t.text).strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let back = ui.small_button("Back to the whole video");
            ctx.auto("mfx.vm.back", &back, "Back to the whole video");
            if back.clicked() {
                vm.sel = None;
                ctx.state.selected_node = None;
            }
        });
    });
    let state = if p.error.is_empty() { p.state.words().to_string() } else { format!("{}: {}", p.state.words(), p.error) };
    ui.colored_label(if p.state == PieceState::Failed { t.error } else { t.text_dim }, state);
}

/// OK / Fix / Cut it (and taking a verdict back), the same as the shots. Fix opens a note.
fn verdicts(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm, p: &Piece, what: &str) {
    let t = ctx.theme;
    ui.horizontal(|ui| {
        let (ok_label, ok_action) = if p.state == PieceState::Ok { ("✓ OK · Take back", "clear") } else { ("OK", "ok") };
        let ok = ui.add_enabled(p.state != PieceState::Cut, egui::Button::new(RichText::new(ok_label).color(Color32::WHITE)).fill(t.accent));
        ctx.auto("mfx.vm.ok", &ok, "OK");
        let fix = ui.button("Fix");
        ctx.auto("mfx.vm.fix", &fix, "Fix");
        let (cut_label, cut_action) = if p.state == PieceState::Cut { ("Put it back", "clear") } else { ("Cut it", "cut") };
        let cut = ui.button(cut_label);
        ctx.auto("mfx.vm.cut", &cut, cut_label);
        if ok.clicked() {
            let done =
                if ok_action == "ok" { format!("{what} {} is OK. The AI won't touch it.", p.label) } else { format!("{what} {}: OK taken back.", p.label) };
            send(ctx, vm, "node.edit", json!({"node": p.id, "action": ok_action}), done);
        }
        if cut.clicked() {
            let done = if cut_action == "cut" {
                format!("{what} {} is cut. It stays in the project, out of the video.", p.label)
            } else {
                format!("{what} {} is back in the video.", p.label)
            };
            send(ctx, vm, "node.edit", json!({"node": p.id, "action": cut_action}), done);
        }
        if fix.clicked() {
            vm.fix = Some((p.id.clone(), String::new()));
        }
    });
    if let Some((id, mut text)) = vm.fix.clone().filter(|f| f.0 == p.id) {
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut text)
                    .hint_text(format!("Tell Mortiflix what to change in {} {}…", what.to_lowercase(), p.label))
                    .desired_width((ui.available_width() - 70.0).max(80.0)),
            );
            ctx.auto("mfx.vm.fix.text", &r, "What to change");
            let go = ui.add_enabled(!text.trim().is_empty(), egui::Button::new("Send"));
            ctx.auto("mfx.vm.fix.send", &go, "Send");
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if (go.clicked() || enter) && !text.trim().is_empty() {
                let done = format!("Sent: only {} {} is remade. The rest keeps playing.", what.to_lowercase(), p.label);
                send(ctx, vm, "node.edit", json!({"node": id, "action": "fix", "text": text.trim()}), done);
                vm.fix = None;
            } else {
                vm.fix = Some((id, text));
            }
        });
    }
}

fn level_slider(
    ui: &mut egui::Ui,
    ctx: &mut MfxCtx,
    vm: &mut Vm,
    id: &str,
    which: &'static str,
    value: f32,
    range: std::ops::RangeInclusive<f32>,
) -> Option<f32> {
    let mut v = vm.slider.as_ref().filter(|s| s.0 == id && s.1 == which).map_or(value, |s| s.2);
    ui.spacing_mut().slider_width = (ui.available_width() - 70.0).clamp(80.0, 220.0);
    let r = ui.add_enabled(vm.tl.has_view, egui::Slider::new(&mut v, range).show_value(false));
    ui.colored_label(ctx.theme.text_dim, fmt_db(v));
    ctx.auto(&format!("mfx.vm.{which}"), &r, which);
    if r.dragged() || r.changed() {
        vm.slider = Some((id.to_string(), which, v));
    }
    if r.drag_stopped() || (r.changed() && !r.dragged()) {
        vm.slider = None;
        return Some((v * 10.0).round() / 10.0);
    }
    None
}

fn line_panel(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm, p: &Piece) {
    let t = ctx.theme;
    let Some(info) = p.line.clone() else { return };
    title_row(ui, ctx, vm, p, "line");
    verdicts(ui, ctx, vm, p, "Line");
    let shot = vm.tl.shot_at(p.start).map(|s| s.label.clone()).unwrap_or_default();
    ui.horizontal(|ui| {
        let play = ui.button("▶ Play line");
        ctx.auto("mfx.vm.playLine", &play, "Play line");
        if play.clicked() {
            play_from(ctx, vm, (p.start - 0.5).max(0.0));
        }
        let mut when = format!("{} – {} · {}", fmt_time(p.start), fmt_time(p.end), fmt_secs(p.secs()));
        if !shot.is_empty() {
            when.push_str(&format!(" · shot {}", shot.trim_start_matches('#')));
        }
        ui.colored_label(t.text_dim, when);
    });
    if vm.tl.narration == "own" && !p.own_take() {
        ui.colored_label(t.warn, "Needs your take: an AI stand-in reads it until you record it.");
    }
    if p.state == PieceState::Failed {
        let again = ui.button("Try again");
        ctx.auto("mfx.vm.retry", &again, "Try again");
        if again.clicked() {
            send(
                ctx,
                vm,
                "node.edit",
                json!({"node": p.id, "action": "fix", "text": "Try again: the last attempt couldn't make this line."}),
                format!("Line {}: trying again.", p.label),
            );
        }
    }
    ui.add_space(4.0);

    // Words + Remake line
    let mut words = vm.words.as_ref().filter(|w| w.0 == p.id).map_or(info.words.clone(), |w| w.1.clone());
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, "Words");
        let r = ui.add(egui::TextEdit::singleline(&mut words).desired_width((ui.available_width() - 110.0).max(80.0)));
        ctx.auto("mfx.vm.words", &r, "Words");
        let changed = words.trim() != info.words.trim() && !words.trim().is_empty();
        let remake = ui.add_enabled(changed, egui::Button::new("Remake line"));
        ctx.auto("mfx.vm.remake", &remake, "Remake line");
        if remake.clicked() {
            send(
                ctx,
                vm,
                "voice.line.words",
                json!({"line": p.id, "text": words.trim()}),
                format!("Line {} is remade with the new words. The script changes with it.", p.label),
            );
            vm.words = None;
        } else if changed {
            vm.words = Some((p.id.clone(), words.clone()));
        } else {
            vm.words = None;
        }
    });
    ui.colored_label(t.text_faint, "The script changes with it.");

    // Voice
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, "Voice");
        let all = ui.selectable_label(vm.voice_all, "All lines");
        ctx.auto("mfx.vm.voice.all", &all, "All lines");
        let one = ui.selectable_label(!vm.voice_all, "This line only");
        ctx.auto("mfx.vm.voice.one", &one, "This line only");
        if all.clicked() {
            vm.voice_all = true;
        }
        if one.clicked() {
            vm.voice_all = false;
        }
        let current = info.voice.clone().unwrap_or_else(|| vm.tl.narration.clone());
        let shown = vm
            .tl
            .voices
            .iter()
            .find(|v| v.0 == current)
            .map_or_else(|| if current.is_empty() { "The project's voice".to_string() } else { current.clone() }, |v| v.1.clone());
        let mut picked: Option<String> = None;
        let combo = egui::ComboBox::from_id_salt("mfx-vm-voice").selected_text(shown).show_ui(ui, |ui| {
            for (id, label) in vm.tl.voices.clone() {
                let r = ui.selectable_label(id == current, &label);
                ctx.auto(&format!("mfx.vm.voice.pick.{id}"), &r, &label);
                if r.clicked() {
                    picked = Some(id);
                }
            }
        });
        ctx.auto("mfx.vm.voice", &combo.response, "Voice");
        if let Some(v) = picked {
            // All lines: every line without an OK (and not your own take) gets the voice; OK'd lines keep theirs.
            let targets: Vec<String> = if vm.voice_all {
                vm.tl.lines.iter().filter(|l| !matches!(l.state, PieceState::Ok | PieceState::Cut) && !l.own_take()).map(|l| l.id.clone()).collect()
            } else {
                vec![p.id.clone()]
            };
            let n = targets.len();
            for (i, id) in targets.into_iter().enumerate() {
                let done = if i + 1 < n {
                    String::new()
                } else if vm.voice_all {
                    format!("{n} lines without an OK are remade in that voice. OK'd lines keep theirs.")
                } else {
                    format!("Line {} is remade in that voice.", p.label)
                };
                send(ctx, vm, "mix.set", json!({"node": id, "voice": v}), done);
            }
        }
    });

    // Takes
    ui.horizontal_wrapped(|ui| {
        ui.colored_label(t.text_dim, "Takes");
        for take in &info.takes {
            let playing = info.take.as_deref() == Some(take.id.as_str());
            let label = if playing { format!("{} · playing", take.label) } else { take.label.clone() };
            let label = if take.own { format!("● {label}") } else { label };
            let r = ui.selectable_label(playing, label);
            ctx.auto(&format!("mfx.vm.take.{}", take.id), &r, &take.label);
            if r.clicked() && !playing {
                use_take(ctx, vm, p, take);
            }
        }
        let new = ui.button("New take");
        ctx.auto("mfx.vm.newTake", &new, "New take");
        if new.clicked() {
            send(ctx, vm, "voice.line.take", json!({"line": p.id}), format!("Line {}: a new take is being made. Every take is kept.", p.label));
        }
    });
    ui.horizontal(|ui| {
        let rec = ui.button("● Record it yourself");
        ctx.auto("mfx.vm.record", &rec, "Record it yourself");
        if rec.clicked() {
            vm.booth = Some(p.id.clone());
        }
        let imp = ui.link("Import a recording…");
        ctx.auto("mfx.vm.import", &imp, "Import a recording");
        if imp.clicked() {
            pick_recording(ctx, &p.id);
        }
    });
    ui.colored_label(t.text_faint, "Or drag a recording from the desktop onto the line on the Voice lane.");

    // Timing
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, format!("Timing  Starts {}", fmt_time(p.start)));
        let minus = ui.add_enabled(vm.tl.has_view, egui::Button::new("−0.1 s"));
        ctx.auto("mfx.vm.nudgeBack", &minus, "-0.1 s");
        let plus = ui.add_enabled(vm.tl.has_view, egui::Button::new("+0.1 s"));
        ctx.auto("mfx.vm.nudgeOn", &plus, "+0.1 s");
        for (r, by) in [(&minus, -0.1), (&plus, 0.1)] {
            if r.clicked() {
                send(
                    ctx,
                    vm,
                    "mix.set",
                    json!({"node": p.id, "nudge_sec": by}),
                    format!("Line {} starts 0.1 s {}.", p.label, if by < 0.0 { "earlier" } else { "later" }),
                );
            }
        }
        if info.moved {
            let back = ui.small_button("Back to the script's timing");
            ctx.auto("mfx.vm.timing.reset", &back, "Back to the script's timing");
            if back.clicked() {
                send(ctx, vm, "mix.set", json!({"node": p.id, "start_sec": null}), format!("Line {} is back at the script's timing.", p.label));
            }
        }
    });
    let fit = vm.tl.fit(p);
    match &fit {
        Fit::TooLong { shot, .. } => {
            ui.colored_label(t.error, fit.text());
            ui.horizontal(|ui| {
                let longer = ui.button(format!("Make shot {shot} longer"));
                ctx.auto("mfx.vm.fit.longerShot", &longer, "Make the shot longer");
                let faster = ui.button("Say it faster");
                ctx.auto("mfx.vm.fit.faster", &faster, "Say it faster");
                if longer.clicked()
                    && let Some(sid) = vm.tl.shot_at(p.start).map(|s| s.id.clone())
                {
                    let text = format!(
                        "Make this shot longer so voice line {} fits ({}); the music moves with it, OK'd bars ask first.",
                        p.id,
                        fit.text().trim_start_matches("! ")
                    );
                    send(
                        ctx,
                        vm,
                        "node.edit",
                        json!({"node": sid, "action": "fix", "text": text}),
                        format!("Shot {shot} is made longer; the music moves with it."),
                    );
                }
                if faster.clicked() {
                    send(
                        ctx,
                        vm,
                        "voice.line.take",
                        json!({"line": p.id, "text": "a little faster, so it fits its shot"}),
                        format!("Line {} is read a little faster.", p.label),
                    );
                }
            });
        }
        _ => {
            ui.colored_label(t.ok, fit.text());
        }
    }

    // Level
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, "Level");
        if let Some(db) = level_slider(ui, ctx, vm, &p.id, "lineLevel", info.gain_db, -30.0..=12.0) {
            send(ctx, vm, "mix.set", json!({"node": p.id, "gain_db": db}), format!("Line {} level {}.", p.label, fmt_db(db)));
        }
    });
    if !vm.tl.has_view {
        ui.colored_label(t.text_faint, "Levels, timing and takes need the service's voicemusic.view (update the Mortiflix service).");
    }
}

fn cue_panel(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm, p: &Piece) {
    let t = ctx.theme;
    let Some(info) = p.cue.clone() else { return };
    title_row(ui, ctx, vm, p, "Cue");
    if p.state == PieceState::Planned {
        plan_panel(ui, ctx, vm, p, &info);
        return;
    }
    verdicts(ui, ctx, vm, p, "Cue");
    ui.horizontal(|ui| {
        let play = ui.button("▶ Play cue");
        ctx.auto("mfx.vm.playCue", &play, "Play cue");
        if play.clicked() {
            play_from(ctx, vm, p.start);
        }
        let mut when = format!("{} – {} · {}", fmt_time(p.start), fmt_time(p.end), fmt_secs(p.secs()));
        if let Some((a, b)) = info.bars {
            when.push_str(&format!(" · bars {a:.0}–{b:.0}"));
        }
        ui.colored_label(t.text_dim, when);
    });
    if p.state == PieceState::Failed {
        let again = ui.button("Try again");
        ctx.auto("mfx.vm.retry", &again, "Try again");
        if again.clicked() {
            send(
                ctx,
                vm,
                "music.cue.set",
                json!({"cue": p.id, "text": "Try again: the last attempt couldn't make this cue."}),
                format!("Cue {}: trying again.", p.label),
            );
        }
    }
    {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(t.text_dim, "Versions");
            for v in &p.versions {
                let here = p.current == Some(*v);
                let r = ui.selectable_label(here, if here { format!("v{v} · playing") } else { format!("v{v}") });
                ctx.auto(&format!("mfx.vm.cueVersion.v{v}"), &r, &format!("v{v}"));
                if r.clicked() && !here {
                    send(ctx, vm, "node.pick", json!({"node": p.id, "v": v}), format!("Cue {} plays v{v}.", p.label));
                }
            }
            let swap = ui.button("Swap: try another");
            ctx.auto("mfx.vm.swap", &swap, "Swap: try another");
            if swap.clicked() {
                send(
                    ctx,
                    vm,
                    "music.cue.set",
                    json!({"cue": p.id, "text": "Swap: write a different cue for these bars and keep the music theme."}),
                    format!("Cue {}: a different cue is being written for these bars. The theme stays.", p.label),
                );
            }
        });
        let theme = vm.tl.theme.clone().map_or("Theme: the AI's pick. A swap keeps the theme.".to_string(), |(name, summary)| {
            if summary.is_empty() { format!("Theme: {name}. A swap keeps the theme.") } else { format!("Theme: {name} ({summary}). A swap keeps the theme.") }
        });
        ui.colored_label(t.text_faint, theme);
        ui.horizontal(|ui| {
            ui.colored_label(t.text_dim, "Length");
            let shorter = ui.button("Shorter · −1 bar");
            ctx.auto("mfx.vm.shorter", &shorter, "Shorter");
            let longer = ui.button("Longer · +1 bar");
            ctx.auto("mfx.vm.longer", &longer, "Longer");
            let bars = info.bars_long();
            if let Some(n) = bars {
                ui.colored_label(t.text_faint, format!("{n:.0} bars"));
            }
            for (r, by) in [(&shorter, -1.0), (&longer, 1.0)] {
                if r.clicked()
                    && let Some(n) = bars
                    && n + by >= 1.0
                {
                    send(
                        ctx,
                        vm,
                        "mix.set",
                        json!({"node": p.id, "len_bars": n + by}),
                        format!("Cue {} is made one bar {}. The cues after it move with it.", p.label, if by < 0.0 { "shorter" } else { "longer" }),
                    );
                }
            }
        });
        let land = vm.tl.landing(p);
        let bad = matches!(land, model::Landing::Misses { .. });
        ui.colored_label(if bad { t.error } else { t.ok }, land.text());
    }
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, "Level");
        if let Some(db) = level_slider(ui, ctx, vm, &p.id, "cueLevel", info.gain_db, -30.0..=12.0) {
            send(ctx, vm, "mix.set", json!({"node": p.id, "gain_db": db}), format!("Cue {} level {}.", p.label, fmt_db(db)));
        }
    });
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, "Quieter under the voice");
        if let Some(db) = level_slider(ui, ctx, vm, &p.id, "duck", info.duck_db, -40.0..=0.0) {
            send(ctx, vm, "mix.set", json!({"node": p.id, "duck_db": db}), format!("Cue {} drops {} while a line plays.", p.label, fmt_db(db)));
        }
    });
    let notes = ui
        .add_enabled(false, egui::Button::new("Open the notes (music lane) ›"))
        .on_disabled_hover_text("Note-level edits open in the mini DAW (docs 19-25), not built yet.");
    ctx.auto("mfx.vm.openNotes", &notes, "Open the notes");
}

/// A planned cue (doc 33b, before the score): its feel and energy; Send changes only this cue.
fn plan_panel(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm, p: &Piece, info: &model::CueInfo) {
    let t = ctx.theme;
    ui.colored_label(
        t.text_dim,
        format!("{} – {} · under the shots of beat {}", fmt_time(p.start), fmt_time(p.end), p.beat_no.map_or("?".to_string(), |n| n.to_string())),
    );
    let (mut feel, mut energy) = vm.plan.as_ref().filter(|x| x.0 == p.id).map_or((info.feel.clone(), info.energy.unwrap_or(3)), |x| (x.1.clone(), x.2));
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, "Feel");
        let r = ui.add(egui::TextEdit::singleline(&mut feel).hint_text("The AI picks").desired_width((ui.available_width() - 10.0).max(80.0)));
        ctx.auto("mfx.vm.plan.feel", &r, "Feel");
    });
    ui.horizontal(|ui| {
        ui.colored_label(t.text_dim, "Energy");
        for e in 1..=5u8 {
            let r = ui.selectable_label(energy == e, e.to_string());
            ctx.auto(&format!("mfx.vm.plan.energy{e}"), &r, &format!("Energy {e}"));
            if r.clicked() {
                energy = e;
            }
        }
    });
    if let Some(theme) = vm.tl.theme.clone() {
        ui.colored_label(t.text_faint, format!("Theme: {}. Only this cue changes.", theme.0));
    }
    let changed = feel.trim() != info.feel.trim() || Some(energy) != info.energy;
    let go = ui.add_enabled(changed, egui::Button::new(RichText::new("Send").color(Color32::WHITE)).fill(t.accent));
    ctx.auto("mfx.vm.plan.send", &go, "Send");
    if go.clicked() {
        send(
            ctx,
            vm,
            "music.cue.set",
            json!({"cue": p.id, "feel": feel.trim(), "energy": energy}),
            format!("Cue {} is planned again. Only this cue changes.", p.label),
        );
        vm.plan = None;
    } else {
        vm.plan = changed.then(|| (p.id.clone(), feel, energy));
    }
}

/// Play a take: one of the AI's versions (`node.pick`) or one of yours (`voice.booth.keep`).
fn use_take(ctx: &mut MfxCtx, vm: &mut Vm, p: &Piece, take: &model::Take) {
    let done = format!("Line {} now plays {}.", p.label, take.label);
    if take.own {
        send(ctx, vm, "voice.booth.keep", json!({"line": p.id, "take": take.no}), done);
    } else {
        send(ctx, vm, "node.pick", json!({"node": p.id, "v": take.no}), done);
    }
}

fn shot_panel(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm, p: &Piece) {
    title_row(ui, ctx, vm, p, "shot");
    let t = ctx.theme;
    ui.colored_label(t.text_dim, format!("{} – {} · {}", fmt_time(p.start), fmt_time(p.end), fmt_secs(p.secs())));
    let lines: Vec<(String, String)> = vm.tl.lines.iter().filter(|l| l.start >= p.start && l.start < p.end).map(|l| (l.id.clone(), l.label.clone())).collect();
    if lines.is_empty() {
        ui.colored_label(t.text_faint, "No voice line starts in this shot.");
    }
    ui.horizontal_wrapped(|ui| {
        for (id, label) in lines {
            let r = ui.button(format!("Line {label}"));
            ctx.auto(&format!("mfx.vm.shotLine.{id}"), &r, &label);
            if r.clicked() {
                select(ctx, vm, Kind::Line, &id, true);
            }
        }
    });
    ui.colored_label(t.text_faint, "Shots are OK'd, fixed and cut in the node area (stage 3).");
}

fn summary(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm) {
    let t = ctx.theme;
    ui.label(RichText::new("The whole video").color(t.text).strong());
    ui.colored_label(t.text_dim, "Click a line or a cue on the lanes below to hear it, change it, and OK it. Double-click plays it.");
    if vm.tl.no_narration() {
        ui.horizontal(|ui| {
            ui.colored_label(t.text_dim, "No narration in this video ·");
            let change = ui.link("Change");
            ctx.auto("mfx.vm.narration.change", &change, "Change");
            if change.clicked() {
                ctx.command("mortiflix.setup", json!({}));
            }
        });
    }
    if vm.tl.cues.is_empty() {
        ui.colored_label(t.text_dim, "No music cues in this video yet.");
    }
    let failed: Vec<(Kind, String, String)> =
        vm.tl.lines.iter().chain(&vm.tl.cues).filter(|p| p.state == PieceState::Failed).map(|p| (p.kind, p.id.clone(), p.label.clone())).collect();
    if !failed.is_empty() {
        ui.add_space(4.0);
        ui.colored_label(t.error, "Couldn't make these:");
        ui.horizontal_wrapped(|ui| {
            for (k, id, label) in failed {
                let r = ui.button(format!("! {} {label}", if k == Kind::Line { "line" } else { "cue" }));
                ctx.auto(&format!("mfx.vm.failed.{id}"), &r, &label);
                if r.clicked() {
                    select(ctx, vm, k, &id, true);
                }
            }
        });
    }
    let not_ok = vm.tl.not_ok();
    ui.add_space(4.0);
    if not_ok.is_empty() && !(vm.tl.lines.is_empty() && vm.tl.cues.is_empty()) {
        ui.colored_label(t.ok, "✓ Everything has an OK. Get the video, or edit it in FilmCraft first.");
    } else if !not_ok.is_empty() {
        let shown: Vec<String> = not_ok.iter().take(24).cloned().collect();
        let more = if not_ok.len() > shown.len() { format!(" and {} more", not_ok.len() - shown.len()) } else { String::new() };
        ui.colored_label(t.text_dim, format!("No OK yet: {}{more}. You can still go on: they go in as they are now.", shown.join(", ")));
    }
}

fn lane_actions(ctx: &mut MfxCtx, vm: &mut Vm, out: lanes::LaneOut) {
    if let Some((k, id)) = out.select {
        select(ctx, vm, k, &id, true);
    }
    if let Some((k, id)) = out.play
        && let Some(p) = vm.tl.find(k, &id)
    {
        let at = if k == Kind::Line { (p.start - 0.5).max(0.0) } else { p.start };
        vm.sel = Some((k, id.clone()));
        ctx.state.selected_node = Some(id);
        play_from(ctx, vm, at);
    }
    if let Some((id, at)) = out.moved {
        let label = vm.tl.find(Kind::Line, &id).map(|p| p.label.clone()).unwrap_or_default();
        send(ctx, vm, "mix.set", json!({"node": id, "start_sec": at}), format!("Line {label} now starts at {}.", fmt_time(at)));
    }
    if let Some(at) = out.seek {
        seek_to(ctx, vm, at);
    }
    if let Some((id, path)) = out.dropped {
        import_file(ctx, vm, &id, &path);
    }
}

fn import_file(ctx: &mut MfxCtx, vm: &mut Vm, line: &str, path: &str) {
    let ok_ext = std::path::Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| AUDIO_EXTS.contains(&e.to_ascii_lowercase().as_str()));
    if !ok_ext {
        vm.msg = Some(("Drop a WAV, AIFF, FLAC, MP3, M4A or OGG recording on a line.".into(), true));
        return;
    }
    let label = vm.tl.find(Kind::Line, line).map(|p| p.label.clone()).unwrap_or_default();
    send(ctx, vm, "voice.booth.import", json!({"line": line, "path": path}), format!("Line {label}: your recording is in, marked ● your take."));
}

/// Import a recording…: the OS file dialog, then `voice.booth.import {line, path}` (`mortiflix.chooseFileAndSend`).
fn pick_recording(ctx: &mut MfxCtx, line: &str) {
    ctx.command(
        "mortiflix.chooseFileAndSend",
        json!({"filter": "Recordings", "exts": AUDIO_EXTS, "request": {"type": "voice.booth.import", "params": {"line": line}}, "field": "path"}),
    );
}

/// The recording booth (doc 34, modal): the line in large type with the one before and after, its
/// length and fit, your takes, Record, Keep & next, ← → between lines.
fn booth(ui: &mut egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm) {
    let Some(id) = vm.booth.clone() else { return };
    let Some(i) = vm.tl.lines.iter().position(|l| l.id == id) else {
        vm.booth = None;
        return;
    };
    let t = ctx.theme;
    let n = vm.tl.lines.len();
    let line = vm.tl.lines.get(i).cloned();
    let Some(line) = line else { return };
    let info = line.line.clone();
    let before = i.checked_sub(1).and_then(|j| vm.tl.lines.get(j)).and_then(|l| l.line.as_ref()).map(|l| l.words.clone());
    let after = vm.tl.lines.get(i + 1).and_then(|l| l.line.as_ref()).map(|l| l.words.clone());
    let fit = vm.tl.fit(&line);
    let shot_len = vm.tl.shot_at(line.start).map(|s| s.secs());
    let modal = egui::Modal::new(egui::Id::new("mfx-vm-booth")).show(ui.ctx(), |ui| {
        ui.set_width(560.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Record it yourself · line {} of {n}", i + 1)).color(t.text).strong());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let close = ui.small_button("Close");
                ctx.auto("mfx.vm.booth.close", &close, "Close");
                if close.clicked() {
                    vm.booth = None;
                }
            });
        });
        // progress dots: kept takes are filled
        let (dots, _) = ui.allocate_exact_size(vec2(ui.available_width(), 10.0), Sense::hover());
        for (k, l) in vm.tl.lines.iter().enumerate().take(200) {
            let c = egui::pos2(dots.min.x + 5.0 + k as f32 * 9.0, dots.center().y);
            if c.x > dots.max.x {
                break;
            }
            let kept = l.own_take();
            if kept {
                ui.painter().circle_filled(c, 3.0, t.ok);
            } else {
                ui.painter().circle_stroke(c, 3.0, Stroke::new(1.0, if k == i { t.accent } else { t.text_faint }));
            }
        }
        if let Some(b) = before {
            ui.colored_label(t.text_faint, b);
        }
        let words = info.as_ref().map(|l| l.words.clone()).unwrap_or_default();
        ui.label(RichText::new(words).size(22.0).color(t.text));
        if let Some(a) = after {
            ui.colored_label(t.text_faint, a);
        }
        let mut about = format!("About {}", fmt_secs(line.secs()));
        if let (Fit::Fits { shot, .. } | Fit::TooLong { shot, .. }, Some(len)) = (&fit, shot_len) {
            about.push_str(&format!(" · {} shot {shot} ({})", if matches!(fit, Fit::Fits { .. }) { "fits" } else { "too long for" }, fmt_secs(len)));
        }
        ui.colored_label(t.text_dim, about);
        ui.add_space(6.0);
        let takes: Vec<model::Take> = info.as_ref().map(|l| l.takes.iter().filter(|t| t.own).cloned().collect()).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(t.text_dim, "Your takes");
            if takes.is_empty() {
                ui.colored_label(t.text_faint, "none yet");
            }
            for tk in &takes {
                let playing = info.as_ref().and_then(|l| l.take.as_deref()) == Some(tk.id.as_str());
                let label = match tk.sec {
                    Some(s) => format!("{} · {}", tk.label, fmt_secs(s)),
                    None => tk.label.clone(),
                };
                let r = ui.selectable_label(playing, label);
                ctx.auto(&format!("mfx.vm.booth.take.{}", tk.id), &r, &tk.label);
                if r.clicked() && !playing {
                    use_take(ctx, vm, &line, tk);
                }
            }
        });
        ui.horizontal(|ui| {
            // The microphone take is the app's own recording handed over with voice.booth.import
            // {source: booth}; capturing it from the OS input isn't wired into this stage yet.
            let rec = ui
                .add_enabled(false, egui::Button::new("● Record"))
                .on_disabled_hover_text("Recording from the microphone here isn't built yet: record anywhere, then Import a recording.");
            ctx.auto("mfx.vm.booth.record", &rec, "Record");
            let imp = ui.button("Import a recording…");
            ctx.auto("mfx.vm.booth.import", &imp, "Import a recording");
            if imp.clicked() {
                pick_recording(ctx, &line.id);
            }
            let prev = ui.add_enabled(i > 0, egui::Button::new("←"));
            ctx.auto("mfx.vm.booth.prev", &prev, "Previous line");
            let next = ui.add_enabled(i + 1 < n, egui::Button::new("→"));
            ctx.auto("mfx.vm.booth.next", &next, "Next line");
            let keep = ui.add(egui::Button::new(RichText::new("Keep & next").color(Color32::WHITE)).fill(t.accent));
            ctx.auto("mfx.vm.booth.keep", &keep, "Keep & next");
            if prev.clicked() {
                vm.booth = vm.tl.lines.get(i.saturating_sub(1)).map(|l| l.id.clone());
            }
            if next.clicked() {
                vm.booth = vm.tl.lines.get(i + 1).map(|l| l.id.clone());
            }
            if keep.clicked() {
                let playing = info.as_ref().and_then(|l| l.take.clone());
                if let Some(last) = takes.last().filter(|tk| playing.as_deref() != Some(tk.id.as_str())) {
                    send(ctx, vm, "voice.booth.keep", json!({"line": line.id, "take": last.no}), format!("Line {}: {} kept.", line.label, last.label));
                }
                vm.booth = vm.tl.lines.get(i + 1).map(|l| l.id.clone());
            }
        });
        if let Some((m, bad)) = &vm.msg {
            ui.colored_label(if *bad { t.error } else { t.text_dim }, m);
        }
    });
    if modal.should_close() {
        vm.booth = None;
    }
    if let Some(b) = &vm.booth
        && *b != id
    {
        let next = b.clone();
        select(ctx, vm, Kind::Line, &next, true);
    }
}

/// Doc 34's keys, while the pointer is over this stage and no text field has the keyboard:
/// ← → previous / next line, Enter OK, R record it yourself. FilmCraft's own shortcuts run first,
/// so a chord it binds globally (Space, J/K/L) stays FilmCraft's.
fn keys(ui: &egui::Ui, ctx: &mut MfxCtx, vm: &mut Vm) {
    if !ui.ui_contains_pointer() || ui.ctx().egui_wants_keyboard_input() || vm.booth.is_some() {
        return;
    }
    let (left, right, enter, rec) = ui.ctx().input_mut(|i| {
        (
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
            i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
            i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            i.consume_key(egui::Modifiers::NONE, egui::Key::R),
        )
    });
    let cur = vm.sel.as_ref().filter(|s| s.0 == Kind::Line).map(|s| s.1.clone());
    if (left || right)
        && let Some(id) = vm.tl.step_line(cur.as_deref(), if left { -1 } else { 1 }).map(|l| l.id.clone())
    {
        select(ctx, vm, Kind::Line, &id, true);
    }
    if enter
        && let Some((k, id)) = vm.sel.clone()
        && k != Kind::Shot
        && let Some(p) = vm.tl.find(k, &id)
    {
        let label = p.label.clone();
        send(ctx, vm, "node.edit", json!({"node": id, "action": "ok"}), format!("{label} is OK."));
    }
    if rec && let Some(id) = cur {
        vm.booth = Some(id);
    }
}

/// When the service reports nothing for Voice & music, the stage bar shows this stage's own count
/// (making / needs you / ready) so Get the video asks first only when something failed.
fn stage_status(ctx: &mut MfxCtx, vm: &mut Vm) {
    let i = usize::from(STAGE_VOICE_MUSIC);
    let Some(current) = ctx.state.statuses.get(i).cloned() else { return };
    let ours = vm.status_set.as_ref() == Some(&current);
    if !(current.kind == StatusKind::NotStarted || ours) || (vm.tl.lines.is_empty() && vm.tl.cues.is_empty()) {
        return;
    }
    let ((lo, lt), (co, ct)) = vm.tl.tally();
    let (busy, failed) = (vm.tl.busy(), vm.tl.failed());
    let total = u32::try_from(lt + ct).unwrap_or(u32::MAX);
    let done = u32::try_from(lo + co).unwrap_or(u32::MAX);
    let next = if failed > 0 {
        StageStatus { kind: StatusKind::NeedsYou, need: u32::try_from(failed).unwrap_or(u32::MAX), ..StageStatus::default() }
    } else if busy > 0 {
        StageStatus { kind: StatusKind::Making, done: total.saturating_sub(u32::try_from(busy).unwrap_or(0)), total, ..StageStatus::default() }
    } else {
        StageStatus { kind: StatusKind::Ready, done, total, text: format!("{done} of {total} OK"), ..StageStatus::default() }
    };
    if let Some(slot) = ctx.state.statuses.get_mut(i) {
        *slot = next.clone();
    }
    vm.status_set = Some(next);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MfxTheme, Mortiflix};

    fn frames(m: &mut Mortiflix, n: usize) -> Vec<String> {
        let ctx = egui::Context::default();
        let mut ids = Vec::new();
        for _ in 0..n {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, vec2(900.0, 700.0))), ..Default::default() };
            ctx.run_ui(input, |ui| {
                let mut c = m.ctx(MfxTheme::default());
                show(ui, &mut c);
                ids = c.auto.iter().map(|a| a.0.clone()).collect();
            })
            .textures_delta
            .clear();
        }
        ids
    }

    #[test]
    fn draws_calmly_without_a_service_and_asks_for_nothing_it_cannot_send() {
        let mut m = Mortiflix::default();
        m.state.stage = STAGE_VOICE_MUSIC;
        let ids = frames(&mut m, 3);
        assert!(ids.iter().any(|i| i == "mfx.service.setup"), "service not running: the Setup link shows");
        assert!(ids.iter().any(|i| i == "mfx.vm.playAll"));
    }

    #[test]
    fn theme_cards_for_the_chat_draw_calmly_without_a_service() {
        let mut m = Mortiflix::default();
        let ctx = egui::Context::default();
        for _ in 0..2 {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut c = m.ctx(MfxTheme::default());
                show_theme_card(ui, &mut c);
                show_cue_plan(ui, &mut c);
                assert!(c.commands.is_empty());
            })
            .textures_delta
            .clear();
        }
    }

    #[test]
    fn status_fill_follows_the_pieces_and_never_overrides_the_service() {
        let mut m = Mortiflix::default();
        let mut c = m.ctx(MfxTheme::default());
        let mut vm = Vm {
            tl: model::build(
                &json!({"nodes": [
                {"id": "b01-1", "kind": "line", "order": 0, "title": "One.", "versions": [{"v": 1, "file": "a.wav"}], "current": 1, "state": "approved", "owner": "ok"},
                {"id": "b01-2", "kind": "line", "order": 1, "title": "Two.", "versions": [{"v": 1, "file": "b.wav"}], "current": 1, "state": "ready"}]}),
                None,
            ),
            ..Vm::default()
        };
        stage_status(&mut c, &mut vm);
        assert_eq!(c.state.status(STAGE_VOICE_MUSIC).line(), "1 of 2 OK");
        // ours: it keeps following
        vm.tl.lines[1].state = PieceState::Failed;
        stage_status(&mut c, &mut vm);
        assert_eq!(c.state.status(STAGE_VOICE_MUSIC).kind, StatusKind::NeedsYou);
        // the service said something: it stays
        c.state.statuses[usize::from(STAGE_VOICE_MUSIC)] = StageStatus::of(StatusKind::Done);
        stage_status(&mut c, &mut vm);
        assert_eq!(c.state.status(STAGE_VOICE_MUSIC).kind, StatusKind::Done);
    }

    #[test]
    fn lane_actions_turn_into_requests_and_commands() {
        let mut m = Mortiflix::default();
        let mut c = m.ctx(MfxTheme::default());
        let mut vm = Vm {
            tl: model::build(
                &json!({"nodes": [{"id": "b01-1", "kind": "line", "order": 0, "title": "One.", "start_sec": 2.0, "end_sec": 3.0, "versions": [{"v": 1, "file": "a.wav"}], "current": 1}]}),
                Some(&json!({})),
            ),
            ..Vm::default()
        };
        let out = lanes::LaneOut {
            select: Some((Kind::Line, "b01-1".into())),
            moved: Some(("b01-1".into(), 2.5)),
            dropped: Some(("b01-1".into(), "/tmp/take.txt".into())),
            ..Default::default()
        };
        lane_actions(&mut c, &mut vm, out);
        assert_eq!(c.state.selected_node.as_deref(), Some("b01-1"));
        assert_eq!(c.commands.first().map(|x| x.0.as_str()), Some("playhead.set"));
        assert_eq!(vm.cursor, 2.0);
        assert_eq!(vm.ops.len(), 1, "the new start; a non-audio drop is refused locally");
        assert!(vm.msg.as_ref().is_some_and(|m| m.1));
        import_file(&mut c, &mut vm, "b01-1", "/tmp/My Take.WAV");
        assert_eq!(vm.ops.len(), 2);
        pick_recording(&mut c, "b01-1");
        let pick = c.commands.last().cloned().unwrap();
        assert_eq!(pick.0, "mortiflix.chooseFileAndSend");
        assert_eq!((pick.1["request"]["type"].as_str(), pick.1["field"].as_str()), (Some("voice.booth.import"), Some("path")));
    }
}
