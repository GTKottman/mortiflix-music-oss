//! Talking with the AI (doc 37): one **Mortiflix** panel and one history across all stages, docked
//! left on stages 1-4 and right in FilmCraft on stage 5, and the **Assets** tab beside it (doc 32b,
//! [`assets`]).
//!
//! The panel, top to bottom: the "Talking about" bar (crumbs synced both ways with
//! `MfxState::selected_node`, History ▾, "● 1 question waiting"), the thread (stage dividers, your
//! messages, Mortiflix's messages with inline cards, one question at a time with its default ticked,
//! quiet action lines, folded node threads), the live line with Details, the Moves row (a button for
//! every main move in this context) and the composer.
//!
//! Chat and buttons are one history (doc 37 "How chat and buttons never conflict"): a button press is
//! the node's own move (`node.edit`) and writes a grey line into the chat; typed words in a node's
//! context become that node's Fix (New take, Change it) and say so; typed words about the whole video
//! go to the AI (`chat.send`). Messages the app writes itself are logged with the service
//! (`chat.note`) so the history survives a restart. Protocol: `docs/mortiflix/protocol.md`.

mod assets;
mod cards;
pub mod model;

use std::collections::BTreeMap;

use egui::{Align, Align2, Color32, FontId, Key, Layout, Rect, RichText, Sense, Stroke, UiBuilder, pos2, vec2};
use serde_json::{Value, json};

pub use assets::{AssetDrag, show_assets};
use model::{Act, ActionButton, Move, Msg, NodeInfo, Role, Row};

use crate::state::{ChatMessage, StatusKind};
use crate::{MfxCtx, STAGE_FILMCRAFT, STAGE_RENDER, STAGE_VOICE_MUSIC, stage_key, stage_name};

/// Links are blue and underlined (the owner's standard, doc 37).
pub const LINK: Color32 = Color32::from_rgb(0x57, 0x94, 0xec);
/// Longest message the composer sends.
const MAX_TEXT: usize = 20_000;
/// Replies not back after this long are given up on (the service keeps the newest 256).
const PENDING_TIMEOUT: f64 = 300.0;

const BAR_H: f32 = 34.0;
const NOW_H: f32 = 28.0;
const DETAILS_H: f32 = 132.0;
const MOVES_H: f32 = 34.0;
const COMPOSER_H: f32 = 52.0;
const HINT_H: f32 = 18.0;
const CHIPS_H: f32 = 26.0;
const ASK_H: f32 = 58.0;
const PILL_H: f32 = 26.0;

/// What a request the panel sent was for (its reply decides what the chat shows).
#[derive(Clone, Debug)]
enum Pending {
    /// `chat.send`: words about the whole video (or a beat).
    Send { text: String, context: Option<String>, files: Vec<String>, assets: Vec<String> },
    /// `node.edit` fix from typed words or a Fix button.
    Fix { node: String, name: String, mv: Move, text: String },
    /// OK / Cut it / taking one back.
    Decide,
    /// `chat.history`: the project's whole history.
    History,
    /// Anything else (answers, notes, card buttons): only errors show.
    Quiet(String),
}

/// Typed words on a node that is OK'd: asked first (doc 37 rule 4).
#[derive(Clone, Debug, Default)]
struct AskFirst {
    node: String,
    name: String,
    text: String,
}

/// A file or asset attached to the next message.
#[derive(Clone, Debug, PartialEq)]
enum Attachment {
    File(String),
    Asset { id: String, name: String },
}

impl Attachment {
    fn label(&self) -> String {
        match self {
            Attachment::File(p) => model::file_name(p),
            Attachment::Asset { name, .. } => name.clone(),
        }
    }
}

/// The panel's own state between frames (egui memory; the chat itself is in `MfxState`).
#[derive(Clone, Debug, Default)]
struct ChatUi {
    draft: String,
    attachments: Vec<Attachment>,
    pending: Vec<(u64, Pending, f64)>,
    /// Sent while the service was down: sent again when it is back.
    queued: Vec<Pending>,
    /// Answers given here (question id → choice id or own words), until the service echoes them.
    answered: BTreeMap<String, String>,
    /// OK / Cut it pressed here (node → "ok" | "cut" | ""), until the graph catches up.
    decided: BTreeMap<String, String>,
    /// Custom… on this question: the next words answer it.
    custom_for: Option<String>,
    ask_first: Option<AskFirst>,
    details_open: bool,
    history_open: bool,
    history_search: String,
    scroll_to: Option<usize>,
    scroll_bottom: bool,
    /// Messages there were when the thread was last at the bottom.
    seen: usize,
    focus_composer: bool,
    /// The AI was asked something (chat length then, time): "Mortiflix is replying".
    awaiting: Option<(usize, f64)>,
    /// Card folds (Read it all, All 18 lines), by message index.
    open_cards: BTreeMap<usize, bool>,
    event_seq: u64,
    todos: Vec<(String, String)>,
    /// Render progress from status events: (done 0-1, line).
    render: Option<(f32, String)>,
    history_for: Option<String>,
}

fn ui_id() -> egui::Id {
    egui::Id::new("mfx-chat-ui")
}

/// The chat panel (PanelKind::MfxChat): left on stages 1-4, docked right in FilmCraft on stage 5.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let mut st: ChatUi = ui.data_mut(|d| d.get_temp(ui_id())).unwrap_or_default();
    let now = ui.input(|i| i.time);
    take_files(ctx, &mut st);
    read_replies(ctx, &mut st, now);
    read_events(ctx, &mut st);
    load_history(ctx, &mut st);
    send_queued(ctx, &mut st, now);

    let full = ui.max_rect();
    let offline = crate::service_notice(ui, ctx);
    let top = if offline { ui.min_rect().max.y + 4.0 } else { full.min.y };
    let msgs: Vec<Msg> = ctx.state.chat.iter().map(Msg::of).collect();
    let view = ctx.state.selected_node.clone();
    let info = view.as_deref().map(|v| model::node(&ctx.state.graph, v));

    // bottom-up: hint, composer, attachments, ask-first, moves, now (+ details), new-messages pill
    let mut y = full.max.y;
    let mut take = |h: f32| {
        let r = Rect::from_min_max(pos2(full.min.x, (y - h).max(top)), pos2(full.max.x, y));
        y = r.min.y;
        r
    };
    let hint_r = take(HINT_H);
    let composer_r = take(COMPOSER_H);
    let chips_r = (!st.attachments.is_empty()).then(|| take(CHIPS_H));
    let ask_r = st.ask_first.is_some().then(|| take(ASK_H));
    let moves_r = take(MOVES_H);
    let details_r = st.details_open.then(|| take(DETAILS_H));
    let now_r = take(NOW_H);
    let new_count = msgs.len().saturating_sub(st.seen);
    let pill_r = (new_count > 0 && !st.scroll_bottom).then(|| take(PILL_H));
    let bar_r = Rect::from_min_max(pos2(full.min.x, top), pos2(full.max.x, (top + BAR_H).min(y)));
    let thread_r = Rect::from_min_max(pos2(full.min.x, bar_r.max.y), pos2(full.max.x, y.max(bar_r.max.y)));

    let open_q = model::open_questions(&msgs, &st.answered).last().map(|(i, _)| *i);
    region(ui, bar_r, |ui| context_bar(ui, ctx, &mut st, &msgs, open_q));
    region(ui, thread_r, |ui| thread(ui, ctx, &mut st, &msgs, view.as_deref(), now));
    if let Some(r) = pill_r {
        region(ui, r, |ui| new_pill(ui, ctx, &mut st, new_count));
    }
    region(ui, now_r, |ui| now_line(ui, ctx, &mut st, now));
    if let Some(r) = details_r {
        region(ui, r, |ui| details(ui, ctx, &st));
    }
    region(ui, moves_r, |ui| moves_row(ui, ctx, &mut st, info.as_ref()));
    if let Some(r) = ask_r {
        region(ui, r, |ui| ask_first(ui, ctx, &mut st, now));
    }
    if let Some(r) = chips_r {
        region(ui, r, |ui| attachment_chips(ui, ctx, &mut st));
    }
    region(ui, composer_r, |ui| composer(ui, ctx, &mut st, info.as_ref(), &msgs, now));
    region(ui, hint_r, |ui| {
        let text = if offline || !ctx.service.status().is_connected() {
            "Enter sends · messages wait until Mortiflix is back"
        } else {
            "Enter sends · Shift+Enter new line · drop files here to attach them"
        };
        ui.painter().text(
            pos2(ui.max_rect().min.x + 2.0, ui.max_rect().center().y),
            Align2::LEFT_CENTER,
            text,
            FontId::proportional(11.0),
            ctx.theme.text_faint,
        );
    });
    if st.history_open {
        history_menu(ui, ctx, &mut st, &msgs, bar_r);
    }

    // Esc: back to the whole video (the composer's own first Esc only leaves it).
    let composer_focused = ui.memory(|m| m.has_focus(composer_id()));
    if !composer_focused && !st.history_open && ui.rect_contains_pointer(full) && ui.input(|i| i.key_pressed(Key::Escape)) && ctx.state.selected_node.is_some()
    {
        ctx.state.selected_node = None;
    }
    ui.data_mut(|d| d.insert_temp(ui_id(), st));
}

/// Draw `add` into `rect`, clipped to it.
fn region(ui: &mut egui::Ui, rect: Rect, add: impl FnOnce(&mut egui::Ui)) {
    if rect.height() <= 0.0 {
        return;
    }
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child);
}

fn composer_id() -> egui::Id {
    egui::Id::new("mfx-chat-composer")
}

// ---------- the service ----------

/// Note lines the app wrote itself with the service, so the history keeps them.
fn note(ctx: &mut MfxCtx, st: &mut ChatUi, msgs: &[ChatMessage], now: f64) {
    let list: Vec<Value> = msgs.iter().filter_map(|m| serde_json::to_value(m).ok()).collect();
    let id = ctx.service.request("chat.note", json!({"messages": list}));
    st.pending.push((id, Pending::Quiet("note".into()), now));
}

/// Add a line the app writes (a press, what typing became, an error) and note it.
fn add_line(ctx: &mut MfxCtx, st: &mut ChatUi, role: &str, text: String, context: Option<&str>, extra: Value, now: f64) {
    let mut m = ChatMessage { role: role.into(), text, at: String::new(), ..ChatMessage::default() };
    if let Value::Object(o) = extra {
        m.extra = o;
    }
    if let Some(c) = context {
        m.extra.insert("context".into(), Value::String(c.into()));
    }
    m.extra.insert("stage".into(), json!(ctx.state.stage));
    ctx.state.push_chat(m.clone());
    note(ctx, st, &[m], now);
}

fn error_text(r: &crate::service::Reply) -> String {
    r.error.get("message").and_then(Value::as_str).unwrap_or("something went wrong").chars().take(300).collect()
}

fn not_running(r: &crate::service::Reply) -> bool {
    r.error.get("code").and_then(Value::as_str) == Some("not_running")
}

fn read_replies(ctx: &mut MfxCtx, st: &mut ChatUi, now: f64) {
    let pending = std::mem::take(&mut st.pending);
    for (id, what, at) in pending {
        let Some(r) = ctx.service.take_reply(id) else {
            if now - at < PENDING_TIMEOUT {
                st.pending.push((id, what, at));
            }
            continue;
        };
        match what {
            Pending::Send { .. } if !r.ok && not_running(&r) => st.queued.push(what),
            Pending::Send { .. } if !r.ok => {
                st.awaiting = None;
                add_line(ctx, st, "action", format!("Couldn't send that: {}", error_text(&r)), None, json!({"error": true}), now);
            }
            Pending::Send { .. } => {}
            Pending::Fix { node, name, mv, text } => {
                if r.ok {
                    let edit = r.result.pointer("/edit/id").and_then(Value::as_str).map(|e| e.chars().take(20).collect::<String>());
                    let tail = match mv {
                        Move::NewTake => "the other lines keep going",
                        _ => "the other shots keep going",
                    };
                    let text = match &edit {
                        Some(e) => format!("Sent as {} on {} ({e}) · {tail}", mv.verb(), model::lower_name(&name)),
                        None => format!("Sent as {} on {} · {tail}", mv.verb(), model::lower_name(&name)),
                    };
                    add_line(ctx, st, "action", text, Some(&node), json!({"edit": edit}), now);
                } else {
                    let retry = json!([{"label": "Try again", "type": "node.edit", "params": {"node": node, "action": "fix", "text": text}}]);
                    add_line(
                        ctx,
                        st,
                        "action",
                        format!("Couldn't send the {} on {}: {}", mv.verb(), model::lower_name(&name), error_text(&r)),
                        Some(&node),
                        json!({"error": true, "actions": retry}),
                        now,
                    );
                }
            }
            Pending::History => {
                if r.ok
                    && let Some(list) = r.result.get("messages").and_then(Value::as_array)
                {
                    ctx.state.chat.clear();
                    for m in list.iter().rev().take(crate::state::MAX_CHAT).rev() {
                        if let Ok(m) = serde_json::from_value::<ChatMessage>(m.clone()) {
                            ctx.state.push_chat(m);
                        }
                    }
                    st.scroll_bottom = true;
                }
            }
            Pending::Decide | Pending::Quiet(_) => {
                // a note the service doesn't know yet (an older service) is fine; other errors show
                let quiet = matches!(&what, Pending::Quiet(k) if k == "note");
                if !r.ok && !quiet && !not_running(&r) {
                    add_line(ctx, st, "action", format!("That didn't go through: {}", error_text(&r)), None, json!({"error": true}), now);
                }
            }
        }
    }
}

fn read_events(ctx: &mut MfxCtx, st: &mut ChatUi) {
    let mut last = st.event_seq;
    for (seq, ev) in ctx.service.events_since(st.event_seq) {
        last = last.max(*seq);
        if ev.get("event").and_then(Value::as_str) != Some("status") {
            continue;
        }
        let data = ev.get("data").unwrap_or(&Value::Null);
        if let Some(todos) = data.get("todos").and_then(Value::as_array) {
            st.todos = todos
                .iter()
                .take(20)
                .filter_map(|t| {
                    let text = t.get("text").and_then(Value::as_str)?.chars().take(200).collect();
                    let state = t.get("state").and_then(Value::as_str).unwrap_or("todo").chars().take(20).collect();
                    Some((text, state))
                })
                .collect();
        }
        if let Some(r) = data.get("render") {
            st.render = if r.is_null() {
                None
            } else {
                let done = r.get("done").and_then(Value::as_f64).unwrap_or(0.0).clamp(0.0, 1.0) as f32;
                let line = r.get("line").and_then(Value::as_str).unwrap_or("").chars().take(200).collect();
                Some((done, line))
            };
        }
    }
    st.event_seq = last;
    if let Some((len, _)) = st.awaiting
        && ctx.state.chat.iter().skip(len).any(|m| Role::of(&m.role) == Role::Ai)
    {
        st.awaiting = None;
    }
}

/// When the service connects with a project open, ask for the project's whole history once.
fn load_history(ctx: &mut MfxCtx, st: &mut ChatUi) {
    if !ctx.service.status().is_connected() || ctx.state.project_id.is_none() || st.history_for == ctx.state.project_id {
        return;
    }
    st.history_for = ctx.state.project_id.clone();
    let id = ctx.service.request("chat.history", json!({"project": ctx.state.project_id}));
    st.pending.push((id, Pending::History, 0.0));
}

fn send_queued(ctx: &mut MfxCtx, st: &mut ChatUi, now: f64) {
    if st.queued.is_empty() || !ctx.service.status().is_connected() {
        return;
    }
    for p in std::mem::take(&mut st.queued) {
        if let Pending::Send { text, context, files, assets } = &p {
            let id =
                ctx.service.request("chat.send", json!({"text": text, "context": context.as_deref().unwrap_or("video"), "files": files, "assets": assets}));
            st.pending.push((id, p.clone(), now));
            st.awaiting = Some((ctx.state.chat.len(), now));
        }
    }
}

/// Files chosen with + (Attach) or dropped on the window while the chat shows.
fn take_files(ctx: &mut MfxCtx, st: &mut ChatUi) {
    for f in ctx.state.take_files("chat.attach") {
        st.attachments.extend(f.paths.into_iter().map(Attachment::File));
    }
    if ctx.state.show_new_project {
        return; // the New project window takes drops while it is open
    }
    for f in ctx.state.take_files("drop") {
        if ctx.state.selected_node.is_some() {
            // a node: "use this in shot 7" goes with the next words
            st.attachments.extend(f.paths.into_iter().map(Attachment::File));
            st.focus_composer = true;
        } else {
            // the whole video: added to the project's assets
            let n = f.paths.len();
            ctx.service.request("assets.add", json!({"paths": f.paths}));
            add_line(ctx, st, "action", format!("You added {n} file{} to Assets", if n == 1 { "" } else { "s" }), None, json!({}), 0.0);
        }
    }
    st.attachments.dedup();
    st.attachments.truncate(20);
}

// ---------- moves ----------

/// The owner's decision on a node as the panel knows it (a press here wins until the graph has it).
fn decision(st: &ChatUi, info: &NodeInfo) -> Option<String> {
    match st.decided.get(&info.id) {
        Some(d) if d.is_empty() => None,
        Some(d) => Some(d.clone()),
        None => info.owner.clone(),
    }
}

/// Run a move on a node, the same as the node's own button.
fn run_move(ctx: &mut MfxCtx, st: &mut ChatUi, info: &NodeInfo, mv: Move, now: f64) {
    match mv {
        Move::Ok | Move::Cut => {
            let key = mv.key();
            let taking_back = decision(st, info).as_deref() == Some(key);
            let action = if taking_back { "clear" } else { key };
            let id = ctx.service.request("node.edit", json!({"node": info.id, "action": action}));
            st.pending.push((id, Pending::Decide, now));
            st.decided.insert(info.id.clone(), if taking_back { String::new() } else { key.to_string() });
            if taking_back {
                add_line(ctx, st, "action", format!("You took back {} on {}", mv.verb(), info.name), Some(&info.id), json!({}), now);
            } else {
                model::merge_press(&mut ctx.state.chat, mv.verb(), &info.name);
                if let Some(last) = ctx.state.chat.last().cloned() {
                    note(ctx, st, &[last], now);
                }
            }
        }
        Move::Fix | Move::NewTake | Move::ChangeIt => {
            if ctx.state.selected_node.as_deref() != Some(&info.id) {
                ctx.state.selected_node = Some(info.id.clone());
            }
            st.focus_composer = true;
        }
        Move::Play => ctx.state.play_request = Some(info.id.clone()),
    }
}

/// Typed words on a node become its Fix (New take, Change it), and the chat says so.
fn send_fix(ctx: &mut MfxCtx, st: &mut ChatUi, info: &NodeInfo, text: String, files: Vec<String>, now: f64) {
    let mv = model::typed_move(&info.kind);
    let mut words = text.clone();
    for f in &files {
        words.push_str(&format!("\nUse this file here: {f}"));
    }
    add_line(ctx, st, "user", text.clone(), Some(&info.id), json!({"files": files}), now);
    let id = ctx.service.request("node.edit", json!({"node": info.id, "action": "fix", "text": words}));
    st.pending.push((id, Pending::Fix { node: info.id.clone(), name: info.name.clone(), mv, text: words }, now));
    st.decided.remove(&info.id);
}

/// Send what is in the composer.
fn send(ctx: &mut MfxCtx, st: &mut ChatUi, info: Option<&NodeInfo>, now: f64) {
    let text: String = st.draft.trim().chars().take(MAX_TEXT).collect();
    if text.is_empty() && st.attachments.is_empty() {
        return;
    }
    let mut files = Vec::new();
    let mut assets = Vec::new();
    for a in std::mem::take(&mut st.attachments) {
        match a {
            Attachment::File(p) => files.push(p),
            Attachment::Asset { id, .. } => assets.push(id),
        }
    }
    st.draft.clear();
    st.scroll_bottom = true;
    if let Some(q) = st.custom_for.take() {
        let id = ctx.service.request("chat.answer", json!({"question": q, "text": text}));
        st.pending.push((id, Pending::Quiet("answer".into()), now));
        st.answered.insert(q, text);
        return;
    }
    match info {
        Some(n) if !model::moves(&n.kind).iter().any(|m| m.needs_words()) || text.is_empty() => {
            // a beat (or nothing to fix): words go to the AI about it
            send_chat(ctx, st, text, Some(n.id.clone()), files, assets, now);
        }
        Some(n) if decision(st, n).as_deref() == Some("ok") => {
            let mut words = text;
            for a in &assets {
                words.push_str(&format!("\nUse asset {a} here"));
            }
            st.ask_first = Some(AskFirst { node: n.id.clone(), name: n.name.clone(), text: words });
            st.attachments = files.into_iter().map(Attachment::File).collect();
        }
        Some(n) => {
            let mut words = text;
            for a in &assets {
                words.push_str(&format!("\nUse asset {a} here"));
            }
            send_fix(ctx, st, n, words, files, now);
        }
        None => send_chat(ctx, st, text, None, files, assets, now),
    }
}

fn send_chat(ctx: &mut MfxCtx, st: &mut ChatUi, text: String, context: Option<String>, files: Vec<String>, assets: Vec<String>, now: f64) {
    let p = Pending::Send { text: text.clone(), context: context.clone(), files: files.clone(), assets: assets.clone() };
    if !ctx.service.status().is_connected() {
        st.queued.push(p);
        return;
    }
    let id = ctx.service.request("chat.send", json!({"text": text, "context": context.as_deref().unwrap_or("video"), "files": files, "assets": assets}));
    st.pending.push((id, p, now));
    st.awaiting = Some((ctx.state.chat.len(), now));
}

/// A button the service put on a message or card.
fn run_action(ctx: &mut MfxCtx, st: &mut ChatUi, b: &ActionButton, now: f64) {
    match &b.act {
        Act::Request { ty, params } => {
            let id = ctx.service.request(ty, params.clone());
            st.pending.push((id, Pending::Quiet(ty.clone()), now));
        }
        Act::Command { id, params } => ctx.command(id, params.clone()),
    }
}

fn answer(ctx: &mut MfxCtx, st: &mut ChatUi, question: &str, choice: &str, now: f64) {
    let id = ctx.service.request("chat.answer", json!({"question": question, "choice": choice}));
    st.pending.push((id, Pending::Quiet("answer".into()), now));
    st.answered.insert(question.to_string(), choice.to_string());
}

// ---------- drawing ----------

/// A small secondary button (28 px, quiet): cards and rows. `on` shows a done state (OK ✓).
fn small_button(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str, label: &str, primary: bool) -> egui::Response {
    let t = ctx.theme;
    let text = RichText::new(label).size(12.0).color(if primary { Color32::WHITE } else { t.text });
    let b = egui::Button::new(text).min_size(vec2(0.0, 24.0)).fill(if primary { t.accent } else { t.hover }).stroke(Stroke::NONE).corner_radius(4.0);
    let r = ui.add(b);
    ctx.auto(id, &r, label);
    r
}

/// The beat square with its number beside it (the colour is never alone).
fn beat_mark(ui: &mut egui::Ui, ctx: &MfxCtx, info: &NodeInfo) {
    let Some(n) = info.beat_no else { return };
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 14.0), Sense::hover());
    ui.painter().rect_filled(Rect::from_center_size(pos2(r.center().x, r.center().y + 1.0), vec2(8.0, 8.0)), 1.5, info.colour.unwrap_or(ctx.theme.text_faint));
    ui.label(RichText::new(format!("{n} ")).size(12.0).color(ctx.theme.text_dim));
}

fn context_bar(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, msgs: &[Msg], open_q: Option<usize>) {
    let t = ctx.theme;
    let r = ui.max_rect();
    ui.painter().line_segment([pos2(r.min.x, r.max.y - 0.5), pos2(r.max.x, r.max.y - 0.5)], Stroke::new(1.0, t.separator));
    let crumbs = model::crumbs(&ctx.state.graph, ctx.state.selected_node.as_deref());
    let deep = crumbs.len() > 1;
    ui.allocate_ui_with_layout(vec2(r.width(), BAR_H), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.label(RichText::new("Talking about").size(12.0).color(t.text_faint));
        ui.add_space(4.0);
        let last = crumbs.len().saturating_sub(1);
        for (i, (to, label, info)) in crumbs.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("›").size(12.0).color(t.text_faint));
            }
            if let Some(info) = info {
                beat_mark(ui, ctx, info);
            }
            let here = i == last;
            let text = RichText::new(label).size(13.0).color(if here { t.text } else { LINK });
            let resp = if here { ui.add(egui::Label::new(text.strong()).truncate()) } else { ui.add(egui::Label::new(text.underline()).sense(Sense::click())) };
            if !here && resp.clicked() {
                ctx.state.selected_node = to.clone();
            }
            ctx.auto(&format!("mfx.chat.crumb.{i}"), &resp, label);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if deep {
                let x = ui.add(egui::Button::new(RichText::new("✕").size(12.0)).frame(false)).on_hover_text("Back to the whole video (Esc)");
                if x.clicked() {
                    ctx.state.selected_node = None;
                }
                ctx.auto("mfx.chat.context.back", &x, "Back to the whole video");
            }
            let h = ui.add(egui::Button::new(RichText::new("History ▾").size(12.0).color(t.text_dim)).frame(false));
            if h.clicked() {
                st.history_open = !st.history_open;
            }
            ctx.auto("mfx.chat.history", &h, "History");
            let waiting = model::open_questions(msgs, &st.answered).len();
            if waiting > 0 && !deep {
                let label = if waiting == 1 { "1 question waiting".to_string() } else { format!("{waiting} questions waiting") };
                let w = ui.add(egui::Label::new(RichText::new(label.clone()).size(12.0).color(t.text)).sense(Sense::click()));
                let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter().circle_filled(dot.center(), 3.5, t.accent);
                if w.clicked() {
                    st.scroll_to = open_q;
                }
                ctx.auto("mfx.chat.waiting", &w, &label);
            }
        });
    });
}

fn thread(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, msgs: &[Msg], view: Option<&str>, now: f64) {
    let t = ctx.theme;
    if msgs.is_empty() && st.queued.is_empty() {
        empty_state(ui, ctx, now);
        return;
    }
    let rows = model::rows(&ctx.state.graph, msgs, view);
    let open_q = model::open_questions(msgs, &st.answered).last().map(|(i, _)| *i);
    let width = ui.available_width();
    let out = egui::ScrollArea::vertical().id_salt("mfx-chat-thread").auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
        ui.set_width(width - 12.0);
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.add_space(6.0);
        if let Some(v) = view {
            node_intro(ui, ctx, v);
        }
        for (ri, row) in rows.iter().enumerate() {
            match row {
                Row::Divider { msg, stage, at, summary } => {
                    let r = divider(ui, ctx, ri, *stage, at, summary);
                    if st.scroll_to == Some(*msg) {
                        ui.scroll_to_rect(r, Some(Align::TOP));
                        st.scroll_to = None;
                    }
                }
                Row::Message(i) => {
                    let Some(m) = msgs.get(*i) else { continue };
                    let r = ui.scope(|ui| message(ui, ctx, st, *i, m, open_q == Some(*i), now)).response.rect;
                    if st.scroll_to == Some(*i) {
                        ui.scroll_to_rect(r, Some(Align::TOP));
                        st.scroll_to = None;
                    }
                }
                Row::Folded { node, count, said, last, .. } => folded(ui, ctx, node, *count, said, last),
            }
        }
        for q in &st.queued {
            if let Pending::Send { text, .. } = q {
                you_bubble(ui, ctx, text, &[]);
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                    ui.label(RichText::new("Sends when Mortiflix is back").size(11.0).color(t.text_faint));
                });
            }
        }
        if let Some((_, since)) = st.awaiting
            && now - since < 180.0
        {
            ui.horizontal(|ui| {
                let dots = ".".repeat(1 + ((now * 2.0) as usize % 3));
                ui.label(RichText::new(format!("Mortiflix is replying{dots}")).size(12.0).color(t.text_faint));
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        if st.scroll_bottom {
            ui.scroll_to_cursor(Some(Align::BOTTOM));
        }
        ui.add_space(4.0);
    });
    let at_bottom = out.state.offset.y + out.inner_rect.height() >= out.content_size.y - 8.0;
    if at_bottom {
        st.seen = msgs.len();
        st.scroll_bottom = false;
    }
}

fn empty_state(ui: &mut egui::Ui, ctx: &mut MfxCtx, now: f64) {
    let t = ctx.theme;
    ui.add_space(24.0);
    ui.vertical_centered(|ui| {
        if ctx.state.project_id.is_none() {
            ui.label(RichText::new("No project yet.").size(14.0).color(t.text));
            ui.label(RichText::new("Start one and Mortiflix writes the script first.").size(12.0).color(t.text_dim));
            ui.add_space(8.0);
            let b = small_button(ui, ctx, "mfx.chat.newProject", "New project…", true);
            if b.clicked() {
                ctx.command("mortiflix.newProject", json!({}));
            }
        } else {
            let pulse = (0.55 + 0.45 * (now * 3.0).sin()) as f32;
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(10.0, 14.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, t.ok.gamma_multiply(pulse));
                ui.label(RichText::new("Reading your idea and your files…").size(13.0).color(t.text_dim));
            });
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(80));
        }
    });
}

/// In a node's context the AI opens with what it is and what it is made from (doc 09 §1).
fn node_intro(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str) {
    let t = ctx.theme;
    let n = model::node(&ctx.state.graph, id);
    ui.label(RichText::new(format!("Mortiflix · {}", n.name)).size(11.0).color(t.text_faint));
    let mut line = String::new();
    if !n.title.is_empty() && n.kind != "beat" {
        line.push_str(&n.title);
        line.push_str(". ");
    }
    if let Some(b) = n.beat.as_deref().filter(|_| n.kind != "beat") {
        line.push_str(&format!("Made from beat {} {}.", n.beat_no.unwrap_or(0), model::node(&ctx.state.graph, b).name));
    }
    if n.versions > 1 {
        line.push_str(&format!(" {} versions, v{} in use.", n.versions, n.current.unwrap_or(n.versions)));
    }
    if n.kind == "beat" {
        line = format!("Everything in beat {} {}: its shots, lines and music.", n.beat_no.unwrap_or(0), n.name);
    }
    if line.is_empty() {
        line = "Ask for a change, or use the buttons below.".into();
    }
    ui.add(egui::Label::new(RichText::new(line).size(13.0).color(t.text)).wrap());
    ui.add_space(4.0);
}

fn divider(ui: &mut egui::Ui, ctx: &mut MfxCtx, row: usize, stage: u8, at: &str, summary: &str) -> Rect {
    let t = ctx.theme;
    ui.add_space(4.0);
    let w = ui.available_width();
    let (r, resp) = ui.allocate_exact_size(vec2(w, 20.0), Sense::click());
    let label = format!("{}  {}", stage_name(stage), model::clock(at)).trim().to_string();
    let col = if resp.hovered() { t.text } else { t.text_dim };
    let g = ui.painter().layout_no_wrap(label.clone(), FontId::proportional(12.0), col);
    let gx = r.center().x - g.size().x / 2.0;
    let y = r.center().y;
    let s = Stroke::new(1.0, t.separator.gamma_multiply(1.6));
    ui.painter().line_segment([pos2(r.min.x, y), pos2(gx - 8.0, y)], s);
    ui.painter().line_segment([pos2(gx + g.size().x + 8.0, y), pos2(r.max.x, y)], s);
    ui.painter().galley(pos2(gx, y - g.size().y / 2.0), g, col);
    let resp = resp.on_hover_text(format!("Go to {}", stage_name(stage)));
    if resp.clicked() {
        ctx.command(&format!("stages.{}", stage_key(stage)), json!({}));
    }
    ctx.auto(&format!("mfx.chat.divider.{row}"), &resp, &label);
    if !summary.is_empty() {
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(summary).size(12.0).color(t.text_faint));
        });
    }
    r
}

/// Message text with links (shot N, line N, the message's refs): click selects that node.
fn rich_text(ui: &mut egui::Ui, ctx: &mut MfxCtx, msg: usize, text: &str, refs: &[model::Ref], size: f32, col: Color32) {
    let segs = model::segments(text, refs, &ctx.state.graph);
    if segs.iter().all(|s| s.link.is_none()) {
        ui.add(egui::Label::new(RichText::new(text).size(size).color(col)).wrap());
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (k, s) in segs.iter().enumerate() {
            match &s.link {
                None => {
                    ui.label(RichText::new(&s.text).size(size).color(col));
                }
                Some(id) => {
                    let info = model::node(&ctx.state.graph, id);
                    if info.kind != "beat" {
                        beat_mark(ui, ctx, &info);
                    }
                    let r = ui
                        .add(egui::Label::new(RichText::new(&s.text).size(size).color(LINK).underline()).sense(Sense::click()))
                        .on_hover_text(format!("Talk about {}", info.name));
                    if r.clicked() {
                        ctx.state.selected_node = Some(id.clone());
                    }
                    ctx.auto(&format!("mfx.chat.link.{msg}.{k}"), &r, &s.text);
                }
            }
        }
    });
}

fn you_bubble(ui: &mut egui::Ui, ctx: &mut MfxCtx, text: &str, files: &[String]) {
    let t = ctx.theme;
    let w = ui.available_width();
    ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
        let max = (w * 0.8).max(80.0);
        let g = ui.painter().layout(text.to_string(), FontId::proportional(13.0), t.text, max - 20.0);
        let size = g.size() + vec2(20.0, 12.0);
        let (r, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().rect_filled(r, 12.0, t.hover);
        ui.painter().galley(r.min + vec2(10.0, 6.0), g, t.text);
    });
    if !files.is_empty() {
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            for f in files.iter().take(8) {
                ui.label(RichText::new(format!("· {f}")).size(11.0).color(t.text_dim));
            }
        });
    }
}

fn message(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, i: usize, m: &Msg, open: bool, now: f64) {
    let t = ctx.theme;
    if m.role == Role::You {
        you_bubble(ui, ctx, &m.text, &m.files);
        return;
    }
    let ruled = m.error || m.needs_you || m.question.is_some();
    let rule = if m.error || m.needs_you {
        Some(t.error)
    } else if m.question.is_some() {
        Some(if open { t.accent } else { t.separator.gamma_multiply(2.0) })
    } else {
        None
    };
    let inner = ui.scope(|ui| {
        if ruled {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                ui.vertical(|ui| message_body(ui, ctx, st, i, m, now));
            });
        } else {
            message_body(ui, ctx, st, i, m, now);
        }
    });
    if let Some(c) = rule {
        let r = inner.response.rect;
        ui.painter().rect_filled(Rect::from_min_max(r.min, pos2(r.min.x + 3.0, r.max.y)), 1.0, c);
    }
}

fn message_body(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, i: usize, m: &Msg, now: f64) {
    let t = ctx.theme;
    match m.role {
        Role::Action => {
            let col = if m.error { t.text } else { t.text_dim };
            rich_text(ui, ctx, i, &m.text, &m.refs, 12.0, col);
        }
        _ => {
            let who = match (&m.question, m.needs_you) {
                (_, true) => "Mortiflix · needs you",
                (Some(_), _) => "Mortiflix · question",
                _ => "Mortiflix",
            };
            ui.label(RichText::new(who).size(11.0).color(t.text_faint));
            if !m.text.is_empty() {
                ui.scope(|ui| {
                    ui.set_max_width(ui.available_width().min(460.0));
                    rich_text(ui, ctx, i, &m.text, &m.refs, 13.0, t.text);
                });
            }
        }
    }
    if let Some(card) = &m.card {
        let open = st.open_cards.get(&i).copied().unwrap_or(false);
        let mut out = cards::CardOut::default();
        cards::show(ui, ctx, i, card, open, &st.answered, &mut out);
        if out.toggle {
            st.open_cards.insert(i, !open);
        }
        if let Some((q, c)) = out.answer {
            answer(ctx, st, &q, &c, now);
        }
        if let Some(b) = out.action {
            run_action(ctx, st, &b, now);
        }
        if out.change_words {
            ctx.state.selected_node = None;
            st.draft = "Change the words: ".into();
            st.focus_composer = true;
        }
    }
    if let Some(q) = &m.question {
        question(ui, ctx, st, q, now);
    }
    if !m.actions.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for (k, b) in m.actions.iter().enumerate() {
                if small_button(ui, ctx, &format!("mfx.chat.action.{i}.{k}"), &b.label, b.primary).clicked() {
                    run_action(ctx, st, b, now);
                }
            }
        });
    }
}

fn question(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, q: &model::Question, now: f64) {
    let t = ctx.theme;
    let picked = st.answered.get(&q.id).cloned().or_else(|| q.picked.clone());
    if !q.text.is_empty() {
        ui.label(RichText::new(&q.text).size(13.0).color(t.text).strong());
    }
    let in_use = picked.clone().or_else(|| q.default.clone());
    ui.horizontal_wrapped(|ui| {
        for c in &q.choices {
            let on = in_use.as_deref() == Some(c.id.as_str());
            let label = if on { format!("✓ {}", c.label) } else { c.label.clone() };
            let b = egui::Button::new(RichText::new(&label).size(12.0).color(if on { t.text } else { t.text_dim }))
                .min_size(vec2(0.0, 24.0))
                .fill(if on { t.hover } else { t.panel_bg })
                .stroke(Stroke::new(1.0, if on { t.text_dim } else { t.separator.gamma_multiply(2.0) }))
                .corner_radius(12.0);
            let r = ui.add(b);
            if r.clicked() && picked.as_deref() != Some(c.id.as_str()) {
                answer(ctx, st, &q.id, &c.id, now);
            }
            ctx.auto(&format!("mfx.chat.q.{}.{}", q.id, c.id), &r, &c.label);
        }
        if q.custom {
            let r = ui.add(egui::Button::new(RichText::new("Custom…").size(12.0).color(t.text_dim)).min_size(vec2(0.0, 24.0)).frame(false));
            if r.clicked() {
                st.custom_for = Some(q.id.clone());
                st.focus_composer = true;
            }
            ctx.auto(&format!("mfx.chat.q.{}.custom", q.id), &r, "Custom…");
        }
    });
    let line = match (&picked, &q.default) {
        (Some(p), _) => format!("You picked {}.", q.label_of(p)),
        (None, Some(d)) => format!("Going with {} unless you say.", q.label_of(d)),
        (None, None) => "Nothing waits on this: the AI keeps going.".into(),
    };
    ui.label(RichText::new(line).size(12.0).color(t.text_dim));
}

fn folded(ui: &mut egui::Ui, ctx: &mut MfxCtx, node: &str, count: usize, said: &str, last: &str) {
    let t = ctx.theme;
    let info = model::node(&ctx.state.graph, node);
    let w = ui.available_width();
    let resp = ui
        .allocate_ui_with_layout(vec2(w, 26.0), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            beat_mark(ui, ctx, &info);
            let words = if said.is_empty() { last } else { said };
            ui.add(egui::Label::new(RichText::new(format!("{}: \u{201c}{words}\u{201d}", info.name)).size(12.0).color(t.text)).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let n = if count == 1 { "1 message ›".to_string() } else { format!("{count} messages ›") };
                ui.label(RichText::new(n).size(12.0).color(t.text_faint));
            });
        })
        .response;
    let resp = ui.interact(resp.rect, egui::Id::new(("mfx-chat-fold", node)), Sense::click());
    if resp.hovered() {
        ui.painter().rect_stroke(resp.rect, 4.0, Stroke::new(1.0, t.separator.gamma_multiply(2.0)), egui::StrokeKind::Inside);
    }
    if resp.clicked() {
        ctx.state.selected_node = Some(node.to_string());
    }
    ctx.auto(&format!("mfx.chat.fold.{node}"), &resp, &info.name);
}

fn new_pill(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, n: usize) {
    ui.vertical_centered(|ui| {
        let label = format!("↓ {n} new");
        if small_button(ui, ctx, "mfx.chat.newPill", &label, false).clicked() {
            st.scroll_bottom = true;
        }
    });
}

fn making(ctx: &MfxCtx) -> bool {
    ctx.state.statuses.iter().any(|s| s.kind == StatusKind::Making)
}

fn now_line(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, now: f64) {
    let t = ctx.theme;
    let r = ui.max_rect();
    ui.painter().line_segment([pos2(r.min.x, r.min.y + 0.5), pos2(r.max.x, r.min.y + 0.5)], Stroke::new(1.0, t.separator));
    let busy = making(ctx) || !ctx.state.ai_line.is_empty();
    let line = if ctx.state.ai_line.is_empty() {
        if making(ctx) { "Working…".to_string() } else { "Waiting for you · nothing is being made".to_string() }
    } else {
        ctx.state.ai_line.clone()
    };
    ui.allocate_ui_with_layout(vec2(r.width(), NOW_H), Layout::left_to_right(Align::Center), |ui| {
        let (d, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
        if busy {
            let pulse = (0.55 + 0.45 * (now * 3.0).sin()) as f32;
            ui.painter().circle_filled(d.center(), 3.5, t.ok.gamma_multiply(pulse));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            ui.painter().circle_stroke(d.center(), 3.5, Stroke::new(1.2, t.text_faint));
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = if st.details_open { "Details ▴" } else { "Details ▾" };
            let b = ui.add(egui::Button::new(RichText::new(label).size(12.0).color(t.text_dim)).frame(false));
            if b.clicked() {
                st.details_open = !st.details_open;
            }
            ctx.auto("mfx.chat.details", &b, "Details");
            let resp = ui.add(egui::Label::new(RichText::new(&line).size(12.0).color(if busy { t.text } else { t.text_dim })).truncate());
            ctx.auto("mfx.chat.now", &resp, &line);
        });
    });
}

fn details(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &ChatUi) {
    let t = ctx.theme;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for i in 0..crate::STAGES.len() {
            let Ok(s) = u8::try_from(i) else { break };
            let status = ctx.state.status(s);
            let mark = match status.kind {
                StatusKind::Done => "✓",
                StatusKind::Making => "●",
                StatusKind::NeedsYou | StatusKind::Stopped => "!",
                StatusKind::Ready => "▸",
                StatusKind::NotStarted => "○",
            };
            let col = if s == ctx.state.stage { t.text } else { t.text_dim };
            let count = if status.kind == StatusKind::Making && status.total > 0 {
                format!(" {}/{}", status.done.min(status.total), status.total)
            } else {
                String::new()
            };
            ui.label(RichText::new(format!("{} {mark}{count}", stage_name(s))).size(12.0).color(col));
        }
    });
    ui.add_space(4.0);
    if st.todos.is_empty() {
        ui.label(RichText::new("No steps running.").size(12.0).color(t.text_faint));
    }
    for (text, state) in st.todos.iter().take(3) {
        let (mark, col) = match state.as_str() {
            "done" => ("✓", t.text_faint),
            "doing" | "running" => ("●", t.text),
            "failed" => ("!", t.error),
            _ => ("○", t.text_dim),
        };
        ui.add(egui::Label::new(RichText::new(format!("{mark} {text}")).size(12.0).color(col)).truncate());
    }
    if let Some((done, line)) = &st.render {
        ui.add(egui::ProgressBar::new(*done).desired_height(6.0));
        ui.label(RichText::new(line).size(11.0).color(t.text_dim));
    }
}

fn moves_row(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, info: Option<&NodeInfo>) {
    let t = ctx.theme;
    let now = ui.input(|i| i.time);
    ui.allocate_ui_with_layout(vec2(ui.available_width(), MOVES_H), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        match info {
            Some(n) if !model::moves(&n.kind).is_empty() => {
                ui.label(RichText::new(&n.name).size(12.0).color(t.text_faint));
                let decided = decision(st, n);
                for mv in model::moves(&n.kind) {
                    let done = decided.as_deref() == Some(mv.key());
                    let label = if done { format!("{} ✓", mv.label(&n.kind)) } else { mv.label(&n.kind).to_string() };
                    if small_button(ui, ctx, &format!("mfx.chat.move.{}", mv.key()), &label, false).clicked() {
                        run_move(ctx, st, n, *mv, now);
                    }
                }
            }
            _ => {
                ui.label(RichText::new("Moves").size(12.0).color(t.text_faint));
                if small_button(ui, ctx, "mfx.chat.move.playAll", "▶ Play everything", false).clicked() {
                    ctx.state.play_request = Some("all".into());
                }
                let next: &[(&str, &str, &str)] = match ctx.state.stage {
                    2 => &[("hearIt", "Hear the voice and music", "stages.voiceMusic")],
                    STAGE_VOICE_MUSIC => &[("editVideo", "Edit the video", "stages.editVideo"), ("getVideo", "Get the video", "stages.getVideo")],
                    STAGE_FILMCRAFT => &[("export", "Go to Export", "stages.render")],
                    s if s >= STAGE_RENDER => &[],
                    _ => &[],
                };
                for (key, label, cmd) in next {
                    if small_button(ui, ctx, &format!("mfx.chat.move.{key}"), label, false).clicked() {
                        ctx.command(cmd, json!({}));
                    }
                }
            }
        }
    });
}

fn ask_first(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, now: f64) {
    let t = ctx.theme;
    let Some(a) = st.ask_first.clone() else { return };
    let r = ui.max_rect();
    ui.painter().rect_filled(Rect::from_min_max(r.min + vec2(0.0, 4.0), pos2(r.min.x + 3.0, r.max.y - 4.0)), 1.0, t.accent);
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(format!("{} is OK'd. Remake it anyway?", a.name)).size(13.0).color(t.text));
    });
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        if small_button(ui, ctx, "mfx.chat.askFirst.remake", "Remake it", true).clicked() {
            st.ask_first = None;
            let info = model::node(&ctx.state.graph, &a.node);
            let files = std::mem::take(&mut st.attachments)
                .into_iter()
                .filter_map(|x| match x {
                    Attachment::File(p) => Some(p),
                    Attachment::Asset { .. } => None,
                })
                .collect();
            send_fix(ctx, st, &info, a.text.clone(), files, now);
        }
        if small_button(ui, ctx, "mfx.chat.askFirst.keep", "Keep it", false).clicked() {
            st.ask_first = None;
            st.draft = a.text.clone();
        }
    });
}

fn attachment_chips(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi) {
    let t = ctx.theme;
    let mut remove = None;
    ui.horizontal(|ui| {
        for (k, a) in st.attachments.iter().enumerate() {
            let r = ui.add(
                egui::Button::new(RichText::new(format!("{}  ✕", a.label())).size(11.0).color(t.text_dim))
                    .min_size(vec2(0.0, 22.0))
                    .fill(t.hover)
                    .corner_radius(11.0),
            );
            if r.clicked() {
                remove = Some(k);
            }
            ctx.auto(&format!("mfx.chat.attachment.{k}"), &r, &a.label());
        }
    });
    if let Some(k) = remove
        && k < st.attachments.len()
    {
        st.attachments.remove(k);
    }
}

fn composer(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, info: Option<&NodeInfo>, msgs: &[Msg], now: f64) {
    let t = ctx.theme;
    let r = ui.max_rect();
    let hint = match &st.custom_for {
        Some(q) => {
            let text = msgs.iter().filter_map(|m| m.question.as_ref()).find(|x| &x.id == q).map(|x| x.text.clone()).unwrap_or_default();
            if text.is_empty() { "Your own answer…".to_string() } else { format!("Your own answer: {text}") }
        }
        None => model::placeholder(info),
    };
    // Enter sends, Shift+Enter is a new line: take Enter before the text box sees it.
    let focused = ui.memory(|m| m.has_focus(composer_id()));
    let enter = focused && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
    let mut send_now = enter;
    ui.allocate_ui_with_layout(r.size(), Layout::left_to_right(Align::Center), |ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let plus =
            ui.add(egui::Button::new(RichText::new("+").size(16.0)).min_size(vec2(30.0, 30.0)).fill(t.hover).corner_radius(4.0)).on_hover_text("Attach files");
        if plus.clicked() {
            ctx.command("mortiflix.pickFiles", json!({"purpose": "chat.attach", "multiple": true}));
        }
        ctx.auto("mfx.chat.attach", &plus, "Attach files");
        let has_text = !st.draft.trim().is_empty() || !st.attachments.is_empty();
        let send_w = 58.0;
        let edit_w = (ui.available_width() - send_w - 6.0).max(60.0);
        let edit = ui.add_sized(
            [edit_w, (r.height() - 10.0).max(24.0)],
            egui::TextEdit::multiline(&mut st.draft)
                .id(composer_id())
                .hint_text(hint.as_str())
                .desired_rows(2)
                .font(FontId::proportional(13.0))
                .background_color(t.field_bg),
        );
        if let Some(a) = edit.dnd_release_payload::<AssetDrag>() {
            st.attachments.push(Attachment::Asset { id: a.id.clone(), name: a.name.clone() });
        }
        if st.focus_composer {
            edit.request_focus();
            st.focus_composer = false;
        }
        ctx.auto("mfx.chat.input", &edit, "Message");
        let b = egui::Button::new(RichText::new("Send").size(13.0).color(if has_text { Color32::WHITE } else { t.text_dim }))
            .min_size(vec2(send_w, 30.0))
            .fill(if has_text { t.accent } else { t.hover })
            .corner_radius(4.0);
        let s = ui.add(b);
        ctx.auto("mfx.chat.send", &s, "Send");
        if s.clicked() {
            send_now = true;
        }
    });
    if send_now {
        send(ctx, st, info, now);
    }
}

fn history_menu(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ChatUi, msgs: &[Msg], bar: Rect) {
    let t = ctx.theme;
    let pos = pos2(bar.min.x + 8.0, bar.max.y + 2.0);
    let width = (bar.width() - 16.0).clamp(200.0, 360.0);
    let area = egui::Area::new(egui::Id::new("mfx-chat-history")).order(egui::Order::Foreground).fixed_pos(pos).show(ui.ctx(), |ui| {
        egui::Frame::popup(ui.style()).fill(t.panel_bg).show(ui, |ui| {
            ui.set_width(width);
            let search = ui.add(egui::TextEdit::singleline(&mut st.history_search).hint_text("Find words in any message").desired_width(width));
            ctx.auto("mfx.chat.history.search", &search, "Find");
            egui::ScrollArea::vertical().max_height(420.0).show(ui, |ui| {
                let q = st.history_search.trim().to_lowercase();
                if !q.is_empty() {
                    let hits: Vec<(usize, &Msg)> = msgs.iter().enumerate().filter(|(_, m)| m.text.to_lowercase().contains(&q)).rev().take(30).collect();
                    if hits.is_empty() {
                        ui.label(RichText::new("No message has those words.").size(12.0).color(t.text_faint));
                    }
                    for (i, m) in hits {
                        let line: String = m.text.chars().take(70).collect();
                        let r = ui.add(egui::Button::new(RichText::new(line).size(12.0)).frame(false));
                        if r.clicked() {
                            go_to_message(ctx, st, m, i);
                        }
                        ctx.auto(&format!("mfx.chat.history.hit.{i}"), &r, "Message");
                    }
                    return;
                }
                heading(ui, ctx, "Stages");
                let mut last = None;
                for (i, m) in msgs.iter().enumerate() {
                    if let Some(s) = m.stage.filter(|s| Some(*s) != last) {
                        last = Some(s);
                        let label = format!("{}  {}", stage_name(s), model::clock(&m.at));
                        let r = ui.add(egui::Button::new(RichText::new(label.trim()).size(12.0)).frame(false));
                        if r.clicked() {
                            ctx.state.selected_node = None;
                            st.scroll_to = Some(i);
                            st.history_open = false;
                        }
                        ctx.auto(&format!("mfx.chat.history.stage.{s}"), &r, stage_name(s));
                    }
                }
                heading(ui, ctx, "Talked about");
                let mut seen: Vec<(String, usize)> = Vec::new();
                for m in msgs.iter().rev() {
                    if let Some(c) = &m.context {
                        match seen.iter_mut().find(|(id, _)| id == c) {
                            Some(e) => e.1 += 1,
                            None => seen.push((c.clone(), 1)),
                        }
                    }
                }
                if seen.is_empty() {
                    ui.label(RichText::new("Nothing yet: click a shot or line to talk about it.").size(12.0).color(t.text_faint));
                }
                for (id, n) in seen.iter().take(30) {
                    let info = model::node(&ctx.state.graph, id);
                    let label = format!("{} · {n} message{}", info.name, if *n == 1 { "" } else { "s" });
                    let r = ui.add(egui::Button::new(RichText::new(label.clone()).size(12.0)).frame(false));
                    if r.clicked() {
                        ctx.state.selected_node = Some(id.clone());
                        st.history_open = false;
                    }
                    ctx.auto(&format!("mfx.chat.history.node.{id}"), &r, &label);
                }
                heading(ui, ctx, "Questions");
                let mut qs: Vec<(usize, &model::Question, bool)> = msgs
                    .iter()
                    .enumerate()
                    .filter_map(|(i, m)| m.question.as_ref().map(|q| (i, q, q.picked.is_none() && !st.answered.contains_key(&q.id))))
                    .collect();
                qs.sort_by_key(|(_, _, open)| !open);
                if qs.is_empty() {
                    ui.label(RichText::new("No questions yet.").size(12.0).color(t.text_faint));
                }
                for (i, q, open) in qs.into_iter().take(30) {
                    let label = format!("{}{}", if open { "● " } else { "" }, if q.text.is_empty() { "Question" } else { &q.text });
                    let r = ui.add(egui::Button::new(RichText::new(label).size(12.0)).frame(false));
                    if r.clicked() {
                        ctx.state.selected_node = None;
                        st.scroll_to = Some(i);
                        st.history_open = false;
                    }
                    ctx.auto(&format!("mfx.chat.history.q.{}", q.id), &r, &q.text);
                }
            });
        });
    });
    let clicked_out = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p) && !bar.contains(p)));
    if clicked_out || ui.input(|i| i.key_pressed(Key::Escape)) {
        st.history_open = false;
    }
}

fn heading(ui: &mut egui::Ui, ctx: &MfxCtx, text: &str) {
    ui.add_space(6.0);
    ui.label(RichText::new(text).size(11.0).color(ctx.theme.text_faint));
}

fn go_to_message(ctx: &mut MfxCtx, st: &mut ChatUi, m: &Msg, i: usize) {
    ctx.state.selected_node = m.context.clone();
    st.scroll_to = Some(i);
    st.history_open = false;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Mortiflix;

    fn run(m: &mut Mortiflix, frames: usize, mut each: impl FnMut(&mut egui::Context)) -> egui::Context {
        let mut ectx = egui::Context::default();
        for _ in 0..frames {
            each(&mut ectx);
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(440.0, 900.0))), ..Default::default() };
            let mut out = ectx.run_ui(input, |ui| {
                let mut ctx = m.ctx(crate::MfxTheme::default());
                show(ui, &mut ctx);
            });
            out.textures_delta.clear();
        }
        ectx
    }

    #[test]
    fn draws_every_kind_of_message_without_a_service() {
        let mut m = Mortiflix::default();
        m.state.project_id = Some("p1".into());
        for v in [
            json!({"role": "ai", "text": "A 90 second explainer. Shot 7 opens it.", "stage": 0, "at": "2026-10-16T14:04:00",
                "card": {"kind": "script", "beats": [{"id": "b01", "title": "Cold open", "words": "Dartmouth, summer 1956", "start": "0:00"}], "words": 212, "seconds": 90}}),
            json!({"role": "ai", "text": "Narrator voice?", "question": {"id": "q1", "text": "Calm or warm?", "choices": ["Calm", "Warm"], "default": "Warm"}}),
            json!({"role": "ai", "text": "Three looks", "stage": 1, "card": {"kind": "looks", "question": "q2", "looks": [{"id": "a", "name": "Archive grain", "colour": "#806040", "picked": true}, {"id": "b", "name": "Warm paper"}]}}),
            json!({"role": "user", "text": "make it feel colder", "context": "s07", "stage": 2}),
            json!({"role": "ai", "text": "v2 ready", "context": "s07", "card": {"kind": "versions", "node": "s07", "versions": [{"v": 1}, {"v": 2, "inUse": true}, {"v": 3, "making": true, "eta": "about 40 s"}]}}),
            json!({"role": "ai", "text": "Voice is on", "stage": 3, "card": {"kind": "voice", "lines": [{"id": "l01", "beat": "b01", "words": "In the summer of 1956", "seconds": 4.2}]}}),
            json!({"role": "ai", "card": {"kind": "music", "themes": [{"id": "A", "name": "Steady pulse", "bpm": 92, "picked": true}]}}),
            json!({"role": "ai", "card": {"kind": "render", "done": 0.43, "line": "Frame 1,161 / 2,700 · about 4 min left"}}),
            json!({"role": "action", "text": "Couldn't remake shot 7 (E007)", "error": true, "actions": [{"label": "Try again", "type": "node.edit", "params": {"node": "s07", "action": "fix", "text": "again"}}]}),
            json!({"role": "stage", "stage": 4, "text": "14 shots · 12 OK"}),
            json!({"role": "ai", "card": {"kind": "bogus"}, "question": 5}),
        ] {
            m.state.apply_service_message(&json!({"event": "chat.message", "data": v}));
        }
        let c = run(&mut m, 3, |_| {});
        let st: ChatUi = c.data_mut(|d| d.get_temp(ui_id())).unwrap();
        assert!(!st.details_open);
        // node context, then a hostile selection
        m.state.selected_node = Some("s07".into());
        run(&mut m, 2, |_| {});
        m.state.selected_node = Some("\u{0}weird".into());
        run(&mut m, 2, |_| {});
    }

    #[test]
    fn typed_words_on_a_node_become_its_fix_and_ok_asks_first() {
        let mut m = Mortiflix::default();
        m.state.graph = json!({"nodes": {"s03": {"kind": "shot", "owner": "ok"}, "s07": {"kind": "shot"}}});
        let mut ctx = m.ctx(crate::MfxTheme::default());
        let mut st = ChatUi::default();
        let s7 = model::node(&ctx.state.graph, "s07");
        st.draft = "colder".into();
        send(&mut ctx, &mut st, Some(&s7), 0.0);
        assert_eq!(ctx.state.chat.last().map(|c| c.text.as_str()), Some("colder"));
        assert!(st.pending.iter().any(|(_, p, _)| matches!(p, Pending::Fix { node, .. } if node == "s07")));
        // the service is down: the Fix's error shows with Try again
        read_replies(&mut ctx, &mut st, 1.0);
        let last = ctx.state.chat.last().unwrap();
        assert!(last.text.starts_with("Couldn't send the Fix on shot 7"));
        assert!(last.extra.get("actions").is_some());
        // an OK'd shot asks first
        let s3 = model::node(&ctx.state.graph, "s03");
        st.draft = "redo".into();
        send(&mut ctx, &mut st, Some(&s3), 2.0);
        assert_eq!(st.ask_first.as_ref().map(|a| a.node.as_str()), Some("s03"));
        // whole video while offline: queued, not lost
        st.draft = "make it shorter".into();
        send(&mut ctx, &mut st, None, 3.0);
        assert_eq!(st.queued.len(), 1);
        // OK presses merge into one line
        let before = ctx.state.chat.len();
        run_move(&mut ctx, &mut st, &s7, Move::Ok, 4.0);
        let s8 = model::node(&Value::Null, "s08");
        run_move(&mut ctx, &mut st, &s8, Move::Ok, 4.0);
        assert_eq!(ctx.state.chat.len(), before + 1);
        assert_eq!(ctx.state.chat.last().map(|c| c.text.as_str()), Some("You pressed OK on Shot 7, Shot 8"));
        // pressing it again takes it back
        run_move(&mut ctx, &mut st, &s7, Move::Ok, 5.0);
        assert_eq!(st.decided.get("s07").map(String::as_str), Some(""));
        run_move(&mut ctx, &mut st, &s7, Move::Play, 5.0);
        assert_eq!(ctx.state.play_request.as_deref(), Some("s07"));
    }

    #[test]
    fn drops_and_attachments_follow_the_context() {
        let mut m = Mortiflix::default();
        m.state.push_files("chat.attach", vec!["/a/photo.jpg".into()]);
        m.state.push_files("drop", vec!["/a/logo.png".into()]);
        let mut ctx = m.ctx(crate::MfxTheme::default());
        let mut st = ChatUi::default();
        take_files(&mut ctx, &mut st);
        assert_eq!(st.attachments, vec![Attachment::File("/a/photo.jpg".into())]);
        assert!(ctx.state.chat.last().is_some_and(|c| c.text == "You added 1 file to Assets"));
        ctx.state.selected_node = Some("s01".into());
        ctx.state.push_files("drop", vec!["/a/x.png".into()]);
        take_files(&mut ctx, &mut st);
        assert_eq!(st.attachments.len(), 2);
        ctx.state.show_new_project = true;
        ctx.state.push_files("drop", vec!["/a/y.png".into()]);
        take_files(&mut ctx, &mut st);
        assert_eq!(ctx.state.files_in.len(), 1, "the New project window takes it");
    }
}
