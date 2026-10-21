//! The Mortiflix state the stage bar and the parts share. Serde, so the control channel can read it
//! (`mortiflix.state`) and the service can send parts of it.
//!
//! Parts that need typed data later (the node graph, presets) start from the `serde_json::Value`
//! slots here and give them a type in their own module.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Most chat messages kept in memory; older ones stay with the service.
pub const MAX_CHAT: usize = 2000;

/// Where one stage is at (doc 30: "Where each stage is at").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum StatusKind {
    #[default]
    NotStarted,
    /// The AI is making this stage's pieces (`done` of `total`).
    Making,
    /// Everything made, nothing needs you: check it and press Next.
    Ready,
    /// `need` pieces need you (a failed Fix, a question).
    NeedsYou,
    /// You pressed Next on this stage.
    Done,
    /// The AI stopped on this stage with an error.
    Stopped,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct StageStatus {
    pub kind: StatusKind,
    pub done: u32,
    pub total: u32,
    pub need: u32,
    /// Optional own words for the status line (e.g. "making voice"), else built from the counts.
    pub text: String,
}

impl StageStatus {
    pub fn of(kind: StatusKind) -> Self {
        Self { kind, ..Self::default() }
    }
    /// The short status line under the stage name (without the icon).
    pub fn line(&self) -> String {
        if !self.text.is_empty() {
            return self.text.clone();
        }
        match self.kind {
            StatusKind::NotStarted => "not started".into(),
            StatusKind::Making if self.total > 0 => format!("making {}/{}", self.done.min(self.total), self.total),
            StatusKind::Making => "making".into(),
            StatusKind::Ready => "ready to check".into(),
            StatusKind::NeedsYou if self.need == 1 => "1 needs you".into(),
            StatusKind::NeedsYou => format!("{} need you", self.need),
            StatusKind::Done => "done".into(),
            StatusKind::Stopped => "stopped".into(),
        }
    }
    /// Next is blue only when the stage is ready (doc 30): everything made and nothing needs you.
    pub fn ready(&self) -> bool {
        matches!(self.kind, StatusKind::Ready | StatusKind::Done)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ChatMessage {
    /// "user", "ai" or "system" (a grey line: a button press, doc 37).
    pub role: String,
    pub text: String,
    /// ISO time from the service, if it sent one.
    pub at: String,
    /// Everything else the service sent with it (`stage`, `context`, `card`, `question`, `refs`,
    /// `actions`, …): the chat part reads these (doc 37, protocol.md `chat.message`).
    #[serde(flatten)]
    pub extra: serde_json::Map<String, Value>,
}

/// Files the person chose in an OS dialog (`mortiflix.chooseFiles`) or dropped on the window during
/// stages 1-4 (`purpose: "drop"`): the part that asked takes them by its purpose.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct FilesIn {
    pub purpose: String,
    pub paths: Vec<String>,
}

/// The pop-up Next shows when something on the stage still needs you (doc 30).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NextConfirm {
    pub stage: u8,
    pub need: u32,
    /// Go to Render (Get the video) rather than the next stage.
    pub to_render: bool,
    /// When the pop-up first showed (egui time): a click outside it right then never closes it.
    #[serde(skip)]
    pub opened_at: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MfxState {
    /// Mortiflix is on: the stage bar shows and stage commands switch Mortiflix Studio's layout.
    pub active: bool,
    /// The Mortiflix project id (the service's `state/<id>`), None before New project / Open.
    /// The service connections the state has seen (a restarted service opens the project again).
    #[serde(skip)]
    pub seen_connects: u64,
    pub project_id: Option<String>,
    pub project_title: String,
    /// The stage on screen, 0-7 (0-9 in the app studio; see [`crate::stages`]).
    pub stage: u8,
    /// The furthest stage reached; looking at an earlier one shows the "looking back" hint.
    pub furthest: u8,
    pub statuses: [StageStatus; 10],
    /// What the AI is doing now ("Making shot 9 of 14 · about 4 min left"), from status events.
    pub ai_line: String,
    pub selected_node: Option<String>,
    pub chat: Vec<ChatMessage>,
    /// The node graph as the service sends it (doc 35 types it).
    pub graph: Value,
    /// Pipeline presets (doc 31 types them).
    pub presets: Value,
    /// Open windows (New project, Setup), by the commands that open them.
    pub show_new_project: bool,
    pub show_setup: bool,
    /// Next asked first: something still needs you.
    pub confirm_next: Option<NextConfirm>,
    /// Chosen or dropped files waiting for the part that asked (see [`FilesIn`]), oldest first.
    pub files_in: Vec<FilesIn>,
    /// The node area (part 35): view, selection, where nodes sit, the typed graph.
    pub nodes: crate::nodes::NodeArea,
    /// The loose preview (doc 36): its plan, scope, loop and the player as the host saw it.
    pub preview: crate::preview::PreviewState,
    /// The Music stage's score: its Strudel and its MIDI under the animatic.
    #[serde(skip)]
    pub score: crate::score::Score,
    /// The Animatic's motion menu: the motion and rule packs, the owner's picks.
    #[serde(skip)]
    pub motion: crate::motion_menu::MotionState,
    /// The Transitions stage: the transition pack picked and the board's Fix box.
    #[serde(skip)]
    pub cuts: crate::transitions::CutsState,
    /// Home's templates: the packs installed, the category picked.
    #[serde(skip)]
    pub home: crate::home::HomeState,
    /// Into Edit (doc 38): the hand-off and the stage-5 Mortiflix panel.
    pub handoff: crate::into_edit::Handoff,
    /// The script, line by line, and the owner's unsaved draft (Script panel + chat card).
    pub script: crate::script::ScriptState,
    /// Stage 2 Look: the looks, the one in use, the music theme (Look panel + chat's looks card).
    pub look: crate::look::LookState,
    /// Put the keyboard in the chat's message box (Ask for changes on a look).
    pub focus_chat: bool,
    /// What the AI is working on now (`work.active`): the pieces every part animates.
    pub work: crate::work::Work,
    /// The AI stopped and needs you (paused with `needs_you`): Resume / Link Claude Code.
    pub stop: Option<crate::resume::Stop>,
    /// Waiting to resume after Link Claude Code.
    #[serde(skip)]
    pub resume: crate::resume::ResumeState,
    /// The Setup page to open on (`mortiflix.setup {step}`), taken by the Setup window.
    pub setup_page: Option<String>,
    /// Another project just opened: the stage it was last looked at, for the stage bar to go to.
    #[serde(skip)]
    pub open_stage: Option<u8>,
}

impl Default for MfxState {
    fn default() -> Self {
        Self {
            active: false,
            seen_connects: 0,
            project_id: None,
            project_title: String::new(),
            stage: 0,
            furthest: 0,
            statuses: Default::default(),
            ai_line: String::new(),
            selected_node: None,
            chat: Vec::new(),
            graph: Value::Null,
            presets: Value::Null,
            show_new_project: false,
            show_setup: false,
            confirm_next: None,
            files_in: Vec::new(),
            nodes: crate::nodes::NodeArea::default(),
            preview: Default::default(),
            score: crate::score::Score::default(),
            motion: crate::motion_menu::MotionState::default(),
            cuts: crate::transitions::CutsState::default(),
            home: crate::home::HomeState::default(),
            handoff: Default::default(),
            script: Default::default(),
            look: Default::default(),
            focus_chat: false,
            work: crate::work::Work::default(),
            stop: None,
            resume: crate::resume::ResumeState::default(),
            setup_page: None,
            open_stage: None,
        }
    }
}

impl MfxState {
    pub fn status(&self, stage: u8) -> StageStatus {
        self.statuses.get(usize::from(stage)).cloned().unwrap_or_default()
    }
    pub fn looking_back(&self) -> bool {
        self.stage < self.furthest
    }
    /// Whether stage `i` can be opened: one already reached, or every stage before it passed its gate
    /// (Next). Later ones stay grey until then (owner, 2026-10-18: "we can't get there without crossing
    /// the gate").
    pub fn reachable(&self, i: u8) -> bool {
        i <= self.stage || i <= self.furthest || (0..i).all(|k| self.status(k).kind == StatusKind::Done)
    }
    /// Move to `to` (0-7); `done` marks the stage being left as done (Next). Returns the old stage.
    pub fn go_to(&mut self, to: u8, done: bool) -> Option<u8> {
        if usize::from(to) >= crate::stages().len() {
            return None;
        }
        let from = self.stage;
        if done && let Some(s) = self.statuses.get_mut(usize::from(from)) {
            *s = StageStatus::of(StatusKind::Done);
        }
        self.stage = to;
        self.furthest = self.furthest.max(to);
        self.confirm_next = None;
        Some(from)
    }
    /// Hand chosen or dropped files to the part that asked (bounded: 16 waiting, 500 paths each).
    pub fn push_files(&mut self, purpose: &str, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        self.files_in.push(FilesIn { purpose: purpose.chars().take(200).collect(), paths: paths.into_iter().take(500).collect() });
        if self.files_in.len() > 16 {
            self.files_in.remove(0);
        }
    }
    /// Take the waiting files whose purpose is `purpose` or starts with `purpose` + `:`.
    pub fn take_files(&mut self, purpose: &str) -> Vec<FilesIn> {
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut self.files_in)
            .into_iter()
            .partition(|f| f.purpose == purpose || f.purpose.strip_prefix(purpose).is_some_and(|r| r.starts_with(':')));
        self.files_in = rest;
        mine
    }
    /// The paths of every waiting file for `purpose` (see [`MfxState::take_files`]), in order.
    pub fn take_paths(&mut self, purpose: &str) -> Vec<String> {
        self.take_files(purpose).into_iter().flat_map(|f| f.paths).collect()
    }
    pub fn push_chat(&mut self, msg: ChatMessage) {
        self.chat.push(msg);
        if self.chat.len() > MAX_CHAT {
            let extra = self.chat.len() - MAX_CHAT;
            self.chat.drain(..extra);
        }
    }

    /// Apply a message from the service that the shared state tracks: `status` events (stage
    /// statuses, the AI's line, the stage) and `chat.message` events. Everything else is left to the
    /// parts (they read [`crate::Service::events`] / [`crate::Service::take_reply`]). Hostile input:
    /// unknown or malformed fields are ignored.
    pub fn apply_service_message(&mut self, msg: &Value) {
        let Some(event) = msg.get("event").and_then(Value::as_str) else { return };
        let data = msg.get("data").unwrap_or(&Value::Null);
        match event {
            "status" => {
                if let Some(line) = data.get("line").and_then(Value::as_str) {
                    self.ai_line = line.chars().take(300).collect();
                }
                if let Some(list) = data.get("stages").and_then(Value::as_array) {
                    for (slot, v) in self.statuses.iter_mut().zip(list) {
                        if let Ok(s) = serde_json::from_value::<StageStatus>(v.clone()) {
                            *slot = s;
                        }
                    }
                }
                if let Some(p) = data.get("project").and_then(Value::as_str) {
                    let p: String = p.chars().take(200).collect();
                    // another project opened (Open from the chat's list): back where it was left
                    // (run 2: a reopened project always showed 1 Script)
                    if self.project_id.as_deref() != Some(p.as_str())
                        && let Some(stage) = data.get("stage").and_then(Value::as_u64).and_then(|s| u8::try_from(s).ok()).filter(|s| *s <= 5)
                    {
                        self.open_stage = Some(stage);
                    }
                    self.project_id = Some(p);
                }
                // stopped by the AI (Resume), only for a status that says the project's state
                if data.get("state").is_some() {
                    self.stop = crate::resume::stop_of(data);
                }
                // the project's title (window title, export names)
                if let Some(t) = data.get("title").and_then(Value::as_str).map(str::trim).filter(|t| !t.is_empty()) {
                    self.project_title = t.chars().take(200).collect();
                }
            }
            "chat.message" => {
                if let Ok(m) = serde_json::from_value::<ChatMessage>(data.clone()) {
                    self.push_chat(m);
                }
            }
            "work.active" => self.work = crate::work::Work::from_json(data),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn next_marks_done_and_back_is_looking_back() {
        let mut s = MfxState::default();
        assert_eq!(s.go_to(1, true), Some(0));
        assert_eq!(s.status(0).kind, StatusKind::Done);
        s.go_to(0, false);
        assert!(s.looking_back());
        assert_eq!(s.go_to(42, false), None);
    }

    /// Run 2: a project opened from the chat's list always showed 1 Script; it goes back to the
    /// stage its status names, once, and only when the project changes.
    #[test]
    fn an_opened_project_goes_back_to_its_stage() {
        let mut s = MfxState::default();
        s.apply_service_message(&json!({"event": "status", "data": {"project": "p1", "stage": 3}}));
        assert_eq!(s.open_stage, Some(3));
        s.open_stage = None;
        s.apply_service_message(&json!({"event": "status", "data": {"project": "p1", "stage": 2}}));
        assert_eq!(s.open_stage, None, "the same project: no jump");
        s.apply_service_message(&json!({"event": "status", "data": {"project": "p2", "stage": 99}}));
        assert_eq!(s.open_stage, None, "a stage out of range is ignored");
        assert_eq!(s.project_id.as_deref(), Some("p2"));
    }

    #[test]
    fn status_and_chat_events_apply_and_hostile_ones_are_ignored() {
        let mut s = MfxState::default();
        s.apply_service_message(&json!({"event": "status", "data": {"line": "Making shot 9 of 14", "stages": [{"kind": "done"}, {"kind": "making", "done": 9, "total": 14}, "junk", {"kind": "needsYou", "need": 2}]}}));
        assert_eq!(s.ai_line, "Making shot 9 of 14");
        assert_eq!(s.status(1).line(), "making 9/14");
        assert_eq!(s.status(2).kind, StatusKind::NotStarted);
        assert_eq!(s.status(3).line(), "2 need you");
        s.apply_service_message(&json!({"event": "status", "data": {"project": "p1", "title": "Why the Sky Is Blue"}}));
        assert_eq!(s.project_title, "Why the Sky Is Blue");
        s.apply_service_message(&json!({"event": "chat.message", "data": {"role": "ai", "text": "Hi"}}));
        s.apply_service_message(&json!({"event": "chat.message", "data": 7}));
        s.apply_service_message(&json!({"event": 3}));
        s.apply_service_message(&json!([1, 2]));
        assert_eq!(s.chat.len(), 1);
        let back: MfxState = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn files_in_go_to_their_purpose_and_stay_bounded() {
        let mut s = MfxState::default();
        s.push_files("drop", vec!["/a".into()]);
        s.push_files("assets.replace:a01", vec!["/b".into()]);
        s.push_files("assets.add", vec![]);
        assert!(s.take_files("assets").is_empty());
        assert_eq!(s.take_files("assets.replace").len(), 1);
        assert_eq!(s.take_files("drop")[0].paths, ["/a"]);
        assert!(s.files_in.is_empty());
        for i in 0..20 {
            s.push_files("x", vec![i.to_string()]);
        }
        assert_eq!(s.files_in.len(), 16);
    }

    #[test]
    fn chat_extras_round_trip() {
        let mut s = MfxState::default();
        s.apply_service_message(&json!({"event": "chat.message", "data": {"role": "ai", "text": "Pick", "question": {"id": "q1"}, "stage": 1}}));
        assert_eq!(s.chat[0].extra.get("question"), Some(&json!({"id": "q1"})));
        let back: MfxState = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn chat_is_bounded() {
        let mut s = MfxState::default();
        for i in 0..MAX_CHAT + 5 {
            s.push_chat(ChatMessage { role: "user".into(), text: i.to_string(), ..ChatMessage::default() });
        }
        assert_eq!(s.chat.len(), MAX_CHAT);
        assert_eq!(s.chat.first().map(|m| m.text.as_str()), Some("5"));
    }
}
