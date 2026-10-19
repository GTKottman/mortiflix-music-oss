//! The chat's data and rules, with no drawing: messages read from [`MfxState::chat`], lookups in
//! the node graph, which messages show in which context (doc 37 "talking about"), stage dividers,
//! links in message text, merged action lines and what typed text becomes. Tested headless.
//!
//! Every value here came from the service: hostile input. Unknown or malformed fields fall back to
//! nothing, never to a panic.
//!
//! [`MfxState::chat`]: crate::state::MfxState::chat

use egui::Color32;
use serde_json::{Map, Value};

use crate::state::ChatMessage;

/// Longest text taken from one field (labels, names, choices).
const MAX_FIELD: usize = 400;

/// Who wrote a message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The person (`user`, `you`).
    You,
    /// Mortiflix, the AI (`ai`).
    Ai,
    /// A quiet grey line: a button press or what typing became (`system`, `action`).
    Action,
    /// A stage divider with an optional summary (`stage`).
    Stage,
}

impl Role {
    pub fn of(role: &str) -> Self {
        match role {
            "user" | "you" => Role::You,
            "ai" | "assistant" | "mortiflix" => Role::Ai,
            "stage" => Role::Stage,
            _ => Role::Action,
        }
    }
}

/// One choice of a question or a pick card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Choice {
    pub id: String,
    pub label: String,
}

/// A question with chips (doc 37: one open at a time, the default already in use).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Question {
    pub id: String,
    pub text: String,
    pub choices: Vec<Choice>,
    /// The choice in use until the person answers (its id).
    pub default: Option<String>,
    /// What the person picked (its id, or their own words for Custom…).
    pub picked: Option<String>,
    /// Offer Custom… (own words). On unless the service says `custom: false`.
    pub custom: bool,
}

impl Question {
    pub fn label_of(&self, id: &str) -> String {
        self.choices.iter().find(|c| c.id == id).map_or_else(|| id.to_string(), |c| c.label.clone())
    }
}

/// What a button on a message or card does: a service request or an app command.
#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    Request { ty: String, params: Value },
    Command { id: String, params: Value },
}

/// A button the service put on a message or card (`actions: [{label, type?, params?, command?}]`).
#[derive(Clone, Debug, PartialEq)]
pub struct ActionButton {
    pub label: String,
    pub act: Act,
    pub primary: bool,
}

/// A link in a message: `text` as it appears, pointing at node `id`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ref {
    pub id: String,
    pub text: String,
}

/// A chat message with its extra fields read (see protocol.md, `chat.message`).
#[derive(Clone, Debug, PartialEq)]
pub struct Msg {
    pub role: Role,
    pub text: String,
    pub at: String,
    /// Stage it belongs to (0-5), when the service said.
    pub stage: Option<u8>,
    /// The node or beat it is about; None = the whole video.
    pub context: Option<String>,
    pub card: Option<Value>,
    pub question: Option<Question>,
    pub refs: Vec<Ref>,
    pub actions: Vec<ActionButton>,
    /// A failed remake or other error: red rule.
    pub error: bool,
    /// Something only the person can fix (a missing key): red rule and one button.
    pub needs_you: bool,
    /// The edit request it is about (E007).
    pub edit: Option<String>,
    /// Attached files (names) shown as chips under your message.
    pub files: Vec<String>,
}

fn s(v: &Value, max: usize) -> Option<String> {
    let t = v.as_str()?.trim();
    if t.is_empty() { None } else { Some(t.chars().take(max).collect()) }
}

fn field(m: &Map<String, Value>, key: &str) -> Option<String> {
    m.get(key).and_then(|v| s(v, MAX_FIELD))
}

/// A stage given as a number (0-7) or a key (`styleFrames`).
pub fn stage_of(v: &Value) -> Option<u8> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|n| u8::try_from(n).ok()).filter(|n| usize::from(*n) < crate::STAGES.len()),
        Value::String(k) => crate::stage_by_key(k),
        _ => None,
    }
}

fn context_of(v: Option<&Value>) -> Option<String> {
    let c = s(v?, 120)?;
    if c == "video" || c == "whole" { None } else { Some(c) }
}

fn choice_of(v: &Value) -> Option<Choice> {
    match v {
        Value::String(_) => s(v, MAX_FIELD).map(|l| Choice { id: l.clone(), label: l }),
        Value::Object(o) => {
            let label = field(o, "label").or_else(|| field(o, "name")).or_else(|| field(o, "id"))?;
            let id = field(o, "id").unwrap_or_else(|| label.clone());
            Some(Choice { id, label })
        }
        _ => None,
    }
}

/// A question from `{id, text, choices, default?, picked?, custom?}` (choices: strings or
/// `{id, label}`).
pub fn question_of(v: &Value) -> Option<Question> {
    let o = v.as_object()?;
    let id = field(o, "id")?;
    let choices: Vec<Choice> = o.get("choices").and_then(Value::as_array).map(|a| a.iter().take(12).filter_map(choice_of).collect()).unwrap_or_default();
    let pick = |k: &str| o.get(k).and_then(choice_of).map(|c| c.id);
    Some(Question {
        id,
        text: field(o, "text").unwrap_or_default(),
        default: pick("default"),
        picked: pick("picked"),
        custom: o.get("custom").and_then(Value::as_bool).unwrap_or(true),
        choices,
    })
}

/// Buttons from `[{label, type?, params?, command?, primary?}]`.
pub fn actions_of(v: Option<&Value>) -> Vec<ActionButton> {
    let Some(a) = v.and_then(Value::as_array) else { return Vec::new() };
    a.iter()
        .take(6)
        .filter_map(|b| {
            let o = b.as_object()?;
            let label = field(o, "label")?;
            let params = o.get("params").filter(|p| p.is_object()).cloned().unwrap_or_else(|| Value::Object(Map::new()));
            let act = if let Some(ty) = field(o, "type").filter(|t| t.len() <= 100) {
                Act::Request { ty, params }
            } else {
                Act::Command { id: field(o, "command").filter(|c| c.len() <= 100)?, params }
            };
            Some(ActionButton { label, act, primary: o.get("primary").and_then(Value::as_bool).unwrap_or(false) })
        })
        .collect()
}

impl Msg {
    pub fn of(m: &ChatMessage) -> Self {
        let x = &m.extra;
        let flag = |k: &str| x.get(k).and_then(Value::as_bool).unwrap_or(false);
        Msg {
            role: Role::of(&m.role),
            text: m.text.clone(),
            at: m.at.clone(),
            stage: x.get("stage").and_then(stage_of),
            context: context_of(x.get("context")),
            card: x.get("card").filter(|c| c.is_object()).cloned(),
            question: x.get("question").and_then(question_of),
            refs: x
                .get("refs")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(40)
                        .filter_map(|r| match r {
                            // the service sends ids; the link text is then the node's label or id
                            Value::String(_) => Some(Ref { id: s(r, 120)?, text: String::new() }),
                            _ => Some(Ref {
                                id: s(r.get("id")?, 120)?,
                                text: r.get("text").or_else(|| r.get("label")).and_then(|t| s(t, 200)).unwrap_or_default(),
                            }),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            actions: actions_of(x.get("actions")),
            error: flag("error"),
            needs_you: flag("needsYou"),
            edit: x.get("edit").and_then(|v| s(v, 40)),
            files: x
                .get("files")
                .and_then(Value::as_array)
                .map(|a| a.iter().take(20).filter_map(|f| s(f, 200)).map(|f| file_name(&f)).collect())
                .unwrap_or_default(),
        }
    }
}

/// The last part of a path (both separators, so a Windows path reads right anywhere).
pub fn file_name(path: &str) -> String {
    path.rsplit(['/', '\\']).find(|p| !p.is_empty()).unwrap_or(path).to_string()
}

// ---------- the node graph ----------

/// What the chat needs to know about one node (or beat, or asset).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NodeInfo {
    pub id: String,
    /// shot, transition, line, cue, beat, frame, asset, …
    pub kind: String,
    /// "Shot 7", "Line 6", "M03 Winter", "Winter" (a beat).
    pub name: String,
    /// What it shows / says, when known.
    pub title: String,
    pub beat: Option<String>,
    pub beat_no: Option<u32>,
    pub colour: Option<Color32>,
    /// The owner's decision: "ok", "cut" or None.
    pub owner: Option<String>,
    pub state: String,
    pub versions: u32,
    pub current: Option<u32>,
}

/// The number in an id: `s07` → 7.
pub fn id_number(id: &str) -> Option<u32> {
    let digits: String = id.chars().skip_while(|c| !c.is_ascii_digit()).take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// The kind an id names by its letter (doc 12 ids): s shot, t transition, l line, m cue, b beat,
/// a asset, f frame.
pub fn kind_of_id(id: &str) -> &'static str {
    if id.starts_with("cut:") {
        return "cut";
    }
    if id.starts_with("stage:") {
        return "stage";
    }
    if id.starts_with("sf") && id.get(2..).is_some_and(|r| r.chars().next().is_some_and(|c| c.is_ascii_digit())) {
        return "frame";
    }
    if id.starts_with('b') && id.contains('-') {
        return "line"; // voice lines are b01-1, b01-2 … (part 08)
    }
    let mut chars = id.chars();
    let (Some(c), Some(d)) = (chars.next(), chars.next()) else { return "" };
    if !d.is_ascii_digit() {
        return "";
    }
    match c.to_ascii_lowercase() {
        's' => "shot",
        't' => "transition",
        'l' => "line",
        'm' => "cue",
        'b' => "beat",
        'a' => "asset",
        'f' => "frame",
        _ => "",
    }
}

/// A beat colour when the graph has none: eight calm hues, by beat number.
pub fn beat_colour(n: u32) -> Color32 {
    const P: [(u8, u8, u8); 8] =
        [(214, 120, 84), (86, 156, 214), (196, 168, 72), (122, 178, 112), (176, 120, 196), (84, 178, 172), (210, 104, 140), (150, 150, 160)];
    let (r, g, b) = P.get(usize::try_from(n.saturating_sub(1) % 8).unwrap_or(0)).copied().unwrap_or((150, 150, 160));
    Color32::from_rgb(r, g, b)
}

/// A colour as `"#rrggbb"` or the service's `{hex, label}`.
pub fn hex_colour(v: &Value) -> Option<Color32> {
    let v = v.get("hex").unwrap_or(v);
    let h = v.as_str()?.trim().trim_start_matches('#');
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let p = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
    Some(Color32::from_rgb(p(0)?, p(2)?, p(4)?))
}

/// The raw node object for `id`: `graph.nodes` as a map (canvas view) or a list (`node.list`).
pub fn raw_node<'a>(graph: &'a Value, id: &str) -> Option<&'a Value> {
    let found = match graph.get("nodes") {
        Some(Value::Object(m)) => m.get(id),
        Some(Value::Array(a)) => a.iter().find(|n| n.get("id").and_then(Value::as_str) == Some(id)),
        _ => None,
    };
    // `graph.get` lists the beats on their own too
    found.or_else(|| graph.get("beats").and_then(Value::as_array).and_then(|a| a.iter().find(|n| n.get("id").and_then(Value::as_str) == Some(id))))
}

/// Every node id in the graph, in its order.
pub fn node_ids(graph: &Value) -> Vec<String> {
    match graph.get("nodes") {
        Some(Value::Object(m)) => m.keys().cloned().collect(),
        Some(Value::Array(a)) => a.iter().filter_map(|n| n.get("id").and_then(Value::as_str).map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

pub fn has_nodes(graph: &Value) -> bool {
    graph.get("nodes").is_some_and(|n| n.as_object().is_some_and(|m| !m.is_empty()) || n.as_array().is_some_and(|a| !a.is_empty()))
}

/// The display name of a node of `kind` with this id, label and title.
fn display_name(kind: &str, id: &str, label: &str, title: &str) -> String {
    let n = id_number(id);
    match (kind, n) {
        ("shot", Some(n)) => format!("Shot {n}"),
        ("line", Some(n)) if !id.contains('-') => format!("Line {n}"),
        ("line", _) if !title.is_empty() => format!("Line \u{201c}{}\u{201d}", title.chars().take(24).collect::<String>()),
        ("transition", Some(n)) => format!("Transition {n}"),
        ("cut", _) => {
            let mut parts = id.split(':').skip(1).map(|p| id_number(p).map_or_else(|| p.to_uppercase(), |n| n.to_string()));
            match (parts.next(), parts.next()) {
                (Some(a), Some(b)) => format!("The cut {a} › {b}"),
                _ => "A cut".to_string(),
            }
        }
        ("stage", _) => format!("The {} stage", id.trim_start_matches("stage:")),
        ("cue", _) if !label.is_empty() && !title.is_empty() => format!("{label} {title}"),
        ("beat", _) if !title.is_empty() => title.to_string(),
        ("beat", Some(n)) => format!("Beat {n}"),
        ("frame" | "asset", _) if !title.is_empty() => title.to_string(),
        _ if !label.is_empty() => label.to_string(),
        _ => id.to_uppercase(),
    }
}

/// The `context` sent with `chat.context` / `chat.send` for a selection: the whole video, a piece id,
/// a cut without a transition (`cut:s02:s03`) or a stage (`stage:look`), lower case. Anything else
/// (a stale or odd selection) is the whole video, so a send never fails on what is being talked about.
pub fn context_param(sel: Option<&str>) -> String {
    let Some(c) = sel.map(|c| c.trim().to_ascii_lowercase()) else { return "video".into() };
    let piece = |p: &str| {
        let mut ch = p.chars();
        (2..=24).contains(&p.len()) && ch.next().is_some_and(|c| c.is_ascii_lowercase()) && ch.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    let ok = if let Some(rest) = c.strip_prefix("cut:") {
        rest.split_once(':').is_some_and(|(a, b)| piece(a) && piece(b))
    } else if let Some(st) = c.strip_prefix("stage:") {
        (3..=20).contains(&st.len()) && st.chars().all(|c| c.is_ascii_lowercase())
    } else {
        piece(&c)
    };
    if ok { c } else { "video".into() }
}

/// Everything known about `id`: from the graph when it is there, else from the id alone (the chat
/// works before the node area has loaded the graph).
pub fn node(graph: &Value, id: &str) -> NodeInfo {
    let raw = raw_node(graph, id);
    let get = |k: &str| raw.and_then(|r| r.get(k)).and_then(|v| s(v, MAX_FIELD)).unwrap_or_default();
    let kind = Some(get("kind")).filter(|k| !k.is_empty()).unwrap_or_else(|| kind_of_id(id).to_string());
    // a beat is called by its short name (graph.get `name`), never its first words cut off
    let title = if kind == "beat" {
        Some(get("name")).filter(|t| !t.is_empty()).unwrap_or_else(|| get("title"))
    } else {
        Some(get("title")).filter(|t| !t.is_empty()).unwrap_or_else(|| get("name"))
    };
    let beat = if kind == "beat" { Some(id.to_string()) } else { Some(get("beat")).filter(|b| !b.is_empty()) };
    let field_no = |r: &Value| r.get("beat_no").or_else(|| r.get("no")).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    let beat_no = raw.and_then(field_no).or_else(|| beat.as_deref().and_then(id_number));
    let colour = raw
        .and_then(|r| r.get("colour").or_else(|| r.get("color")))
        .and_then(hex_colour)
        .or_else(|| beat.as_deref().and_then(|b| raw_node(graph, b)).and_then(|b| b.get("colour").or_else(|| b.get("color"))).and_then(hex_colour))
        .or_else(|| beat_no.map(beat_colour));
    let versions = raw.and_then(|r| r.get("versions")).and_then(Value::as_array).map_or(0, |a| u32::try_from(a.len()).unwrap_or(u32::MAX));
    let current = raw
        .and_then(|r| r.get("current"))
        .and_then(|c| c.as_u64().or_else(|| c.as_str().and_then(id_number).map(u64::from)))
        .and_then(|c| u32::try_from(c).ok());
    NodeInfo {
        id: id.to_string(),
        name: display_name(&kind, id, &get("label"), &title),
        title,
        beat,
        beat_no,
        colour,
        owner: Some(get("owner")).filter(|o| !o.is_empty()),
        state: get("state"),
        versions,
        current,
        kind,
    }
}

/// The beats in the graph, in order (for the "Where" chips and crumbs).
pub fn beats(graph: &Value) -> Vec<NodeInfo> {
    let mut ids = node_ids(graph);
    if let Some(a) = graph.get("beats").and_then(Value::as_array) {
        ids.extend(a.iter().take(500).filter_map(|b| b.get("id").and_then(Value::as_str).map(str::to_string)));
    }
    ids.sort();
    ids.dedup();
    let mut v: Vec<NodeInfo> = ids.iter().map(|id| node(graph, id)).filter(|n| n.kind == "beat").collect();
    v.sort_by_key(|b| b.beat_no.unwrap_or(u32::MAX));
    v
}

/// At most `max` characters, with "…" when cut.
pub fn short(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let cut: String = text.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", cut.trim_end())
}

/// "Talking about" crumbs: (selection to go to, label, the node when it is one). The first is
/// always (None, "The whole video").
pub fn crumbs(graph: &Value, sel: Option<&str>) -> Vec<(Option<String>, String, Option<NodeInfo>)> {
    let mut out = vec![(None, "The whole video".to_string(), None)];
    let Some(id) = sel else { return out };
    let info = node(graph, id);
    if info.kind != "beat"
        && let Some(b) = info.beat.as_deref()
    {
        let bi = node(graph, b);
        out.push((Some(b.to_string()), short(&bi.name, 22), Some(bi)));
    }
    out.push((Some(id.to_string()), short(&info.name, 28), Some(info)));
    out
}

// ---------- what shows in which context ----------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Show {
    Full,
    /// Folded into one row ("Shot 7: make it feel colder · 3 messages ›").
    Fold,
    Hide,
}

/// Whether a message about `msg` shows while talking about `view` (doc 37: one node's own thread;
/// in the whole video a node's conversation folds into one row).
pub fn visibility(graph: &Value, msg: Option<&str>, view: Option<&str>) -> Show {
    match (view, msg) {
        (None, None) => Show::Full,
        (None, Some(_)) => Show::Fold,
        (Some(v), Some(m)) if v == m => Show::Full,
        (Some(_), None) => Show::Hide,
        (Some(v), Some(m)) => {
            if node(graph, v).kind == "beat" && node(graph, m).beat.as_deref() == Some(v) {
                Show::Fold
            } else {
                Show::Hide
            }
        }
    }
}

/// One row of the thread as drawn.
#[derive(Clone, Debug, PartialEq)]
pub enum Row {
    /// `── Look 2:06 pm ──`, with the summary of a `stage` message.
    Divider { msg: usize, stage: u8, at: String, summary: String },
    /// Message `i` of the chat, in full.
    Message(usize),
    /// Presses in a row, merged into one quiet line: "You pressed OK on S01, S02" (doc 37).
    Presses { first: usize, verb: String, names: Vec<String> },
    /// Messages about one node, folded: (node, the first of them, how many, the first words you
    /// wrote, the last line).
    Folded { node: String, first: usize, count: usize, said: String, last: String },
}

/// The thread's rows for `view`: dividers where the stage changes, messages in full or folded.
pub fn rows(graph: &Value, msgs: &[Msg], view: Option<&str>) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut stage: Option<u8> = None;
    for (i, m) in msgs.iter().enumerate() {
        let new_stage = m.stage.filter(|s| Some(*s) != stage);
        if m.role == Role::Stage {
            if let Some(st) = m.stage.or(stage) {
                stage = Some(st);
                if view.is_none() {
                    rows.push(Row::Divider { msg: i, stage: st, at: m.at.clone(), summary: m.text.clone() });
                }
            }
            continue;
        }
        if let Some(st) = new_stage {
            stage = Some(st);
            if view.is_none() {
                rows.push(Row::Divider { msg: i, stage: st, at: m.at.clone(), summary: String::new() });
            }
        }
        match visibility(graph, m.context.as_deref(), view) {
            Show::Hide => {}
            Show::Full => {
                let press = if m.role == Role::Action { press_of(&m.text) } else { None };
                match (press, rows.last_mut()) {
                    (Some((verb, what)), Some(Row::Presses { verb: v, names, .. })) if v == verb => {
                        if !names.iter().any(|n| n == what) && names.len() < 200 {
                            names.push(what.to_string());
                        }
                    }
                    (Some((verb, what)), _) => rows.push(Row::Presses { first: i, verb: verb.to_string(), names: vec![what.to_string()] }),
                    (None, _) => rows.push(Row::Message(i)),
                }
            }
            Show::Fold => {
                let node_id = m.context.clone().unwrap_or_default();
                let short = |t: &str| t.lines().next().unwrap_or("").chars().take(80).collect::<String>();
                if let Some(Row::Folded { node, count, said, last, .. }) = rows.last_mut()
                    && *node == node_id
                {
                    *count += 1;
                    if said.is_empty() && m.role == Role::You {
                        *said = short(&m.text);
                    }
                    *last = short(&m.text);
                } else {
                    let said = if m.role == Role::You { short(&m.text) } else { String::new() };
                    rows.push(Row::Folded { node: node_id, first: i, count: 1, said, last: short(&m.text) });
                }
            }
        }
    }
    rows
}

/// The open questions (not picked), newest last, as (message index, question).
pub fn open_questions<'a>(msgs: &'a [Msg], answered: &std::collections::BTreeMap<String, String>) -> Vec<(usize, &'a Question)> {
    msgs.iter()
        .enumerate()
        .filter_map(|(i, m)| m.question.as_ref().map(|q| (i, q)))
        .filter(|(_, q)| q.picked.is_none() && !answered.contains_key(&q.id))
        .collect()
}

// ---------- text ----------

static LOCAL_OFFSET: std::sync::OnceLock<fn(i64) -> i32> = std::sync::OnceLock::new();

/// The desktop app gives the chat the local UTC offset (seconds at a unix time), so times read in
/// the person's own clock. Without it times show as the service wrote them.
pub fn set_local_offset(f: fn(i64) -> i32) {
    let _ = LOCAL_OFFSET.set(f);
}

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm), with checked inputs.
fn days_from_civil(y: i64, m: i64, d: i64) -> Option<i64> {
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || !(1..=9999).contains(&y) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// (hour, minute) of an ISO time, moved to local time when it says UTC (`Z` or `+hh:mm`).
fn local_hm(at: &str) -> Option<(i64, i64)> {
    let (date, time) = at.split_once(['T', ' '])?;
    let num = |s: Option<&str>| s.and_then(|x| x.parse::<i64>().ok());
    let mut dp = date.split('-');
    let (y, mo, d) = (num(dp.next()), num(dp.next()), num(dp.next()));
    let h = num(time.get(0..2))?;
    let mi = num(time.get(3..5))?;
    if !(0..24).contains(&h) || !(0..60).contains(&mi) {
        return None;
    }
    let tail = time.get(5..).unwrap_or("");
    let zone = if tail.ends_with('Z') {
        Some(0)
    } else {
        tail.rfind(['+', '-']).and_then(|i| {
            let sign = if tail.get(i..=i) == Some("-") { -1 } else { 1 };
            let z = tail.get(i + 1..)?;
            Some(sign * (num(z.get(0..2))? * 3600 + num(z.get(3..5)).unwrap_or(0) * 60))
        })
    };
    let (Some(zone), Some(f), Some(y), Some(mo), Some(d)) = (zone, LOCAL_OFFSET.get(), y, mo, d) else { return Some((h, mi)) };
    let unix = days_from_civil(y, mo, d)? * 86_400 + h * 3600 + mi * 60 - zone;
    let local = unix + i64::from(f(unix));
    let sod = local.rem_euclid(86_400);
    Some((sod / 3600, (sod % 3600) / 60))
}

/// (month, day) of a UTC ISO time (`…Z`) in local time (the token panel's run dates: a run at 01:51Z is
/// the evening before for someone west of UTC). The UTC date without the app's offset.
pub fn local_month_day(at: &str) -> Option<(i64, i64)> {
    month_day_in(at, LOCAL_OFFSET.get().copied())
}

fn month_day_in(at: &str, offset: Option<fn(i64) -> i32>) -> Option<(i64, i64)> {
    let (date, time) = at.split_once(['T', ' '])?;
    let num = |s: Option<&str>| s.and_then(|x| x.parse::<i64>().ok());
    let mut dp = date.split('-');
    let (y, mo, d) = (num(dp.next())?, num(dp.next())?, num(dp.next())?);
    let days = days_from_civil(y, mo, d)?;
    let (Some(f), Some(h), Some(mi)) = (offset, num(time.get(0..2)), num(time.get(3..5))) else { return Some((mo, d)) };
    if !time.ends_with('Z') || !(0..24).contains(&h) || !(0..60).contains(&mi) {
        return Some((mo, d));
    }
    let unix = days * 86_400 + h * 3600 + mi * 60;
    let local_days = (unix + i64::from(f(unix))).div_euclid(86_400);
    // civil from days (Howard Hinnant's algorithm)
    let z = local_days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    Some((month, day))
}

/// "14:04" in an ISO time → "2:04 pm" (local time when the app gave the offset). Empty when it has
/// no time.
pub fn clock(at: &str) -> String {
    let Some((h, m)) = local_hm(at) else { return String::new() };
    let (h12, ap) = match h {
        0 => (12, "am"),
        1..=11 => (h, "am"),
        12 => (12, "pm"),
        _ => (h - 12, "pm"),
    };
    format!("{h12}:{m:02} {ap}")
}

/// A piece of message text: plain, or a link to a node.
#[derive(Clone, Debug, PartialEq)]
pub struct Seg {
    pub text: String,
    pub link: Option<String>,
}

/// Split `text` into plain pieces and links: the message's own refs, then "shot N" / "line N" when
/// that node exists (or there is no graph yet).
pub fn segments(text: &str, refs: &[Ref], graph: &Value) -> Vec<Seg> {
    // (start, end, id) of every link, no overlaps
    let mut spans: Vec<(usize, usize, String)> = Vec::new();
    let free = |spans: &[(usize, usize, String)], a: usize, b: usize| spans.iter().all(|(x, y, _)| b <= *x || a >= *y);
    for r in refs {
        let candidates = if r.text.is_empty() {
            let label = raw_node(graph, &r.id).and_then(|n| n.get("label")).and_then(|l| s(l, 80)).unwrap_or_default();
            vec![label, r.id.to_uppercase(), r.id.clone()]
        } else {
            vec![r.text.clone()]
        };
        let Some((a, len)) = candidates.iter().filter(|c| !c.is_empty()).find_map(|c| text.find(c.as_str()).map(|a| (a, c.len()))) else { continue };
        if free(&spans, a, a + len) {
            spans.push((a, a + len, r.id.clone()));
        }
    }
    let lower = text.to_ascii_lowercase();
    let any_graph = has_nodes(graph);
    for (word, letter) in [("shot ", 's'), ("line ", 'l')] {
        let mut from = 0;
        while let Some(off) = lower.get(from..).and_then(|rest| rest.find(word)) {
            let a = from + off;
            let num_start = a + word.len();
            let digits: String = lower.get(num_start..).unwrap_or("").chars().take_while(char::is_ascii_digit).take(3).collect();
            from = num_start;
            let word_start = a == 0 || !lower.as_bytes().get(a - 1).is_some_and(u8::is_ascii_alphanumeric);
            if digits.is_empty() || !word_start {
                continue;
            }
            let b = num_start + digits.len();
            let Ok(n) = digits.parse::<u32>() else { continue };
            let id = format!("{letter}{n:02}");
            if (!any_graph || raw_node(graph, &id).is_some()) && free(&spans, a, b) {
                spans.push((a, b, id));
            }
        }
    }
    spans.sort_by_key(|s| s.0);
    let mut out = Vec::new();
    let mut at = 0;
    for (a, b, id) in spans {
        if a > at
            && let Some(t) = text.get(at..a)
        {
            out.push(Seg { text: t.to_string(), link: None });
        }
        if let Some(t) = text.get(a..b) {
            out.push(Seg { text: t.to_string(), link: Some(id) });
        }
        at = b;
    }
    if let Some(t) = text.get(at..).filter(|t| !t.is_empty()) {
        out.push(Seg { text: t.to_string(), link: None });
    }
    out
}

/// A press line from the service ("OK on S01", "Cut it on S07"): (verb, what). Fix lines (with
/// words) and anything else are not presses.
pub fn press_of(text: &str) -> Option<(&str, &str)> {
    let (verb, what) = text.split_once(" on ")?;
    (matches!(verb, "OK" | "Cut it") && !what.is_empty() && !what.contains(':') && !what.contains(' ')).then_some((verb, what))
}

// ---------- moves ----------

/// A main move (doc 37 "Moves row"; the same as the node's buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Ok,
    Fix,
    NewTake,
    ChangeIt,
    Cut,
    Play,
}

impl Move {
    pub fn label(self, kind: &str) -> &'static str {
        match self {
            Move::Ok => "OK",
            Move::Fix => "Fix",
            Move::NewTake => "New take",
            Move::ChangeIt => "Change it",
            Move::Cut => "Cut it",
            Move::Play => match kind {
                "transition" => "▶ Play the cut",
                "beat" => "▶ Play this beat",
                _ => "▶ Play it",
            },
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Move::Ok => "ok",
            Move::Fix => "fix",
            Move::NewTake => "newTake",
            Move::ChangeIt => "change",
            Move::Cut => "cut",
            Move::Play => "play",
        }
    }
    /// The words the move writes into the chat ("You pressed OK on …").
    pub fn verb(self) -> &'static str {
        match self {
            Move::Ok => "OK",
            Move::Cut => "Cut it",
            Move::Fix => "Fix",
            Move::NewTake => "New take",
            Move::ChangeIt => "Change it",
            Move::Play => "Play",
        }
    }
    /// Asks for words first (focuses the composer).
    pub fn needs_words(self) -> bool {
        matches!(self, Move::Fix | Move::NewTake | Move::ChangeIt)
    }
}

/// The moves of a node kind (doc 37: shot OK · Fix · Cut it · Play it, …).
pub fn moves(kind: &str) -> &'static [Move] {
    match kind {
        "shot" => &[Move::Ok, Move::Fix, Move::Cut, Move::Play],
        "transition" => &[Move::ChangeIt, Move::Play],
        "line" => &[Move::Ok, Move::NewTake, Move::Cut, Move::Play],
        "cue" => &[Move::Ok, Move::Fix, Move::Play],
        "frame" => &[Move::Ok, Move::Fix],
        "beat" => &[Move::Play],
        _ => &[],
    }
}

/// The move typed words become on this node: Fix (a line: New take, a transition: Change it).
pub fn typed_move(kind: &str) -> Move {
    match kind {
        "line" => Move::NewTake,
        "transition" => Move::ChangeIt,
        _ => Move::Fix,
    }
}

/// "shot 7" in running text: the name with a lower-case first word.
pub fn lower_name(name: &str) -> String {
    let mut c = name.chars();
    match c.next() {
        Some(f) if name.split(' ').nth(1).is_some_and(|w| w.chars().all(|c| c.is_ascii_digit())) => f.to_lowercase().chain(c).collect(),
        _ => name.to_string(),
    }
}

/// The composer's placeholder for a context.
pub fn placeholder(info: Option<&NodeInfo>) -> String {
    match info {
        None => "Talk to Mortiflix about the whole video…".into(),
        Some(n) if n.kind == "beat" => format!("Talk to Mortiflix about {}…", n.name),
        Some(n) if moves(&n.kind).iter().any(|m| m.needs_words()) => format!("What should change in {}?", lower_name(&n.name)),
        Some(n) => format!("Talk to Mortiflix about {}…", lower_name(&n.name)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(v: Value) -> Msg {
        Msg::of(&serde_json::from_value::<ChatMessage>(v).unwrap())
    }

    fn graph() -> Value {
        json!({"nodes": {
            "b03": {"kind": "beat", "title": "Winter", "colour": "#5797ec"},
            "s07": {"kind": "shot", "label": "S07", "title": "An empty lab", "beat": "b03", "owner": "ok", "versions": [1, 2], "current": 2},
            "s08": {"kind": "shot", "beat": "b04"},
            "l06": {"kind": "line", "beat": "b03"},
            "m03": {"kind": "cue", "label": "M03", "title": "Winter"}
        }})
    }

    #[test]
    fn messages_read_their_extras_and_survive_junk() {
        let m = msg(json!({"role": "ai", "text": "Pick one", "stage": "look", "context": "s07", "edit": "E007",
            "question": {"id": "q1", "text": "Voice?", "choices": ["Calm", {"id": "w", "label": "Warm"}, 5], "default": "w"},
            "refs": [{"id": "s07", "text": "Shot 7"}, {"id": 3}], "actions": [{"label": "Try again", "type": "node.edit", "params": {"node": "s07"}}, {"label": "x"}],
            "files": ["/home/a/b/photo.jpg", "C:\\x\\logo.png"]}));
        assert_eq!((m.role, m.stage, m.context.as_deref(), m.edit.as_deref()), (Role::Ai, Some(crate::STAGE_STYLE_FRAMES), Some("s07"), Some("E007")));
        let q = m.question.unwrap();
        assert_eq!((q.choices.len(), q.default.as_deref(), q.custom), (2, Some("w"), true));
        assert_eq!(q.label_of("w"), "Warm");
        assert_eq!(m.refs.len(), 1);
        assert_eq!(m.actions.len(), 1);
        assert_eq!(m.files, vec!["photo.jpg", "logo.png"]);
        let junk = msg(json!({"role": "robot", "text": "hi", "stage": 99, "context": ["x"], "card": "no", "question": {"text": "no id"}, "actions": "x"}));
        assert_eq!((junk.role, junk.stage, junk.context, junk.card, junk.question), (Role::Action, None, None, None, None));
        assert_eq!(msg(json!({"role": "user", "context": "video"})).context, None);
    }

    #[test]
    fn nodes_come_from_the_graph_or_their_id() {
        let g = graph();
        let s7 = node(&g, "s07");
        assert_eq!((s7.name.as_str(), s7.beat_no, s7.owner.as_deref(), s7.versions, s7.current), ("Shot 7", Some(3), Some("ok"), 2, Some(2)));
        assert_eq!(s7.colour, Some(Color32::from_rgb(0x57, 0x97, 0xec)));
        assert_eq!(node(&g, "m03").name, "M03 Winter");
        assert_eq!(node(&g, "b03").name, "Winter");
        let bare = node(&Value::Null, "l12");
        assert_eq!((bare.kind.as_str(), bare.name.as_str()), ("line", "Line 12"));
        assert_eq!(node(&json!({"nodes": [{"id": "t02", "kind": "transition"}]}), "t02").name, "Transition 2");
        assert_eq!(node(&Value::Null, "x").name, "X");
        assert_eq!(beats(&g).len(), 1);
        let c = crumbs(&g, Some("s07"));
        assert_eq!(c.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(), ["The whole video", "Winter", "Shot 7"]);
        assert_eq!(crumbs(&g, None).len(), 1);
        assert_eq!(short("Three ideas, one per beat, each", 12), "Three ideas…");
        assert_eq!(hex_colour(&json!("#zzzzzz")), None);
        assert_eq!(hex_colour(&json!("#é1234")), None);
    }

    #[test]
    fn rows_fold_node_threads_and_divide_stages() {
        let g = graph();
        let msgs: Vec<Msg> = [
            json!({"role": "ai", "text": "Script ready", "stage": 0, "at": "2026-10-16T14:04:00"}),
            json!({"role": "ai", "text": "Three looks", "stage": 1}),
            json!({"role": "stage", "text": "14 shots", "stage": 2}),
            json!({"role": "user", "text": "make it feel colder", "context": "s07"}),
            json!({"role": "action", "text": "Sent as Fix on shot 7", "context": "s07"}),
            json!({"role": "ai", "text": "v2 ready", "context": "s07"}),
            json!({"role": "ai", "text": "about line 6", "context": "l06"}),
            json!({"role": "ai", "text": "whole again"}),
        ]
        .into_iter()
        .map(msg)
        .collect();
        let r = rows(&g, &msgs, None);
        assert_eq!(r[0], Row::Divider { msg: 0, stage: 0, at: "2026-10-16T14:04:00".into(), summary: String::new() });
        assert!(matches!(&r[2], Row::Divider { stage: 1, .. }));
        assert!(matches!(&r[4], Row::Divider { stage: 2, summary, .. } if summary == "14 shots"));
        assert_eq!(r[5], Row::Folded { node: "s07".into(), first: 3, count: 3, said: "make it feel colder".into(), last: "v2 ready".into() });
        assert!(matches!(&r[6], Row::Folded { node, count: 1, .. } if node == "l06"));
        assert_eq!(r[7], Row::Message(7));
        assert_eq!(r.len(), 8);
        // one shot's own thread
        assert_eq!(rows(&g, &msgs, Some("s07")), vec![Row::Message(3), Row::Message(4), Row::Message(5)]);
        // a beat: its nodes fold
        assert_eq!(rows(&g, &msgs, Some("b03")).len(), 2);
        let mut answered = std::collections::BTreeMap::new();
        let qs =
            vec![msg(json!({"role": "ai", "question": {"id": "q1", "choices": ["a"]}})), msg(json!({"role": "ai", "question": {"id": "q2", "picked": "a"}}))];
        assert_eq!(open_questions(&qs, &answered).len(), 1);
        answered.insert("q1".into(), "a".into());
        assert!(open_questions(&qs, &answered).is_empty());
    }

    #[test]
    fn run_dates_in_local_time() {
        fn central(_: i64) -> i32 {
            -5 * 3600
        }
        fn tokyo(_: i64) -> i32 {
            9 * 3600
        }
        // 01:51 UTC on Oct 18 is the evening of Oct 17 in Texas, still Oct 18 in Tokyo
        assert_eq!(month_day_in("2026-10-18T01:51:43.331Z", Some(central)), Some((10, 17)));
        assert_eq!(month_day_in("2026-10-18T01:51:43.331Z", Some(tokyo)), Some((10, 18)));
        assert_eq!(month_day_in("2026-01-01T02:00:00Z", Some(central)), Some((12, 31)));
        assert_eq!(month_day_in("2026-10-18T01:51:43.331Z", None), Some((10, 18)));
        assert_eq!(month_day_in("junk", Some(central)), None);
    }

    #[test]
    fn text_links_clock_and_merged_presses() {
        let g = graph();
        let segs = segments("Shot 7 is cold; shot 8 and line 6 and shot 99. Upshot 7 no.", &[], &g);
        let links: Vec<_> = segs.iter().filter_map(|s| s.link.clone()).collect();
        assert_eq!(links, ["s07", "s08", "l06"]);
        assert_eq!(segs.iter().map(|s| s.text.as_str()).collect::<String>(), "Shot 7 is cold; shot 8 and line 6 and shot 99. Upshot 7 no.");
        let segs = segments("The M03 Winter music", &[Ref { id: "m03".into(), text: "M03 Winter".into() }], &Value::Null);
        assert_eq!(segs[1], Seg { text: "M03 Winter".into(), link: Some("m03".into()) });
        assert_eq!(segments("", &[], &g), vec![]);
        assert_eq!(segments("ünïcode shot 3 ✓", &[], &Value::Null).len(), 3);
        assert_eq!(clock("2026-10-16T14:04:00.000Z"), "2:04 pm");
        assert_eq!(clock("2026-10-16T00:30"), "12:30 am");
        assert_eq!(clock("nope"), "");
        assert_eq!(clock("x T99:00"), "");
        assert_eq!(days_from_civil(1970, 1, 1), Some(0));
        assert_eq!(days_from_civil(2026, 10, 17), Some(20_743));
        assert_eq!(days_from_civil(2026, 13, 1), None);
        assert_eq!(press_of("OK on S01"), Some(("OK", "S01")));
        assert_eq!(press_of("Fix on S01: colder"), None);
        assert_eq!(press_of("New film order: S01 › S02"), None);
        let presses: Vec<Msg> = ["OK on S01", "OK on S02", "OK on S02", "Cut it on S03"].iter().map(|t| msg(json!({"role": "action", "text": t}))).collect();
        let r = rows(&Value::Null, &presses, None);
        assert_eq!(
            r,
            vec![
                Row::Presses { first: 0, verb: "OK".into(), names: vec!["S01".into(), "S02".into()] },
                Row::Presses { first: 3, verb: "Cut it".into(), names: vec!["S03".into()] }
            ]
        );
    }

    #[test]
    fn moves_and_placeholders_follow_the_kind() {
        assert_eq!(moves("shot"), &[Move::Ok, Move::Fix, Move::Cut, Move::Play]);
        assert_eq!(Move::Play.label("transition"), "▶ Play the cut");
        assert_eq!(typed_move("line"), Move::NewTake);
        assert_eq!(placeholder(None), "Talk to Mortiflix about the whole video…");
        let g = graph();
        assert_eq!(placeholder(Some(&node(&g, "s07"))), "What should change in shot 7?");
        assert_eq!(placeholder(Some(&node(&g, "b03"))), "Talk to Mortiflix about Winter…");
        assert_eq!(lower_name("M03 Winter"), "M03 Winter");
        assert_eq!(id_number("s007x"), Some(7));
        assert_eq!(beat_colour(9), beat_colour(1));
    }

    #[test]
    fn every_talking_about_target_is_a_context_the_service_takes() {
        // the node area's cut pills (no transition yet) were sent as-is and refused (owner's first run)
        assert_eq!(context_param(Some("cut:s02:s03")), "cut:s02:s03");
        assert_eq!(context_param(Some("S03")), "s03");
        assert_eq!(context_param(Some("b01-2")), "b01-2");
        assert_eq!(context_param(Some("stage:Look")), "stage:look");
        assert_eq!(context_param(None), "video");
        for bad in ["../../x", "cut:s02", "", "a", "stage:x", "s01 s02", "\u{0}weird"] {
            assert_eq!(context_param(Some(bad)), "video", "{bad}");
        }
        assert_eq!(node(&Value::Null, "cut:s02:s03").name, "The cut 2 › 3");
        assert_eq!(crumbs(&Value::Null, Some("cut:s02:s03")).len(), 2);
    }
}
