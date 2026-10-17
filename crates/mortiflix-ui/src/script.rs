//! Stage 1 Script: the whole script, beat by beat, edited in place (doc 11 §9.1, doc 30 stage 1).
//!
//! One editor, drawn in two places: the Script panel (PanelKind::MfxStage on stage 1) and the
//! chat's script card ([`editor`] with `compact`). Owner, 2026-10-16: "click on any of the sentences
//! and just live edit them and then save the edits". So:
//! - every sentence (a line of narration) is clickable: it becomes a text field right where it was.
//!   Enter or clicking away keeps the change, Esc drops it;
//! - kept changes are a **draft** (marked, counted) until **Save** (or Ctrl/Cmd+S) sends them all
//!   at once (`script.edit {edits}`); **Discard** drops them. Leaving the stage with a draft asks
//!   first ([`leave_popup`], from [`crate::Mortiflix::navigate`]);
//! - add a line, delete a line (Undo until saved), rename a beat; each line shows the shots and
//!   voice line it feeds (links that select the node) and, after Save, "changed · remaking 1 shot"
//!   until the AI has remade them;
//! - while the AI is still writing, lines appear as they are written and the newest is read-only.
//!
//! Data: `script.get` (beats → lines, what each line feeds), refreshed on `script.changed`,
//! `node.changed` and `status`; `script.edit` saves (protocol: `docs/mortiflix/protocol.md`). The
//! script and the draft live in [`crate::MfxState::script`], so both places show the same draft.

use std::collections::{BTreeMap, BTreeSet};

use egui::{Align, Color32, FontId, Key, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::state::MfxState;
use crate::{MfxCtx, Service};

/// Longest line the service takes.
pub const MAX_WORDS: usize = 2000;
/// Most beats / lines read from the service (hostile input stays bounded).
const MAX_BEATS: usize = 200;
const MAX_LINES: usize = 200;
const SAVE_BAR_H: f32 = 40.0;

fn text(v: &Value, key: &str, max: usize) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(max).collect()).unwrap_or_default()
}

fn count(v: &Value, key: &str) -> u32 {
    v.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0)
}

/// Words as one paragraph: whitespace collapsed, trimmed, bounded.
pub fn clean(words: &str) -> String {
    words.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(MAX_WORDS).collect()
}

/// A piece a line feeds: its voice line or one of its beat's shots.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Feed {
    pub node: String,
    /// `line` or `shot`.
    pub kind: String,
    pub label: String,
    /// It exists already (so a change remakes it).
    pub made: bool,
    /// A remake of it is under way.
    pub remaking: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Line {
    pub id: String,
    pub words: String,
    /// The owner changed it (saved).
    pub changed: bool,
    /// The AI is writing this one right now.
    pub read_only: bool,
    pub remaking_shots: u32,
    pub remaking_lines: u32,
    pub feeds: Vec<Feed>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Beat {
    pub id: String,
    pub no: u32,
    pub name: String,
    /// The name is the beat's first words (the owner never named it).
    pub name_auto: bool,
    pub colour: Option<[u8; 3]>,
    pub lines: Vec<Line>,
}

impl Beat {
    fn colour(&self, fallback: Color32) -> Color32 {
        self.colour.map_or(fallback, |[r, g, b]| Color32::from_rgb(r, g, b))
    }
}

/// The script as `script.get` sent it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Script {
    pub loaded: bool,
    /// The AI is still writing it.
    pub writing: bool,
    pub beats: Vec<Beat>,
}

impl Script {
    /// From a `script.get` result; unknown or malformed fields are skipped.
    pub fn parse(v: &Value) -> Self {
        let list = |v: &Value, k: &str| v.get(k).and_then(Value::as_array).cloned().unwrap_or_default();
        let beats = list(v, "beats")
            .iter()
            .take(MAX_BEATS)
            .filter_map(|b| {
                let id = text(b, "id", 24);
                if id.is_empty() {
                    return None;
                }
                let colour = b.get("colour").and_then(crate::chat::model::hex_colour).map(|c| [c.r(), c.g(), c.b()]);
                let lines = list(b, "lines")
                    .iter()
                    .take(MAX_LINES)
                    .filter_map(|l| {
                        let id = text(l, "id", 24);
                        if id.is_empty() {
                            return None;
                        }
                        let remaking = l.get("remaking").unwrap_or(&Value::Null);
                        let feeds = list(l, "feeds")
                            .iter()
                            .take(64)
                            .filter_map(|f| {
                                let node = text(f, "node", 24);
                                (!node.is_empty()).then(|| Feed {
                                    node,
                                    kind: text(f, "kind", 16),
                                    label: text(f, "label", 40),
                                    made: f.get("made").and_then(Value::as_bool).unwrap_or(false),
                                    remaking: f.get("remaking").and_then(Value::as_bool).unwrap_or(false),
                                })
                            })
                            .collect();
                        Some(Line {
                            id,
                            words: text(l, "words", MAX_WORDS),
                            changed: l.get("changed").and_then(Value::as_bool).unwrap_or(false),
                            read_only: l.get("read_only").and_then(Value::as_bool).unwrap_or(false),
                            remaking_shots: count(remaking, "shots"),
                            remaking_lines: count(remaking, "lines"),
                            feeds,
                        })
                    })
                    .collect();
                Some(Beat {
                    id,
                    no: count(b, "no"),
                    name: text(b, "name", 80),
                    name_auto: b.get("name_auto").and_then(Value::as_bool).unwrap_or(false),
                    colour,
                    lines,
                })
            })
            .collect();
        Self { loaded: true, writing: v.get("writing").and_then(Value::as_bool).unwrap_or(false), beats }
    }

    pub fn line(&self, id: &str) -> Option<&Line> {
        self.beats.iter().flat_map(|b| &b.lines).find(|l| l.id == id)
    }

    pub fn beat(&self, id: &str) -> Option<&Beat> {
        self.beats.iter().find(|b| b.id == id)
    }

    /// The first line the owner can edit (the AI is not writing it).
    pub fn first_line(&self) -> Option<&Line> {
        self.beats.iter().flat_map(|b| &b.lines).find(|l| !l.read_only)
    }

    /// Show a draft as if saved (between Save and the service's fresh script, so nothing flickers back).
    fn apply(&mut self, d: &Draft) {
        for b in &mut self.beats {
            if let Some(n) = d.names.get(&b.id) {
                b.name = n.clone();
                b.name_auto = n.is_empty();
            }
            b.lines.retain(|l| !d.deleted.contains(&l.id));
            for l in &mut b.lines {
                if let Some(w) = d.words.get(&l.id) {
                    l.words = w.clone();
                    l.changed = true;
                }
            }
            for n in d.added.iter().filter(|n| n.beat == b.id && !n.words.is_empty()) {
                let at = n.after.as_ref().and_then(|a| b.lines.iter().position(|l| &l.id == a)).map_or(0, |i| i + 1);
                b.lines.insert(at.min(b.lines.len()), Line { id: n.key.clone(), words: n.words.clone(), changed: true, read_only: true, ..Line::default() });
            }
        }
    }
}

/// A line added in the draft (it gets its real id when saved).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct NewLine {
    /// `new1`, `new2` … (the service's `key`).
    pub key: String,
    pub beat: String,
    /// The line it follows: a line id, another new line's key, or None = first in the beat.
    pub after: Option<String>,
    pub words: String,
}

/// Changes kept but not saved yet.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Draft {
    /// Line id → its new words.
    pub words: BTreeMap<String, String>,
    /// Beat id → its new name ("" = back to its first words).
    pub names: BTreeMap<String, String>,
    pub deleted: BTreeSet<String>,
    pub added: Vec<NewLine>,
    pub next_key: u32,
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        self.words.is_empty() && self.names.is_empty() && self.deleted.is_empty() && !self.added.iter().any(|n| !n.words.is_empty())
    }

    /// How many changes it holds (for "3 unsaved changes").
    pub fn len(&self) -> usize {
        self.words.len() + self.names.len() + self.deleted.len() + self.added.iter().filter(|n| !n.words.is_empty()).count()
    }

    /// A new, empty line after `after` (a line id or key; None = first) in `beat`. Returns its key.
    pub fn add(&mut self, beat: &str, after: Option<String>) -> String {
        self.next_key = self.next_key.saturating_add(1);
        let key = format!("new{}", self.next_key);
        self.added.push(NewLine { key: key.clone(), beat: beat.to_string(), after, words: String::new() });
        key
    }

    /// Delete a line (a saved one is marked, Undo brings it back; a new one just goes).
    pub fn delete(&mut self, id: &str) {
        if let Some(i) = self.added.iter().position(|n| n.key == id) {
            let gone = self.added.remove(i);
            for n in self.added.iter_mut().filter(|n| n.after.as_deref() == Some(id)) {
                n.after.clone_from(&gone.after);
            }
        } else {
            self.words.remove(id);
            self.deleted.insert(id.to_string());
        }
    }

    pub fn undo_delete(&mut self, id: &str) {
        self.deleted.remove(id);
    }

    /// The `script.edit` edits, in an order the service can follow (renames, words, new lines in
    /// the order made, deletes).
    pub fn edits(&self, script: &Script) -> Vec<Value> {
        let beat_of = |line: &str| script.beats.iter().find(|b| b.lines.iter().any(|l| l.id == line)).map(|b| b.id.clone());
        let mut out = Vec::new();
        for (beat, name) in &self.names {
            out.push(json!({"beat": beat, "beat_name": name}));
        }
        for (line, words) in self.words.iter().filter(|(l, _)| !self.deleted.contains(*l)) {
            if let Some(beat) = beat_of(line) {
                out.push(json!({"beat": beat, "line": line, "words": words}));
            }
        }
        for n in self.added.iter().filter(|n| !n.words.is_empty()) {
            out.push(json!({"beat": n.beat, "add_after": n.after, "words": n.words, "key": n.key}));
        }
        for line in &self.deleted {
            if let Some(beat) = beat_of(line) {
                out.push(json!({"beat": beat, "line": line, "delete": true}));
            }
        }
        out
    }

    /// Put a draft that failed to save back under what was changed since (newer wins).
    fn merge_back(&mut self, old: Draft) {
        for (k, v) in old.words {
            self.words.entry(k).or_insert(v);
        }
        for (k, v) in old.names {
            self.names.entry(k).or_insert(v);
        }
        self.deleted.extend(old.deleted);
        let mut added = old.added;
        added.append(&mut self.added);
        self.added = added;
        self.next_key = self.next_key.max(old.next_key);
    }
}

/// What the open text field edits.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "id", rename_all = "camelCase")]
pub enum Target {
    /// A saved line (its id).
    Line(String),
    /// A line added in the draft (its key).
    New(String),
    /// A beat's name (the beat id).
    Name(String),
}

/// The one open text field (only one at a time, in one place).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Editing {
    /// Where it is open: `panel` or `chat.<message>`.
    pub scope: String,
    pub target: Target,
    pub text: String,
    /// Give it the keyboard on the next frame.
    pub focus: bool,
}

/// The script, the draft and the open field: shared by the Script panel and the chat's card.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ScriptState {
    pub script: Script,
    pub draft: Draft,
    pub editing: Option<Editing>,
    /// Open the first line for editing as soon as the Script panel shows (Change the words).
    pub focus_first: bool,
    /// A stage command waiting on "Save your script changes?" (`stages.<what>`).
    pub leave: Option<String>,
    /// A save is on its way.
    pub saving: bool,
    /// The last save's outcome or a read error, in plain words.
    pub note: String,
    #[serde(skip)]
    loaded_for: Option<String>,
    #[serde(skip)]
    fetch_soon: bool,
    #[serde(skip)]
    fetching: Option<u64>,
    #[serde(skip)]
    saving_req: Option<(u64, Draft)>,
}

impl ScriptState {
    /// Changes not saved: the draft, or an open field whose words differ from what it edits.
    pub fn unsaved(&self) -> bool {
        !self.draft.is_empty() || self.editing.as_ref().is_some_and(|e| self.field_changes(e))
    }

    /// How many unsaved changes (the open field counts as one when it changes something new).
    pub fn unsaved_count(&self) -> usize {
        let open = self.editing.as_ref().is_some_and(|e| {
            self.field_changes(e)
                && match &e.target {
                    Target::Line(id) => !self.draft.words.contains_key(id),
                    Target::New(key) => self.draft.added.iter().any(|n| &n.key == key && n.words.is_empty()),
                    Target::Name(b) => !self.draft.names.contains_key(b),
                }
        });
        self.draft.len() + usize::from(open)
    }

    fn saved_words(&self, t: &Target) -> String {
        match t {
            Target::Line(id) => self.draft.words.get(id).cloned().or_else(|| self.script.line(id).map(|l| l.words.clone())).unwrap_or_default(),
            Target::New(key) => self.draft.added.iter().find(|n| &n.key == key).map(|n| n.words.clone()).unwrap_or_default(),
            Target::Name(b) => self.draft.names.get(b).cloned().or_else(|| self.script.beat(b).map(|x| x.name.clone())).unwrap_or_default(),
        }
    }

    fn field_changes(&self, e: &Editing) -> bool {
        let now = clean(&e.text);
        now != self.saved_words(&e.target) && !(now.is_empty() && !matches!(e.target, Target::Name(_)))
    }

    /// The words a line shows: the draft's, else the saved ones.
    pub fn shown_words(&self, line: &Line) -> String {
        self.draft.words.get(&line.id).cloned().unwrap_or_else(|| line.words.clone())
    }

    /// Open a field on `target` in `scope` (a field open elsewhere is kept first).
    pub fn open(&mut self, scope: &str, target: Target) {
        self.commit();
        let text = self.saved_words(&target);
        self.editing = Some(Editing { scope: scope.to_string(), target, text, focus: true });
    }

    /// Keep what the open field says in the draft (Enter, clicking away) and close it.
    pub fn commit(&mut self) {
        let Some(e) = self.editing.take() else { return };
        let words = clean(&e.text);
        match e.target {
            Target::Line(id) => {
                let saved = self.script.line(&id).map(|l| l.words.clone()).unwrap_or_default();
                if words.is_empty() || words == saved {
                    if words == saved {
                        self.draft.words.remove(&id);
                    }
                } else {
                    self.draft.words.insert(id, words);
                }
            }
            Target::New(key) => {
                if words.is_empty() {
                    self.draft.delete(&key);
                } else if let Some(n) = self.draft.added.iter_mut().find(|n| n.key == key) {
                    n.words = words;
                }
            }
            Target::Name(beat) => {
                let saved = self.script.beat(&beat).map(|b| (b.name.clone(), b.name_auto)).unwrap_or_default();
                let words: String = words.chars().take(80).collect();
                if words == saved.0 || (words.is_empty() && saved.1) {
                    self.draft.names.remove(&beat);
                } else {
                    self.draft.names.insert(beat, words);
                }
            }
        }
    }

    /// Close the open field without keeping it (Esc). A new line left empty goes.
    pub fn cancel(&mut self) {
        if let Some(Editing { target: Target::New(key), .. }) = self.editing.take()
            && self.draft.added.iter().any(|n| n.key == key && n.words.is_empty())
        {
            self.draft.delete(&key);
        }
    }

    /// Drop every unsaved change.
    pub fn discard(&mut self) {
        self.editing = None;
        self.draft = Draft { next_key: self.draft.next_key, ..Draft::default() };
        self.note = "Changes dropped.".into();
    }

    /// Ask for the script again soon (an event said it changed).
    pub fn refresh(&mut self) {
        self.fetch_soon = true;
    }

    /// Events that change what the script shows: read it again.
    pub fn on_message(&mut self, msg: &Value) {
        if matches!(msg.get("event").and_then(Value::as_str), Some("script.changed" | "node.changed")) {
            self.fetch_soon = true;
        }
    }
}

/// Save every unsaved change at once (Save, Ctrl/Cmd+S, "Save and go"). Nothing to save: nothing sent.
pub fn save(st: &mut ScriptState, service: &mut Service) {
    st.commit();
    if st.draft.is_empty() || st.saving_req.is_some() {
        return;
    }
    let edits = st.draft.edits(&st.script);
    let draft = std::mem::take(&mut st.draft);
    st.draft.next_key = draft.next_key;
    if edits.is_empty() {
        return;
    }
    let id = service.request("script.edit", json!({"edits": edits}));
    st.script.apply(&draft);
    st.saving_req = Some((id, draft));
    st.saving = true;
    st.note = "Saving…".into();
}

/// Every frame (from [`crate::Mortiflix::poll`]): follow the open project, read the script when it
/// changed, take the replies. Never blocks.
pub fn tick(state: &mut MfxState, service: &mut Service) {
    let st = &mut state.script;
    if st.loaded_for != state.project_id {
        *st = ScriptState { loaded_for: state.project_id.clone(), fetch_soon: state.project_id.is_some(), ..ScriptState::default() };
    }
    if st.fetch_soon && st.fetching.is_none() && st.loaded_for.is_some() && service.status().is_connected() {
        st.fetch_soon = false;
        st.fetching = Some(service.request("script.get", json!({})));
    }
    if let Some(id) = st.fetching
        && let Some(r) = service.take_reply(id)
    {
        st.fetching = None;
        if r.ok {
            let mut script = Script::parse(&r.result);
            // a save on its way: show it as saved until the next read has it
            if let Some((_, d)) = &st.saving_req {
                script.apply(d);
            }
            // a line deleted by someone else while its field was open closes the field
            if let Some(Editing { target: Target::Line(l), .. }) = &st.editing
                && script.line(l).is_none()
            {
                st.editing = None;
            }
            st.script = script;
        } else if r.error.get("code").and_then(Value::as_str) != Some("no_project") {
            st.note = format!("Couldn't read the script: {}", text(&r.error, "message", 200));
        }
    }
    if let Some((id, _)) = &st.saving_req
        && let Some(r) = service.take_reply(*id)
    {
        let draft = st.saving_req.take().map(|(_, d)| d).unwrap_or_default();
        st.saving = false;
        if r.ok {
            let shots = r
                .result
                .get("remaking")
                .and_then(Value::as_array)
                .map_or(0, |a| a.iter().filter(|x| x.get("kind").and_then(Value::as_str) == Some("shot")).count());
            let lines = r
                .result
                .get("remaking")
                .and_then(Value::as_array)
                .map_or(0, |a| a.iter().filter(|x| x.get("kind").and_then(Value::as_str) == Some("line")).count());
            st.note = match remaking_words(shots, lines) {
                Some(w) => format!("Saved · remaking {w}"),
                None => "Saved.".into(),
            };
        } else {
            st.draft.merge_back(draft);
            st.note = format!("Couldn't save: {}", text(&r.error, "message", 200));
        }
        st.fetch_soon = true;
    }
}

/// "2 shots and 1 voice line", or None when nothing is being remade.
pub fn remaking_words(shots: usize, lines: usize) -> Option<String> {
    let n = |k: usize, one: &str, many: &str| if k == 1 { format!("1 {one}") } else { format!("{k} {many}") };
    match (shots, lines) {
        (0, 0) => None,
        (s, 0) => Some(n(s, "shot", "shots")),
        (0, l) => Some(n(l, "voice line", "voice lines")),
        (s, l) => Some(format!("{} and {}", n(s, "shot", "shots"), n(l, "voice line", "voice lines"))),
    }
}

/// Stage 1's main panel (PanelKind::MfxStage on stage 1).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    let full = ui.max_rect();
    ui.allocate_ui_with_layout(vec2(full.width(), 28.0), Layout::left_to_right(Align::Center), |ui| {
        ui.label(RichText::new("Script").size(15.0).color(t.text).strong());
        let s = &ctx.state.script.script;
        let lines: usize = s.beats.iter().map(|b| b.lines.len()).sum();
        let words: usize = s.beats.iter().flat_map(|b| &b.lines).map(|l| l.words.split_whitespace().count()).sum();
        if s.loaded && !s.beats.is_empty() {
            ui.label(RichText::new(format!("{} beats · {lines} lines · {words} words", s.beats.len())).size(12.0).color(t.text_faint));
        }
        if s.writing {
            ui.label(RichText::new("· the AI is still writing").size(12.0).color(t.ok));
        }
    });
    if crate::service_notice(ui, ctx) {
        return;
    }
    if ctx.state.script.focus_first {
        if let Some(id) = ctx.state.script.script.first_line().map(|l| l.id.clone()) {
            ctx.state.script.focus_first = false;
            ctx.state.script.open("panel", Target::Line(id));
        } else if ctx.state.script.script.loaded {
            ctx.state.script.focus_first = false;
        }
    }
    let top = ui.min_rect().max.y + 6.0;
    let bar = ctx.state.script.unsaved() || ctx.state.script.saving || !ctx.state.script.note.is_empty();
    let bottom = if bar { full.max.y - SAVE_BAR_H } else { full.max.y };
    let body = Rect::from_min_max(pos2(full.min.x, top), pos2(full.max.x, bottom.max(top)));
    let mut child = ui.new_child(UiBuilder::new().max_rect(body).layout(Layout::top_down(Align::Min)));
    egui::ScrollArea::vertical().id_salt("mfx-script-scroll").auto_shrink([false, false]).show(&mut child, |ui| {
        ui.set_width(body.width() - 12.0);
        editor(ui, ctx, "panel", false, true);
    });
    if bar {
        let r = Rect::from_min_max(pos2(full.min.x, bottom), full.max);
        ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, t.separator));
        let mut b = ui.new_child(UiBuilder::new().max_rect(r.shrink2(vec2(0.0, 6.0))).layout(Layout::left_to_right(Align::Center)));
        save_bar(&mut b, ctx, "panel");
    }
    ui.allocate_rect(full, Sense::hover());
}

/// The draft's line: "3 unsaved changes · Discard · Save" (or the last save's outcome).
pub fn save_bar(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str) {
    let t = ctx.theme;
    let st = &ctx.state.script;
    if st.unsaved() {
        let n = st.unsaved_count().max(1);
        let what = if n == 1 { "1 unsaved change".to_string() } else { format!("{n} unsaved changes") };
        dot(ui, t.warn);
        ui.label(RichText::new(what).size(12.0).color(t.warn));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let save_b =
                ui.add(egui::Button::new(RichText::new("Save").size(12.0).color(Color32::WHITE)).fill(t.accent).min_size(vec2(64.0, 24.0)).corner_radius(4.0));
            let save_b = save_b.on_hover_text("Save every change (Ctrl+S): only the shots and voice lines that used the words are made again");
            ctx.auto(&format!("mfx.script.{scope}.save"), &save_b, "Save");
            let discard =
                ui.add(egui::Button::new(RichText::new("Discard").size(12.0).color(t.text)).fill(t.hover).min_size(vec2(0.0, 24.0)).corner_radius(4.0));
            ctx.auto(&format!("mfx.script.{scope}.discard"), &discard, "Discard");
            if save_b.clicked() {
                let MfxCtx { state, service, .. } = ctx;
                save(&mut state.script, service);
            }
            if discard.clicked() {
                ctx.state.script.discard();
            }
        });
    } else if st.saving {
        ui.label(RichText::new("Saving…").size(12.0).color(t.text_dim));
    } else if !st.note.is_empty() {
        let col = if st.note.starts_with("Couldn't") { t.error } else { t.text_dim };
        ui.label(RichText::new(st.note.clone()).size(12.0).color(col));
    }
}

fn dot(ui: &mut egui::Ui, col: Color32) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
    ui.painter().circle_filled(pos2(r.center().x, r.center().y + 1.0), 3.0, col);
}

/// The script editor in `scope` (`panel`, or `chat.<message>` for a chat card). `compact`: the chat
/// card (no feeds row; folded = each beat's first line). Returns false when there is no script yet
/// (the chat card then shows its own copy).
pub fn editor(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str, compact: bool, open: bool) -> bool {
    let t = ctx.theme;
    let script = ctx.state.script.script.clone();
    if script.beats.is_empty() {
        if !compact {
            let words = if script.writing || !script.loaded { "The script appears here as it's written." } else { "No script yet." };
            ui.add_space(24.0);
            ui.vertical_centered(|ui| ui.label(RichText::new(words).size(13.0).color(t.text_faint)));
        }
        return false;
    }
    keys(ui, ctx, scope);
    for beat in &script.beats {
        beat_block(ui, ctx, scope, beat, compact, open);
        ui.add_space(if compact { 4.0 } else { 10.0 });
    }
    true
}

/// Enter keeps the open field, Esc drops it (taken before the text box sees them; Shift+Enter is
/// left to the box).
fn keys(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str) {
    let Some(e) = &ctx.state.script.editing else { return };
    if e.scope != scope || !ui.memory(|m| m.has_focus(field_id(scope))) {
        return;
    }
    let (enter, esc) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::Enter), i.consume_key(egui::Modifiers::NONE, Key::Escape)));
    if esc {
        ctx.state.script.cancel();
    } else if enter {
        ctx.state.script.commit();
    }
}

fn field_id(scope: &str) -> egui::Id {
    egui::Id::new(("mfx-script-field", scope))
}

/// The open text field, if it is `target` in `scope`. True when it drew.
fn field(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str, target: &Target, size: f32) -> bool {
    let t = ctx.theme;
    let Some(e) = ctx.state.script.editing.as_mut() else { return false };
    if e.scope != scope || &e.target != target {
        return false;
    }
    let single = matches!(target, Target::Name(_));
    let mut edit = if single { egui::TextEdit::singleline(&mut e.text) } else { egui::TextEdit::multiline(&mut e.text).desired_rows(1) };
    edit = edit.id(field_id(scope)).font(FontId::proportional(size)).desired_width(ui.available_width()).background_color(t.field_bg).margin(vec2(4.0, 2.0));
    if !single {
        edit = edit.hint_text("Type the new words · Enter keeps them, Esc drops them");
    }
    let r = ui.add(edit);
    if e.focus {
        r.request_focus();
        e.focus = false;
    } else if r.lost_focus() {
        // clicked away: keep it
        ctx.state.script.commit();
    }
    ctx.auto(&format!("mfx.script.{scope}.field"), &r, "Script words");
    true
}

fn beat_block(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str, beat: &Beat, compact: bool, open: bool) {
    let t = ctx.theme;
    let colour = beat.colour(t.text_faint);
    let top = ui.cursor().min;
    let inner = ui
        .horizontal_top(|ui| {
            ui.add_space(if compact { 8.0 } else { 12.0 });
            ui.vertical(|ui| {
                // header: number + name (click the name to rename the beat)
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.label(RichText::new(beat.no.to_string()).size(if compact { 12.0 } else { 13.0 }).color(t.text_dim).monospace());
                    let target = Target::Name(beat.id.clone());
                    if !field(ui, ctx, scope, &target, if compact { 12.0 } else { 14.0 }) {
                        let name = ctx.state.script.draft.names.get(&beat.id).cloned().unwrap_or_else(|| beat.name.clone());
                        let renamed = ctx.state.script.draft.names.contains_key(&beat.id);
                        let shown = if name.is_empty() { "Name this beat".to_string() } else { name };
                        let r = ui
                            .add(egui::Label::new(RichText::new(shown).size(if compact { 12.0 } else { 14.0 }).color(t.text).strong()).sense(Sense::click()))
                            .on_hover_cursor(egui::CursorIcon::Text)
                            .on_hover_text("Click to rename this beat");
                        if r.clicked() {
                            ctx.state.script.open(scope, target);
                        }
                        ctx.auto(&format!("mfx.script.{scope}.name.{}", beat.id), &r, "Beat name");
                        if renamed {
                            dot(ui, t.warn);
                        }
                    }
                });
                let lines: Vec<&Line> = if compact && !open { beat.lines.iter().take(1).collect() } else { beat.lines.iter().collect() };
                let hidden = beat.lines.len().saturating_sub(lines.len());
                let mut prev: Option<String> = None;
                // new lines at the very top of the beat
                new_lines(ui, ctx, scope, beat, None, compact);
                for line in lines {
                    line_row(ui, ctx, scope, line, compact);
                    new_lines(ui, ctx, scope, beat, Some(line.id.clone()), compact);
                    prev = Some(line.id.clone());
                }
                if hidden > 0 {
                    ui.label(
                        RichText::new(if hidden == 1 { "+1 more line".to_string() } else { format!("+{hidden} more lines") }).size(11.0).color(t.text_faint),
                    );
                } else if !compact || open {
                    let last = ctx.state.script.draft.added.iter().rev().find(|n| n.beat == beat.id && n.after == prev).map(|n| n.key.clone()).or(prev);
                    let add = ui
                        .add(egui::Label::new(RichText::new("+ Add a line").size(12.0).color(t.text_faint)).sense(Sense::click()))
                        .on_hover_cursor(egui::CursorIcon::PointingHand);
                    if add.clicked() {
                        let key = ctx.state.script.draft.add(&beat.id, last);
                        ctx.state.script.open(scope, Target::New(key));
                    }
                    ctx.auto(&format!("mfx.script.{scope}.add.{}", beat.id), &add, "Add a line");
                }
            });
        })
        .response;
    // the beat's colour bar down its left edge (the number beside it: never colour alone)
    let bar = Rect::from_min_max(pos2(top.x, top.y + 2.0), pos2(top.x + 3.0, inner.rect.max.y - 2.0));
    ui.painter().rect_filled(bar, 1.0, colour);
}

/// Lines added in the draft right after `after` (None = at the top of the beat), in order.
fn new_lines(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str, beat: &Beat, after: Option<String>, compact: bool) {
    let mut at = after;
    loop {
        let next = ctx.state.script.draft.added.iter().find(|n| n.beat == beat.id && n.after == at).cloned();
        let Some(n) = next else { break };
        let line = Line { id: n.key.clone(), words: n.words.clone(), ..Line::default() };
        line_row(ui, ctx, scope, &line, compact);
        at = Some(n.key);
    }
}

fn line_row(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str, line: &Line, compact: bool) {
    let t = ctx.theme;
    let is_new = ctx.state.script.draft.added.iter().any(|n| n.key == line.id);
    let target = if is_new { Target::New(line.id.clone()) } else { Target::Line(line.id.clone()) };
    let deleted = ctx.state.script.draft.deleted.contains(&line.id);
    let edited = is_new || ctx.state.script.draft.words.contains_key(&line.id);
    let size = if compact { 12.0 } else { 13.0 };
    ui.horizontal_top(|ui| {
        // the unsaved mark sits in the gutter, so nothing moves when it shows
        let (g, _) = ui.allocate_exact_size(vec2(10.0, size + 4.0), Sense::hover());
        if edited || deleted {
            ui.painter().circle_filled(pos2(g.center().x, g.center().y + 1.0), 3.0, t.warn);
        }
        ui.vertical(|ui| {
            if field(ui, ctx, scope, &target, size) {
                return;
            }
            let words = ctx.state.script.shown_words(line);
            let mut rich = RichText::new(if words.is_empty() { "(empty)".to_string() } else { words }).size(size);
            rich = if deleted {
                rich.color(t.text_faint).strikethrough()
            } else if line.read_only {
                rich.color(t.text_dim).italics()
            } else {
                rich.color(t.text)
            };
            let mut r = ui.add(egui::Label::new(rich).wrap().sense(if line.read_only || deleted { Sense::hover() } else { Sense::click() }));
            if !line.read_only && !deleted {
                r = r.on_hover_cursor(egui::CursorIcon::Text);
                if r.hovered() {
                    ui.painter().rect_stroke(r.rect.expand(2.0), 3.0, Stroke::new(1.0, t.separator.gamma_multiply(2.0)), egui::StrokeKind::Outside);
                }
                if r.clicked() {
                    ctx.state.script.open(scope, target.clone());
                }
            }
            ctx.auto(&format!("mfx.script.{scope}.line.{}", line.id), &r, "Script line");
            meta(ui, ctx, scope, line, compact, edited, deleted, is_new);
        });
    });
}

/// Under a line: unsaved / deleted (Undo) / being written / "changed · remaking 1 shot", then (in
/// the panel) the pieces it feeds and Delete.
#[allow(clippy::too_many_arguments, clippy::fn_params_excessive_bools)]
fn meta(ui: &mut egui::Ui, ctx: &mut MfxCtx, scope: &str, line: &Line, compact: bool, edited: bool, deleted: bool, is_new: bool) {
    let t = ctx.theme;
    let remaking = remaking_words(line.remaking_shots as usize, line.remaking_lines as usize);
    let status = if deleted {
        Some(("deleted, not saved".to_string(), t.warn))
    } else if is_new {
        Some(("new, not saved".to_string(), t.warn))
    } else if edited {
        Some(("changed, not saved".to_string(), t.warn))
    } else if line.read_only {
        Some(("being written…".to_string(), t.text_faint))
    } else {
        remaking.filter(|_| line.changed).map(|w| (format!("changed · remaking {w}"), t.accent))
    };
    let feeds = !compact && !line.feeds.is_empty() && !is_new;
    let delete = !compact && !line.read_only && !deleted;
    if status.is_none() && !feeds && !delete && !deleted {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if let Some((s, col)) = status {
            ui.label(RichText::new(s).size(11.0).color(col));
        }
        if deleted {
            let undo = ui.add(egui::Label::new(RichText::new("Undo").size(11.0).color(crate::chat::LINK).underline()).sense(Sense::click()));
            if undo.clicked() {
                ctx.state.script.draft.undo_delete(&line.id);
            }
            ctx.auto(&format!("mfx.script.{scope}.undo.{}", line.id), &undo, "Undo delete");
        }
        if feeds {
            ui.label(RichText::new("feeds").size(11.0).color(t.text_faint));
            for f in &line.feeds {
                let label = if f.kind == "line" { format!("voice {}", f.label) } else { f.label.clone() };
                let col = if f.remaking { t.accent } else { crate::chat::LINK };
                let link = ui.add(egui::Label::new(RichText::new(label).size(11.0).color(col).underline()).sense(Sense::click())).on_hover_text(if f.made {
                    "Select it"
                } else {
                    "Select it (not made yet)"
                });
                if link.clicked() {
                    ctx.state.selected_node = Some(f.node.clone());
                }
                ctx.auto(&format!("mfx.script.{scope}.feed.{}.{}", line.id, f.node), &link, &f.label);
            }
        }
        if delete {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let del = ui
                    .add(egui::Label::new(RichText::new("Delete").size(11.0).color(t.text_faint)).sense(Sense::click()))
                    .on_hover_text("Delete this line (Undo until you save)");
                if del.clicked() {
                    if ctx.state.script.editing.as_ref().is_some_and(|e| e.target == Target::Line(line.id.clone()) || e.target == Target::New(line.id.clone()))
                    {
                        ctx.state.script.editing = None;
                    }
                    ctx.state.script.draft.delete(&line.id);
                }
                ctx.auto(&format!("mfx.script.{scope}.delete.{}", line.id), &del, "Delete line");
            });
        }
    });
}

/// "Save your script changes?" when a stage command would leave with a draft (doc 30's pop-ups).
pub fn leave_popup(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let Some(what) = ctx.state.script.leave.clone() else { return };
    let n = ctx.state.script.unsaved_count().max(1);
    let modal = egui::Modal::new(egui::Id::new("mfx-script-leave")).show(ui.ctx(), |ui| {
        ui.set_width(400.0);
        ui.heading("Save your script changes?");
        ui.label(if n == 1 { "1 change to the script isn't saved yet.".to_string() } else { format!("{n} changes to the script aren't saved yet.") });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let stay = ui.button("Keep editing");
            ctx.auto("mfx.script.leave.stay", &stay, "Keep editing");
            if stay.clicked() {
                ctx.state.script.leave = None;
            }
            let discard = ui.button("Discard");
            ctx.auto("mfx.script.leave.discard", &discard, "Discard");
            if discard.clicked() {
                ctx.state.script.leave = None;
                ctx.state.script.discard();
                ctx.command(&format!("stages.{what}"), json!({}));
            }
            let save_b = ui.add(egui::Button::new(RichText::new("Save and go on").color(Color32::WHITE)).fill(ctx.theme.accent));
            ctx.auto("mfx.script.leave.save", &save_b, "Save and go on");
            if save_b.clicked() {
                ctx.state.script.leave = None;
                let MfxCtx { state, service, .. } = ctx;
                save(&mut state.script, service);
                ctx.command(&format!("stages.{what}"), json!({}));
            }
        });
    });
    if modal.should_close() {
        ctx.state.script.leave = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Script {
        Script::parse(&json!({"writing": false, "beats": [
            {"id": "b01", "no": 1, "name": "Open on a", "name_auto": true, "colour": {"hex": "#380ea7"}, "lines": [
                {"id": "b01-1", "words": "Open on a dot.", "feeds": [{"node": "b01-1", "kind": "line", "label": "b01-1", "made": true}, {"node": "s01", "kind": "shot", "label": "S01", "made": true}]},
                {"id": "b01-2", "words": "It grows.", "remaking": {"shots": 1, "lines": 0}, "changed": true}]},
            {"id": "b02", "no": 2, "name": "Ideas", "lines": [{"id": "b02-1", "words": "Three ideas.", "read_only": true}]},
            {"no": 3}, "junk"
        ]}))
    }

    #[test]
    fn parses_and_skips_junk() {
        let s = sample();
        assert!(s.loaded);
        assert_eq!(s.beats.len(), 2);
        assert_eq!(s.beats[0].colour, Some([0x38, 0x0e, 0xa7]));
        assert_eq!(s.beats[0].lines[0].feeds.len(), 2);
        assert_eq!(s.beats[0].lines[1].remaking_shots, 1);
        assert_eq!(s.first_line().map(|l| l.id.as_str()), Some("b01-1"));
        assert!(s.beats[1].lines[0].read_only);
        assert_eq!(Script::parse(&json!(7)).beats.len(), 0);
    }

    #[test]
    fn a_field_keeps_drops_and_counts_changes() {
        let mut st = ScriptState { script: sample(), ..ScriptState::default() };
        st.open("panel", Target::Line("b01-1".into()));
        assert!(!st.unsaved(), "opening is not a change");
        if let Some(e) = st.editing.as_mut() {
            e.text = "  A dot   opens its eyes. ".into();
        }
        assert!(st.unsaved());
        assert_eq!(st.unsaved_count(), 1);
        st.commit();
        assert_eq!(st.draft.words.get("b01-1").map(String::as_str), Some("A dot opens its eyes."));
        // Esc drops a second change, the first stays
        st.open("chat.3", Target::Line("b01-1".into()));
        if let Some(e) = st.editing.as_mut() {
            e.text = "Something else".into();
        }
        st.cancel();
        assert_eq!(st.draft.words.get("b01-1").map(String::as_str), Some("A dot opens its eyes."));
        // typing the saved words back is no change
        st.open("panel", Target::Line("b01-1".into()));
        if let Some(e) = st.editing.as_mut() {
            e.text = "Open on a dot.".into();
        }
        st.commit();
        assert!(st.draft.is_empty());
        // an empty field keeps the old words
        st.open("panel", Target::Line("b01-2".into()));
        if let Some(e) = st.editing.as_mut() {
            e.text = "   ".into();
        }
        st.commit();
        assert!(st.draft.is_empty());
    }

    #[test]
    fn adds_deletes_renames_become_one_batch() {
        let mut st = ScriptState { script: sample(), ..ScriptState::default() };
        let k1 = st.draft.add("b01", Some("b01-1".into()));
        st.open("panel", Target::New(k1.clone()));
        if let Some(e) = st.editing.as_mut() {
            e.text = "A new line.".into();
        }
        st.commit();
        let k2 = st.draft.add("b01", Some(k1.clone()));
        st.open("panel", Target::New(k2.clone()));
        st.cancel();
        assert_eq!(st.draft.added.len(), 1, "an empty new line goes on Esc");
        st.draft.delete("b01-2");
        st.draft.words.insert("b01-1".into(), "Changed.".into());
        st.open("panel", Target::Name("b02".into()));
        if let Some(e) = st.editing.as_mut() {
            e.text = "The ideas".into();
        }
        st.commit();
        assert_eq!(st.draft.len(), 4);
        let edits = st.draft.edits(&st.script);
        assert_eq!(edits[0], json!({"beat": "b02", "beat_name": "The ideas"}));
        assert_eq!(edits[1], json!({"beat": "b01", "line": "b01-1", "words": "Changed."}));
        assert_eq!(edits[2], json!({"beat": "b01", "add_after": "b01-1", "words": "A new line.", "key": k1}));
        assert_eq!(edits[3], json!({"beat": "b01", "line": "b01-2", "delete": true}));
        st.draft.undo_delete("b01-2");
        assert_eq!(st.draft.len(), 3);
        // deleting a new line re-points the ones after it
        let k3 = st.draft.add("b01", Some(k1.clone()));
        if let Some(n) = st.draft.added.iter_mut().find(|n| n.key == k3) {
            n.words = "Third.".into();
        }
        st.draft.delete(&k1);
        assert_eq!(st.draft.added.iter().find(|n| n.key == k3).and_then(|n| n.after.clone()), Some("b01-1".into()));
        st.discard();
        assert!(!st.unsaved());
    }

    #[test]
    fn save_sends_one_request_and_shows_it_saved() {
        let mut st = ScriptState { script: sample(), ..ScriptState::default() };
        let mut service = Service::default();
        save(&mut st, &mut service);
        assert!(st.saving_req.is_none(), "nothing to save: nothing sent");
        st.draft.words.insert("b01-1".into(), "New words.".into());
        st.draft.delete("b01-2");
        save(&mut st, &mut service);
        assert!(st.draft.is_empty());
        assert_eq!(st.script.line("b01-1").map(|l| l.words.as_str()), Some("New words."));
        assert!(st.script.line("b01-2").is_none());
        // not running: the reply is an error, and the draft comes back
        let mut state = MfxState { script: st, ..MfxState::default() };
        tick(&mut state, &mut service);
        assert!(state.script.note.starts_with("Couldn't save"), "{}", state.script.note);
        assert_eq!(state.script.draft.words.get("b01-1").map(String::as_str), Some("New words."));
        assert!(state.script.draft.deleted.contains("b01-2"));
    }

    #[test]
    fn remaking_words_read_naturally() {
        assert_eq!(remaking_words(0, 0), None);
        assert_eq!(remaking_words(1, 1).as_deref(), Some("1 shot and 1 voice line"));
        assert_eq!(remaking_words(2, 0).as_deref(), Some("2 shots"));
        assert_eq!(remaking_words(0, 3).as_deref(), Some("3 voice lines"));
    }

    #[test]
    fn events_ask_for_the_script_again() {
        let mut st = ScriptState::default();
        st.on_message(&json!({"event": "chat.message", "data": {}}));
        assert!(!st.fetch_soon);
        st.on_message(&json!({"event": "script.changed", "data": {}}));
        assert!(st.fetch_soon);
    }
}
