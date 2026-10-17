//! The node graph as the service sends it (`node.list`, doc 12's canvas graph), typed, plus the film
//! chain the node area draws: shots in film order, a cut (with its transition) between every two.
//!
//! Everything here comes from another process and is hostile input: unknown kinds and fields are
//! ignored, strings are capped, numbers are checked, and a node list is capped at [`MAX_NODES`].

use std::collections::HashMap;

use egui::Color32;
use serde_json::Value;

/// Most nodes read from one `node.list` (a 133-shot film has about 600).
pub const MAX_NODES: usize = 5000;
/// Most versions kept per node.
const MAX_VERSIONS: usize = 200;
/// Longest text kept from the service.
const MAX_TEXT: usize = 4000;

/// What a node is (doc 12's kinds; `asset` is new in doc 35: your files as input nodes).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Kind {
    Beat,
    /// A style frame: the look a shot is made in (an input node).
    Frame,
    Shot,
    /// A voice line said over shots.
    Line,
    /// A music cue (one per beat).
    Cue,
    /// The mixed music (one per film).
    Music,
    Transition,
    /// One of your own files (an input node).
    Asset,
    #[default]
    Other,
}

impl Kind {
    pub fn parse(s: &str) -> Kind {
        match s {
            "beat" => Kind::Beat,
            "frame" | "look" => Kind::Frame,
            "shot" => Kind::Shot,
            "line" | "voice" => Kind::Line,
            "cue" => Kind::Cue,
            "music" => Kind::Music,
            "transition" => Kind::Transition,
            "asset" => Kind::Asset,
            _ => Kind::Other,
        }
    }
    /// The word used on screen ("shot", "voice line"…).
    pub fn word(self) -> &'static str {
        match self {
            Kind::Beat => "beat",
            Kind::Frame => "look",
            Kind::Shot => "shot",
            Kind::Line => "voice line",
            Kind::Cue => "music cue",
            Kind::Music => "music",
            Kind::Transition => "transition",
            Kind::Asset => "asset",
            Kind::Other => "piece",
        }
    }
}

/// A node's state on screen (doc 35 "Node states"), mapped from doc 12's five states plus `stale`.
/// Always shown with a glyph and a word, never colour alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum NodeState {
    /// In the script, not started (no version yet).
    Planned,
    /// First version being made.
    Making,
    /// Made, not reviewed.
    #[default]
    Ready,
    /// You approved this version.
    Ok,
    /// Your Fix is being made (the old version stays on screen).
    Remaking,
    /// A fix failed or a question has no default.
    NeedsYou,
    /// Something it was made from changed.
    OutOfDate,
    /// You cut it (nothing is deleted).
    Cut,
}

impl NodeState {
    pub fn glyph(self) -> &'static str {
        match self {
            NodeState::Planned => "·",
            NodeState::Making | NodeState::Remaking => "»",
            NodeState::Ready => "○",
            NodeState::Ok => "✓",
            NodeState::NeedsYou => "!",
            NodeState::OutOfDate => "*",
            NodeState::Cut => "×",
        }
    }
    pub fn word(self) -> &'static str {
        match self {
            NodeState::Planned => "Planned",
            NodeState::Making => "Making…",
            NodeState::Ready => "Ready",
            NodeState::Ok => "OK",
            NodeState::Remaking => "Remaking…",
            NodeState::NeedsYou => "Needs you",
            NodeState::OutOfDate => "Out of date",
            NodeState::Cut => "Cut",
        }
    }
    /// Being made right now (first version or a fix).
    pub fn making(self) -> bool {
        matches!(self, NodeState::Making | NodeState::Remaking)
    }
}

/// One version of a node (doc 12: every version it has had).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Version {
    pub v: u32,
    /// The media file (relative to the project's state folder, or absolute).
    pub file: String,
    /// A still picture of it, when the service made one (else `file` when it is a picture).
    pub still: String,
    pub from_sec: Option<f64>,
    pub to_sec: Option<f64>,
    pub made_at: String,
}

/// One edit request on a node (Fix), for the inspector's History.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Edit {
    pub id: String,
    pub text: String,
    pub state: String,
    pub version: Option<u32>,
}

/// The beat's FilmCraft label colour (fill) and its marker colour (the chip beside the number).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeatColour {
    pub fill: Color32,
    pub chip: Color32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Node {
    pub id: String,
    pub kind: Kind,
    /// Short name (`S04`, `B02`, `b02-1`).
    pub label: String,
    /// The words (a shot's or a line's text, a beat's words, a transition's kind).
    pub title: String,
    /// A beat's short name, when the service sends one ("The bet").
    pub name: String,
    pub beat: Option<String>,
    pub order: i64,
    pub colour: Option<BeatColour>,
    pub state: NodeState,
    pub versions: Vec<Version>,
    pub current: Option<u32>,
    /// Transition: the shots on either side of the cut.
    pub from: Option<String>,
    pub to: Option<String>,
    /// Transition length in seconds.
    pub sec: Option<f64>,
    /// Voice line: where it is said (seconds in the film).
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    /// Music cue: its bars.
    pub bars: Option<[f64; 2]>,
    /// Shot: its look (a frame node id).
    pub look: Option<String>,
    /// Shot: your assets it uses (asset node ids); asset: the shots that use it.
    pub uses: Vec<String>,
    /// Why it needs you (a failed fix's note).
    pub reason: String,
    pub edits: Vec<Edit>,
    /// The text of the Fix being made now.
    pub pending: Option<String>,
    pub notes: Vec<String>,
    /// Gone from the files (ids are forever: it stays, marked missing).
    pub missing: bool,
    /// Where it was dropped on the canvas (saved by the service), world units.
    pub pos: Option<[f32; 2]>,
}

impl Node {
    /// The version shown: `current`, else the last one.
    pub fn shown_version(&self) -> Option<&Version> {
        self.current.and_then(|c| self.versions.iter().find(|v| v.v == c)).or_else(|| self.versions.last())
    }
    /// Length in seconds of the shown version (shots), if known.
    pub fn seconds(&self) -> Option<f64> {
        let v = self.shown_version()?;
        let (a, b) = (v.from_sec?, v.to_sec?);
        (b.is_finite() && a.is_finite() && b > a).then_some(b - a)
    }
    /// The still to show: a picture file of the shown version, if any.
    pub fn still(&self) -> Option<&str> {
        let v = self.shown_version()?;
        if !v.still.is_empty() {
            return Some(&v.still);
        }
        is_picture(&v.file).then_some(v.file.as_str())
    }
    /// `v2/3` when there is more than one version.
    pub fn version_tag(&self) -> Option<String> {
        (self.versions.len() > 1).then(|| format!("v{}/{}", self.current.unwrap_or(0), self.versions.len()))
    }
    /// Shown everywhere a node is named: `#4` for shots (its name, not its position).
    pub fn display_name(&self) -> String {
        match self.kind {
            Kind::Shot => {
                let digits: String = self.id.chars().filter(char::is_ascii_digit).collect();
                let n = digits.trim_start_matches('0');
                if n.is_empty() { format!("#{}", self.label) } else { format!("#{n}") }
            }
            Kind::Line => format!("line {}", self.label.trim_start_matches('b')),
            _ => self.label.clone(),
        }
    }
}

/// A file name that is a picture the node area can show as a still.
pub fn is_picture(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg", ".webp"].iter().any(|e| p.ends_with(e))
}

fn text(v: &Value, key: &str) -> String {
    v.get(key).and_then(Value::as_str).map(|s| s.chars().take(MAX_TEXT).collect()).unwrap_or_default()
}

fn opt_text(v: &Value, key: &str) -> Option<String> {
    let s = text(v, key);
    (!s.is_empty()).then_some(s)
}

fn num(v: &Value, key: &str) -> Option<f64> {
    v.get(key).and_then(Value::as_f64).filter(|f| f.is_finite())
}

/// `#rrggbb` → colour.
pub fn hex_colour(s: &str) -> Option<Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.is_ascii() {
        return None;
    }
    let p = |i: usize| h.get(i..i + 2).and_then(|x| u8::from_str_radix(x, 16).ok());
    Some(Color32::from_rgb(p(0)?, p(2)?, p(4)?))
}

/// A position: `[x, y]` (the service's `node.move` / `graph.get` form) or `{x, y}`.
fn pos_of(v: &Value) -> Option<[f32; 2]> {
    let (x, y) = match v {
        Value::Array(a) if a.len() == 2 => (a.first()?.as_f64()?, a.get(1)?.as_f64()?),
        _ => (v.get("x").and_then(Value::as_f64)?, v.get("y").and_then(Value::as_f64)?),
    };
    let ok = |f: f64| f.is_finite() && f.abs() < 1.0e7;
    #[allow(clippy::cast_possible_truncation)] // bounded above
    (ok(x) && ok(y)).then_some([x as f32, y as f32])
}

/// Map the service's state words (doc 12 + `stale`, `planned`) onto [`NodeState`].
fn state_of(v: &Value, versions: usize) -> NodeState {
    let st = v.get("state").and_then(Value::as_str).unwrap_or("");
    let planned = v.get("planned").and_then(Value::as_bool).unwrap_or(false);
    let pending = v.get("pending").is_some_and(|p| !p.is_null());
    let stale = v.get("stale").and_then(Value::as_bool).unwrap_or(false);
    match st {
        "cut" => NodeState::Cut,
        "needs you" | "needsYou" | "failed" => NodeState::NeedsYou,
        "generating" | "making" if versions > 0 && pending => NodeState::Remaking,
        "generating" | "making" if planned && !pending => NodeState::Planned,
        "generating" | "making" => NodeState::Making,
        "planned" => NodeState::Planned,
        _ if stale => NodeState::OutOfDate,
        "approved" | "ok" => NodeState::Ok,
        _ if planned => NodeState::Planned,
        _ => NodeState::Ready,
    }
}

impl Node {
    /// One node from the service's JSON; None when it has no usable id.
    pub fn from_json(v: &Value) -> Option<Node> {
        let id = text(v, "id");
        if id.is_empty() || id.len() > 200 {
            return None;
        }
        let versions: Vec<Version> = v
            .get("versions")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .take(MAX_VERSIONS)
                    .filter_map(|x| {
                        let n = x.get("v").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())?;
                        Some(Version {
                            v: n,
                            // `path` is absolute (graph.get); `file` is relative to the project's state
                            file: opt_text(x, "path").unwrap_or_else(|| text(x, "file")),
                            still: text(x, "still"),
                            from_sec: num(x, "from_sec"),
                            to_sec: num(x, "to_sec"),
                            made_at: text(x, "made_at"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let colour = v.get("colour").and_then(|c| {
            let fill = hex_colour(c.get("hex").and_then(Value::as_str).unwrap_or(""))?;
            let chip = hex_colour(c.get("chip").and_then(Value::as_str).unwrap_or("")).unwrap_or(fill);
            Some(BeatColour { fill, chip })
        });
        let reason = v
            .get("failed")
            .and_then(|f| f.get("note").or_else(|| f.get("text")))
            .and_then(Value::as_str)
            .map(|s| s.chars().take(MAX_TEXT).collect())
            .or_else(|| opt_text(v, "reason"))
            .unwrap_or_default();
        let edits = v
            .get("edits")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .rev()
                    .take(50)
                    .rev()
                    .map(|e| Edit {
                        id: text(e, "id"),
                        text: text(e, "text"),
                        state: text(e, "state"),
                        version: e.get("version").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let strings = |key: &str| -> Vec<String> {
            v.get(key)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_str).take(200).map(|s| s.chars().take(200).collect()).collect())
                .unwrap_or_default()
        };
        let bars = v.get("bars").and_then(Value::as_array).and_then(|b| {
            let a = b.first()?.as_f64()?;
            let c = b.get(1)?.as_f64()?;
            (a.is_finite() && c.is_finite()).then_some([a, c])
        });
        let pos = v.get("pos").and_then(pos_of);
        let mut uses = strings("uses");
        uses.extend(strings("used_by"));
        Some(Node {
            kind: Kind::parse(v.get("kind").and_then(Value::as_str).unwrap_or("")),
            label: opt_text(v, "label").unwrap_or_else(|| id.to_uppercase()),
            title: text(v, "title"),
            name: text(v, "name"),
            beat: opt_text(v, "beat"),
            order: v.get("order").and_then(Value::as_i64).unwrap_or(i64::MAX / 2),
            colour,
            state: state_of(v, versions.len()),
            // the version that plays (an older one picked with node.pick), else the newest
            current: ["plays", "current"].iter().find_map(|k| v.get(*k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok())),
            versions,
            from: opt_text(v, "from"),
            to: opt_text(v, "to"),
            sec: num(v, "sec").or_else(|| num(v, "length")),
            start_sec: num(v, "start_sec"),
            end_sec: num(v, "end_sec"),
            bars,
            look: opt_text(v, "look"),
            uses,
            reason,
            edits,
            pending: v.get("pending").and_then(|p| p.get("text")).and_then(Value::as_str).map(|s| s.chars().take(MAX_TEXT).collect()),
            notes: strings("notes"),
            missing: v.get("missing").and_then(Value::as_bool).unwrap_or(false),
            pos,
            id,
        })
    }
}

/// A cut between two shots in film order, and the transition on it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    /// Stable id: the transition node's, else `cut:<from>:<to>`.
    pub id: String,
    pub from: String,
    pub to: String,
    /// The transition node on this cut, if one exists.
    pub transition: Option<String>,
    /// No transition picked yet on a film that has them (a new cut after a move).
    pub pick: bool,
}

/// The typed graph plus what the node area derives from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Graph {
    pub nodes: Vec<Node>,
    index: HashMap<String, usize>,
    /// Shot ids in film order (the chain, not their positions).
    pub film: Vec<String>,
    /// Beat ids in script order.
    pub beats: Vec<String>,
    pub cuts: Vec<Cut>,
    /// The film has transitions at all (a transition board was made).
    pub has_board: bool,
    pub open_edits: u32,
    /// The folder relative media paths start from, when the service says.
    pub base: String,
}

impl Graph {
    /// Build from a `node.list` result (`{nodes: [...], open_edits, order?, base?}`) or a raw
    /// `graph.json` (`{nodes: {id: node}}`).
    pub fn from_json(v: &Value) -> Graph {
        let mut nodes: Vec<Node> = match v.get("nodes") {
            Some(Value::Array(a)) => a.iter().take(MAX_NODES).filter_map(Node::from_json).collect(),
            Some(Value::Object(m)) => m
                .iter()
                .take(MAX_NODES)
                .filter_map(|(k, n)| {
                    let mut n = n.clone();
                    if let Some(o) = n.as_object_mut() {
                        o.entry("id").or_insert_with(|| Value::String(k.clone()));
                    }
                    Node::from_json(&n)
                })
                .collect(),
            _ => Vec::new(),
        };
        // your own files (graph.get `assets`) are input nodes too
        if let Some(assets) = v.get("assets").and_then(Value::as_array) {
            for a in assets.iter().take(2000) {
                let Some(id) = a.get("id").and_then(Value::as_str).filter(|s| !s.is_empty() && s.len() <= 200) else { continue };
                let path = text(a, "path");
                let used: Vec<String> = a
                    .get("used_in")
                    .and_then(Value::as_array)
                    .map(|u| u.iter().filter_map(|x| x.get("node").and_then(Value::as_str)).take(200).map(str::to_string).collect())
                    .unwrap_or_default();
                nodes.push(Node {
                    id: id.to_string(),
                    kind: Kind::Asset,
                    label: id.to_uppercase(),
                    title: text(a, "name"),
                    versions: vec![Version { v: 1, still: if is_picture(&path) { path.clone() } else { String::new() }, file: path, ..Version::default() }],
                    current: Some(1),
                    uses: used,
                    pos: a.get("pos").and_then(pos_of),
                    ..Node::default()
                });
            }
        }
        // positions may also come as one map (`layout: {id: {x, y}}`)
        if let Some(layout) = v.get("layout").and_then(Value::as_object) {
            for n in &mut nodes {
                if n.pos.is_none() {
                    n.pos = layout.get(&n.id).and_then(pos_of);
                }
            }
        }
        let mut g = Graph {
            nodes,
            open_edits: v.get("open_edits").and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
            base: text(v, "base"),
            ..Graph::default()
        };
        // dedupe ids (hostile input): the first wins
        let mut seen = std::collections::HashSet::new();
        g.nodes.retain(|n| seen.insert(n.id.clone()));
        g.reindex();
        let explicit: Option<Vec<String>> = v
            .get("order")
            .and_then(|o| o.get("film").or_else(|| o.get("shots")))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect());
        let mut film = explicit.unwrap_or_default();
        let mut in_film = std::collections::HashSet::new();
        film.retain(|id| g.get(id).is_some_and(|n| n.kind == Kind::Shot) && in_film.insert(id.clone()));
        let mut rest: Vec<&Node> = g.nodes.iter().filter(|n| n.kind == Kind::Shot && !n.missing && !in_film.contains(&n.id)).collect();
        rest.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
        for n in rest {
            in_film.insert(n.id.clone());
            film.push(n.id.clone());
        }
        let mut beats: Vec<&Node> = g.nodes.iter().filter(|n| n.kind == Kind::Beat && !n.missing).collect();
        beats.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
        g.beats = beats.iter().map(|n| n.id.clone()).collect();
        // beats named by shots but missing as nodes still get a band
        for id in &film {
            if let Some(b) = g.get(id).and_then(|n| n.beat.clone())
                && !g.beats.contains(&b)
            {
                g.beats.push(b);
            }
        }
        g.has_board = g.nodes.iter().any(|n| n.kind == Kind::Transition && !n.missing);
        g.set_film(film);
        g
    }

    fn reindex(&mut self) {
        self.index = self.nodes.iter().enumerate().map(|(i, n)| (n.id.clone(), i)).collect();
    }

    pub fn get(&self, id: &str) -> Option<&Node> {
        self.index.get(id).and_then(|&i| self.nodes.get(i))
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut Node> {
        let i = *self.index.get(id)?;
        self.nodes.get_mut(i)
    }

    /// Set the film order (shot ids) and rebuild the cuts between them. Cut shots stay in the
    /// order (a sliver where they were) but the film goes around them.
    pub fn set_film(&mut self, film: Vec<String>) {
        self.film = film;
        let live: Vec<&String> = self.film.iter().filter(|id| self.get(id).is_some_and(|n| n.state != NodeState::Cut)).collect();
        // The AI's own order of the shots: a cut it made the board for (its two shots side by side
        // there) and left without a transition is its straight cut, not a choice left to you.
        let mut ai: Vec<(&String, i64)> = live.iter().filter_map(|id| self.get(id).map(|n| (*id, n.order))).collect();
        ai.sort_by_key(|(_, o)| *o);
        let ai_next: HashMap<&str, &str> = ai.windows(2).filter_map(|w| Some((w.first()?.0.as_str(), w.get(1)?.0.as_str()))).collect();
        let mut cuts = Vec::new();
        for w in live.windows(2) {
            let (Some(a), Some(b)) = (w.first(), w.get(1)) else { continue };
            let t = self
                .nodes
                .iter()
                .find(|n| n.kind == Kind::Transition && !n.missing && n.from.as_deref() == Some(a.as_str()) && n.to.as_deref() == Some(b.as_str()));
            cuts.push(Cut {
                id: t.map_or_else(|| format!("cut:{a}:{b}"), |t| t.id.clone()),
                from: (*a).clone(),
                to: (*b).clone(),
                transition: t.map(|t| t.id.clone()),
                pick: t.is_none() && self.has_board && ai_next.get(a.as_str()) != Some(&b.as_str()),
            });
        }
        self.cuts = cuts;
    }

    /// Position of a shot in film order.
    pub fn film_index(&self, id: &str) -> Option<usize> {
        self.film.iter().position(|s| s == id)
    }

    /// The beat's number (1-based, script order) and short name ("The bet").
    pub fn beat_title(&self, beat: &str) -> (usize, String) {
        let n = self.beats.iter().position(|b| b == beat).map_or(0, |i| i + 1);
        let name = self.get(beat).map(|b| if b.name.is_empty() { short_name(&b.title) } else { b.name.clone() }).unwrap_or_default();
        (n, name)
    }

    /// Shots of a beat in film order.
    pub fn beat_shots(&self, beat: &str) -> Vec<&Node> {
        self.film.iter().filter_map(|id| self.get(id)).filter(|n| n.beat.as_deref() == Some(beat)).collect()
    }

    /// The beat colour of a node: its own, else its beat node's.
    pub fn colour_of(&self, n: &Node) -> Option<BeatColour> {
        n.colour.or_else(|| n.beat.as_deref().and_then(|b| self.get(b)).and_then(|b| b.colour))
    }

    /// The shots a voice line is said over: its beat's shots, the k-th line under the k-th shot.
    pub fn line_shot(&self, line: &Node) -> Option<String> {
        let beat = line.beat.as_deref()?;
        let shots = self.beat_shots(beat);
        let mut lines: Vec<&Node> = self.nodes.iter().filter(|n| n.kind == Kind::Line && n.beat.as_deref() == Some(beat) && !n.missing).collect();
        lines.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.id.cmp(&b.id)));
        let k = lines.iter().position(|l| l.id == line.id)?;
        shots.get(k.min(shots.len().saturating_sub(1))).map(|s| s.id.clone())
    }

    /// Move `node` (a shot) to right after `after` in film order (`None` = first). False when
    /// either is not a shot or nothing changes.
    pub fn reorder(&mut self, node: &str, after: Option<&str>) -> bool {
        if after == Some(node) || self.film_index(node).is_none() || after.is_some_and(|a| self.film_index(a).is_none()) {
            return false;
        }
        let before = self.film.clone();
        let mut film = self.film.clone();
        film.retain(|s| s != node);
        let at = match after {
            Some(a) => film.iter().position(|s| s == a).map_or(film.len(), |i| i + 1),
            None => 0,
        };
        film.insert(at.min(film.len()), node.to_string());
        if film == before {
            return false;
        }
        self.set_film(film);
        true
    }

    /// The shot before `node` in film order (None = it is first).
    pub fn film_prev(&self, node: &str) -> Option<String> {
        let i = self.film_index(node)?;
        i.checked_sub(1).and_then(|j| self.film.get(j)).cloned()
    }

    /// Counts for the panel header: shots, OK, needs you, making, transitions to pick.
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for n in self.film.iter().filter_map(|id| self.get(id)) {
            if n.state == NodeState::Cut {
                continue;
            }
            c.shots += 1;
            match n.state {
                NodeState::Ok => c.ok += 1,
                NodeState::NeedsYou => c.need += 1,
                s if s.making() => c.making += 1,
                _ => {}
            }
        }
        c.pick = self.cuts.iter().filter(|k| k.pick).count();
        c
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub shots: usize,
    pub ok: usize,
    pub need: usize,
    pub making: usize,
    pub pick: usize,
}

impl Counts {
    /// `14 shots · 3 OK · 1 needs you · 3 making · 1 transition to pick`
    pub fn line(&self) -> String {
        let mut parts = vec![if self.shots == 1 { "1 shot".to_string() } else { format!("{} shots", self.shots) }];
        if self.ok > 0 {
            parts.push(format!("{} OK", self.ok));
        }
        if self.need > 0 {
            parts.push(if self.need == 1 { "1 needs you".into() } else { format!("{} need you", self.need) });
        }
        if self.making > 0 {
            parts.push(format!("{} making", self.making));
        }
        if self.pick > 0 {
            parts.push(if self.pick == 1 { "1 transition to pick".into() } else { format!("{} transitions to pick", self.pick) });
        }
        parts.join(" · ")
    }
}

/// A beat's short name from its words: the first few words ("The bet that…" → "The bet that").
pub fn short_name(words: &str) -> String {
    let w: Vec<&str> = words.split_whitespace().take(4).collect();
    let mut s = w.join(" ");
    s = s.trim_end_matches([',', '.', ';', ':', '—', '-']).to_string();
    if words.split_whitespace().count() > 4 {
        s.push('…');
    }
    s
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::json;

    /// A small film: 2 beats, 4 shots, 1 transition, 2 lines, 1 cue, 1 look.
    pub fn sample() -> Value {
        json!({"open_edits": 1, "nodes": [
            {"id": "b01", "kind": "beat", "label": "B01", "beat": "b01", "order": 0, "title": "Cold open on a winter road", "colour": {"hex": "#380ea7", "chip": "#a98caf"}},
            {"id": "b02", "kind": "beat", "label": "B02", "beat": "b02", "order": 1, "title": "The bet", "name": "The bet", "colour": {"hex": "#803f17", "chip": "#da7639"}},
            {"id": "sf01", "kind": "frame", "label": "SF01", "beat": "b01", "order": 0, "versions": [{"v": 1, "file": "canvas/media/sf01/a.png"}], "current": 1, "state": "ready"},
            {"id": "s01", "kind": "shot", "label": "S01", "title": "Road", "beat": "b01", "order": 0, "look": "sf01", "versions": [{"v": 1, "file": "v.mp4", "from_sec": 0, "to_sec": 4}], "current": 1, "state": "approved"},
            {"id": "s02", "kind": "shot", "label": "S02", "title": "Car", "beat": "b01", "order": 1, "versions": [{"v": 1, "file": "v.mp4", "from_sec": 4, "to_sec": 7.5}, {"v": 2, "file": "s02.mp4", "from_sec": 0, "to_sec": 3}], "current": 2, "state": "ready"},
            {"id": "s03", "kind": "shot", "label": "S03", "title": "Men at a table", "beat": "b02", "order": 2, "versions": [], "planned": true, "state": "generating"},
            {"id": "s04", "kind": "shot", "label": "S04", "title": "Handshake", "beat": "b02", "order": 3, "versions": [{"v": 1, "file": "v.mp4"}], "current": 1, "state": "needs you", "failed": {"note": "the render failed"}},
            {"id": "t01", "kind": "transition", "label": "T01", "title": "Crossfade", "from": "s01", "to": "s02", "order": 0, "sec": 0.5},
            {"id": "b01-1", "kind": "line", "label": "b01-1", "title": "It was cold.", "beat": "b01", "order": 0},
            {"id": "b02-1", "kind": "line", "label": "b02-1", "title": "They bet.", "beat": "b02", "order": 1},
            {"id": "m01", "kind": "cue", "label": "M01", "title": "Cold open", "beat": "b01", "order": 0, "bars": [1, 8]},
            {"id": "x", "kind": "shot"},
            {"id": "", "kind": "shot"},
            7, "junk"
        ]})
    }

    #[test]
    fn parses_nodes_states_and_the_film_chain() {
        let g = Graph::from_json(&sample());
        assert_eq!(g.film, ["s01", "s02", "s03", "s04", "x"]);
        assert_eq!(g.beats, ["b01", "b02"]);
        assert_eq!(g.get("s01").map(|n| n.state), Some(NodeState::Ok));
        assert_eq!(g.get("s03").map(|n| n.state), Some(NodeState::Planned));
        assert_eq!(g.get("s04").map(|n| (n.state, n.reason.as_str())), Some((NodeState::NeedsYou, "the render failed")));
        assert_eq!(g.get("s02").and_then(Node::version_tag).as_deref(), Some("v2/2"));
        assert_eq!(g.get("s02").and_then(Node::seconds), Some(3.0));
        assert_eq!(g.get("s04").map(Node::display_name).as_deref(), Some("#4"));
        assert_eq!(g.get("sf01").and_then(Node::still), Some("canvas/media/sf01/a.png"));
        assert!(g.has_board);
        assert_eq!(g.cuts.len(), 4);
        assert_eq!(g.cuts.first().and_then(|c| c.transition.as_deref()), Some("t01"));
        // the AI made the board with s02 next to s03 and no transition there: its straight cut, not "Pick one"
        assert!(g.cuts.get(1).is_some_and(|c| !c.pick && c.id == "cut:s02:s03"));
        assert_eq!(g.beat_title("b02"), (2, "The bet".to_string()));
        assert_eq!(g.beat_title("b01").1, "Cold open on a…");
        assert_eq!(g.counts().line(), "5 shots · 1 OK · 1 needs you");
        // a cut that only exists because the order changed is a choice left to you
        let mut moved = g.clone();
        moved.set_film(vec!["s01".into(), "s03".into(), "s02".into(), "s04".into(), "x".into()]);
        assert!(moved.cuts.iter().find(|c| c.id == "cut:s01:s03").is_some_and(|c| c.pick));
        let line = g.get("b02-1").cloned().unwrap();
        assert_eq!(g.line_shot(&line).as_deref(), Some("s03"));
    }

    #[test]
    fn reorder_rewires_the_chain_and_cut_shots_are_skipped() {
        let mut g = Graph::from_json(&sample());
        assert!(g.reorder("s04", Some("s01")));
        assert_eq!(g.film, ["s01", "s04", "s02", "s03", "x"]);
        assert!(g.cuts.first().is_some_and(|c| c.from == "s01" && c.to == "s04" && c.pick));
        assert!(!g.reorder("s01", None));
        assert!(g.reorder("x", None));
        assert_eq!(g.film.first().map(String::as_str), Some("x"));
        assert!(!g.reorder("s01", Some("s01")));
        assert!(!g.reorder("nope", None));
        assert!(!g.reorder("s01", Some("b01")));
        if let Some(n) = g.get_mut("s02") {
            n.state = NodeState::Cut;
        }
        g.set_film(g.film.clone());
        assert!(!g.cuts.iter().any(|c| c.from == "s02" || c.to == "s02"));
        assert_eq!(g.film_prev("s04").as_deref(), Some("s01"));
    }

    #[test]
    fn hostile_input_is_ignored() {
        for v in [json!(null), json!(3), json!({"nodes": 5}), json!({"nodes": [{"id": 5}, {"id": "a".repeat(500)}]}), json!({"nodes": {"a": 1}})] {
            let g = Graph::from_json(&v);
            assert!(g.nodes.is_empty(), "{v}");
        }
        let g = Graph::from_json(&json!({"nodes": [
            {"id": "s1", "kind": "shot", "pos": {"x": 1e30, "y": 0}, "versions": [{"v": -1}, {"v": 2, "from_sec": "x"}], "current": 99, "colour": {"hex": "#zzzzzz"}, "bars": [1]},
            {"id": "s1", "kind": "beat"},
            {"id": "s2", "kind": "shot", "pos": {"x": 10.5, "y": -3}}
        ], "order": {"shots": ["s2", "s2", "ghost", 4]}, "layout": {"s1": {"x": 1, "y": 2}}}));
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.film, ["s2", "s1"]);
        assert_eq!(g.get("s1").and_then(|n| n.pos), Some([1.0, 2.0]));
        assert_eq!(g.get("s2").and_then(|n| n.pos), Some([10.5, -3.0]));
        assert!(g.get("s1").is_some_and(|n| n.colour.is_none() && n.bars.is_none() && n.versions.len() == 1 && n.shown_version().is_some()));
        assert_eq!(hex_colour("#ff0080"), Some(Color32::from_rgb(255, 0, 128)));
        assert_eq!(hex_colour("#ff00"), None);
        assert_eq!(hex_colour("#ééé"), None);
    }
}
