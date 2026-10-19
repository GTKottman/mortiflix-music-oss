//! The data behind the New project window (doc 31), its Assets list (doc 32a) and Music row (doc 33a):
//! presets as the service sends them, attached files and how the AI should use them, the music theme,
//! and the `project.new` / `presets.save` parameters built from them. No drawing here, so it is tested
//! without a window. Everything from the service or the disk is hostile input.

use std::collections::BTreeMap;
use std::io::Read as _;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Most presets kept from one `presets.list` reply.
pub const MAX_PRESETS: usize = 200;
/// Most attached files.
pub const MAX_ASSETS: usize = 500;
/// Files read from one dropped folder (one level deep).
pub const MAX_FOLDER_FILES: usize = 200;
/// The steps a preset can never turn off (doc 31: Brief, Script and Final always run).
pub const ALWAYS_STEPS: [&str; 3] = ["brief", "script", "final"];

// ------------------------------------------------------------------ presets

/// One intake question of a preset (`pipeline.json` `intake[]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Question {
    pub id: String,
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub required: bool,
    pub choices: Vec<String>,
    #[serde(deserialize_with = "lenient_string")]
    pub default: String,
    pub help: String,
    /// `false`: only the listed choices. Otherwise a choice can also be answered in your own words (the service takes
    /// it unless the pipeline says `"custom": false`).
    pub custom: Option<bool>,
}

/// One step of a preset (`pipeline.json` `steps[]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Step {
    pub key: String,
    pub name: String,
    pub describe: String,
    pub review: String,
    pub work: Vec<String>,
    /// Stops for your OK (`waits` in `presets.list`).
    #[serde(alias = "waits")]
    pub waits_for_approval: bool,
    pub delivers: bool,
    /// Brief, Script and the final: can't be turned off.
    pub always: bool,
    /// The stage it belongs to (`script`, `voice`, `styleFrames`, `animatic`, …).
    pub stage: String,
    /// `"music"`: runs only when the video has music.
    pub when: String,
    /// Turned off by the preset (doc 31 page 6).
    pub off: bool,
}

/// The look a preset carries (doc 31 page 3).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Look {
    pub words: String,
    pub pictures: Vec<String>,
    pub frames: u8,
}

/// The narration voice a preset carries (doc 31 page 4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VoicePick {
    pub name: String,
    pub delivery: String,
}

/// A skill of a preset (`presets.get` `skills[]`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillInfo {
    pub name: String,
    #[serde(deserialize_with = "lenient_string")]
    pub description: String,
    /// One of the studio's shared skills (else a folder inside the preset).
    pub shared: bool,
}

/// A preset (a pipeline): the summary `presets.list` sends, filled in by `presets.get`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub slug: String,
    pub name: String,
    #[serde(deserialize_with = "lenient_string")]
    pub description: String,
    /// `"built-in"` or `"studio"` (yours).
    pub source: String,
    #[serde(deserialize_with = "lenient_string")]
    pub based_on: String,
    /// Set when the preset can't be opened; it shows greyed with the reason.
    #[serde(deserialize_with = "lenient_string")]
    pub error: String,
    /// The defaults in words (`presets.list`): "60 s", "16:9", "Narrated", the music choice.
    #[serde(deserialize_with = "lenient_string")]
    pub length: String,
    #[serde(deserialize_with = "lenient_string")]
    pub aspect: String,
    #[serde(deserialize_with = "lenient_string")]
    pub voice: String,
    #[serde(deserialize_with = "lenient_string")]
    pub music: String,
    /// The intake question The idea fills.
    #[serde(deserialize_with = "lenient_string")]
    pub idea_field: String,
    pub steps: Vec<Step>,
    /// From `presets.get`.
    pub intake: Vec<Question>,
    pub skills: Vec<SkillInfo>,
    pub shared_skills: Vec<String>,
    #[serde(deserialize_with = "lenient_opt")]
    pub look: Option<Look>,
    #[serde(deserialize_with = "lenient_opt")]
    pub voice_preset: Option<VoicePick>,
    #[serde(deserialize_with = "lenient_opt")]
    pub music_theme: Option<MusicTheme>,
    /// `presets.get` filled it in.
    #[serde(skip)]
    pub detailed: bool,
}

/// An optional block that is dropped (not fatal) when it doesn't parse.
fn lenient_opt<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(Value::deserialize(d)?).ok())
}

fn lenient_string<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s,
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    })
}

fn cap(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

impl Preset {
    pub fn built_in(&self) -> bool {
        self.source == "built-in"
    }
    pub fn broken(&self) -> bool {
        !self.error.is_empty()
    }
    pub fn question(&self, id: &str) -> Option<&Question> {
        self.intake.iter().find(|q| q.id == id)
    }
    /// The idea: `idea_field`, else the first required long answer (today's `ideaOf`), else the
    /// first long one.
    pub fn idea_question(&self) -> Option<&Question> {
        self.intake
            .iter()
            .find(|q| !self.idea_field.is_empty() && q.id == self.idea_field)
            .or_else(|| self.intake.iter().find(|q| q.kind == "long" && q.required))
            .or_else(|| self.intake.iter().find(|q| q.kind == "long"))
    }
    /// The skills by name (`shared_skills`, else the shared ones `presets.get` lists).
    pub fn skill_names(&self) -> Vec<String> {
        if !self.shared_skills.is_empty() {
            return self.shared_skills.clone();
        }
        self.skills.iter().filter(|s| s.shared && !s.name.is_empty()).map(|s| s.name.clone()).collect()
    }
    /// The first file question the preset requires (Logo sting's "Your logo").
    pub fn required_files(&self) -> Option<&Question> {
        self.intake.iter().find(|q| q.kind == "files" && q.required)
    }
    /// Questions shown under ▸ Preset settings: everything except the idea, files and the music
    /// choice (that one lives on the Music theme page, doc 33 "More").
    pub fn settings_questions(&self) -> impl Iterator<Item = &Question> {
        let idea = self.idea_question().map(|q| q.id.clone());
        self.intake.iter().filter(move |q| q.kind != "files" && q.id != "music" && Some(&q.id) != idea.as_ref())
    }
    pub fn default_of(&self, id: &str) -> String {
        if let Some(q) = self.question(id) {
            return q.default.clone();
        }
        match id {
            "length" => self.length.clone(),
            "aspect" => self.aspect.clone(),
            "voice" => self.voice.clone(),
            "music" => self.music.clone(),
            _ => String::new(),
        }
    }
    /// "60 s · 16:9" for the dropdown rows.
    pub fn shape_line(&self) -> String {
        [self.default_of("length"), self.default_of("aspect")].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ")
    }
    /// The steps it runs, by name: "Brief · Script · Style frames · …".
    pub fn stage_line(&self) -> String {
        self.steps.iter().filter(|s| !s.off).map(|s| s.name.as_str()).collect::<Vec<_>>().join(" · ")
    }
}

/// Parse a `presets.list` result (`{presets: [...]}` or a bare list). A preset that doesn't parse
/// stays in the list, broken, so the user sees it and why.
pub fn parse_presets(v: &Value) -> Vec<Preset> {
    // `{built_in: [...], yours: [...]}` (protocol), or `{presets: [...]}` / a bare list
    let mut items: Vec<(Value, &str)> = Vec::new();
    if v.get("built_in").is_some() || v.get("yours").is_some() {
        for (key, source) in [("built_in", "built-in"), ("yours", "studio")] {
            for item in v.get(key).and_then(Value::as_array).into_iter().flatten() {
                items.push((item.clone(), source));
            }
        }
    } else if let Some(a) = v.get("presets").unwrap_or(v).as_array() {
        items = a.iter().map(|x| (x.clone(), "")).collect();
    }
    let mut out = Vec::new();
    for (item, source) in items.iter().take(MAX_PRESETS) {
        let Some(mut p) = parse_preset(item) else { continue };
        if p.source.is_empty() {
            p.source = (*source).to_string();
        }
        out.push(p);
    }
    // Built in first, then yours; each by name.
    out.sort_by_key(|p| (!p.built_in(), p.name.to_lowercase()));
    out
}

/// Merge a `presets.get` reply (the whole preset) into the list.
pub fn merge_detail(list: &mut [Preset], v: &Value) {
    let Some(mut p) = parse_preset(v) else { return };
    if let Some(slot) = list.iter_mut().find(|x| x.slug == p.slug) {
        if p.source.is_empty() {
            p.source = slot.source.clone();
        }
        p.detailed = true;
        *slot = p;
    }
}

/// One preset, capped; None without a slug. One that doesn't parse comes back broken, with why.
fn parse_preset(item: &Value) -> Option<Preset> {
    let mut p = match serde_json::from_value::<Preset>(item.clone()) {
        Ok(p) => p,
        Err(e) => Preset {
            slug: item.get("slug").and_then(Value::as_str).map(|s| cap(s, 80)).unwrap_or_default(),
            name: item.get("name").and_then(Value::as_str).map(|s| cap(s, 80)).unwrap_or_default(),
            source: item.get("source").and_then(Value::as_str).map(|s| cap(s, 20)).unwrap_or_default(),
            error: cap(&e.to_string(), 200),
            ..Preset::default()
        },
    };
    if p.slug.is_empty() {
        return None;
    }
    p.slug = cap(&p.slug, 80);
    p.name = if p.name.trim().is_empty() { p.slug.clone() } else { cap(&p.name, 80) };
    p.description = cap(&p.description, 1000);
    p.error = cap(&p.error, 200);
    p.intake.truncate(60);
    p.steps.truncate(60);
    p.skills.truncate(60);
    p.shared_skills.truncate(60);
    for q in &mut p.intake {
        q.choices.truncate(40);
    }
    Some(p)
}

// ------------------------------------------------------------------ assets

/// What kind of file an asset is (doc 32 "Kind").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    Logo,
    #[default]
    Image,
    Footage,
    Audio,
    Music,
    Font,
    Document,
    Brand,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::Logo => "Logo",
            Kind::Image => "Image",
            Kind::Footage => "Footage",
            Kind::Audio => "Audio",
            Kind::Music => "Music",
            Kind::Font => "Font",
            Kind::Document => "Document",
            Kind::Brand => "Brand file",
        }
    }
    /// The default "How to use it" (doc 32): logos and fonts must be used; pictures, footage and
    /// sounds when they fit; music, documents and brand files are read for reference.
    pub fn default_use(self) -> Use {
        match self {
            Kind::Logo | Kind::Font => Use::Must,
            Kind::Image | Kind::Footage | Kind::Audio => Use::Fits,
            Kind::Music | Kind::Document | Kind::Brand => Use::Reference,
        }
    }
    pub fn is_audio(self) -> bool {
        matches!(self, Kind::Audio | Kind::Music)
    }
}

/// How the AI should use an asset (doc 32).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Use {
    Must,
    #[default]
    Fits,
    Reference,
}

impl Use {
    pub const ALL: [Use; 3] = [Use::Must, Use::Fits, Use::Reference];
    pub fn label(self) -> &'static str {
        match self {
            Use::Must => "Must use",
            Use::Fits => "Use if it fits",
            Use::Reference => "Just for reference",
        }
    }
    /// The protocol word (`must` | `fits` | `reference`).
    pub fn key(self) -> &'static str {
        match self {
            Use::Must => "must",
            Use::Fits => "fits",
            Use::Reference => "reference",
        }
    }
}

/// One attached file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Asset {
    pub path: String,
    pub name: String,
    pub kind: Kind,
    /// "Logo · SVG · 14 KB".
    pub facts: String,
    pub bytes: u64,
    pub how: Use,
    /// Where, or a note, in the user's words.
    pub note: String,
    /// Can't be used: why, in plain words. Its controls are off and Create ignores it.
    pub error: String,
}

pub const IMAGE_EXTS: [&str; 9] = ["jpg", "jpeg", "png", "webp", "gif", "bmp", "tif", "tiff", "svg"];
const FOOTAGE_EXTS: [&str; 9] = ["mp4", "mov", "mkv", "webm", "avi", "m4v", "mxf", "mts", "mpg"];
const AUDIO_EXTS: [&str; 9] = ["wav", "mp3", "flac", "ogg", "m4a", "aac", "opus", "aif", "aiff"];
const FONT_EXTS: [&str; 4] = ["ttf", "otf", "woff", "woff2"];
const DOC_EXTS: [&str; 7] = ["pdf", "docx", "doc", "txt", "md", "rtf", "odt"];
const BRAND_EXTS: [&str; 4] = ["ase", "aco", "json", "gpl"];
const UNREADABLE_PICTURES: [&str; 4] = ["heic", "heif", "avif", "jxl"];

/// Every extension the Add files… dialog offers.
pub fn all_exts() -> Vec<&'static str> {
    let mut v = Vec::new();
    for group in [&IMAGE_EXTS[..], &FOOTAGE_EXTS, &AUDIO_EXTS, &FONT_EXTS, &DOC_EXTS, &BRAND_EXTS] {
        v.extend_from_slice(group);
    }
    v
}

/// The extensions for audio (Like this song).
pub fn audio_exts() -> Vec<&'static str> {
    AUDIO_EXTS.to_vec()
}

/// "14 KB", "2.1 MB", "2.8 GB".
pub fn human_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    let b = bytes as f64;
    if b < KB {
        format!("{bytes} B")
    } else if b < KB * KB {
        format!("{:.0} KB", b / KB)
    } else if b < KB * KB * KB {
        format!("{:.1} MB", b / KB / KB)
    } else {
        format!("{:.1} GB", b / KB / KB / KB)
    }
}

/// "2:48".
pub fn mmss(seconds: f64) -> String {
    if !seconds.is_finite() || seconds < 0.0 {
        return "0:00".into();
    }
    let s = seconds.round().min(99.0 * 3600.0) as u64;
    format!("{}:{:02}", s / 60, s % 60)
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).unwrap_or_default()
}

/// Read at most `n` bytes from the start of a file.
fn head(path: &Path, n: u64) -> Option<Vec<u8>> {
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(n).read_to_end(&mut buf).ok()?;
    Some(buf)
}

fn be_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_be_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

fn le_u32(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// PNG: (width, height, has alpha) from the IHDR chunk.
pub fn png_info(b: &[u8]) -> Option<(u32, u32, bool)> {
    if b.get(..8)? != b"\x89PNG\r\n\x1a\n" || b.get(12..16)? != b"IHDR" {
        return None;
    }
    let (w, h) = (be_u32(b, 16)?, be_u32(b, 20)?);
    let color_type = *b.get(25)?;
    // 4 = grey + alpha, 6 = RGBA; a tRNS chunk early on also means transparency
    let alpha = matches!(color_type, 4 | 6) || b.windows(4).any(|w| w == b"tRNS");
    Some((w, h, alpha))
}

/// WAV: length in seconds from the `fmt ` byte rate and the `data` size.
pub fn wav_seconds(b: &[u8]) -> Option<f64> {
    if b.get(..4)? != b"RIFF" || b.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12usize;
    let mut byte_rate = None;
    for _ in 0..64 {
        let id = b.get(at..at.checked_add(4)?)?;
        let size = le_u32(b, at.checked_add(4)?)?;
        let body = at.checked_add(8)?;
        if id == b"fmt " {
            byte_rate = le_u32(b, body.checked_add(8)?);
        } else if id == b"data" {
            let rate = byte_rate.filter(|r| *r > 0)?;
            return Some(f64::from(size) / f64::from(rate));
        }
        at = body.checked_add(usize::try_from(size).ok()?)?.checked_add(usize::try_from(size % 2).ok()?)?;
    }
    None
}

/// Look at a file and say what it is (doc 32 "Kind", "Can't read"). Reads at most 64 KB.
pub fn inspect(path_str: &str) -> Asset {
    let path = Path::new(path_str);
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| path_str.to_string());
    let ext = ext_of(path);
    let lower = name.to_lowercase();
    let mut a = Asset { path: path_str.to_string(), name: cap(&name, 300), ..Asset::default() };
    let meta = match std::fs::metadata(path) {
        Ok(m) if m.is_file() => m,
        _ => {
            a.error = "Can't read this file: it isn't there any more.".into();
            return a;
        }
    };
    a.bytes = meta.len();
    let size = human_size(a.bytes);
    let fmt = ext.to_uppercase();
    if a.bytes == 0 {
        a.error = "Can't read this file: it's empty.".into();
        return a;
    }
    if UNREADABLE_PICTURES.contains(&ext.as_str()) {
        a.error = "Can't read this file. Save it as JPG or PNG and add it again.".into();
        return a;
    }
    let Some(bytes) = head(path, 64 * 1024) else {
        a.error = "Can't read this file: it can't be opened.".into();
        return a;
    };
    let logo_named = lower.contains("logo");
    if ext == "svg" {
        a.kind = Kind::Logo;
        a.facts = format!("Logo · SVG · {size}");
    } else if ext == "png" {
        let info = png_info(&bytes);
        let Some((w, h, alpha)) = info else {
            a.error = "Can't read this file: it isn't a PNG picture inside.".into();
            return a;
        };
        a.kind = if logo_named || (alpha && w.max(h) <= 2048) { Kind::Logo } else { Kind::Image };
        a.facts = format!("{} · PNG · {w} × {h} · {size}", a.kind.label());
    } else if IMAGE_EXTS.contains(&ext.as_str()) {
        a.kind = if logo_named { Kind::Logo } else { Kind::Image };
        a.facts = format!("{} · {fmt} · {size}", a.kind.label());
    } else if FOOTAGE_EXTS.contains(&ext.as_str()) {
        a.kind = Kind::Footage;
        a.facts = format!("Footage · {fmt} · {size}");
    } else if AUDIO_EXTS.contains(&ext.as_str()) {
        let secs = if ext == "wav" { wav_seconds(&bytes) } else { None };
        let named_music = ["music", "song", "score", "theme", "track"].iter().any(|w| lower.contains(w));
        // without a length: compressed audio over ~1.5 MB is more than about 30 s
        let long = secs.map_or(a.bytes > 1_500_000, |s| s >= 30.0);
        a.kind = if named_music || long { Kind::Music } else { Kind::Audio };
        a.facts = match secs {
            Some(s) => format!("{} · {fmt} · {}", a.kind.label(), mmss(s)),
            None => format!("{} · {fmt} · {size}", a.kind.label()),
        };
    } else if FONT_EXTS.contains(&ext.as_str()) {
        a.kind = Kind::Font;
        let stem = path.file_stem().map(|s| s.to_string_lossy().replace(['-', '_'], " ")).unwrap_or_default();
        a.facts = format!("Font · {} · {size}", cap(&stem, 60));
    } else if DOC_EXTS.contains(&ext.as_str()) {
        a.kind = Kind::Document;
        a.facts = format!("Document · {fmt} · {size}");
    } else if BRAND_EXTS.contains(&ext.as_str()) || lower.contains("brand") {
        a.kind = Kind::Brand;
        a.facts = format!("Brand file · {fmt} · {size}");
    } else {
        a.error = if ext.is_empty() {
            "Mortiflix can't use this kind of file.".into()
        } else {
            format!("Mortiflix can't use .{} files. Try a picture, video, sound, font or document.", cap(&ext, 10))
        };
        return a;
    }
    a.how = a.kind.default_use();
    a
}

/// What a drop or a dialog gave: files as they are, a folder's files one level deep (doc 32).
pub fn expand_paths(paths: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for p in paths.iter().take(MAX_ASSETS) {
        let path = Path::new(p);
        if path.is_dir() {
            let Ok(rd) = std::fs::read_dir(path) else { continue };
            let mut files: Vec<String> =
                rd.flatten().map(|e| e.path()).filter(|f| f.is_file()).map(|f| f.to_string_lossy().to_string()).take(MAX_FOLDER_FILES).collect();
            files.sort();
            out.extend(files);
        } else {
            out.push(p.clone());
        }
    }
    out
}

fn same_file(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// Add files to the list. Returns the index of the first row that was already there (the window
/// flashes its name once), if any.
pub fn add_assets(list: &mut Vec<Asset>, paths: &[String]) -> Option<usize> {
    let mut dup = None;
    for p in expand_paths(paths) {
        if let Some(i) = list.iter().position(|a| same_file(&a.path, &p)) {
            dup = dup.or(Some(i));
            continue;
        }
        if list.len() >= MAX_ASSETS {
            break;
        }
        list.push(inspect(&p));
    }
    dup
}

// ------------------------------------------------------------------ music theme

pub const MOODS: [&str; 7] = ["Calm", "Warm", "Hopeful", "Tense", "Dark", "Playful", "Big"];
pub const INSTRUMENTS: [&str; 9] = ["Piano", "Strings", "Soft synth", "Bass", "Drums", "Guitar", "Pads", "Brass", "Percussion"];
/// Most moods picked at once; a third replaces the oldest (doc 33).
pub const MAX_MOODS: usize = 2;

/// Slow / Medium / Fast (any other word from the service reads as Any).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum TempoFeel {
    #[default]
    Any,
    Slow,
    Medium,
    Fast,
}

impl From<String> for TempoFeel {
    fn from(s: String) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "slow" => TempoFeel::Slow,
            "medium" => TempoFeel::Medium,
            "fast" => TempoFeel::Fast,
            _ => TempoFeel::Any,
        }
    }
}

impl From<TempoFeel> for String {
    fn from(f: TempoFeel) -> Self {
        if f == TempoFeel::Any { String::new() } else { f.label().to_ascii_lowercase() }
    }
}

impl TempoFeel {
    pub const ALL: [TempoFeel; 3] = [TempoFeel::Slow, TempoFeel::Medium, TempoFeel::Fast];
    pub fn label(self) -> &'static str {
        match self {
            TempoFeel::Any => "Any",
            TempoFeel::Slow => "Slow",
            TempoFeel::Medium => "Medium",
            TempoFeel::Fast => "Fast",
        }
    }
    pub fn bpm(self) -> Option<(u32, u32)> {
        match self {
            TempoFeel::Any => None,
            TempoFeel::Slow => Some((60, 85)),
            TempoFeel::Medium => Some((85, 110)),
            TempoFeel::Fast => Some((110, 140)),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tempo {
    pub feel: TempoFeel,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm_min: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm_max: Option<u32>,
    /// A fixed BPM (More); otherwise the BPM is fitted to the cuts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bpm_fixed: Option<u32>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Instruments {
    #[serde(rename = "use")]
    pub use_: Vec<String>,
    pub avoid: Vec<String>,
}

/// A "like this song" reference: a file, or one of the attached assets.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Reference {
    pub path: String,
    pub name: String,
    /// "2:41" when known.
    pub length: String,
}

/// One 15 s sample (`music.samples`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Sample {
    pub id: String,
    pub name: String,
    pub desc: String,
    pub bpm: Option<u32>,
    pub key: String,
    pub wav: String,
    /// The row couldn't be made: why.
    pub error: String,
}

/// `music/theme.json` (doc 33 "What it needs"). Every field is optional: empty = the AI picks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicTheme {
    /// `"ai"` or `"you"`.
    pub picked_by: String,
    pub mood: Vec<String>,
    pub genre: String,
    pub tempo: Tempo,
    pub instruments: Instruments,
    pub key: String,
    /// Who finishes the score: `"studio"` (Mortiflix renders it) or `"own"` (you, from the MIDI).
    pub finish: String,
    /// Anything else, in your words.
    pub words: String,
    pub reference: Option<Reference>,
    pub samples: Vec<Sample>,
    pub using: String,
    /// Music on (More › Music off = "No music").
    pub music: bool,
}

impl Default for MusicTheme {
    fn default() -> Self {
        Self {
            picked_by: "ai".into(),
            mood: Vec::new(),
            genre: String::new(),
            tempo: Tempo::default(),
            instruments: Instruments::default(),
            key: String::new(),
            finish: "studio".into(),
            words: String::new(),
            reference: None,
            samples: Vec::new(),
            using: String::new(),
            music: true,
        }
    }
}

impl MusicTheme {
    pub fn ai_picks(&self) -> bool {
        self.picked_by != "you"
    }
    /// Toggle a mood chip: at most two, a third replaces the oldest.
    pub fn toggle_mood(&mut self, mood: &str) {
        if let Some(i) = self.mood.iter().position(|m| m == mood) {
            self.mood.remove(i);
            return;
        }
        self.mood.push(mood.to_string());
        while self.mood.len() > MAX_MOODS {
            self.mood.remove(0);
        }
    }
    pub fn toggle_instrument(&mut self, name: &str) {
        let name = cap(name.trim(), 40);
        if name.is_empty() {
            return;
        }
        if let Some(i) = self.instruments.use_.iter().position(|m| m.eq_ignore_ascii_case(&name)) {
            self.instruments.use_.remove(i);
        } else if self.instruments.use_.len() < 20 {
            self.instruments.use_.push(name);
        }
    }
    pub fn set_tempo(&mut self, feel: TempoFeel) {
        self.tempo.feel = if self.tempo.feel == feel { TempoFeel::Any } else { feel };
        let range = self.tempo.feel.bpm();
        self.tempo.bpm_min = range.map(|r| r.0);
        self.tempo.bpm_max = range.map(|r| r.1);
    }
    /// The sample in use, if any.
    pub fn used_sample(&self) -> Option<&Sample> {
        self.samples.iter().find(|s| !self.using.is_empty() && s.id == self.using)
    }
    /// The Music row's summary (doc 33a-1): "Let the AI pick", "No music", or what you chose.
    pub fn summary(&self) -> String {
        if !self.music {
            return "No music".into();
        }
        let mut parts: Vec<String> = Vec::new();
        if !self.ai_picks() {
            if !self.mood.is_empty() {
                parts.push(self.mood.join(", "));
            }
            if !self.genre.trim().is_empty() {
                parts.push(cap(self.genre.trim(), 60));
            }
            if self.tempo.feel != TempoFeel::Any {
                parts.push(self.tempo.feel.label().into());
            }
            if let Some(b) = self.tempo.bpm_fixed {
                parts.push(format!("{b} BPM"));
            }
            if !self.instruments.use_.is_empty() {
                parts.push(self.instruments.use_.join(", ").to_lowercase());
            }
            if let Some(r) = &self.reference {
                parts.push(format!("like {}", r.name));
            }
        }
        if let Some(s) = self.used_sample() {
            parts.push(format!("using {}", s.name));
        }
        if parts.is_empty() { "Let the AI pick".into() } else { parts.join(" · ") }
    }
}

// ------------------------------------------------------------------ the draft and Create

/// What the New project window holds (the draft is kept when the window closes, doc 31).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    pub name: String,
    pub preset: String,
    pub idea: String,
    /// The other intake answers (▸ Preset settings), by question id.
    pub answers: BTreeMap<String, String>,
    pub assets: Vec<Asset>,
    pub music: MusicTheme,
    /// The music theme was changed by hand (otherwise it follows the preset's).
    pub music_touched: bool,
}

/// Why Create can't go yet, and which field to outline.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub field: &'static str,
    pub message: String,
}

impl Draft {
    /// Picking a preset: its answers become the defaults (name, idea and files stay).
    pub fn pick_preset(&mut self, p: &Preset) {
        self.preset = p.slug.clone();
        self.answers = p.settings_questions().filter(|q| !q.default.is_empty()).map(|q| (q.id.clone(), q.default.clone())).collect();
        // before `presets.get` arrives: the defaults the list gave
        for id in ["length", "aspect", "voice", "music"] {
            let v = p.default_of(id);
            if !v.is_empty() && (id == "music" || p.intake.is_empty()) {
                self.answers.insert(id.into(), v);
            }
        }
        if !self.music_touched {
            self.music = p.music_theme.clone().unwrap_or_default();
            self.music.music = !self.answers.get("music").is_some_and(|m| is_no_music(m));
        }
    }

    /// The music choice ("Original score: I finish it from the MIDI", "No music", …).
    pub fn music_choice(&mut self) -> &mut String {
        self.answers.entry("music".into()).or_default()
    }

    /// Create's checks (doc 31 / 32a): a name, the idea where the preset needs it, a file where it
    /// needs one.
    pub fn problem(&self, p: Option<&Preset>) -> Option<Problem> {
        if self.name.trim().is_empty() {
            return Some(Problem { field: "name", message: "Give it a name.".into() });
        }
        let Some(p) = p else {
            return Some(Problem { field: "preset", message: "Pick a preset.".into() });
        };
        if p.broken() {
            return Some(Problem { field: "preset", message: format!("Can't open this preset: {}", p.error) });
        }
        if let Some(q) = p.idea_question()
            && q.required
            && self.idea.trim().is_empty()
        {
            return Some(Problem { field: "idea", message: "Write the idea.".into() });
        }
        if let Some(q) = p.required_files()
            && !self.assets.iter().any(|a| a.error.is_empty())
        {
            let what = if q.label.is_empty() { "a file".to_string() } else { q.label.to_lowercase() };
            return Some(Problem { field: "assets", message: format!("Attach {what} first.") });
        }
        None
    }

    /// The answers sent with Create: the preset settings and the music choice (the idea goes as
    /// `idea`).
    pub fn answers_for(&self, p: &Preset) -> BTreeMap<String, String> {
        let known = |k: &str| if p.intake.is_empty() { ["length", "aspect", "voice", "music"].contains(&k) } else { p.question(k).is_some() };
        let idea = p.idea_question().map(|q| q.id.clone()).unwrap_or_else(|| p.idea_field.clone());
        let mut out: BTreeMap<String, String> =
            self.answers.iter().filter(|(k, v)| known(k) && **k != idea && !v.trim().is_empty()).map(|(k, v)| (k.clone(), v.clone())).collect();
        if !self.music.music {
            let no = p.question("music").and_then(|q| q.choices.iter().find(|c| is_no_music(c)).cloned()).unwrap_or_else(|| "No music".into());
            out.insert("music".into(), no);
        }
        out
    }

    /// `project.new` params (protocol): preset, title, idea, answers, assets, and the music theme
    /// when you set one (otherwise the preset's theme is used).
    pub fn project_new_params(&self, p: &Preset) -> Value {
        let mut assets: Vec<Value> =
            self.assets.iter().filter(|a| a.error.is_empty()).map(|a| json!({"path": a.path, "use": a.how.key(), "note": a.note.trim()})).collect();
        // "Like this song" travels as an asset for reference, unless it's attached already
        if let Some(r) = self.music.reference.as_ref().filter(|r| !r.path.is_empty())
            && !self.assets.iter().any(|a| a.path == r.path)
        {
            assets.push(json!({"path": r.path, "use": "reference", "note": "Like this song: the music reference (studied, never copied)"}));
        }
        let mut v = json!({
            "preset": p.slug,
            "title": self.name.trim(),
            "idea": self.idea.trim(),
            "answers": self.answers_for(p),
            "assets": assets,
        });
        if self.music_touched
            && self.music.music
            && let Some(o) = v.as_object_mut()
        {
            let mut theme = self.music.clone();
            theme.finish = finish_of(self.answers.get("music").map_or("", String::as_str)).into();
            o.insert("music".into(), json!({"theme": theme}));
        }
        v
    }

    /// "90 s · 16:9 · Narrated · Original score" beside ▸ Preset settings.
    pub fn settings_summary(&self, p: &Preset) -> String {
        if p.intake.is_empty() {
            return [p.length.as_str(), p.aspect.as_str(), p.voice.as_str()].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
        }
        p.settings_questions()
            .filter(|q| q.kind == "choice")
            .filter_map(|q| self.answers.get(&q.id))
            .filter(|v| !v.is_empty())
            .map(|v| short(v))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

/// "No music" (any choice that says so).
pub fn is_no_music(choice: &str) -> bool {
    choice.trim().to_lowercase().starts_with("no music")
}

/// The theme's `finish` for a music choice: `own` when you finish it from the MIDI.
pub fn finish_of(choice: &str) -> &'static str {
    let c = choice.to_lowercase();
    if c.contains("i finish") || c.contains("from the midi") || c.contains("myself") { "own" } else { "studio" }
}

/// A choice's short form: the words before a ":" or "(" ("Original score: I finish it…" → "Original score").
pub fn short(v: &str) -> String {
    let cut = [": ", " (", ", "].iter().filter_map(|sep| v.find(sep)).min().and_then(|i| v.get(..i)).unwrap_or(v).trim();
    cap(cut, 40)
}

// ------------------------------------------------------------------ the preset creator's draft

/// One row of page 6 (Stages).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StepRow {
    pub key: String,
    pub name: String,
    pub describe: String,
    pub runs: bool,
    pub waits: bool,
    /// Brief, Script and Final: always run.
    pub locked: bool,
    pub work: Vec<String>,
    pub stage: String,
}

/// One row of page 7 (Skills).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SkillRow {
    pub name: String,
    pub on: bool,
}

/// Everything the 8 pages edit.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PresetDraft {
    /// The preset it starts from (page 1).
    pub from: String,
    pub from_built_in: bool,
    /// Change mode on one of your own presets: saving replaces it.
    pub replace: String,
    pub length: String,
    pub aspect: String,
    pub look: Look,
    pub narration: String,
    pub voice: VoicePick,
    pub music_choice: String,
    pub theme: MusicTheme,
    pub steps: Vec<StepRow>,
    pub skills: Vec<SkillRow>,
    pub skill_dirs: Vec<String>,
    pub name: String,
    pub description: String,
    /// The description was edited by hand (otherwise it's written from the answers).
    pub description_touched: bool,
}

/// Which running steps need a skill (page 7: a skill a running stage needs can't be unticked).
pub fn skill_needed_by(skill: &str, steps: &[StepRow]) -> Vec<String> {
    let uses = |s: &StepRow| -> bool {
        let work = |w: &str| s.work.iter().any(|x| x == w);
        let key = |k: &[&str]| k.contains(&s.key.as_str());
        match skill {
            "music" => key(&["music"]) || work("music"),
            "voiceover" => work("audio") || key(&["animatic", "voice"]),
            "transition-board" => key(&["transitions"]),
            "final-pass" => key(&["final", "build"]),
            "motion-design" => work("stills") || work("motion") || key(&["style-frames", "animatic"]),
            "remotion-motion" => work("motion") || key(&["animatic", "build", "final"]),
            _ => false,
        }
    };
    steps.iter().filter(|s| s.runs && uses(s)).map(|s| s.name.clone()).collect()
}

impl PresetDraft {
    /// Start from a preset (page 1; also ✎ on a preset). The preset itself is never changed.
    pub fn from_preset(p: &Preset, change: bool) -> Self {
        let steps: Vec<StepRow> = p
            .steps
            .iter()
            .map(|s| {
                let locked = s.always || ALWAYS_STEPS.contains(&s.key.as_str()) || s.delivers;
                StepRow {
                    key: s.key.clone(),
                    name: s.name.clone(),
                    describe: s.describe.clone(),
                    runs: locked || !s.off,
                    waits: s.waits_for_approval && (locked || !s.off),
                    locked,
                    work: s.work.clone(),
                    stage: s.stage.clone(),
                }
            })
            .collect();
        let look = p.look.clone().map(|mut l| {
            l.frames = l.frames.clamp(3, 5);
            l
        });
        let mut d = Self {
            from: p.slug.clone(),
            from_built_in: p.built_in(),
            replace: if change && !p.built_in() { p.slug.clone() } else { String::new() },
            length: p.default_of("length"),
            aspect: p.default_of("aspect"),
            look: look.unwrap_or(Look { frames: 3, ..Look::default() }),
            narration: p.default_of("voice"),
            voice: p.voice_preset.clone().unwrap_or_default(),
            music_choice: p.default_of("music"),
            theme: p.music_theme.clone().unwrap_or_default(),
            skills: p.skill_names().into_iter().map(|name| SkillRow { name, on: true }).collect(),
            steps,
            skill_dirs: Vec::new(),
            name: String::new(),
            description: String::new(),
            description_touched: false,
        };
        d.name = if change && !p.built_in() {
            p.name.clone()
        } else if change {
            format!("My {}", lower_first(&p.name))
        } else {
            String::new()
        };
        if change {
            d.description = p.description.clone();
            d.description_touched = true;
        }
        d
    }

    pub fn narrated(&self) -> bool {
        !self.narration.to_lowercase().starts_with("on-screen")
    }

    /// Turn a step on or off (locked rows stay on; off un-ticks its Waits).
    pub fn set_runs(&mut self, i: usize, on: bool) {
        if let Some(s) = self.steps.get_mut(i)
            && !s.locked
        {
            s.runs = on;
            if !on {
                s.waits = false;
            }
        }
    }

    /// The description written from the answers (page 8), until you edit it.
    pub fn auto_description(&self) -> String {
        let mut parts = vec![format!("A {} video in {}", self.length, self.aspect)];
        parts.push(if self.narrated() {
            if self.voice.name.is_empty() { "narrated".into() } else { format!("narrated by {}", self.voice.name) }
        } else {
            "with on-screen text and music".into()
        });
        let mut s = parts.join(", ");
        if !self.look.words.trim().is_empty() {
            s.push_str(&format!(". Look: {}", cap(self.look.words.trim().trim_end_matches('.'), 200)));
        }
        if self.theme.music && !self.theme.ai_picks() {
            s.push_str(&format!(". Music: {}", self.theme.summary()));
        } else if !self.theme.music {
            s.push_str(". No music");
        }
        s.push('.');
        s
    }

    /// Save's checks before the service sees it (doc 31 "Save preset").
    pub fn problem(&self, presets: &[Preset]) -> Option<String> {
        let name = self.name.trim();
        if name.is_empty() {
            return Some("Give the preset a name.".into());
        }
        if let Some(b) = presets.iter().find(|p| p.built_in() && (p.name.eq_ignore_ascii_case(name) || p.slug.eq_ignore_ascii_case(name))) {
            return Some(format!("{} is a built-in preset: pick another name", b.name));
        }
        if let Some(mine) = presets.iter().find(|p| !p.built_in() && p.name.eq_ignore_ascii_case(name))
            && mine.slug != self.replace
        {
            return Some(format!("You already have a preset called \"{}\".", mine.name));
        }
        None
    }

    /// `presets.save` params (protocol.md).
    pub fn save_params(&self) -> Value {
        let mut defaults = serde_json::Map::new();
        for (k, v) in [("length", &self.length), ("aspect", &self.aspect), ("voice", &self.narration), ("music", &self.music_choice)] {
            if !v.trim().is_empty() {
                defaults.insert(k.into(), json!(v.trim()));
            }
        }
        let description = if self.description_touched || !self.description.trim().is_empty() { self.description.clone() } else { self.auto_description() };
        let steps: serde_json::Map<String, Value> = self.steps.iter().map(|s| (s.key.clone(), json!({"on": s.runs, "waits": s.waits}))).collect();
        let mut theme = self.theme.clone();
        theme.finish = finish_of(&self.music_choice).into();
        json!({
            "from": self.from,
            "name": self.name.trim(),
            "description": description.trim(),
            "replace": !self.replace.is_empty(),
            "defaults": defaults,
            "steps": steps,
            "skills": self.skills.iter().filter(|s| s.on).map(|s| s.name.clone()).collect::<Vec<_>>(),
            "skill_folders": self.skill_dirs,
            "look": self.look,
            "voice": self.voice,
            "music_theme": theme,
        })
    }
}

fn lower_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_lowercase().chain(c).collect(),
        None => String::new(),
    }
}

/// Does this folder hold a skill (a SKILL.md)?
pub fn is_skill_dir(path: &str) -> bool {
    Path::new(path).join("SKILL.md").is_file()
}

/// A narration voice (`voice.list` presets).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub desc: String,
}

/// `voice.list` (Settings › Narration): the voices and whether a voice engine is set up.
pub fn parse_voices(v: &Value) -> (Vec<Voice>, bool) {
    let set_up = v.get("engine").and_then(Value::as_str).is_some_and(|e| !e.is_empty() && e != "none");
    let voices = v
        .get("presets")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .take(100)
                .filter_map(|x| {
                    let id = x.get("id").and_then(Value::as_str)?;
                    let about = x.get("about").and_then(Value::as_str).unwrap_or_default();
                    let lang = x.get("language").and_then(Value::as_str).unwrap_or_default();
                    let desc = [lang, about].iter().filter(|s| !s.is_empty()).copied().collect::<Vec<_>>().join(" · ");
                    Some(Voice { id: cap(id, 60), name: cap(&id.replace('_', " "), 60), desc: cap(&desc, 120) })
                })
                .collect()
        })
        .unwrap_or_default();
    (voices, set_up)
}

pub fn parse_samples(v: &Value) -> Vec<Sample> {
    let list = v.get("samples").unwrap_or(v);
    list.as_array()
        .map(|a| {
            a.iter()
                .take(8)
                .filter_map(|x| serde_json::from_value::<Sample>(x.clone()).ok())
                .map(|mut x| {
                    x.name = cap(&x.name, 60);
                    x.desc = cap(&x.desc, 160);
                    x
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explainer() -> Value {
        json!({"slug": "explainer", "name": "Explainer video", "source": "built-in", "description": "A 30 second to 2 minute explainer.",
        "shared_skills": ["motion-design", "music", "transition-board", "voiceover"],
        "intake": [
            {"id": "topic", "label": "What should the video explain?", "type": "long", "required": true},
            {"id": "audience", "label": "Who's watching?", "type": "text"},
            {"id": "length", "type": "choice", "choices": ["30 s", "60 s", "90 s"], "default": "60 s"},
            {"id": "aspect", "type": "choice", "choices": ["16:9", "9:16"], "default": "16:9"},
            {"id": "voice", "type": "choice", "choices": ["Narrated", "On-screen text and music only"], "default": "Narrated"},
            {"id": "music", "type": "choice", "choices": ["Original score: I finish it from the MIDI (recommended)", "No music"], "default": "Original score: I finish it from the MIDI (recommended)"},
            {"id": "brand", "type": "files"}
        ],
        "steps": [
            {"key": "brief", "name": "Brief"}, {"key": "script", "name": "Script", "work": ["script"]},
            {"key": "transitions", "name": "Transition board", "work": ["motion"]},
            {"key": "music", "name": "Music", "work": ["music", "audio"], "when": "music"},
            {"key": "build", "name": "Final build", "waits_for_approval": true},
            {"key": "final", "name": "Final", "delivers": true}
        ]})
    }

    #[test]
    fn presets_parse_sort_and_keep_broken_ones() {
        let v = json!({"presets": [
            {"slug": "zeta", "name": "Zeta", "source": "studio"},
            explainer(),
            {"slug": "bad", "name": "Bad", "source": "studio", "intake": "nope"},
            {"name": "no slug"},
            7,
            {"slug": "err", "source": "built-in", "error": "steps[0] has no key"}
        ]});
        let p = parse_presets(&v);
        let names: Vec<&str> = p.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["err", "Explainer video", "Bad", "Zeta"]);
        assert!(p.iter().find(|x| x.slug == "bad").unwrap().broken());
        assert!(p.iter().find(|x| x.slug == "err").unwrap().broken());
        let e = p.iter().find(|x| x.slug == "explainer").unwrap();
        assert_eq!(e.shape_line(), "60 s · 16:9");
        assert_eq!(e.idea_question().map(|q| q.id.as_str()), Some("topic"));
        assert_eq!(e.settings_questions().map(|q| q.id.as_str()).collect::<Vec<_>>(), ["audience", "length", "aspect", "voice"]);
        assert!(parse_presets(&json!("junk")).is_empty());
        assert!(parse_presets(&json!(null)).is_empty());
    }

    #[test]
    fn draft_checks_and_project_new_params() {
        let p = parse_presets(&json!([explainer()])).remove(0);
        let mut d = Draft::default();
        d.pick_preset(&p);
        assert_eq!(d.problem(Some(&p)).map(|x| x.field), Some("name"));
        d.name = "  The Rise of AI ".into();
        assert_eq!(d.problem(None).map(|x| x.field), Some("preset"));
        assert_eq!(d.problem(Some(&p)).map(|x| x.field), Some("idea"));
        d.idea = "How five companies raced".into();
        assert_eq!(d.problem(Some(&p)), None);
        assert_eq!(d.settings_summary(&p), "60 s · 16:9 · Narrated");
        d.assets.push(Asset {
            path: "/x/logo.svg".into(),
            name: "logo.svg".into(),
            kind: Kind::Logo,
            how: Use::Must,
            note: " last shot ".into(),
            ..Asset::default()
        });
        d.assets.push(Asset { path: "/x/a.heic".into(), error: "Can't read".into(), ..Asset::default() });
        let v = d.project_new_params(&p);
        assert_eq!(v["preset"], "explainer");
        assert_eq!(v["title"], "The Rise of AI");
        assert_eq!(v["idea"], "How five companies raced");
        assert!(v["answers"].get("topic").is_none(), "the idea goes as `idea`");
        assert_eq!(v["answers"]["length"], "60 s");
        assert_eq!(v["answers"]["music"], "Original score: I finish it from the MIDI (recommended)");
        assert_eq!(v["assets"].as_array().map(Vec::len), Some(1));
        assert_eq!(v["assets"][0]["use"], "must");
        assert_eq!(v["assets"][0]["note"], "last shot");
        assert!(v.get("music").is_none(), "untouched: the preset's theme is used");
        d.music_touched = true;
        d.music.picked_by = "you".into();
        d.music.reference = Some(Reference { path: "/x/song.wav".into(), name: "song.wav".into(), length: String::new() });
        let v = d.project_new_params(&p);
        assert_eq!(v["music"]["theme"]["picked_by"], "you");
        assert_eq!(v["music"]["theme"]["finish"], "own");
        assert_eq!(v["assets"][1]["path"], "/x/song.wav");
        assert_eq!(v["assets"][1]["use"], "reference");
        d.music.music = false;
        let v = d.project_new_params(&p);
        assert_eq!(v["answers"]["music"], "No music");
        assert!(v.get("music").is_none());
        assert_eq!(d.music.summary(), "No music");
    }

    #[test]
    fn protocol_list_and_detail_merge() {
        let mut list =
            parse_presets(&json!({"built_in": [{"slug": "explainer", "name": "Explainer video", "source": "built-in", "length": "60 s", "aspect": "16:9",
            "voice": "Narrated", "music": "No music", "idea_field": "topic", "based_on": null,
            "steps": [{"key": "brief", "name": "Brief", "waits": true, "always": true, "stage": "script"}]}],
            "yours": [{"slug": "dry", "name": "Dry", "error": "bad json"}, {"name": "no slug"}]}));
        assert_eq!(list.len(), 2);
        assert_eq!(list[1].source, "studio");
        assert_eq!(list[0].shape_line(), "60 s · 16:9");
        let mut d = Draft::default();
        d.pick_preset(&list[0]);
        assert_eq!(d.answers.get("length").map(String::as_str), Some("60 s"));
        assert!(!d.music.music, "No music by default");
        let pd = PresetDraft::from_preset(&list[0], false);
        assert!(pd.steps[0].locked && pd.steps[0].waits);
        merge_detail(
            &mut list,
            &json!({"slug": "explainer", "name": "Explainer video", "intake": [{"id": "topic", "type": "long", "required": true}],
            "skills": [{"name": "music", "shared": true}, {"name": "own", "shared": false}], "voice_preset": {"name": "Ryan", "delivery": "calm"},
            "music_theme": {"tempo": {"feel": "", "bpm_min": null}, "finish": "own"}, "look": "junk"}),
        );
        assert!(list[0].detailed);
        assert_eq!(list[0].source, "built-in");
        assert_eq!(list[0].idea_question().map(|q| q.id.as_str()), Some("topic"));
        assert_eq!(list[0].skill_names(), ["music"]);
        assert_eq!(list[0].voice_preset.as_ref().map(|v| v.name.as_str()), Some("Ryan"));
        assert_eq!(list[0].music_theme.as_ref().map(|t| t.tempo.feel), Some(TempoFeel::Any));
        assert!(list[0].look.is_none());
    }

    #[test]
    fn required_file_question_blocks_create() {
        let p = parse_presets(&json!([{"slug": "logo-sting", "name": "Logo sting", "source": "built-in",
            "intake": [{"id": "logo", "label": "Your logo", "type": "files", "required": true}]}]))
        .remove(0);
        let mut d = Draft { name: "Sting".into(), ..Draft::default() };
        d.pick_preset(&p);
        assert_eq!(d.problem(Some(&p)).map(|x| x.message), Some("Attach your logo first.".into()));
        d.assets.push(Asset { path: "/l.svg".into(), ..Asset::default() });
        assert_eq!(d.problem(Some(&p)), None);
    }

    #[test]
    fn music_theme_moods_tempo_and_summary() {
        let mut t = MusicTheme::default();
        assert_eq!(t.summary(), "Let the AI pick");
        t.picked_by = "you".into();
        t.toggle_mood("Calm");
        t.toggle_mood("Tense");
        t.toggle_mood("Dark");
        assert_eq!(t.mood, ["Tense", "Dark"]);
        t.toggle_mood("Tense");
        assert_eq!(t.mood, ["Dark"]);
        t.set_tempo(TempoFeel::Medium);
        assert_eq!((t.tempo.bpm_min, t.tempo.bpm_max), (Some(85), Some(110)));
        t.set_tempo(TempoFeel::Medium);
        assert_eq!(t.tempo.feel, TempoFeel::Any);
        t.toggle_instrument("Piano");
        t.toggle_instrument("  ");
        t.genre = "Ambient documentary".into();
        t.reference = Some(Reference { path: "/r.wav".into(), name: "r.wav".into(), length: String::new() });
        assert_eq!(t.summary(), "Dark · Ambient documentary · piano · like r.wav");
        t.samples = parse_samples(&json!({"samples": [{"id": "s2", "name": "Cold machines"}, 5]}));
        t.using = "s2".into();
        assert!(t.summary().ends_with("using Cold machines"));
        let back: MusicTheme = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        let hostile: MusicTheme = serde_json::from_value(json!({"mood": "x"})).unwrap_or_default();
        assert!(hostile.ai_picks());
    }

    #[test]
    fn creator_draft_locks_always_steps_and_skills() {
        let p = parse_presets(&json!([explainer()])).remove(0);
        let mut d = PresetDraft::from_preset(&p, false);
        assert_eq!(d.from, "explainer");
        assert!(d.from_built_in && d.replace.is_empty() && d.name.is_empty());
        let brief = d.steps.iter().position(|s| s.key == "brief").unwrap();
        d.set_runs(brief, false);
        assert!(d.steps[brief].runs, "Brief always runs");
        let build = d.steps.iter().position(|s| s.key == "build").unwrap();
        assert!(d.steps[build].waits);
        d.set_runs(build, false);
        assert!(!d.steps[build].runs && !d.steps[build].waits);
        assert_eq!(skill_needed_by("music", &d.steps), ["Music"]);
        let music = d.steps.iter().position(|s| s.key == "music").unwrap();
        d.set_runs(music, false);
        assert!(skill_needed_by("music", &d.steps).is_empty());
        d.set_runs(99, false);
        let presets = vec![p.clone()];
        assert_eq!(d.problem(&presets), Some("Give the preset a name.".into()));
        d.name = "explainer video".into();
        assert_eq!(d.problem(&presets), Some("Explainer video is a built-in preset: pick another name".into()));
        d.name = "Dry docs".into();
        assert_eq!(d.problem(&presets), None);
        let v = d.save_params();
        assert_eq!(v["from"], "explainer");
        assert_eq!(v["replace"], false);
        assert_eq!(v["defaults"]["length"], "60 s");
        assert_eq!(v["steps"]["music"]["on"], false);
        assert_eq!(v["steps"]["brief"]["on"], true);
        assert_eq!(v["skills"], json!(["motion-design", "music", "transition-board", "voiceover"]));
        assert_eq!(v["music_theme"]["finish"], "own");
        assert!(v["description"].as_str().unwrap().starts_with("A 60 s video in 16:9, narrated"));
        // changing a built-in preset suggests your own copy
        let c = PresetDraft::from_preset(&p, true);
        assert_eq!(c.name, "My explainer video");
        assert!(c.replace.is_empty());
        let mine = Preset { source: "studio".into(), slug: "dry".into(), name: "Dry".into(), ..p.clone() };
        let c = PresetDraft::from_preset(&mine, true);
        assert_eq!((c.replace.as_str(), c.name.as_str()), ("dry", "Dry"));
        let all = vec![p, mine.clone()];
        assert_eq!(c.problem(&all), None, "saving your own preset under its own name");
        let mut n = PresetDraft::from_preset(&mine, false);
        n.name = "dry".into();
        assert_eq!(n.problem(&all), Some("You already have a preset called \"Dry\".".into()));
    }

    #[test]
    fn file_kinds_facts_and_errors() {
        let dir = std::env::temp_dir().join(format!("mfx-np-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let w = |name: &str, bytes: &[u8]| {
            let p = dir.join(name);
            std::fs::write(&p, bytes).unwrap();
            p.to_string_lossy().to_string()
        };
        // 2x1 RGBA png header
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        png.extend_from_slice(&[0, 0, 0, 2, 0, 0, 0, 1, 8, 6, 0, 0, 0]);
        let a = inspect(&w("brand-mark.png", &png));
        assert_eq!((a.kind, a.how), (Kind::Logo, Use::Must));
        assert!(a.facts.starts_with("Logo · PNG · 2 × 1"), "{}", a.facts);
        let mut rgb = png.clone();
        rgb[25] = 2;
        assert_eq!(inspect(&w("photo.png", &rgb)).kind, Kind::Image);
        assert!(!inspect(&w("fake.png", b"not a png")).error.is_empty());
        // 40 s mono 8 kHz 8-bit wav: byte rate 8000, data 320000
        let mut wav = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&[1, 0, 1, 0]);
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&8000u32.to_le_bytes());
        wav.extend_from_slice(&[1, 0, 8, 0]);
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&320_000u32.to_le_bytes());
        let a = inspect(&w("ref.wav", &wav));
        assert_eq!((a.kind, a.how), (Kind::Music, Use::Reference));
        assert_eq!(a.facts, "Music · WAV · 0:40");
        assert_eq!(inspect(&w("click.mp3", b"ID3")).kind, Kind::Audio);
        assert_eq!(inspect(&w("Old-Standard.ttf", b"x")).facts, "Font · Old Standard · 1 B");
        assert_eq!(inspect(&w("lab.mp4", b"x")).kind, Kind::Footage);
        assert_eq!(inspect(&w("notes.pdf", b"x")).how, Use::Reference);
        assert!(inspect(&w("team.heic", b"x")).error.contains("JPG or PNG"));
        assert!(!inspect(&w("thing.xyz", b"x")).error.is_empty());
        assert!(!inspect(&w("empty.png", b"")).error.is_empty());
        assert!(!inspect(&dir.join("missing.png").to_string_lossy()).error.is_empty());
        // hostile wav sizes never panic
        let mut bad = wav.clone();
        bad[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(wav_seconds(&bad), None);
        assert_eq!(wav_seconds(b"RIFF"), None);
        // dropping a folder adds its files; the same file twice is flagged
        let sub = dir.join("folder");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(sub.join("a.jpg"), b"x").unwrap();
        std::fs::write(sub.join("b.svg"), b"x").unwrap();
        let mut list = Vec::new();
        assert_eq!(add_assets(&mut list, &[sub.to_string_lossy().to_string()]), None);
        assert_eq!(list.len(), 2);
        assert_eq!(add_assets(&mut list, &[sub.join("b.svg").to_string_lossy().to_string()]), Some(1));
        assert_eq!(list.len(), 2);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sizes_and_times() {
        assert_eq!(human_size(14 * 1024), "14 KB");
        assert_eq!(human_size(2_202_009), "2.1 MB");
        assert_eq!(mmss(168.0), "2:48");
        assert_eq!(mmss(f64::NAN), "0:00");
        assert_eq!(short("Original score: I finish it"), "Original score");
        let (v, set_up) = parse_voices(&json!({"engine": "none", "presets": [{"id": "Uncle_Fu", "language": "Chinese", "about": "low"}, {"x": 1}, 3]}));
        assert_eq!((v.len(), set_up), (1, false));
        assert_eq!((v[0].name.as_str(), v[0].desc.as_str()), ("Uncle Fu", "Chinese · low"));
    }
}
