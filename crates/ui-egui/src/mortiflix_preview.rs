//! Mortiflix FilmCraft's loose preview, FilmCraft side (doc 36): the hidden "Loose preview"
//! sequence kept in step with the graph, the Program monitor's picture in the Mortiflix Preview
//! panel, one Play with a scope (All / From here / This shot / This beat) and Loop, following the
//! node selection, and the `mortiflix.preview.*` commands (so the chat, the node area and agents
//! drive it like the buttons do).
//!
//! The panel's controls and the plan (what goes where) are in `filmcraft-mortiflix-ui`
//! (`preview.rs`); this file turns the plan into FilmCraft clips with ordinary commands
//! (`file.import`, `file.newColorMatte`, `timeline.place`, `edit.clear`), folded into one undo
//! step, "Mortiflix: Loose preview". Nothing is rendered: FilmCraft plays the files on disk.

use std::collections::HashMap;

use egui::Color32;
use filmcraft_mortiflix_ui::preview::{self as pv, Media, Plan, Scope};
use filmcraft_mortiflix_ui::{self as mfx};
use filmcraft_project::{BinId, ItemId, ItemKind};
use filmcraft_time::{Tick, TimeDisplay, format_time};
use serde_json::{Value, json};

use crate::FilmcraftApp;
use crate::menus::UiCommand;
use crate::panels::monitor::{self, Which};
use crate::panels::monitor_view;

/// The undo step a rebuild folds into.
pub const UNDO_LABEL: &str = "Mortiflix: Loose preview";
/// The bin the preview's media is imported into.
pub const BIN_NAME: &str = "Loose preview media";

const fn cmd(id: &'static str, label: &'static str) -> UiCommand {
    UiCommand { id, label, menu: &[], shortcut: None }
}

/// The preview's commands (chained into `mortiflix::COMMANDS`). Loop's Ctrl+L / ⌘L is a panel
/// shortcut of the Mortiflix Preview panel (`FILMCRAFT_PANEL`): app-wide Ctrl+L is Link.
pub const COMMANDS: &[UiCommand] = &[
    cmd("mortiflix.preview.play", "Play the Loose Preview"),
    cmd("mortiflix.preview.toggle", "Play or Pause the Loose Preview"),
    cmd("mortiflix.preview.pause", "Pause the Loose Preview"),
    cmd("mortiflix.preview.seek", "Go to a Shot in the Loose Preview"),
    cmd("mortiflix.preview.prevShot", "Previous Shot"),
    cmd("mortiflix.preview.nextShot", "Next Shot"),
    cmd("mortiflix.preview.scope", "Loose Preview Plays"),
    cmd("mortiflix.preview.loop", "Loop the Loose Preview"),
    cmd("mortiflix.preview.follow", "Follow the Playhead"),
    cmd("mortiflix.preview.refresh", "Rebuild the Loose Preview"),
    cmd("mortiflix.preview.reveal", "Show the Shot's File"),
];

/// What the host remembers for the preview.
#[derive(Default)]
pub struct HostPreview {
    /// The "Loose preview" sequence.
    seq: Option<ItemId>,
    bin: Option<BinId>,
    /// The plan signature the sequence was built from; a failed build is not retried until the
    /// plan changes or `mortiflix.preview.refresh`.
    built: Option<u64>,
    failed: Option<u64>,
    /// Imported files and colour mattes, reused across rebuilds.
    items: HashMap<String, ItemId>,
    mattes: HashMap<String, ItemId>,
    /// What Play is playing (start, end) when it isn't FilmCraft's own In/Out loop.
    range: Option<(Tick, Tick)>,
    was_playing: bool,
    last_selected: Option<String>,
    /// Show the File: the item to reveal on the next frame (it needs the egui context).
    reveal: Option<ItemId>,
}

fn secs(t: Tick) -> f64 {
    t.seconds()
}

fn ticks(s: f64) -> Tick {
    if s.is_finite() { Tick::from_seconds_f64(s.max(0.0)) } else { Tick::ZERO }
}

fn loose_active(app: &FilmcraftApp) -> bool {
    app.mfx.preview.seq.is_some() && app.session.state.active_sequence == app.mfx.preview.seq
}

/// Every frame (from `mortiflix::frame_begin`): keep the plan fresh, the sequence built and on the
/// Program monitor during stages 1-4, Play's scope and Loop, follow the node selection, and tell
/// the panel what FilmCraft's player is doing.
pub fn frame(app: &mut FilmcraftApp, ctx: &egui::Context) {
    if !app.mfx.core.state.active {
        return;
    }
    if let Some(item) = app.mfx.preview.reveal.take()
        && let Err(e) = crate::menus::invoke(app, ctx, "file.reveal", json!({"items": [item.0]}))
    {
        app.ui.status = e;
    }
    let core = &mut app.mfx.core;
    let now = ctx.input(|i| i.time);
    pv::poll(&mut core.state, &mut core.service, now);
    if core.state.stage >= mfx::STAGE_FILMCRAFT {
        // FilmCraft's own sequence is on the monitor now; the loose one stays in the bin
        app.mfx.preview.range = None;
        app.mfx.core.state.preview.player.ready = false;
        return;
    }
    let plan_empty = app.mfx.core.state.preview.plan.shots.is_empty();
    let sig = app.mfx.core.state.preview.plan.signature();
    let h = &app.mfx.preview;
    let seq_gone = h.seq.is_some_and(|s| app.session.project.sequence(s).is_none());
    if !plan_empty && (h.built != Some(sig) || seq_gone) && h.failed != Some(sig) {
        match rebuild(app) {
            Ok(()) => {
                app.mfx.preview.built = Some(sig);
                app.mfx.preview.failed = None;
                app.mfx.core.state.preview.problem = None;
            }
            Err(e) => {
                log::warn!("loose preview: {e}");
                app.mfx.preview.failed = Some(sig);
                app.mfx.core.state.preview.problem = Some(format!("The preview couldn't be built: {e}"));
            }
        }
    }
    // back on stages 1-4: the loose preview is what the monitor plays
    if !plan_empty
        && let Some(seq) = app.mfx.preview.seq
        && app.session.state.active_sequence != Some(seq)
        && app.session.project.sequence(seq).is_some()
    {
        if app.playback.playing {
            app.stop();
        }
        if let Err(e) = app.session.execute("sequence.open", json!({"item": seq.0})) {
            log::warn!("loose preview: {e}");
        }
    }
    follow_selection(app);
    enforce_scope(app);
    fill_player(app);
}

/// Draw the Program monitor's picture where the panel's layout puts it (before the controls).
pub fn picture(app: &mut FilmcraftApp, ui: &mut egui::Ui, rect: egui::Rect) {
    let l = pv::layout(rect.shrink(8.0));
    let p = &app.mfx.core.state.preview;
    if !p.player.ready || p.plan.shots.is_empty() || l.picture.height() < 8.0 || !loose_active(app) {
        return;
    }
    ui.painter().rect_filled(l.picture, 0.0, Color32::BLACK);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(l.picture));
    child.set_clip_rect(l.picture.intersect(ui.clip_rect()));
    monitor::show_picture(app, &mut child, l.picture);
}

fn follow_selection(app: &mut FilmcraftApp) {
    let sel = app.mfx.core.state.selected_node.clone();
    if sel == app.mfx.preview.last_selected {
        return;
    }
    app.mfx.preview.last_selected = sel.clone();
    let Some(id) = sel else { return };
    let Some((start, _)) = app.mfx.core.state.preview.plan.span_of(&id) else { return };
    if !loose_active(app) {
        return;
    }
    let playing = app.playback.playing;
    app.session.set_playhead(snap(app, ticks(start)));
    if playing {
        // jump and keep playing (doc 36 default), with the scope from the new place
        let scope = app.mfx.core.state.preview.scope;
        play(app, scope, Some(&id));
    }
}

fn snap(app: &FilmcraftApp, t: Tick) -> Tick {
    app.session.sequence_rate().snap_nearest(t)
}

/// Play's scope end and Loop, and Space / L starting playback the way Play does.
fn enforce_scope(app: &mut FilmcraftApp) {
    let playing = app.playback.playing;
    let looping = app.mfx.core.state.preview.looping;
    let frame = app.session.sequence_rate().frame_duration();
    let t = app.session.playhead();
    if !loose_active(app) {
        app.mfx.preview.range = None;
        app.mfx.preview.was_playing = playing;
        return;
    }
    // FilmCraft's own Play (Space) started forward playback: it plays the scope, like Play
    if playing && !app.mfx.preview.was_playing && app.mfx.preview.range.is_none() && (app.playback.speed - 1.0).abs() < 1e-9 && !app.playback.looping {
        let scope = app.mfx.core.state.preview.scope;
        let sel = app.mfx.core.state.selected_node.clone();
        if scope != Scope::FromHere || looping {
            play(app, scope, sel.as_deref());
        } else {
            let end = ticks(app.mfx.core.state.preview.plan.total);
            app.mfx.preview.range = Some((t, end));
        }
    }
    if let Some((a, b)) = app.mfx.preview.range {
        if app.playback.playing && app.playback.speed > 0.0 && t + frame >= b {
            if looping {
                app.session.set_playhead(a);
                app.play(app.playback.speed);
            } else {
                app.stop();
                app.session.set_playhead(Tick(b.0.saturating_sub(frame.0)).max(a));
                app.mfx.preview.range = None;
            }
        } else if !app.playback.playing && app.mfx.preview.was_playing {
            // FilmCraft stopped at the sequence end: Loop starts over, else Play is done
            if looping && t + frame + frame >= b {
                app.session.set_playhead(a);
                app.play(1.0);
            } else {
                app.mfx.preview.range = None;
            }
        }
    }
    app.mfx.preview.was_playing = app.playback.playing;
}

fn fill_player(app: &mut FilmcraftApp) {
    let ready = loose_active(app) && app.mfx.preview.built.is_some();
    let (rate, df, dur, marked) = match app.session.active_sequence() {
        Some(q) => (q.settings.frame_rate, q.settings.drop_frame, q.duration(), q.mark_in.is_some() && q.mark_out.is_some()),
        None => (app.session.sequence_rate(), false, Tick::ZERO, false),
    };
    let t = app.session.playhead();
    let mv = monitor_view::view(app, Which::Program);
    let res = mv.res.label().to_string();
    let zoom = mv.zoom.map_or_else(|| "Fit".to_string(), |z| format!("{}%", (z * 100.0).round() as i32));
    let at = app.mfx.core.state.preview.plan.shot_at(secs(t)).map(|s| s.id.clone());
    let p = &mut app.mfx.core.state.preview;
    p.player = pv::Player {
        ready,
        playing: app.playback.playing,
        speed: app.playback.speed,
        time: secs(t),
        duration: secs(dur),
        fps: rate.as_f64(),
        timecode: format_time(t, rate, df, TimeDisplay::Timecode, 48000),
        total: format_time(dur, rate, df, TimeDisplay::Timecode, 48000),
        res,
        zoom,
        marked,
    };
    p.shot_at_playhead = if ready { at } else { None };
}

/// Start Play for `scope` (from the playhead, the selected node, or the node given).
fn play(app: &mut FilmcraftApp, scope: Scope, node: Option<&str>) {
    if !loose_active(app) {
        return;
    }
    let looping = app.mfx.core.state.preview.looping;
    let frame = app.session.sequence_rate().frame_duration();
    let marked = app.session.active_sequence().and_then(|q| q.mark_in.zip(q.mark_out));
    if looping && let Some((i, _)) = marked {
        // In / Out marked: Loop loops that range instead (FilmCraft's own loop)
        app.mfx.preview.range = None;
        app.playback.looping = true;
        app.session.set_playhead(i);
        app.play(1.0);
        return;
    }
    app.playback.looping = false;
    let at = secs(app.session.playhead());
    let sel = node.map(str::to_string).or_else(|| app.mfx.core.state.selected_node.clone());
    let (mut a, b) = app.mfx.core.state.preview.range(scope, at, sel.as_deref());
    let (mut ta, tb) = (snap(app, ticks(a)), snap(app, ticks(b)));
    if ta + frame >= tb {
        // at the very end: From here starts over
        a = 0.0;
        ta = snap(app, ticks(a));
    }
    app.session.set_playhead(ta);
    app.play(1.0);
    app.playback.stop_at = None;
    app.mfx.preview.range = Some((ta, tb.max(ta + frame)));
}

/// Make the sequence match the plan: one undo step, "Mortiflix: Loose preview".
fn rebuild(app: &mut FilmcraftApp) -> Result<(), String> {
    let plan = app.mfx.core.state.preview.plan.clone();
    let n0 = app.session.history.undo.len();
    let r = build(app, &plan);
    if let Some(first) = app.session.history.undo.get(n0).map(|s| s.1.clone()) {
        app.session.history.undo.truncate(n0);
        app.session.history.undo.push((UNDO_LABEL.to_string(), first));
    }
    r
}

fn exec(app: &mut FilmcraftApp, id: &str, params: Value) -> Result<Value, String> {
    app.session.execute(id, params).map_err(|e| e.to_string())
}

fn ensure_sequence(app: &mut FilmcraftApp) -> Result<ItemId, String> {
    if let Some(s) = app.mfx.preview.seq.filter(|s| app.session.project.sequence(*s).is_some()) {
        return Ok(s);
    }
    // a project saved with one already has it
    let found = app.session.project.items.values().find(|i| i.name == pv::SEQUENCE_NAME && matches!(i.kind, ItemKind::Sequence(_))).map(|i| i.id);
    let seq = match found {
        Some(s) => s,
        None => {
            let r = exec(app, "file.newSequence", json!({"name": pv::SEQUENCE_NAME, "width": 1920, "height": 1080, "fps": 30.0, "video": 1, "audio": 2}))?;
            r["sequence"].as_u64().or_else(|| r["item"].as_u64()).map(ItemId).ok_or("no sequence was made")?
        }
    };
    app.mfx.preview.seq = Some(seq);
    Ok(seq)
}

fn ensure_bin(app: &mut FilmcraftApp) -> Option<BinId> {
    if let Some(b) = app.mfx.preview.bin.filter(|b| app.session.project.root.find_bin(*b).is_some()) {
        return Some(b);
    }
    let b = exec(app, "file.newBin", json!({"name": BIN_NAME})).ok()?["bin"].as_u64().map(BinId)?;
    app.mfx.preview.bin = Some(b);
    Some(b)
}

fn import(app: &mut FilmcraftApp, file: &str) -> Result<ItemId, String> {
    if let Some(i) = app.mfx.preview.items.get(file).copied().filter(|i| app.session.project.item(*i).is_some()) {
        return Ok(i);
    }
    let mut params = json!({"paths": [file]});
    if let Some(b) = ensure_bin(app) {
        params["bin"] = json!(b.0);
    }
    let r = exec(app, "file.import", params)?;
    let id = r["items"].as_array().and_then(|a| a.first()).and_then(Value::as_u64).map(ItemId).ok_or_else(|| format!("{file} didn't import"))?;
    app.mfx.preview.items.insert(file.to_string(), id);
    Ok(id)
}

fn matte(app: &mut FilmcraftApp, colour: &str, seconds: f64) -> Result<ItemId, String> {
    let card = pv::plan::card_colour(colour);
    if let Some(i) = app.mfx.preview.mattes.get(&card).copied().filter(|i| app.session.project.item(*i).is_some()) {
        return Ok(i);
    }
    let r = exec(app, "file.newColorMatte", json!({"color": card, "seconds": seconds.max(1.0), "name": format!("Not made yet {card}")}))?;
    let id = r["item"].as_u64().map(ItemId).ok_or("no colour matte was made")?;
    if let Some(b) = ensure_bin(app) {
        let _ = exec(app, "project.moveToBin", json!({"items": [id.0], "bin": b.0}));
    }
    app.mfx.preview.mattes.insert(card, id);
    Ok(id)
}

/// The media's own length, when it is a movie or a sound (stills and mattes stretch).
fn media_len(app: &FilmcraftApp, item: ItemId) -> Option<Tick> {
    match &app.session.project.item(item)?.kind {
        ItemKind::Media(m)
            if matches!(m.info.kind, filmcraft_media::MediaKind::Movie | filmcraft_media::MediaKind::AudioOnly) && m.info.duration > Tick::ZERO =>
        {
            Some(m.info.duration)
        }
        _ => None,
    }
}

fn build(app: &mut FilmcraftApp, plan: &Plan) -> Result<(), String> {
    let seq = ensure_sequence(app)?;
    if app.session.state.active_sequence != Some(seq) {
        exec(app, "sequence.open", json!({"item": seq.0}))?;
    }
    let old: Vec<u64> = app.session.project.sequence(seq).map(|q| q.all_tracks().flat_map(|t| t.items.iter().map(|i| i.id.0)).collect()).unwrap_or_default();
    if !old.is_empty() {
        exec(app, "edit.clear", json!({"clips": old}))?;
    }
    // V1 for the pictures, A1 for the voice, A2 for the music
    let need_audio = 2usize.saturating_sub(app.session.project.sequence(seq).map_or(0, |q| q.audio_tracks.len()));
    if need_audio > 0 {
        exec(app, "sequence.addTracks", json!({"video": 0, "audio": need_audio}))?;
    }
    let q = app.session.project.sequence(seq).ok_or("the sequence went away")?;
    let v1 = q.video_tracks.first().map(|t| t.id.0).ok_or("the sequence has no video track")?;
    let a1 = q.audio_tracks.first().map(|t| t.id.0).ok_or("the sequence has no audio track")?;
    let a2 = q.audio_tracks.get(1).map_or(a1, |t| t.id.0);
    let mut problems: Vec<String> = Vec::new();
    for shot in &plan.shots {
        let (at, dur) = (ticks(shot.start), ticks(shot.len));
        // the best media first; a piece that won't import falls back to the still, then the card
        let mut tries: Vec<&Media> = vec![&shot.media];
        let still = shot.still.clone().map(|file| Media::Still { file });
        if let Some(s) = &still
            && !matches!(shot.media, Media::Still { .. })
        {
            tries.push(s);
        }
        let card = Media::Card;
        tries.push(&card);
        for (n, m) in tries.into_iter().enumerate() {
            let placed = match m {
                Media::Motion { file, from, .. } => import(app, file).and_then(|item| {
                    let sin = ticks(*from);
                    let len = media_len(app, item).map_or(dur, |l| dur.min(Tick(l.0.saturating_sub(sin.0))));
                    if len <= Tick::ZERO {
                        return Err(format!("{}: the piece is past the end of {file}", shot.label));
                    }
                    exec(app, "timeline.place", json!({"item": item.0, "time": at.0, "sourceIn": sin.0, "duration": len.0, "track": v1, "audio": false}))
                }),
                Media::Still { file } => import(app, file)
                    .and_then(|item| exec(app, "timeline.place", json!({"item": item.0, "time": at.0, "duration": dur.0, "track": v1, "audio": false}))),
                Media::Card => matte(app, &shot.colour, plan.total)
                    .and_then(|item| exec(app, "timeline.place", json!({"item": item.0, "time": at.0, "duration": dur.0, "track": v1}))),
            };
            match placed {
                Ok(_) => break,
                Err(e) if n == 0 => problems.push(format!("{}: {e}", shot.label)),
                Err(_) => {}
            }
        }
    }
    let sounds = plan.voice.iter().map(|v| (v, a1)).chain(plan.music.iter().map(|m| (m, a2)));
    for (snd, track) in sounds {
        let Some(file) = snd.file.as_deref() else { continue };
        let at = ticks(snd.start);
        let r = import(app, file).and_then(|item| {
            let want = ticks(snd.len);
            let len = media_len(app, item).map_or(want, |l| l.min(want));
            if len <= Tick::ZERO {
                return Ok(Value::Null);
            }
            exec(app, "timeline.place", json!({"item": item.0, "time": at.0, "duration": len.0, "track": track, "video": false}))
        });
        if let Err(e) = r {
            problems.push(format!("{}: {e}", snd.id));
        }
    }
    // pictures of another size fill the frame (an animatic made at 720p plays full size)
    let pictures: Vec<filmcraft_project::ClipId> =
        app.session.project.sequence(seq).and_then(|q| q.video_tracks.first()).map(|t| t.items.iter().map(|i| i.id).collect()).unwrap_or_default();
    if !pictures.is_empty() {
        app.session.state.selection = pictures;
        if let Err(e) = exec(app, "clip.scaleToFrameSize", json!({})) {
            log::info!("loose preview: scale to frame size: {e}");
        }
    }
    app.session.state.selection.clear();
    if let Some(p) = problems.first() {
        log::warn!("loose preview: {} piece(s) fell back: {}", problems.len(), problems.join("; "));
        app.ui.status = format!("Loose preview: {p}");
    }
    Ok(())
}

fn scope_of(params: &Value) -> Option<Scope> {
    params.get("scope").and_then(Value::as_str).and_then(Scope::parse)
}

fn on_of(params: &Value, cur: bool) -> bool {
    params.get("on").and_then(Value::as_bool).unwrap_or(!cur)
}

fn state(app: &FilmcraftApp) -> Value {
    let p = &app.mfx.core.state.preview;
    json!({"scope": p.scope, "loop": p.looping, "follow": p.follow_playhead, "player": p.player, "shots": p.plan.shots.len(), "shotAtPlayhead": p.shot_at_playhead})
}

/// `mortiflix.preview.<what>`.
pub fn command(app: &mut FilmcraftApp, what: &str, params: &Value) -> Result<Value, String> {
    let needs_player = matches!(what, "play" | "toggle" | "seek" | "prevShot" | "nextShot");
    if needs_player && (!loose_active(app) || app.mfx.core.state.stage >= mfx::STAGE_FILMCRAFT) {
        return Err(if app.mfx.core.state.preview.plan.shots.is_empty() {
            "Nothing to play yet: the preview fills in as soon as the script has its shots.".into()
        } else {
            "The loose preview plays on stages 1-4 (Script to Voice & music).".into()
        });
    }
    match what {
        "play" => {
            if let Some(s) = scope_of(params) {
                app.mfx.core.state.preview.scope = s;
            }
            if let Some(l) = params.get("loop").and_then(Value::as_bool) {
                app.mfx.core.state.preview.looping = l;
            }
            let node = params.get("node").and_then(Value::as_str).map(str::to_string);
            if let Some(n) = &node {
                app.mfx.core.state.preview.plan.span_of(n).ok_or_else(|| format!("{n} isn't in the preview"))?;
            }
            let scope = app.mfx.core.state.preview.scope;
            play(app, scope, node.as_deref());
        }
        "toggle" => {
            if app.playback.playing {
                app.stop();
                app.mfx.preview.range = None;
            } else {
                let scope = app.mfx.core.state.preview.scope;
                play(app, scope, None);
            }
        }
        "pause" => {
            if app.playback.playing {
                app.stop();
            }
            app.mfx.preview.range = None;
        }
        "seek" => {
            let t = if let Some(n) = params.get("node").and_then(Value::as_str) {
                app.mfx.core.state.preview.plan.span_of(n).map(|s| s.0).ok_or_else(|| format!("{n} isn't in the preview"))?
            } else {
                params.get("seconds").and_then(Value::as_f64).filter(|s| s.is_finite()).ok_or("seek needs a `node` or `seconds`")?
            };
            let t = snap(app, ticks(t.clamp(0.0, app.mfx.core.state.preview.plan.total)));
            app.session.set_playhead(t);
            if app.playback.playing {
                app.play(app.playback.speed);
            }
        }
        "prevShot" | "nextShot" => {
            let half = secs(app.session.sequence_rate().frame_duration()) / 2.0;
            let at = secs(app.session.playhead());
            let starts = app.mfx.core.state.preview.plan.shots.iter().map(|s| s.start);
            let to =
                if what == "prevShot" { starts.rev().find(|s| *s < at - half).unwrap_or(0.0) } else { starts.clone().find(|s| *s > at + half).unwrap_or(at) };
            app.session.set_playhead(snap(app, ticks(to)));
            if app.playback.playing {
                app.play(app.playback.speed);
            }
        }
        "scope" => {
            app.mfx.core.state.preview.scope = scope_of(params).ok_or("scope: all, fromHere, shot or beat")?;
        }
        "loop" => {
            let p = &mut app.mfx.core.state.preview;
            p.looping = on_of(params, p.looping);
            if !p.looping {
                app.playback.looping = false;
            }
        }
        "follow" => {
            let p = &mut app.mfx.core.state.preview;
            p.follow_playhead = on_of(params, p.follow_playhead);
        }
        "refresh" => {
            app.mfx.preview.built = None;
            app.mfx.preview.failed = None;
            app.mfx.core.state.preview.refetch();
        }
        "reveal" => {
            let n = params.get("node").and_then(Value::as_str).ok_or("reveal needs a `node`")?;
            let shot = app.mfx.core.state.preview.plan.shot(n).ok_or_else(|| format!("{n} isn't in the preview"))?;
            let file = shot.media.file().or(shot.still.as_deref()).map(str::to_string).ok_or("this shot has no file yet")?;
            let item = app.mfx.preview.items.get(&file).copied().ok_or_else(|| format!("The file isn't in the project: {file}"))?;
            app.mfx.preview.reveal = Some(item);
        }
        "state" => {}
        other => return Err(format!("unknown command `mortiflix.preview.{other}`")),
    }
    Ok(state(app))
}

#[cfg(test)]
mod tests {
    use super::*;
    use filmcraft_mortiflix_ui::preview::{PlanBeat, PlanShot};

    fn app_with_plan() -> (FilmcraftApp, egui::Context) {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        crate::menus::invoke(&mut app, &ctx, "stages.shots", json!({})).unwrap();
        let shot = |id: &str, beat: &str, start: f64, len: f64, colour: &str| PlanShot {
            id: id.into(),
            label: format!("Shot {}", &id[1..]),
            beat: Some(beat.into()),
            colour: colour.into(),
            start,
            len,
            ..PlanShot::default()
        };
        app.mfx.core.state.preview.plan = Plan {
            shots: vec![shot("s01", "b01", 0.0, 2.0, "#380ea7"), shot("s02", "b01", 2.0, 1.5, "#380ea7"), shot("s03", "b02", 3.5, 2.5, "#803f17")],
            beats: vec![
                PlanBeat { id: "b01".into(), start: 0.0, end: 3.5, first_shot: Some("s01".into()), ..PlanBeat::default() },
                PlanBeat { id: "b02".into(), start: 3.5, end: 6.0, first_shot: Some("s03".into()), ..PlanBeat::default() },
            ],
            total: 6.0,
            ..Plan::default()
        };
        (app, ctx)
    }

    #[test]
    fn the_plan_becomes_the_loose_preview_sequence_in_one_undo_step() {
        let (mut app, ctx) = app_with_plan();
        let undo0 = app.session.history.undo.len();
        frame(&mut app, &ctx);
        assert!(app.mfx.core.state.preview.problem.is_none(), "{:?}", app.mfx.core.state.preview.problem);
        let seq = app.mfx.preview.seq.unwrap();
        assert_eq!(app.session.state.active_sequence, Some(seq));
        let q = app.session.project.sequence(seq).unwrap();
        let v1 = &q.video_tracks[0];
        assert_eq!(v1.items.len(), 3);
        let starts: Vec<f64> = v1.items.iter().map(|i| i.start.seconds()).collect();
        assert!((starts[2] - 3.5).abs() < 0.05, "{starts:?}");
        assert!((q.duration().seconds() - 6.0).abs() < 0.05);
        assert_eq!(app.session.history.undo.len(), undo0 + 1);
        assert_eq!(app.session.history.undo.last().map(|u| u.0.as_str()), Some(UNDO_LABEL));
        assert!(app.mfx.core.state.preview.player.ready);
        // the same plan is not rebuilt; a changed one is, in place
        frame(&mut app, &ctx);
        assert_eq!(app.session.history.undo.len(), undo0 + 1);
        app.mfx.core.state.preview.plan.shots[2].len = 4.0;
        app.mfx.core.state.preview.plan.total = 7.5;
        frame(&mut app, &ctx);
        let q = app.session.project.sequence(seq).unwrap();
        assert_eq!(q.video_tracks[0].items.len(), 3);
        assert!((q.duration().seconds() - 7.5).abs() < 0.05);
    }

    #[test]
    fn play_scopes_stop_and_loop_at_the_end_of_the_range() {
        let (mut app, ctx) = app_with_plan();
        frame(&mut app, &ctx);
        let r = command(&mut app, "play", &json!({"scope": "shot", "node": "s02"})).unwrap();
        assert_eq!(r["scope"], "shot");
        assert!(app.playback.playing);
        assert!((app.session.playhead().seconds() - 2.0).abs() < 0.05);
        // the playhead reaches the end of shot 2: Play stops there
        app.session.set_playhead(ticks(3.49));
        frame(&mut app, &ctx);
        assert!(!app.playback.playing);
        assert!(app.session.playhead().seconds() < 3.5);
        // with Loop it starts over
        command(&mut app, "loop", &json!({"on": true})).unwrap();
        command(&mut app, "play", &json!({"scope": "beat", "node": "s03"})).unwrap();
        assert!((app.session.playhead().seconds() - 3.5).abs() < 0.05);
        app.session.set_playhead(ticks(5.99));
        frame(&mut app, &ctx);
        assert!(app.playback.playing);
        assert!((app.session.playhead().seconds() - 3.5).abs() < 0.05);
        command(&mut app, "pause", &json!({})).unwrap();
        assert!(!app.playback.playing);
        // next / previous shot, and hostile params
        command(&mut app, "seek", &json!({"seconds": 0.0})).unwrap();
        command(&mut app, "nextShot", &json!({})).unwrap();
        assert!((app.session.playhead().seconds() - 2.0).abs() < 0.05);
        command(&mut app, "prevShot", &json!({})).unwrap();
        assert!(app.session.playhead().seconds() < 0.05);
        assert!(command(&mut app, "seek", &json!({"seconds": "x"})).is_err());
        assert!(command(&mut app, "seek", &json!({"node": "nope"})).is_err());
        assert!(command(&mut app, "seek", &json!({"seconds": f64::MAX})).is_ok());
        assert!(command(&mut app, "scope", &json!({"scope": 3})).is_err());
        assert!(command(&mut app, "bogus", &json!({})).is_err());
    }

    #[test]
    fn selecting_a_node_moves_the_playhead_and_space_plays_the_scope() {
        let (mut app, ctx) = app_with_plan();
        frame(&mut app, &ctx);
        app.mfx.core.state.selected_node = Some("b02".into());
        frame(&mut app, &ctx);
        assert!((app.session.playhead().seconds() - 3.5).abs() < 0.05);
        assert_eq!(app.mfx.core.state.preview.shot_at_playhead.as_deref(), Some("s03"));
        // Space (FilmCraft's own Play) with "This shot": plays the selected shot from its start
        app.mfx.core.state.preview.scope = Scope::Shot;
        app.mfx.core.state.selected_node = Some("s01".into());
        frame(&mut app, &ctx);
        crate::menus::invoke(&mut app, &ctx, "playback.toggle", json!({})).unwrap();
        frame(&mut app, &ctx);
        assert!(app.playback.playing);
        assert_eq!(app.mfx.preview.range.map(|r| r.1.seconds().round()), Some(2.0));
        // stage 5: the loose preview steps aside
        crate::menus::invoke(&mut app, &ctx, "stages.editVideo", json!({"force": true})).unwrap();
        frame(&mut app, &ctx);
        assert!(!app.mfx.core.state.preview.player.ready);
        assert!(command(&mut app, "play", &json!({})).is_err());
    }
}
