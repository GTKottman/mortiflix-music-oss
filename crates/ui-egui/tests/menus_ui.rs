//! Headless UI tests of the M3.11 menu long tail: Find, Create Search Bin (and its Project panel
//! rows), Project Settings, Scene Edit Detection, Simplify Sequence, Automate to Sequence,
//! Normalize Mix Track, Dynamic Audio Waveforms, Reveal Log Files and the System Compatibility
//! Report, all driven by automation id through the control channel.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use mortiflix_engine::Session;
use mortiflix_engine::project::ItemId;
use mortiflix_ui_egui::StudioApp;
use mortiflix_ui_egui::control::ControlRequest;
use serde_json::{Value, json};

struct Driver {
    harness: Harness<'static, StudioApp>,
    tx: Sender<ControlRequest>,
}

impl Driver {
    fn new() -> Self {
        let mut s = Session::default();
        s.execute("file.openDemoProject", json!({})).unwrap();
        let (tx, rx) = channel();
        let mut app = StudioApp::new(s).with_control(rx);
        // never hand paths to the real OS opener from a test
        app.hooks.open_path = Some(Box::new(|_, _| Ok(())));
        let harness = Harness::builder().with_size(egui::vec2(1600.0, 980.0)).with_max_steps(10_000).build_eframe(move |_cc| app);
        let mut d = Driver { harness, tx };
        d.frames(4);
        d
    }

    fn frames(&mut self, n: usize) {
        for _ in 0..n {
            let ctx = self.harness.ctx.clone();
            let mut raw = std::mem::take(self.harness.input_mut());
            eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut raw);
            *self.harness.input_mut() = raw;
            self.harness.step();
        }
    }

    fn call(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                return v;
            }
        }
        panic!("no reply to {method} {params}");
    }

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let v = self.call(method, params.clone());
        assert_eq!(v["ok"], json!(true), "{method} {params} failed: {v}");
        v["result"].clone()
    }

    fn exec(&mut self, command: &str, params: Value) -> Value {
        self.ok("engine.execute", json!({"command": command, "params": params}))
    }

    fn menu(&mut self, id: &str) -> Value {
        let r = self.ok("ui.menu.invoke", json!({"id": id}));
        self.frames(3);
        r
    }

    fn click(&mut self, id: &str) {
        self.ok("ui.click", json!({"id": id}));
        self.frames(3);
    }

    fn has(&mut self, id: &str) -> bool {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        v.as_array().unwrap().iter().any(|e| e["id"] == json!(id))
    }

    fn app(&mut self) -> &mut StudioApp {
        self.harness.state_mut()
    }

    fn item_named(&mut self, name: &str) -> u64 {
        self.app().session.project.items.values().find(|i| i.name == name).unwrap().id.0
    }

    fn dialog_open(&mut self) -> bool {
        self.app().ui.extras.dialog.is_some()
    }
}

#[test]
fn menu_items_toggles_and_help() {
    let mut d = Driver::new();
    let menus = d.ok("ui.menu.list", json!({}));
    let find = |id: &str| menus.as_array().unwrap().iter().find(|m| m["id"] == id).cloned().unwrap_or_else(|| panic!("{id} not in menus"));
    assert_eq!(find("view.dynamicAudioWaveforms")["checked"], true);
    assert_eq!(find("help.systemCompatibilityReport")["path"], json!(["Help"]));
    assert_eq!(find("file.mediaProperties")["path"], json!(["File", "Get Media File Properties for"]));
    assert_eq!(find("clip.sceneEditDetection")["label"], "Scene Edit Detection…");
    // Dynamic Audio Waveforms toggles
    d.menu("view.dynamicAudioWaveforms");
    assert!(!d.app().ui.extras.dynamic_waveforms);
    let menus = d.ok("ui.menu.list", json!({}));
    assert_eq!(menus.as_array().unwrap().iter().find(|m| m["id"] == "view.dynamicAudioWaveforms").unwrap()["checked"], false);
    // System Compatibility Report
    assert_eq!(d.menu("help.systemCompatibilityReport")["dialog"], "systemReport");
    assert!(d.has("systemReport.ok"));
    d.click("systemReport.ok");
    assert!(!d.dialog_open());
    // Reveal Log Files writes this session's log and reveals it
    let r = d.menu("help.revealLogFiles");
    let path = r["path"].as_str().unwrap().to_string();
    assert!(std::fs::read_to_string(&path).unwrap().contains("\"os\""));
    assert_eq!(d.app().ui.extras.opened.last(), Some(&path));
    let _ = std::fs::remove_file(&path);
    // Import from Media Browser needs a Media Browser selection
    assert_eq!(d.call("ui.menu.invoke", json!({"id": "file.importFromMediaBrowser"}))["ok"], false);
}

#[test]
fn import_image_sequence_from_the_file_menu_and_media_browser() {
    let dir = std::env::temp_dir().join(format!("fc-ui-imgseq-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let paths: Vec<String> = (1..=5)
        .map(|n| {
            let p = dir.join(format!("plate_{n:03}.png"));
            image::RgbaImage::from_pixel(16, 9, image::Rgba([n as u8 * 40, 0, 0, 255])).save(&p).unwrap();
            p.to_string_lossy().to_string()
        })
        .collect();
    let mut d = Driver::new();
    let menus = d.ok("ui.menu.list", json!({}));
    let item = menus.as_array().unwrap().iter().find(|m| m["id"] == "file.importImageSequence").cloned().expect("File ▸ Import Image Sequence…");
    assert_eq!(item["path"], json!(["File"]));
    // File ▸ Import Image Sequence… with the first frame chosen
    let r = d.ok("ui.menu.invoke", json!({"id": "file.importImageSequence", "params": {"path": paths[0]}}));
    assert_eq!(r["imageSequences"][0]["frames"], 5);
    let id = ItemId(r["items"][0].as_u64().unwrap());
    let kind = d.app().session.project.item(id).unwrap().as_media().unwrap().info.kind;
    assert_eq!(kind, mortiflix_media::MediaKind::ImageSequence);
    // Media Browser ▸ Import as Image Sequence (the selection, from frame 3)
    d.exec("mediaBrowser.select", json!({"paths": [paths[2]]}));
    let r = d.ok("ui.menu.invoke", json!({"id": "file.importFromMediaBrowser", "params": {"imageSequence": true}}));
    assert_eq!(r["imageSequences"][0]["frames"], 3);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn find_dialog_and_search_bins() {
    let mut d = Driver::new();
    d.ok("ui.set", json!({"focused": "Project"}));
    assert_eq!(d.menu("edit.find")["dialog"], "find");
    for id in
        ["find.scope.project", "find.row.0.column", "find.row.0.operator", "find.row.0.text", "find.row.1.text", "find.match.any", "find.ok", "find.cancel"]
    {
        assert!(d.has(id), "{id}");
    }
    d.ok(
        "ui.set",
        json!({"menuDialog": {"rows": [{"column": "Name", "operator": "beginsWith", "text": "Neon"}, {"column": "Name", "operator": "contains", "text": ""}]}}),
    );
    d.click("find.ok");
    let neon = d.item_named("Neon_Loop.mov");
    assert_eq!(d.app().session.state.project_selection, vec![ItemId(neon)]);
    assert!(d.dialog_open(), "Find stays open until Done");
    d.click("find.cancel");
    assert!(!d.dialog_open());
    // timeline Find from the Timeline panel
    d.ok("ui.set", json!({"focused": "Timeline"}));
    d.menu("edit.find");
    assert_eq!(d.app().ui.extras.dialog.as_ref().unwrap().params["scope"], "timeline");
    d.ok("ui.set", json!({"menuDialog": {"rows": [{"column": "Name", "operator": "contains", "text": "forest"}]}}));
    d.click("find.ok");
    assert_eq!(d.app().session.state.selection.len(), 1);
    d.click("find.cancel");

    // Create Search Bin, shown in the Project panel's list view with its live contents
    d.exec("project.view.set", json!({"view": "list"}));
    d.ok("ui.panel.show", json!({"panel": "Project"}));
    d.menu("file.newSearchBin");
    assert!(d.has("searchBin.text") && d.has("searchBin.column"));
    d.ok("ui.set", json!({"menuDialog": {"text": "ocean", "name": "Ocean shots"}}));
    d.click("searchBin.ok");
    let bin = d.app().session.project.search_bins[0].id.0;
    d.frames(3);
    assert!(d.has(&format!("project.searchBin.{bin}")));
    d.click(&format!("project.searchBin.{bin}"));
    let ocean = d.item_named("Ocean_Sunset.mp4");
    assert!(d.app().ui.expanded_bins.contains(&bin));
    let items = d.ok("ui.elements", json!({"prefix": format!("project.item.{ocean}")}));
    assert!(!items.as_array().unwrap().is_empty(), "listed under the expanded search bin: {items}");
}

#[test]
fn project_settings_dialog() {
    let mut d = Driver::new();
    assert_eq!(d.menu("file.projectSettings.scratchDisks")["dialog"], "projectSettings");
    assert_eq!(d.app().ui.extras.dialog.as_ref().unwrap().params["tab"], "scratchDisks");
    for id in ["projectSettings.scratch.captured", "projectSettings.scratch.videoPreviews.browse", "projectSettings.scratch.autoSave.same"] {
        assert!(d.has(id), "{id}");
    }
    d.click("projectSettings.tab.general");
    for id in ["projectSettings.renderer", "projectSettings.videoDisplay", "projectSettings.titleSafe.horizontal", "projectSettings.captureFormat"] {
        assert!(d.has(id), "{id}");
    }
    d.ok(
        "ui.set",
        json!({"menuDialog": {"titleSafe": [15.0, 12.0], "renderer": mortiflix_engine::project_tools::RENDERER_SOFTWARE, "captured": "/tmp/fc-captured"}}),
    );
    d.click("projectSettings.tab.ingest");
    assert!(d.has("projectSettings.ingest.enabled"));
    d.click("projectSettings.ok");
    assert!(!d.dialog_open());
    let st = d.app().session.project.settings.clone();
    assert_eq!(st.title_safe, (15.0, 12.0));
    assert_eq!(st.renderer, mortiflix_engine::project_tools::RENDERER_SOFTWARE);
    assert_eq!(st.scratch.captured.as_deref(), Some("/tmp/fc-captured"));
    assert!(mortiflix_ui_egui::panels::menu_dialogs::software_renderer(d.app()));
}

#[test]
fn clip_and_sequence_dialogs() {
    let mut d = Driver::new();
    // Scene Edit Detection needs a selected video clip
    assert_eq!(d.call("ui.menu.invoke", json!({"id": "clip.sceneEditDetection"}))["ok"], false);
    let c = d.app().session.active_sequence().unwrap().video_tracks[0].items[0].id.0;
    d.exec("timeline.select", json!({"clips": [c]}));
    d.menu("clip.sceneEditDetection");
    for id in ["sceneDetect.applyCuts", "sceneDetect.createSubclips", "sceneDetect.generateMarkers", "sceneDetect.sensitivity", "sceneDetect.ok"] {
        assert!(d.has(id), "{id}");
    }
    d.click("sceneDetect.cancel");
    assert!(!d.dialog_open());
    // Normalize Mix Track
    d.menu("sequence.normalizeMixTrack");
    d.ok("ui.set", json!({"menuDialog": {"db": -6.0}}));
    d.click("normalizeMix.ok");
    assert!(!d.dialog_open());
    assert!(d.app().session.active_sequence().unwrap().master_volume_db != 0.0);
    // Simplify Sequence
    let n = d.app().session.project.items.len();
    d.menu("sequence.simplify");
    assert!(d.has("simplify.keep.audio") && d.has("simplify.closeGaps"));
    d.click("simplify.closeGaps");
    d.click("simplify.ok");
    assert_eq!(d.app().session.project.items.len(), n + 1);
    let active = d.app().session.state.active_sequence.unwrap();
    assert_eq!(d.app().session.project.item(active).unwrap().name, "Main Edit (Simplified)");
    // Automate to Sequence into a new sequence
    d.exec("file.newSequence", json!({"name": "Auto"}));
    let ids: Vec<u64> = ["Ocean_Sunset.mp4", "Misty_Forest.mp4"].iter().map(|n| d.item_named(n)).collect();
    d.exec("project.select", json!({"items": ids}));
    assert_eq!(d.menu("clip.automateToSequence")["dialog"], "automate");
    d.click("automate.method.overwrite");
    d.click("automate.ordering.selection");
    d.click("automate.ok");
    assert!(!d.dialog_open());
    let q = d.app().session.active_sequence().unwrap().clone();
    assert_eq!(q.video_tracks[0].items.iter().map(|i| i.item.0).collect::<Vec<_>>(), ids);
    // Get Media File Properties for ▸ Selection shows the probe info
    d.exec("project.select", json!({"items": [ids[0]]}));
    assert_eq!(d.menu("file.mediaProperties")["dialog"], "properties");
    assert_eq!(d.app().ui.extras.dialog.as_ref().unwrap().info[0]["video"]["width"], 1920);
    d.click("properties.ok");
    assert!(!d.dialog_open());
}

/// #29: File ▸ New ▸ Color Matte… asks for the color (and name, size, duration), and an existing
/// matte's color can be changed afterwards (double-click in the Project panel, or
/// `project.matteColor`), which used to leave every matte stuck at its first, dark grey color.
#[test]
fn color_matte_dialog_picks_and_changes_the_color() {
    let mut d = Driver::new();
    let matte_color = |d: &mut Driver, id: u64| match &d.app().session.project.item(ItemId(id)).unwrap().kind {
        mortiflix_engine::project::ItemKind::Media(m) => match &m.media {
            mortiflix_engine::project::MediaRef::Generator(mortiflix_media::Generator::ColorMatte { color }) => mortiflix_color::to_hex(*color),
            g => panic!("not a matte: {g:?}"),
        },
        _ => panic!("not media"),
    };
    assert_eq!(d.menu("file.newColorMatte")["dialog"], "colorMatte");
    for id in ["colorMatte.color", "colorMatte.name", "colorMatte.width", "colorMatte.height", "colorMatte.seconds", "colorMatte.ok"] {
        assert!(d.has(id), "{id}");
    }
    d.ok("ui.set", json!({"menuDialog": {"color": "#ff8000", "name": "Orange"}}));
    d.click("colorMatte.ok");
    assert!(!d.dialog_open());
    let orange = d.item_named("Orange");
    assert_eq!(matte_color(&mut d, orange), "#ff8000");
    // change it: the dialog starts from the current color
    d.exec("project.select", json!({"items": [orange]}));
    assert_eq!(d.menu("project.matteColor")["dialog"], "matteColor");
    assert_eq!(d.app().ui.extras.dialog.as_ref().unwrap().params["color"], "#ff8000");
    assert!(d.has("matteColor.color"));
    d.ok("ui.set", json!({"menuDialog": {"color": "#2040c0"}}));
    d.click("matteColor.ok");
    assert_eq!(matte_color(&mut d, orange), "#2040c0");
    // without a Color Matte selected there is nothing to change
    let seq = d.app().session.state.active_sequence.unwrap().0;
    d.exec("project.select", json!({"items": [seq]}));
    assert_eq!(d.call("ui.menu.invoke", json!({"id": "project.matteColor"}))["ok"], json!(false));
    assert!(!d.dialog_open());
}

#[test]
fn reveal_in_finder_opens_an_imported_clip() {
    let dir = std::env::temp_dir().join(format!("fc-ui-reveal-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clip.png");
    image::RgbaImage::from_pixel(8, 8, image::Rgba([10, 20, 30, 255])).save(&path).unwrap();
    let mut d = Driver::new();
    let item = d.exec("file.import", json!({"paths": [path.to_string_lossy()]}))["items"][0].as_u64().unwrap();
    assert!(!d.app().session.is_enabled("file.reveal"));
    d.exec("project.select", json!({"items": [item]}));
    assert!(d.app().session.is_enabled("file.reveal"));
    let r = d.ok("ui.menu.invoke", json!({"id": "file.reveal"}));
    assert_eq!(r["paths"][0], path.to_string_lossy().as_ref());
    assert_eq!(d.app().ui.extras.opened.last().map(String::as_str), Some(path.to_string_lossy().as_ref()));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A project saved for the first time is listed at the top of Home's Recent list and of File ▸
/// Open Recent, both of which reopen it; Clear Recent Files empties the list (#1461).
#[test]
fn saved_projects_are_listed_in_home_and_open_recent() {
    let dir = std::env::temp_dir().join(format!("fc-ui-recent-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let a = dir.join("first cut.fcproj").to_string_lossy().to_string();
    let b = dir.join("second cut.fcproj").to_string_lossy().to_string();
    let mut d = Driver::new();
    d.exec("file.saveAs", json!({"path": a}));
    d.exec("file.newProject", json!({"name": "Second"}));
    d.exec("file.saveAs", json!({"path": b}));
    let menus = d.ok("ui.menu.list", json!({}));
    let recent: Vec<&Value> = menus.as_array().unwrap().iter().filter(|m| m["path"] == json!(["File", "Open Recent"])).collect();
    let labels: Vec<&str> = recent.iter().map(|m| m["label"].as_str().unwrap()).collect();
    assert_eq!(labels, ["second cut.fcproj", "first cut.fcproj", "Clear Recent Files"]);
    assert_eq!(recent[0]["id"], "file.openRecent.0");
    // File ▸ Open Recent reopens the older one, which moves to the top
    d.menu("file.openRecent.1");
    assert_eq!(d.app().session.path.as_deref(), Some(a.as_str()));
    assert_eq!(d.app().session.prefs.general.recent_projects, [a.clone(), b.clone()]);
    // Home lists them too, newest first, and a click opens one
    d.click("header.home");
    let rows = d.ok("ui.elements", json!({"prefix": "import.recent."}));
    let rows: Vec<&str> = rows.as_array().unwrap().iter().map(|e| e["label"].as_str().unwrap()).collect();
    assert_eq!(rows, ["first cut.fcproj", "second cut.fcproj"]);
    d.click("import.recent.1");
    assert_eq!(d.app().session.path.as_deref(), Some(b.as_str()));
    // Clear Recent Files empties both lists
    d.menu("file.clearRecent");
    assert!(d.app().session.prefs.general.recent_projects.is_empty());
    let menus = d.ok("ui.menu.list", json!({}));
    let clear = menus.as_array().unwrap().iter().find(|m| m["id"] == "file.clearRecent").cloned().expect("Clear Recent Files");
    assert_eq!(clear["enabled"], false);
    assert!(!menus.as_array().unwrap().iter().any(|m| m["id"].as_str().is_some_and(|id| id.starts_with("file.openRecent."))));
    let _ = std::fs::remove_dir_all(&dir);
}

/// #15: Markers ▸ Add Chapter Marker… asks for the chapter's name and comments (like Add Flash
/// Cue Marker…) instead of adding a marker at once; Cancel adds nothing, an empty name is
/// numbered by the engine.
#[test]
fn add_chapter_marker_asks_for_a_name() {
    use mortiflix_engine::project::MarkerKind;
    let mut d = Driver::new();
    let chapters = |d: &mut Driver| -> Vec<(String, String)> {
        let q = d.app().session.active_sequence().unwrap();
        q.markers.iter().filter(|m| m.kind == MarkerKind::Chapter).map(|m| (m.name.clone(), m.comment.clone())).collect()
    };
    let before = chapters(&mut d);
    assert_eq!(d.menu("markers.addChapter")["dialog"], "chapter");
    for id in ["chapter.name", "chapter.comment", "chapter.ok", "chapter.cancel"] {
        assert!(d.has(id), "{id}");
    }
    // nothing is added until OK
    assert_eq!(chapters(&mut d), before);
    d.click("chapter.cancel");
    assert!(!d.dialog_open());
    assert_eq!(chapters(&mut d), before);
    // named
    d.menu("markers.addChapter");
    d.ok("ui.set", json!({"menuDialog": {"name": "Opening", "comment": "cold open"}}));
    d.click("chapter.ok");
    assert!(!d.dialog_open());
    let after = chapters(&mut d);
    assert_eq!(after.len(), before.len() + 1);
    assert!(after.contains(&("Opening".to_string(), "cold open".to_string())), "{after:?}");
    // empty name: the engine numbers it
    d.menu("markers.addChapter");
    d.click("chapter.ok");
    let n = before.len() + 2;
    let after = chapters(&mut d);
    assert_eq!(after.len(), n);
    assert!(after.contains(&(format!("Chapter {n}"), String::new())), "{after:?}");
}

/// #14: Clip ▸ Rename… opens a box with the selected clip's name to edit (as in Premiere Pro)
/// instead of running `clip.rename` without a name ("need `name`"); Cancel and an empty name
/// change nothing, OK renames the clip.
#[test]
fn clip_rename_asks_for_the_new_name() {
    let mut d = Driver::new();
    let c = d.app().session.active_sequence().unwrap().video_tracks[0].items[0].id.0;
    let name = |d: &mut Driver| -> String {
        let q = d.app().session.active_sequence().unwrap();
        q.find_item(mortiflix_engine::project::ClipId(c)).unwrap().1.name.clone()
    };
    let before = name(&mut d);
    d.exec("project.select", json!({"items": []}));
    d.exec("timeline.select", json!({"clips": [c]}));
    assert_eq!(d.menu("clip.rename")["dialog"], "clipRename");
    for id in ["clipRename.name", "clipRename.ok", "clipRename.cancel"] {
        assert!(d.has(id), "{id}");
    }
    // the box starts with the clip's current name; nothing changes until OK
    assert_eq!(d.app().ui.extras.dialog.as_ref().unwrap().params["name"], json!(before));
    d.click("clipRename.cancel");
    assert!(!d.dialog_open());
    assert_eq!(name(&mut d), before);
    // an empty name keeps the box open with an error
    d.menu("clip.rename");
    d.ok("ui.set", json!({"menuDialog": {"name": "  "}}));
    d.click("clipRename.ok");
    assert!(d.dialog_open());
    assert_eq!(name(&mut d), before);
    // OK renames the clip
    d.ok("ui.set", json!({"menuDialog": {"name": "Sunset Opener"}}));
    d.click("clipRename.ok");
    assert!(!d.dialog_open());
    assert_eq!(name(&mut d), "Sunset Opener");
    assert!(!d.app().ui.status.contains("need `name`"), "{}", d.app().ui.status);
}

/// #10: the Sequence and Markers toggles (Snap in Timeline, Linked Selection, Selection Follows
/// Playhead, Show Through Edits, Ripple Sequence Markers, Copy Paste Includes Sequence Markers)
/// showed no check mark in either state; the menu now shows whether each one is on.
#[test]
fn sequence_and_marker_toggles_show_a_check_mark() {
    let mut d = Driver::new();
    let checked = |d: &mut Driver, id: &str| -> Option<bool> {
        let menus = d.ok("ui.menu.list", json!({}));
        let it = menus.as_array().unwrap().iter().find(|m| m["id"] == id).cloned().unwrap_or_else(|| panic!("{id} not in menus"));
        it["checked"].as_bool()
    };
    for id in [
        "sequence.snap",
        "sequence.linkedSelection",
        "sequence.selectionFollowsPlayhead",
        "sequence.showThroughEdits",
        "markers.rippleSequenceMarkers",
        "markers.copyPasteIncludesSequenceMarkers",
    ] {
        let before = checked(&mut d, id).unwrap_or_else(|| panic!("{id} shows no check state"));
        d.menu(id);
        assert_eq!(checked(&mut d, id), Some(!before), "{id} after one click");
        d.menu(id);
        assert_eq!(checked(&mut d, id), Some(before), "{id} after two clicks");
    }
    // the state follows the engine, whoever changes it
    d.exec("sequence.snap", json!({"on": false}));
    assert_eq!(checked(&mut d, "sequence.snap"), Some(false));
    d.exec("sequence.snap", json!({"on": true}));
    assert_eq!(checked(&mut d, "sequence.snap"), Some(true));
}

/// Whether a menu item is enabled in the menus right now.
fn menu_enabled(d: &mut Driver, id: &str) -> bool {
    let menus = d.ok("ui.menu.list", json!({}));
    let it = menus.as_array().unwrap().iter().find(|m| m["id"] == id).cloned().unwrap_or_else(|| panic!("{id} not in menus"));
    it["enabled"].as_bool().unwrap()
}

/// The status bar after a menu item failed: a plain reason, never the command's internal name.
fn assert_plain_status(d: &mut Driver, id: &str) {
    let status = d.app().ui.status.clone();
    let shown = mortiflix_ui_egui::menus::status_text(&status).into_owned();
    assert!(!shown.is_empty(), "{id}: no reason in the status bar");
    assert!(!shown.contains(id) && !shown.contains("invalid parameters") && !shown.contains("not available right now"), "{id}: {shown:?}");
}

/// #9: Graphics and Titles ▸ Upgrade Caption to Graphic was enabled with no caption selected and
/// the status bar read "invalid parameters for `graphics.upgradeCaption`: select captions to
/// upgrade". It is greyed until there is a caption to upgrade (the selected ones, else the one
/// under the playhead), and the status bar shows an engine error's reason as a plain sentence.
#[test]
fn upgrade_caption_is_greyed_without_a_caption_and_errors_read_plainly() {
    let mut d = Driver::new();
    // a caption far from the playhead, and nothing selected
    let cap = d.exec("captions.add", json!({"text": "Later", "seconds": 600, "durationSeconds": 2}))["caption"].clone();
    d.exec("captions.select", json!({"captions": []}));
    assert!(!menu_enabled(&mut d, "graphics.upgradeCaption"), "greyed with no caption selected or under the playhead");
    let r = d.call("ui.menu.invoke", json!({"id": "graphics.upgradeCaption"}));
    assert_eq!(r["ok"], false, "{r}");
    assert_plain_status(&mut d, "graphics.upgradeCaption");
    // selected: usable, and it upgrades
    d.exec("captions.select", json!({"captions": [cap]}));
    assert!(menu_enabled(&mut d, "graphics.upgradeCaption"));
    assert_eq!(d.menu("graphics.upgradeCaption")["upgraded"], 1);
    // the status bar's sentence for an engine error
    use mortiflix_ui_egui::menus::status_text;
    assert_eq!(status_text("invalid parameters for `graphics.upgradeCaption`: select captions to upgrade"), "Select captions to upgrade");
    assert_eq!(status_text("command `clip.rename` is not available right now: no clips selected"), "No clips selected");
    assert_eq!(status_text("Opened https://example.com"), "Opened https://example.com");
}

/// #8: Sequence ▸ Transcript ▸ Transcribe… showed "invalid parameters for `transcript.generate`:
/// nothing to transcribe…" and no dialog, with a sequence full of audio open (and the sequence
/// selected in the Project panel). It opens a Transcribe dialog (language, speakers) like
/// Transcribe Sequence…, and OK transcribes the sequence's sound.
#[test]
fn transcript_transcribe_opens_a_dialog() {
    use mortiflix_engine::project::{Transcript, Word};
    use mortiflix_engine::time::Tick;
    let mut d = Driver::new();
    let a1 = d.app().session.active_sequence().unwrap().audio_tracks[0].items[0].clone();
    let mut t = Transcript { language: "en".into(), ..Default::default() };
    t.words.push(Word::new("Hello", a1.source_in, a1.source_in + Tick::from_seconds_f64(0.5)));
    t.normalize();
    d.app().session.transcriber = Some(std::sync::Arc::new(mortiflix_speech::FixedTranscriber { transcript: t, id: "fixed".into() }));
    let seq = d.app().session.state.active_sequence.unwrap();
    d.exec("project.select", json!({"items": [seq.0]}));
    assert!(menu_enabled(&mut d, "transcript.generate"));
    assert_eq!(d.menu("transcript.generate")["dialog"], "transcript");
    for id in ["transcript.language", "transcript.diarize", "transcript.ok", "transcript.cancel"] {
        assert!(d.has(id), "{id}");
    }
    assert!(!d.app().session.project.transcripts.contains_key(&a1.item), "nothing runs before OK");
    d.click("transcript.ok");
    assert!(!d.dialog_open(), "{:?}", d.app().ui.extras.dialog);
    assert!(d.app().session.project.transcripts.contains_key(&a1.item));
}

/// #7: Sequence ▸ Close Gap with no gap selected put "invalid parameters for `sequence.closeGap`:
/// select a gap or provide `track`" in the status bar; File ▸ Import from Media Browser with
/// nothing selected there did the same. Both are greyed until there is something to act on (a
/// gap selected in the Timeline, files selected in the Media Browser), say why in plain words
/// when a key runs them anyway, and still run for a caller that names its target.
#[test]
fn close_gap_and_import_from_media_browser_wait_for_a_selection() {
    let mut d = Driver::new();
    for id in ["sequence.closeGap", "file.importFromMediaBrowser"] {
        assert!(!menu_enabled(&mut d, id), "{id} greyed with nothing selected");
        let r = d.call("ui.menu.invoke", json!({"id": id}));
        assert_eq!(r["ok"], false, "{r}");
        assert_plain_status(&mut d, id);
    }
    // lift a V1 clip to leave a gap, then select the gap
    let c = d.app().session.active_sequence().unwrap().video_tracks[0].items[1].clone();
    d.exec("edit.clear", json!({"clips": [c.id.0]}));
    let mid = c.start.0 + (c.end().0 - c.start.0) / 2;
    d.exec("timeline.selectGap", json!({"track": "V1", "time": mid}));
    assert!(menu_enabled(&mut d, "sequence.closeGap"), "usable with a gap selected");
    d.menu("sequence.closeGap");
    assert!(d.app().session.state.gap_selection.is_none(), "the gap is closed");
    // files selected in the Media Browser
    let dir = std::env::temp_dir().join(format!("mfx-menu-mb-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("still.png");
    image::RgbaImage::from_pixel(16, 9, image::Rgba([200, 40, 0, 255])).save(&path).unwrap();
    let path = path.to_string_lossy().to_string();
    d.exec("mediaBrowser.select", json!({"paths": [path]}));
    assert!(menu_enabled(&mut d, "file.importFromMediaBrowser"), "usable with files selected");
    let r = d.menu("file.importFromMediaBrowser");
    assert_eq!(r["items"].as_array().map(Vec::len), Some(1), "{r}");
    let _ = std::fs::remove_dir_all(&dir);
}
