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
}

/// The pop-up Next shows when something on the stage still needs you (doc 30).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NextConfirm {
    pub stage: u8,
    pub need: u32,
    /// Go to Render (Get the video) rather than the next stage.
    pub to_render: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MfxState {
    /// Mortiflix is on: the stage bar shows and stage commands switch FilmCraft's layout.
    pub active: bool,
    /// The Mortiflix project id (the service's `state/<id>`), None before New project / Open.
    pub project_id: Option<String>,
    pub project_title: String,
    /// The stage on screen, 0-5 (see [`crate::STAGES`]).
    pub stage: u8,
    /// The furthest stage reached; looking at an earlier one shows the "looking back" hint.
    pub furthest: u8,
    pub statuses: [StageStatus; 6],
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
    /// The loose preview (doc 36): its plan, scope, loop and the player as the host saw it.
    pub preview: crate::preview::PreviewState,
}

impl Default for MfxState {
    fn default() -> Self {
        Self {
            active: false,
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
            preview: Default::default(),
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
    /// Move to `to` (0-5); `done` marks the stage being left as done (Next). Returns the old stage.
    pub fn go_to(&mut self, to: u8, done: bool) -> Option<u8> {
        if usize::from(to) >= crate::STAGES.len() {
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
                    self.project_id = Some(p.chars().take(200).collect());
                }
            }
            "chat.message" => {
                if let Ok(m) = serde_json::from_value::<ChatMessage>(data.clone()) {
                    self.push_chat(m);
                }
            }
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

    #[test]
    fn status_and_chat_events_apply_and_hostile_ones_are_ignored() {
        let mut s = MfxState::default();
        s.apply_service_message(&json!({"event": "status", "data": {"line": "Making shot 9 of 14", "stages": [{"kind": "done"}, {"kind": "making", "done": 9, "total": 14}, "junk", {"kind": "needsYou", "need": 2}]}}));
        assert_eq!(s.ai_line, "Making shot 9 of 14");
        assert_eq!(s.status(1).line(), "making 9/14");
        assert_eq!(s.status(2).kind, StatusKind::NotStarted);
        assert_eq!(s.status(3).line(), "2 need you");
        s.apply_service_message(&json!({"event": "chat.message", "data": {"role": "ai", "text": "Hi"}}));
        s.apply_service_message(&json!({"event": "chat.message", "data": 7}));
        s.apply_service_message(&json!({"event": 3}));
        s.apply_service_message(&json!([1, 2]));
        assert_eq!(s.chat.len(), 1);
        let back: MfxState = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn chat_is_bounded() {
        let mut s = MfxState::default();
        for i in 0..MAX_CHAT + 5 {
            s.push_chat(ChatMessage { role: "user".into(), text: i.to_string(), at: String::new() });
        }
        assert_eq!(s.chat.len(), MAX_CHAT);
        assert_eq!(s.chat.first().map(|m| m.text.as_str()), Some("5"));
    }
}
