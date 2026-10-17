//! Into FilmCraft (doc 38), FilmCraft side: runs the hand-off the `into_filmcraft` part plans, in
//! this process, through `Session::execute`, as one undo step each ("Mortiflix: …" in History).
//!
//! - Entering stage 5 (Edit the video, Next from Voice & music, Ctrl+Alt+5) docks the Mortiflix
//!   panel beside the Mortiflix chat and, the first time, asks the service for the plan.
//! - When the plan arrives the project is built once ([`build`]) and saved as the project's
//!   `.fcproj`; after that new versions are swapped in, never rebuilt.
//! - Each frame on stage 5 the timeline selection is handed to the panel (the clicked clip).
//!
//! Commands: `mortiflix.importEdit {again?}`, `mortiflix.import.run {open_edit?, again?}` (Get the
//! video: `open_edit: false` imports without opening Edit, for Export), `mortiflix.restartEdit` (File ▸ Start the Edit Again
//! from Mortiflix…, asks first), `mortiflix.useVersion {node, version}`,
//! `mortiflix.timelineEdit {label, steps: [{id, params}]}` (several commands as one undo step).

use filmcraft_engine::EngineError;
use filmcraft_mortiflix_ui::{self as mfx, Nav, into_filmcraft as into};
use serde_json::{Value, json};

use crate::FilmcraftApp;
use crate::dock::PanelKind;

/// Most steps one `mortiflix.timelineEdit` runs.
const MAX_STEPS: usize = 64;

/// `mortiflix.importEdit` / `restartEdit` / `useVersion` / `timelineEdit`.
pub fn route(app: &mut FilmcraftApp, id: &str, params: &Value) -> Option<Result<Value, String>> {
    let r = match id {
        "mortiflix.importEdit" => {
            let again = params.get("again").and_then(Value::as_bool).unwrap_or(false);
            go_to_filmcraft(app);
            let core = &mut app.mfx.core;
            let started = mfx::into_filmcraft::start(&mut core.state, &mut core.service, again);
            Ok(json!({"started": started, "phase": core.state.handoff.phase}))
        }
        // Get the video (part 07): import without opening Edit, then Export renders the timeline.
        "mortiflix.import.run" => {
            let open_edit = params.get("open_edit").and_then(Value::as_bool).unwrap_or(true);
            let again = params.get("again").and_then(Value::as_bool).unwrap_or(false);
            if open_edit {
                go_to_filmcraft(app);
            } else {
                app.mfx.core.activate();
            }
            let core = &mut app.mfx.core;
            let started = mfx::into_filmcraft::start(&mut core.state, &mut core.service, again);
            if started {
                core.state.handoff.quiet = !open_edit;
            }
            Ok(json!({"started": started, "phase": core.state.handoff.phase}))
        }
        "mortiflix.restartEdit" => {
            go_to_filmcraft(app);
            app.mfx.core.state.handoff.confirm_restart = true;
            app.show_panel(PanelKind::MfxStage);
            Ok(json!({"asked": true}))
        }
        "mortiflix.useVersion" => {
            let node = params.get("node").and_then(Value::as_str).filter(|n| !n.is_empty() && n.len() <= 200);
            let version = params.get("version").and_then(Value::as_u64).and_then(|v| u32::try_from(v).ok());
            match (node, version) {
                (Some(node), Some(version)) => use_version(app, node, version),
                _ => Err("mortiflix.useVersion needs a `node` and a `version`".into()),
            }
        }
        "mortiflix.timelineEdit" => timeline_edit(app, params),
        _ => return None,
    };
    Some(r)
}

/// Each frame: read the service for the hand-off, run what it asks for, hand over the selection.
pub fn frame(app: &mut FilmcraftApp, ctx: &egui::Context) {
    if !app.mfx.core.state.active {
        return;
    }
    let now = ctx.input(|i| i.time);
    let core = &mut app.mfx.core;
    into::poll(&mut core.state, &mut core.service, now);
    for job in into::take_jobs(&mut app.mfx.core.state) {
        match job {
            into::Job::Build(plan) => build(app, &plan),
            into::Job::Swap(piece) => swap(app, &piece),
        }
    }
    if app.mfx.core.state.stage == mfx::STAGE_FILMCRAFT && app.mfx.core.state.handoff.phase == into::Phase::Done {
        selection(app);
    }
}

/// After the stage changed (`mortiflix::apply_stage`): on stage 5 the Mortiflix panel sits beside
/// the chat, and the first visit brings the video in.
pub fn on_stage(app: &mut FilmcraftApp, stage: u8) {
    if stage != mfx::STAGE_FILMCRAFT {
        return;
    }
    if !app.ui.dock.contains(PanelKind::MfxStage) && !app.ui.floating.iter().any(|g| g.panels.contains(&PanelKind::MfxStage)) {
        let near = if app.ui.dock.contains(PanelKind::MfxChat) { PanelKind::MfxChat } else { PanelKind::Program };
        app.ui.dock.open_near(PanelKind::MfxStage, near);
    }
    app.show_panel(PanelKind::MfxStage);
    let core = &mut app.mfx.core;
    if core.state.handoff.phase == into::Phase::NotYet && core.state.project_id.is_some() {
        into::start(&mut core.state, &mut core.service, false);
    }
}

fn go_to_filmcraft(app: &mut FilmcraftApp) {
    app.mfx.core.activate();
    if app.mfx.core.state.stage != mfx::STAGE_FILMCRAFT
        && let Some(Nav::Moved { to, .. }) = app.mfx.core.navigate("filmcraft", true)
    {
        crate::mortiflix::apply_stage(app, to);
    }
}

fn engine_err(e: String) -> EngineError {
    EngineError::Other(e)
}

/// Build the project from the plan: a fresh project when the open one has nothing unsaved (never
/// lose work), everything in one undo step, the tracks named, saved as the project's `.fcproj`.
fn build(app: &mut FilmcraftApp, plan: &into::Plan) {
    if !app.session.is_dirty()
        && let Err(e) = app.session.execute("file.newProject", json!({"name": plan.title}))
    {
        app.ui.status = format!("Mortiflix: a new FilmCraft project couldn't be made ({e}); it goes into this one");
    }
    let label = format!("Mortiflix: Bring {} into FilmCraft", plan.title);
    let session = &mut app.session;
    let h = &mut app.mfx.core.state.handoff;
    let r = session.edit_group(&label, |s| {
        into::build(h, plan, &mut |id, p| s.execute(id, p).map_err(|e| e.to_string()));
        if h.phase != into::Phase::Done {
            return Err(engine_err(h.error.clone()));
        }
        let names = h.tracks.clone();
        s.edit_sequence("Name Tracks", |q, _, _| {
            for t in &names {
                let list = if t.video { &mut q.video_tracks } else { &mut q.audio_tracks };
                if let Some(track) = list.get_mut(t.index) {
                    track.name = t.name.clone();
                }
            }
            Ok(())
        })
    });
    if let Err(e) = r {
        let h = &mut app.mfx.core.state.handoff;
        h.phase = into::Phase::Failed;
        if h.error.is_empty() {
            h.error = e.to_string();
        }
        return;
    }
    let path = app.mfx.core.state.handoff.fcproj.clone();
    if !path.is_empty() {
        if let Some(dir) = std::path::Path::new(&path).parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = app.session.execute("file.saveAs", json!({"path": path})) {
            app.ui.status = format!("Mortiflix: the edit is placed but couldn't be saved to {path}: {e}");
        }
    }
    // tell the service what came in (done once): later versions are swapped in, never rebuilt
    let core = &mut app.mfx.core;
    core.service.request("handoff.done", into::done_params(&core.state.handoff));
    if !app.mfx.core.state.handoff.quiet {
        app.session.toast("Everything is placed. Click any clip to change it.");
        app.show_panel(PanelKind::MfxStage);
    }
}

fn swap(app: &mut FilmcraftApp, piece: &into::Piece) {
    let label = format!("Mortiflix: {} v{}", piece.label, piece.version);
    let session = &mut app.session;
    let h = &mut app.mfx.core.state.handoff;
    let r = session.edit_group(&label, |s| into::swap(h, piece, &mut |id, p| s.execute(id, p).map_err(|e| e.to_string())).map_err(engine_err));
    match r {
        Ok(name) => {
            let h = &app.mfx.core.state.handoff;
            let mut params = json!({"node": piece.node, "version": piece.version});
            if let Some(p) = h.placed.iter().find(|p| p.node == piece.node) {
                params["clip"] = json!(p.clip);
                params["item"] = json!(p.item);
            }
            if !h.project.is_empty() {
                params["project"] = json!(h.project);
            }
            app.mfx.core.service.request("handoff.swapped", params);
            let _ = app.session.execute("file.save", json!({}));
            app.session.toast(format!("{name} is on the timeline."));
        }
        Err(e) => app.ui.status = format!("Mortiflix: {} v{} couldn't come in: {e}", piece.label, piece.version),
    }
}

fn use_version(app: &mut FilmcraftApp, node: &str, version: u32) -> Result<Value, String> {
    let session = &mut app.session;
    let h = &mut app.mfx.core.state.handoff;
    let label = format!("Mortiflix: Use {} v{version}", h.placed.iter().find(|p| p.node == node).map_or(node, |p| p.label.as_str()));
    let name = session
        .edit_group(&label, |s| into::use_version(h, node, version, &mut |id, p| s.execute(id, p).map_err(|e| e.to_string())).map_err(engine_err))
        .map_err(|e| e.to_string())?;
    // Mortiflix plays that version too (node area, preview, later swaps agree with the timeline)
    let core = &mut app.mfx.core;
    let h = &core.state.handoff;
    let mut swapped = json!({"node": node, "version": version});
    if let Some(p) = h.placed.iter().find(|p| p.node == node) {
        swapped["clip"] = json!(p.clip);
        swapped["item"] = json!(p.item);
    }
    let in_bin = h.items.iter().filter(|i| i.node == node).map(|i| i.version).max().unwrap_or(0);
    let newest = h.nodes.get(node).map_or(0, |n| n.current).max(in_bin);
    let pick = if newest > 0 && newest == version { Value::Null } else { json!(version) };
    core.service.request("node.pick", json!({"node": node, "v": pick}));
    core.service.request("handoff.swapped", swapped);
    let _ = app.session.execute("file.save", json!({}));
    Ok(json!({"clip": name}))
}

/// Several FilmCraft commands as one undo step named "Mortiflix: …" (a trim asked for in words).
fn timeline_edit(app: &mut FilmcraftApp, params: &Value) -> Result<Value, String> {
    let label = params.get("label").and_then(Value::as_str).unwrap_or("Mortiflix: Edit");
    let label: String = label.chars().take(120).collect();
    let label = if label.starts_with("Mortiflix: ") { label } else { format!("Mortiflix: {label}") };
    let steps = params.get("steps").and_then(Value::as_array).ok_or("mortiflix.timelineEdit needs `steps`")?;
    if steps.is_empty() || steps.len() > MAX_STEPS {
        return Err(format!("mortiflix.timelineEdit needs 1 to {MAX_STEPS} steps"));
    }
    let mut list = Vec::new();
    for st in steps {
        let id = st.get("id").and_then(Value::as_str).filter(|i| !i.is_empty() && i.len() <= 100).ok_or("every step needs an `id`")?;
        // only FilmCraft's own engine commands: no Mortiflix or UI commands inside a timeline edit
        if id.starts_with("mortiflix.") || id.starts_with("stages.") {
            return Err(format!("`{id}` can't be a timeline edit step"));
        }
        list.push((id.to_string(), st.get("params").cloned().unwrap_or(json!({}))));
    }
    let results =
        app.session.edit_group(&label, |s| list.into_iter().map(|(id, p)| s.execute(&id, p)).collect::<Result<Vec<_>, _>>()).map_err(|e| e.to_string())?;
    Ok(json!({"label": label, "results": results}))
}

/// Hand the timeline selection to the Mortiflix panel.
fn selection(app: &mut FilmcraftApp) {
    let mut out = Vec::new();
    if let Some(q) = app.session.active_sequence() {
        for c in app.session.state.selection.iter().take(50) {
            let Some((tid, item)) = q.find_item(*c) else { continue };
            let track = if let Some(i) = q.video_tracks.iter().position(|t| t.id == tid) {
                q.video_tracks.get(i).map(|t| format!("V{} {}", i + 1, t.name))
            } else {
                q.audio_tracks.iter().position(|t| t.id == tid).and_then(|i| q.audio_tracks.get(i).map(|t| format!("A{} {}", i + 1, t.name)))
            };
            out.push(into::Selected {
                clip: c.0,
                name: item.name.clone(),
                start_sec: item.start.seconds(),
                end_sec: item.end().seconds(),
                track: track.unwrap_or_default().trim().to_string(),
            });
        }
    }
    let h = &mut app.mfx.core.state.handoff;
    if h.selected != out {
        h.selected = out;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_commands_are_refused_calmly() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        for (id, p) in [
            ("mortiflix.useVersion", json!({})),
            ("mortiflix.useVersion", json!({"node": "s01", "version": -1})),
            ("mortiflix.useVersion", json!({"node": "s01", "version": 2})),
            ("mortiflix.timelineEdit", json!({})),
            ("mortiflix.timelineEdit", json!({"steps": []})),
            ("mortiflix.timelineEdit", json!({"steps": [{"id": "stages.next"}]})),
            ("mortiflix.timelineEdit", json!({"steps": [{"id": "no.such.command"}]})),
        ] {
            assert!(crate::menus::invoke(&mut app, &ctx, id, p.clone()).is_err(), "{id} {p}");
        }
        // a failed step takes nothing with it: the history is as before
        let before = app.session.undo_mark();
        assert!(crate::menus::invoke(&mut app, &ctx, "mortiflix.timelineEdit", json!({"steps": [{"id": "markers.add", "params": {"frame": 0}}]})).is_err());
        assert!(app.session.undo_mark() == before);
    }

    #[test]
    fn import_edit_goes_to_stage_5_and_asks_the_service() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        let r = crate::menus::invoke(&mut app, &ctx, "mortiflix.importEdit", json!({})).unwrap();
        assert_eq!(r["started"], true);
        assert_eq!(app.mfx.core.state.stage, mfx::STAGE_FILMCRAFT);
        assert!(app.ui.dock.contains(PanelKind::MfxStage));
        // the service isn't running: the hand-off fails calmly and can be tried again
        frame(&mut app, &ctx);
        assert_eq!(app.mfx.core.state.handoff.phase, into::Phase::Failed);
        // Get the video: the import without opening Edit (the stage stays)
        let mut quiet = FilmcraftApp::new(filmcraft_engine::Session::default());
        let r = crate::menus::invoke(&mut quiet, &ctx, "mortiflix.import.run", json!({"open_edit": false})).unwrap();
        assert_eq!(r["started"], true);
        assert!(quiet.mfx.core.state.handoff.quiet);
        assert_eq!(quiet.mfx.core.state.stage, 0);
        let r = crate::menus::invoke(&mut app, &ctx, "mortiflix.restartEdit", json!({})).unwrap();
        assert_eq!(r["asked"], true);
        assert!(app.mfx.core.state.handoff.confirm_restart);
    }

    #[test]
    fn a_built_plan_lands_in_the_session_as_one_undo_step() {
        let mut app = FilmcraftApp::new(filmcraft_engine::Session::default());
        let ctx = egui::Context::default();
        let dir = std::env::temp_dir().join(format!("mfx-into-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // two quiet WAV files: a voice line and the music
        let wav = |name: &str, secs: u32| {
            let n = 48_000 * secs;
            let mut b = Vec::new();
            b.extend_from_slice(b"RIFF");
            b.extend_from_slice(&(36 + n * 2).to_le_bytes());
            b.extend_from_slice(b"WAVEfmt ");
            b.extend_from_slice(&16u32.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            b.extend_from_slice(&1u16.to_le_bytes());
            b.extend_from_slice(&48_000u32.to_le_bytes());
            b.extend_from_slice(&96_000u32.to_le_bytes());
            b.extend_from_slice(&2u16.to_le_bytes());
            b.extend_from_slice(&16u16.to_le_bytes());
            b.extend_from_slice(b"data");
            b.extend_from_slice(&(n * 2).to_le_bytes());
            b.resize(b.len() + (n as usize) * 2, 0);
            let p = dir.join(name);
            std::fs::write(&p, b).unwrap();
            p.to_string_lossy().into_owned()
        };
        let (line, music) = (wav("l1.wav", 2), wav("music.wav", 6));
        let plan = into::Plan::parse(&json!({
            "project": "p1", "title": "Sound Only", "fps": 30, "fcproj": dir.join("Sound Only.fcproj").to_string_lossy(),
            "beats": [{"id": "b01", "number": 1, "name": "Cold open", "colour": "Violet", "comment": "Hi", "start_sec": 0}],
            "pieces": [
                {"node": "b01-l1", "kind": "line", "label": "b01-l1", "beat": "b01", "version": 1, "current": true, "file": line, "start_sec": 0.5, "end_sec": 2.5, "text": "Hello there."},
                {"node": "music", "kind": "music", "label": "Music", "version": 1, "current": true, "file": music, "start_sec": 0}
            ],
            "cues": [{"node": "m01", "name": "M01 · Intro", "beat": "b01", "colour": "Violet", "start_sec": 0, "end_sec": 2},
                     {"node": "m02", "name": "M02 · Lift", "beat": "b01", "colour": "Violet", "start_sec": 2, "end_sec": 6}]
        }))
        .unwrap();
        app.mfx.core.activate();
        build(&mut app, &plan);
        let h = &app.mfx.core.state.handoff;
        assert_eq!(h.phase, into::Phase::Done, "{} {:?}", h.error, h.rows);
        assert_eq!(h.placed.len(), 3, "the line and the music cut at its two cues");
        let q = app.session.active_sequence().unwrap();
        assert_eq!(q.audio_tracks.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), ["Voice", "Music"]);
        let line_clip = q.find_item(filmcraft_project::ClipId(h.placed[0].clip)).unwrap().1;
        assert_eq!(line_clip.label, filmcraft_project::Label::Violet);
        assert_eq!(line_clip.name, "B01-L1 · Hello there. · v1");
        assert!(q.markers.iter().any(|m| m.name == "1 Cold open"));
        assert!(dir.join("Sound Only.fcproj").exists(), "saved as the project's .fcproj");
        // a trim asked for in words: one undo step named "Mortiflix: …"
        let clip = app.mfx.core.state.handoff.placed[0].clip;
        let r = crate::menus::invoke(
            &mut app,
            &ctx,
            "mortiflix.timelineEdit",
            json!({"label": "Trim it", "steps": [{"id": "timeline.trim", "params": {"clip": clip, "edge": "out", "mode": "regular", "deltaFrames": -6}}]}),
        );
        assert!(r.is_ok(), "{r:?}");
        assert_eq!(app.session.undo().as_deref(), Some("Mortiflix: Trim it"));
        // the whole hand-off is one undo step (the new project before it is FilmCraft's own)
        assert!(app.session.undo().is_some_and(|l| l.starts_with("Mortiflix: Bring")));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = ctx;
    }
}
