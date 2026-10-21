//! #16: with Mortiflix on, the editor's own menus, menu items, tabs, buttons and shortcuts are
//! there only at the Edit stage. Going back to any other stage hides them and turns the shortcuts
//! off again; the Mortiflix and app-wide ones (Stages, Help, project open/save, Preferences) stay.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use mortiflix_engine::Session;
use mortiflix_ui_egui::StudioApp;
use mortiflix_ui_egui::control::ControlRequest;
use mortiflix_ui_egui::state::Tool;
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

    fn ok(&mut self, method: &str, params: Value) -> Value {
        let (req, reply) = ControlRequest::new(method, params.clone());
        self.tx.send(req).unwrap();
        for _ in 0..600 {
            self.frames(1);
            if let Ok(v) = reply.try_recv() {
                assert_eq!(v["ok"], json!(true), "{method} {params} failed: {v}");
                return v["result"].clone();
            }
        }
        panic!("no reply to {method} {params}");
    }

    fn stage(&mut self, stage: u8) {
        self.ok("engine.execute", json!({"command": "stages.set", "params": {"stage": stage, "force": true}}));
        self.frames(3);
    }

    fn menu_ids(&mut self) -> Vec<String> {
        let v = self.ok("ui.menu.list", json!({}));
        v.as_array().unwrap().iter().map(|it| it["id"].as_str().unwrap().to_string()).collect()
    }

    fn has(&mut self, id: &str) -> bool {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        v.as_array().unwrap().iter().any(|e| e["id"] == json!(id))
    }

    fn tool(&self) -> Tool {
        self.harness.state().ui.tool
    }

    /// What the editor shows at this stage: its menus, a menu item in a shared menu, the mode tabs
    /// and a header button.
    fn editor_on_screen(&mut self) -> [bool; 5] {
        let ids = self.menu_ids();
        [
            self.has("menu.Clip"),
            self.has("menu.Sequence"),
            ids.iter().any(|i| i == "edit.rippleDelete"),
            self.has("header.mode.edit"),
            self.has("header.quickExport"),
        ]
    }
}

#[test]
fn editor_menus_tabs_buttons_and_shortcuts_only_at_the_edit_stage() {
    let mut d = Driver::new();
    // Mortiflix Studio alone (Mortiflix off) is the full editor
    assert_eq!(d.editor_on_screen(), [true; 5]);

    // stage 1 Script: none of the editor's things, and its shortcut (C = Razor) does nothing
    d.stage(0);
    assert_eq!(d.editor_on_screen(), [false; 5]);
    let ids = d.menu_ids();
    for kept in
        ["file.newProject", "file.open", "file.save", "stages.edit", "help.discord", "app.settings.general", "app.language.english", "window.panel.MfxChat"]
    {
        assert!(ids.iter().any(|i| i == kept), "{kept} stays on every stage");
    }
    for gone in ["file.newSequence", "file.exportEdl", "edit.undo", "view.zoomIn", "window.workspace.editing", "window.panel.Timeline"] {
        assert!(!ids.iter().any(|i| i == gone), "{gone} is the editor's");
    }
    assert!(d.has("menu.Stages") && d.has("menu.Help") && d.has("header.discord"));
    let before = d.tool();
    d.ok("ui.key", json!({"key": "C"}));
    d.frames(2);
    assert_eq!(d.tool(), before, "the Razor key is off before Edit");

    // stage 7 Edit: all back, and the shortcut works
    d.stage(6);
    assert_eq!(d.editor_on_screen(), [true; 5]);
    d.ok("ui.key", json!({"key": "C"}));
    d.frames(2);
    assert_eq!(d.tool(), Tool::Razor);

    // back to stage 1: hidden again, shortcut off again
    d.stage(0);
    assert_eq!(d.editor_on_screen(), [false; 5]);
    d.ok("ui.key", json!({"key": "V"}));
    d.frames(2);
    assert_eq!(d.tool(), Tool::Razor, "the Selection key is off again");

    // stage 8 Render is like the stages before Edit
    d.stage(7);
    assert_eq!(d.editor_on_screen(), [false; 5]);
}
