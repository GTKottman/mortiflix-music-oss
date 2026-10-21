//! #4: Edit › Preferences › Mortiflix… (the Mortiflix Settings window) closes on Escape, like the
//! editor's Preferences, Keyboard Shortcuts and Find dialogs. Before, only Done or the x closed it,
//! and with the Nodes panel focused its own Esc shortcut took the key first.

use std::sync::mpsc::{Sender, channel};

use egui_kittest::Harness;
use mortiflix_engine::Session;
use mortiflix_ui_egui::StudioApp;
use mortiflix_ui_egui::control::ControlRequest;
use mortiflix_ui_egui::dock::PanelKind;
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

    fn has(&mut self, id: &str) -> bool {
        let v = self.ok("ui.elements", json!({"prefix": id}));
        v.as_array().unwrap().iter().any(|e| e["id"] == json!(id))
    }

    fn open(&mut self) -> bool {
        self.harness.state().mfx.core.state.show_setup
    }
}

#[test]
fn escape_closes_mortiflix_settings() {
    let mut d = Driver::new();
    for (stage, panel) in [(0u8, None), (0, Some(PanelKind::MfxNodes)), (6, None)] {
        d.ok("engine.execute", json!({"command": "stages.set", "params": {"stage": stage, "force": true}}));
        d.frames(3);
        if let Some(p) = panel {
            d.harness.state_mut().ui.focused = p;
        }
        d.ok("ui.menu.invoke", json!({"id": "mortiflix.setup"}));
        d.frames(3);
        assert!(d.open(), "stage {stage}: Settings opened");
        assert!(d.has("mfx.setup.done"), "stage {stage}: it is Settings (Done), not the first-run steps");
        d.ok("ui.key", json!({"key": "Escape"}));
        d.frames(3);
        assert!(!d.open(), "stage {stage}, focused {panel:?}: Escape closes Mortiflix Settings");
    }
}
