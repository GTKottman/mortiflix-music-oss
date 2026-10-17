//! Render and export (doc 39, mockups `UI Layout/39a Export.html` and `39b Rendered.html`).
//!
//! Stage 6 is FilmCraft's Export mode. While Mortiflix is on, the Export tab shows this simple
//! page instead of FilmCraft's full settings: **Where it goes** (YouTube, Social vertical, Master
//! file, Audio stems, or any of FilmCraft's own formats), a preview, the **Render queue** (one at a
//! time on this PC, shared with the Mortiflix service's heavy jobs), the settings with an
//! **Advanced** fold, and **Render**. When a render finishes, the **Rendered** page hands the file
//! over (Open, Show in Files/Explorer/Finder, Save a copy…), lists this project's renders, and offers
//! **Post…** (always asks first; the AI never presses it) and what to do next.
//!
//! This crate has no FilmCraft dependency: the host (`filmcraft-ui-egui`, `mortiflix_export.rs`)
//! fills a [`TabView`] from the engine each frame, draws the timeline picture where this module
//! asks ([`TabOut::preview`]), and carries out the [`Action`]s. The choices live in [`ExportState`]
//! (serde: `mortiflix.renderVideo {"set": {...}}` on the control channel changes them).
//!
//! Automation ids: `mfx.export.preset.<youtube|social|master|stems>`, `mfx.export.allFormats`,
//! `mfx.export.format.<name>`, `mfx.export.fileName`, `mfx.export.folder.change`,
//! `mfx.export.size.<720p|1080p|4k>`, `mfx.export.framing.<fill|fit>`,
//! `mfx.export.quality.<small|good|best>`, `mfx.export.howMuch.<whole|inOut>`, `mfx.export.advanced`,
//! `mfx.export.advanced.<loudness|captions|hardware>.<value>`, `mfx.export.advanced.full`,
//! `mfx.export.advanced.reset`, `mfx.export.render`, `mfx.export.queue.<id>.<stop|remove|retry|open|show>`,
//! `mfx.export.confirm.<yes|no>`, `mfx.export.rendered.<back|open|show|saveCopy|edit|another|newProject>`,
//! `mfx.export.renders.<n>.<show|open|reveal|again>`, `mfx.export.post.<platform>`,
//! `mfx.export.post.<title|description|now|later|at|go|setup>`.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, pos2, vec2};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{MfxCtx, MfxTheme};

/// Most renders remembered per project (the oldest drop off the list, never off the disk).
pub const MAX_RENDERS: usize = 200;
/// How often the service's render jobs are asked for while the Export tab shows (seconds).
pub const JOBS_POLL_SECONDS: f64 = 2.0;

// ---------------------------------------------------------------------------------------------
// the choices
// ---------------------------------------------------------------------------------------------

/// Where it goes (doc 39 "Where it goes").
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Preset {
    #[default]
    Youtube,
    Social,
    Master,
    Stems,
}

impl Preset {
    pub const ALL: [Preset; 4] = [Preset::Youtube, Preset::Social, Preset::Master, Preset::Stems];
    pub fn key(self) -> &'static str {
        match self {
            Preset::Youtube => "youtube",
            Preset::Social => "social",
            Preset::Master => "master",
            Preset::Stems => "stems",
        }
    }
    pub fn from_key(k: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key().eq_ignore_ascii_case(k))
    }
    /// The name shown ("Social vertical").
    pub fn label(self) -> &'static str {
        match self {
            Preset::Youtube => "YouTube",
            Preset::Social => "Social vertical",
            Preset::Master => "Master file",
            Preset::Stems => "Audio stems",
        }
    }
    /// The file name part ("<Project> – Master").
    pub fn file_label(self) -> &'static str {
        match self {
            Preset::Youtube => "YouTube",
            Preset::Social => "Social vertical",
            Preset::Master => "Master",
            Preset::Stems => "Stems",
        }
    }
    /// FilmCraft 0.8.0's built-in preset each one starts from.
    pub fn filmcraft_preset(self) -> &'static str {
        match self {
            Preset::Youtube => "YouTube 1080p Full HD",
            Preset::Social => "Social Vertical 1080×1920",
            Preset::Master => "Apple ProRes 422 HQ",
            Preset::Stems => "Waveform Audio 48 kHz 24-bit",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Preset::Youtube | Preset::Social => "mp4",
            Preset::Master => "mov",
            Preset::Stems => "wav",
        }
    }
    pub fn has_video(self) -> bool {
        self != Preset::Stems
    }
    /// Quality applies to the bitrate-driven presets only.
    pub fn has_quality(self) -> bool {
        matches!(self, Preset::Youtube | Preset::Social)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Size {
    #[serde(rename = "720p")]
    P720,
    #[default]
    #[serde(rename = "1080p")]
    P1080,
    #[serde(rename = "4k")]
    P2160,
}

impl Size {
    pub const ALL: [Size; 3] = [Size::P720, Size::P1080, Size::P2160];
    pub fn key(self) -> &'static str {
        match self {
            Size::P720 => "720p",
            Size::P1080 => "1080p",
            Size::P2160 => "4k",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Size::P720 => "720p",
            Size::P1080 => "1080p",
            Size::P2160 => "4K",
        }
    }
    /// Lines of the short side.
    pub fn lines(self) -> u32 {
        match self {
            Size::P720 => 720,
            Size::P1080 => 1080,
            Size::P2160 => 2160,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Quality {
    Small,
    #[default]
    Good,
    Best,
}

impl Quality {
    pub const ALL: [Quality; 3] = [Quality::Small, Quality::Good, Quality::Best];
    pub fn key(self) -> &'static str {
        match self {
            Quality::Small => "small",
            Quality::Good => "good",
            Quality::Best => "best",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Quality::Small => "Small file",
            Quality::Good => "Good",
            Quality::Best => "Best",
        }
    }
    /// Bitrate factor against "Good" (FilmCraft's low / medium / high adaptive steps).
    fn factor_percent(self) -> u32 {
        match self {
            Quality::Small => 50,
            Quality::Good => 100,
            Quality::Best => 150,
        }
    }
}

/// Social vertical from a 16:9 timeline: crop the sides (default) or bars top and bottom.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Framing {
    #[default]
    Fill,
    Fit,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HowMuch {
    #[default]
    Whole,
    InOut,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Loudness {
    /// −14 LUFS, true peaks at or under −1 dB (YouTube and the social platforms).
    #[default]
    Platform,
    /// Leave the sound as mixed.
    AsMixed,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Captions {
    #[default]
    Off,
    Burned,
    Srt,
}

/// The platform loudness (doc 27): integrated −14 LUFS, true peak −1 dBTP.
pub const PLATFORM_LUFS: f64 = -14.0;
pub const PLATFORM_TRUE_PEAK: f64 = -1.0;

/// Everything the person picked on the Export page.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Choices {
    pub preset: Preset,
    /// One of FilmCraft's own presets (All FilmCraft formats ›) instead of the four.
    pub filmcraft_preset: Option<String>,
    pub size: Size,
    pub quality: Quality,
    pub framing: Framing,
    pub how_much: HowMuch,
    pub loudness: Loudness,
    pub captions: Captions,
    /// Hardware encoding: off by default, so every PC writes the same file.
    pub hardware: bool,
    /// File name without extension; empty = "<Project> – <Preset>".
    pub file_name: String,
    /// Folder; empty = the default per OS (`<Videos>/Mortiflix/<Project>`).
    pub folder: String,
}

impl Default for Choices {
    fn default() -> Self {
        Self {
            preset: Preset::Youtube,
            filmcraft_preset: None,
            size: Size::P1080,
            quality: Quality::Good,
            framing: Framing::Fill,
            how_much: HowMuch::Whole,
            loudness: Loudness::Platform,
            captions: Captions::Off,
            hardware: false,
            file_name: String::new(),
            folder: String::new(),
        }
    }
}

impl Choices {
    /// What the preset row / settings header calls the pick.
    pub fn title(&self) -> String {
        self.filmcraft_preset.clone().unwrap_or_else(|| self.preset.label().to_string())
    }
    /// The file name to write (without extension).
    pub fn file_stem(&self, project: &str) -> String {
        let name = self.file_name.trim();
        if !name.is_empty() {
            return safe_file_name(name);
        }
        let label = self.filmcraft_preset.as_deref().unwrap_or(self.preset.file_label());
        safe_file_name(&format!("{} – {label}", if project.trim().is_empty() { "Video" } else { project.trim() }))
    }
    /// The folder to write into.
    pub fn folder_or<'a>(&'a self, default: &'a str) -> &'a str {
        if self.folder.trim().is_empty() { default } else { self.folder.trim() }
    }
    /// Apply a JSON patch from the control channel (`mortiflix.renderVideo {"set": {...}}`).
    /// Unknown keys are ignored; bad values are an error that names the key.
    pub fn apply(&mut self, patch: &Value) -> Result<(), String> {
        let Some(obj) = patch.as_object() else { return Err("`set` must be an object".into()) };
        let mut v = serde_json::to_value(&*self).map_err(|e| e.to_string())?;
        if let Some(cur) = v.as_object_mut() {
            for (k, x) in obj {
                if k == "preset"
                    && let Some(s) = x.as_str()
                    && Preset::from_key(s).is_none()
                {
                    // a FilmCraft preset by its own name
                    cur.insert("filmcraftPreset".into(), json!(s.chars().take(200).collect::<String>()));
                    continue;
                }
                if k == "preset" {
                    cur.insert("filmcraftPreset".into(), Value::Null);
                }
                cur.insert(k.clone(), x.clone());
            }
        }
        *self = serde_json::from_value(v).map_err(|e| format!("set: {e}"))?;
        self.file_name = self.file_name.chars().take(200).collect();
        self.folder = self.folder.chars().take(4096).collect();
        Ok(())
    }
}

/// What the host turns into FilmCraft export settings (`export.queue.add`): the FilmCraft preset
/// plus the few overrides the simple page makes.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub filmcraft_preset: String,
    /// Output frame size; None = the preset's.
    pub frame_size: Option<(u32, u32)>,
    /// Video bitrate (kbps); None = the preset's.
    pub bitrate_kbps: Option<u32>,
    pub max_bitrate_kbps: Option<u32>,
    /// AAC bitrate (kbps) and sample rate; None = the preset's.
    pub audio_kbps: Option<u32>,
    pub sample_rate: Option<u32>,
    /// Loudness normalisation (target LUFS, true-peak ceiling dBTP); None = leave as mixed.
    pub loudness: Option<(f64, f64)>,
    /// Crop to fill (true) or fit with bars (false); None = the preset's.
    pub fill: Option<bool>,
    pub burn_captions: bool,
    pub caption_sidecar: Option<String>,
    pub hardware: bool,
    /// `entire` or `inOut`.
    pub range: String,
}

/// The FilmCraft settings for the person's choices.
pub fn recipe(c: &Choices) -> Recipe {
    let range = match c.how_much {
        HowMuch::Whole => "entire",
        HowMuch::InOut => "inOut",
    }
    .to_string();
    let captions = |r: &mut Recipe| match c.captions {
        Captions::Off => {}
        Captions::Burned => r.burn_captions = true,
        Captions::Srt => r.caption_sidecar = Some("srt".into()),
    };
    let loud = (c.loudness == Loudness::Platform).then_some((PLATFORM_LUFS, PLATFORM_TRUE_PEAK));
    let mut r = Recipe {
        filmcraft_preset: c.preset.filmcraft_preset().into(),
        frame_size: None,
        bitrate_kbps: None,
        max_bitrate_kbps: None,
        audio_kbps: None,
        sample_rate: None,
        loudness: None,
        fill: None,
        burn_captions: false,
        caption_sidecar: None,
        hardware: c.hardware,
        range,
    };
    if let Some(name) = &c.filmcraft_preset {
        // FilmCraft's own preset as it is: only the range, captions and hardware choice apply
        r.filmcraft_preset = name.clone();
        captions(&mut r);
        return r;
    }
    let pct = |kbps: u32| kbps.saturating_mul(c.quality.factor_percent()) / 100;
    match c.preset {
        Preset::Youtube => {
            let (w, h, kbps) = match c.size {
                Size::P720 => (1280, 720, 8_000),
                Size::P1080 => (1920, 1080, 16_000),
                Size::P2160 => (3840, 2160, 45_000),
            };
            r.frame_size = Some((w, h));
            r.bitrate_kbps = Some(pct(kbps));
            r.max_bitrate_kbps = Some(pct(kbps).saturating_mul(5) / 4);
            r.audio_kbps = Some(384);
            r.sample_rate = Some(48_000);
            r.loudness = loud;
            r.fill = Some(false);
            captions(&mut r);
        }
        Preset::Social => {
            let (w, h, kbps) = match c.size {
                Size::P720 => (720, 1280, 6_000),
                Size::P1080 => (1080, 1920, 12_000),
                Size::P2160 => (2160, 3840, 35_000),
            };
            r.frame_size = Some((w, h));
            r.bitrate_kbps = Some(pct(kbps));
            r.max_bitrate_kbps = Some(pct(kbps).saturating_mul(4) / 3);
            r.audio_kbps = Some(320);
            r.sample_rate = Some(48_000);
            r.loudness = loud;
            r.fill = Some(c.framing == Framing::Fill);
            captions(&mut r);
        }
        Preset::Master => {
            // ProRes 422 HQ, loudness left as mixed unless asked (doc 39)
            r.frame_size = Some(match c.size {
                Size::P720 => (1280, 720),
                Size::P1080 => (1920, 1080),
                Size::P2160 => (3840, 2160),
            });
            r.sample_rate = Some(48_000);
            r.loudness = None;
            r.fill = Some(false);
            if c.captions == Captions::Srt {
                r.caption_sidecar = Some("srt".into());
            }
        }
        Preset::Stems => {
            r.sample_rate = Some(48_000);
            // stems are never normalised one by one: together they must be the mix
            r.loudness = None;
        }
    }
    r
}

/// The three stems of Audio stems (doc 24 §6, doc 39): every audio track goes to exactly one, so
/// played together at full volume they are the video's sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Stem {
    Voice,
    Music,
    Effects,
}

impl Stem {
    pub const ALL: [Stem; 3] = [Stem::Voice, Stem::Music, Stem::Effects];
    pub fn file(self) -> &'static str {
        match self {
            Stem::Voice => "Voice",
            Stem::Music => "Music",
            Stem::Effects => "Effects",
        }
    }
    /// Which stem an audio track feeds, by its name ("Voice", "VO 2", "Music", "Score", "Sound"…).
    pub fn of_track(name: &str) -> Stem {
        let n = name.to_lowercase();
        let has = |w: &[&str]| w.iter().any(|x| n.contains(x));
        if has(&["voice", "dialog", "dialogue", "narrat", "vo ", "speech"]) || n == "vo" || n.starts_with("vo ") {
            Stem::Voice
        } else if has(&["music", "song", "score", "theme", "beat"]) {
            Stem::Music
        } else {
            Stem::Effects
        }
    }
}

// ---------------------------------------------------------------------------------------------
// files and folders
// ---------------------------------------------------------------------------------------------

/// The operating system, for labels and default folders.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Os {
    #[default]
    Linux,
    Mac,
    Windows,
}

impl Os {
    pub fn this() -> Self {
        if cfg!(target_os = "macos") {
            Os::Mac
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        }
    }
    /// "Show in Files" / "Show in Explorer" / "Show in Finder".
    pub fn reveal_label(self) -> &'static str {
        match self {
            Os::Linux => "Show in Files",
            Os::Mac => "Show in Finder",
            Os::Windows => "Show in Explorer",
        }
    }
}

/// `<Videos>/Mortiflix/<Project>` per OS (doc 39 "Where files go"). `home`: the home folder;
/// `xdg_videos`: `XDG_VIDEOS_DIR` already resolved (Linux), if set.
pub fn default_folder(os: Os, home: &str, xdg_videos: Option<&str>, project: &str) -> String {
    let project = safe_file_name(if project.trim().is_empty() { "Untitled" } else { project.trim() });
    let sep = if os == Os::Windows { '\\' } else { '/' };
    let videos = match os {
        Os::Mac => format!("{}{sep}Movies", home.trim_end_matches(sep)),
        Os::Windows => format!("{}{sep}Videos", home.trim_end_matches(sep)),
        Os::Linux => match xdg_videos.map(str::trim).filter(|d| !d.is_empty() && d.trim_end_matches('/') != home.trim_end_matches('/')) {
            Some(d) => d.trim_end_matches('/').to_string(),
            None => format!("{}/Videos", home.trim_end_matches('/')),
        },
    };
    format!("{videos}{sep}Mortiflix{sep}{project}")
}

/// `XDG_VIDEOS_DIR` from the text of `~/.config/user-dirs.dirs` (`"$HOME/Videos"` style).
pub fn xdg_videos_from_user_dirs(text: &str, home: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|l| l.starts_with("XDG_VIDEOS_DIR="))?;
    let v = line.trim_start_matches("XDG_VIDEOS_DIR=").trim().trim_matches('"');
    let v = v.replace("$HOME", home.trim_end_matches('/'));
    (v.starts_with('/') && !v.contains("..")).then_some(v)
}

/// A name that is safe as a file name on every OS (no separators, no reserved characters).
pub fn safe_file_name(n: &str) -> String {
    let s: String = n.chars().filter(|c| !c.is_control()).map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c }).take(160).collect();
    let s = s.trim().trim_matches('.').trim().to_string();
    if s.is_empty() { "Video".into() } else { s }
}

/// `stem.ext`, or `stem (2).ext`, `stem (3).ext`… when the name is taken: a render never
/// overwrites a file (doc 39). `taken` says whether a path exists.
pub fn unique_path(folder: &str, stem: &str, ext: &str, taken: &dyn Fn(&str) -> bool) -> String {
    let sep = if folder.contains('\\') && !folder.contains('/') { "\\" } else { "/" };
    let dir = folder.trim_end_matches(['/', '\\']);
    let join = |name: String| if dir.is_empty() { name } else { format!("{dir}{sep}{name}") };
    let dot = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    let first = join(format!("{stem}{dot}"));
    if !taken(&first) {
        return first;
    }
    for n in 2..1000 {
        let p = join(format!("{stem} ({n}){dot}"));
        if !taken(&p) {
            return p;
        }
    }
    join(format!("{stem} ({}){dot}", std::process::id()))
}

/// The last part of a path.
pub fn file_name_of(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

/// `1:32` / `1:02:03` for a duration in seconds (hostile values give `0:00`).
pub fn clock(seconds: f64) -> String {
    let s = if seconds.is_finite() && seconds > 0.0 { seconds.min(360_000.0).round() as u64 } else { 0 };
    let (h, m, sec) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{sec:02}") } else { format!("{m}:{sec:02}") }
}

/// `184 MB` style sizes.
pub fn bytes_label(b: u64) -> String {
    const K: f64 = 1000.0;
    let f = b as f64;
    if f >= K * K * K {
        format!("{:.1} GB", f / (K * K * K))
    } else if f >= K * K {
        format!("{:.0} MB", f / (K * K))
    } else if f >= K {
        format!("{:.0} KB", f / K)
    } else {
        format!("{b} bytes")
    }
}

// ---------------------------------------------------------------------------------------------
// the queue the page shows
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RowState {
    #[default]
    Waiting,
    Rendering,
    Done,
    Failed,
    Cancelled,
}

/// One row of the render queue: a FilmCraft export of this app or a Mortiflix service job.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct QueueRow {
    /// FilmCraft's queue id, or the service's job id when `service`.
    pub id: u64,
    pub service: bool,
    /// "The Rise of AI – YouTube.mp4", or the service's label ("Shot 7 v3 · new version").
    pub label: String,
    pub state: RowState,
    /// 0..1
    pub progress: f32,
    pub seconds_left: Option<f64>,
    /// Renders ahead of a waiting row.
    pub ahead: u32,
    pub path: String,
    pub error: String,
    /// Done, but the timeline changed since (doc 39 "made before your last edit").
    pub stale: bool,
    /// "15:38" for done rows.
    pub finished: String,
}

/// One job of the service's render queue (a `render.list` row or a `render.job` event: `{id, label,
/// state, progress: {percent, seconds_left}, renders_ahead, external, cancelled, error}`), as a row.
/// Hostile input: None when it doesn't parse; strings are capped.
pub fn service_row(j: &Value) -> Option<QueueRow> {
    let id = j.get("id").and_then(Value::as_u64)?;
    let cancelled = j.get("cancelled").and_then(Value::as_bool).unwrap_or(false);
    let state = match j.get("state").and_then(Value::as_str)? {
        "waiting" => RowState::Waiting,
        "running" => RowState::Rendering,
        "done" => RowState::Done,
        "failed" if cancelled => RowState::Cancelled,
        "failed" => RowState::Failed,
        _ => return None,
    };
    let progress = j.get("progress");
    let pct = progress.and_then(|p| p.get("percent")).and_then(Value::as_f64).filter(|p| p.is_finite()).unwrap_or(0.0).clamp(0.0, 100.0);
    Some(QueueRow {
        id,
        service: true,
        label: j.get("label").and_then(Value::as_str).unwrap_or("Mortiflix job").chars().take(120).collect(),
        state,
        progress: (pct / 100.0) as f32,
        seconds_left: progress.and_then(|p| p.get("seconds_left")).and_then(Value::as_f64).filter(|s| s.is_finite() && *s >= 0.0),
        ahead: j.get("renders_ahead").and_then(Value::as_u64).map_or(0, |a| u32::try_from(a.min(999)).unwrap_or(0)),
        error: j.get("error").and_then(Value::as_str).unwrap_or("").chars().take(200).collect(),
        path: j.get("file").and_then(Value::as_str).unwrap_or("").chars().take(4096).collect(),
        ..QueueRow::default()
    })
}

/// The service's render queue (`render.list`: an array of jobs), as rows, without the app's own
/// export jobs (`own`: their service ids), which show as the app's rows.
pub fn service_rows(result: &Value, own: &[u64]) -> Vec<QueueRow> {
    let Some(list) = result.as_array().or_else(|| result.get("jobs").and_then(Value::as_array)) else { return Vec::new() };
    list.iter().rev().take(200).rev().filter_map(service_row).filter(|r| !own.contains(&r.id)).collect()
}

/// Whether one of the service's heavy jobs is running now: the app's next export waits for it.
pub fn service_busy(rows: &[QueueRow]) -> bool {
    rows.iter().any(|r| r.service && r.state == RowState::Rendering)
}

/// One queue for the whole PC: the service's running job first, then the app's and the service's
/// rows in order; waiting rows get how many renders are ahead of them.
pub fn merge_queue(app: &[QueueRow], service: &[QueueRow]) -> Vec<QueueRow> {
    let mut out: Vec<QueueRow> = Vec::new();
    out.extend(service.iter().filter(|r| r.state == RowState::Rendering).cloned());
    out.extend(app.iter().filter(|r| r.state == RowState::Rendering).cloned());
    out.extend(app.iter().filter(|r| r.state == RowState::Waiting).cloned());
    out.extend(service.iter().filter(|r| r.state == RowState::Waiting).cloned());
    let mut ahead = 0u32;
    for r in &mut out {
        if r.state == RowState::Waiting {
            r.ahead = ahead;
        }
        ahead = ahead.saturating_add(1);
    }
    out.extend(app.iter().filter(|r| !matches!(r.state, RowState::Rendering | RowState::Waiting)).cloned());
    out.extend(service.iter().filter(|r| !matches!(r.state, RowState::Rendering | RowState::Waiting)).take(5).cloned());
    out
}

// ---------------------------------------------------------------------------------------------
// renders of this project
// ---------------------------------------------------------------------------------------------

/// A check on a finished file (doc 39 "Checks").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    /// The plain line ("1:32.00", "Peaks reach −0.4 dB: louder than YouTube allows…").
    pub text: String,
}

/// A file this project rendered.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RenderRecord {
    /// The FilmCraft queue id(s) that made it (stems: three).
    pub queue_ids: Vec<u64>,
    pub title: String,
    /// The choices it was made with (Render again uses them).
    pub choices: Choices,
    /// The file, or for stems the folder.
    pub path: String,
    /// Every file written (stems: Voice, Music, Effects).
    pub files: Vec<String>,
    /// Seconds since 1970 when it finished; 0 while rendering.
    pub finished_at: u64,
    /// "15:42" local-ish clock the host formats.
    pub finished_label: String,
    /// The timeline it was made from (a hash; another one now = "made before your last edit").
    pub timeline: u64,
    pub bytes: u64,
    pub seconds: f64,
    pub width: u32,
    pub height: u32,
    pub checks: Vec<Check>,
    /// Failed with this reason ("" = fine).
    pub error: String,
    /// The file is gone (moved or deleted outside the app).
    pub missing: bool,
}

impl RenderRecord {
    pub fn done(&self) -> bool {
        self.finished_at > 0 && self.error.is_empty()
    }
    pub fn stale(&self, timeline: u64) -> bool {
        self.done() && self.timeline != timeline
    }
}

/// Which page the Export tab shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Page {
    /// 39a: pick, set, Render.
    #[default]
    Settings,
    /// 39b: the finished render (index into `renders`).
    Rendered,
}

/// A question the page asks before doing something it can't undo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Confirm {
    /// Stop the render (the half-written file is deleted).
    Stop { id: u64, label: String },
    /// Post the video (doc 39: always asks; only the person's click posts).
    Post,
    /// The timeline has offline media / nothing yet.
    Note { text: String },
}

/// The platforms Upload-Post and Postiz can post to (the service's `publish.mjs` lists).
pub const POST_PLATFORMS: [&str; 8] = ["youtube", "tiktok", "instagram", "linkedin", "x", "facebook", "threads", "bluesky"];

/// Post it (doc 39): only shown when posting is set up through the service.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PostForm {
    /// `uploadpost`, `postiz`, `set` (set up, service not named yet) or `none` (from `publish.plan`);
    /// empty = not asked yet.
    pub service: String,
    pub service_name: String,
    /// The platforms the service can post to, and the ones ticked.
    pub platforms: Vec<String>,
    pub picked: Vec<String>,
    pub title: String,
    pub description: String,
    /// Empty = now; else an ISO date and time ("2026-10-20T18:00").
    pub at: String,
    pub later: bool,
    /// One line per account after posting, or the service's error.
    pub results: Vec<String>,
    pub sending: bool,
}

/// The Export page's own state (kept by the host per app; serde for the control channel).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExportState {
    pub choices: Choices,
    pub page: Page,
    /// The render the Rendered page shows (index into `renders`).
    pub shown: usize,
    pub renders: Vec<RenderRecord>,
    pub advanced_open: bool,
    pub all_formats_open: bool,
    /// FilmCraft's full Export settings (Advanced › All FilmCraft settings…) instead of this page.
    pub full_settings: bool,
    pub confirm: Option<Confirm>,
    /// Get the video: render the YouTube video as soon as the timeline is there.
    pub pending_get_video: bool,
    /// The service's render queue without the app's own exports (`render.list` + `render.job`).
    pub service_jobs: Vec<QueueRow>,
    /// Seconds of render time per second of video, learned from finished renders (0 = unknown).
    pub speed: f64,
    pub post: PostForm,
    /// The last thing that went wrong, shown under Render.
    pub note: String,
    /// Requests in flight: (service request id, what for).
    #[serde(skip)]
    pub pending: Vec<(u64, Pending)>,
    #[serde(skip)]
    pub jobs_polled_at: f64,
    /// The last service event read.
    #[serde(skip)]
    pub event_seq: u64,
    /// The service ids of the app's own export jobs (`render.add`): shown as the app's rows.
    #[serde(skip)]
    pub own_service_ids: Vec<u64>,
    /// For the host: `render.add` replies (render index, the job or None when refused) and the
    /// `render.job` events of this poll.
    #[serde(skip)]
    pub added: Vec<(usize, Option<QueueRow>)>,
    #[serde(skip)]
    pub job_events: Vec<QueueRow>,
}

/// What a service request in flight was for.
#[derive(Clone, Debug, PartialEq)]
pub enum Pending {
    Jobs,
    /// `render.add` for render `n`.
    Add(usize),
    PublishSetup,
    Post,
    /// Fire and forget (`render.progress`, `render.finish`, `export.done`, `render.cancel`).
    Other,
}

impl ExportState {
    /// Remember a render; the list is bounded (oldest dropped).
    pub fn push_render(&mut self, r: RenderRecord) -> usize {
        self.renders.push(r);
        if self.renders.len() > MAX_RENDERS {
            let extra = self.renders.len() - MAX_RENDERS;
            self.renders.drain(..extra);
            self.shown = self.shown.saturating_sub(extra);
        }
        self.renders.len().saturating_sub(1)
    }
    /// About how long a render of `seconds` of video takes on this PC (None = not known yet).
    pub fn time_estimate(&self, seconds: f64) -> Option<f64> {
        (self.speed > 0.0 && self.speed.is_finite() && seconds > 0.0).then_some(seconds * self.speed)
    }
    /// Learn from a finished render (`took` seconds for `seconds` of video).
    pub fn learn_speed(&mut self, took: f64, seconds: f64) {
        if !(took.is_finite() && seconds.is_finite() && took > 0.0 && seconds > 0.1) {
            return;
        }
        let s = (took / seconds).clamp(0.01, 1000.0);
        self.speed = if self.speed > 0.0 { self.speed * 0.6 + s * 0.4 } else { s };
    }

    /// Send a request whose reply only matters when it fails (it goes to the note line).
    pub fn send(&mut self, service: &mut crate::Service, ty: &str, params: Value) {
        if service.status().is_connected() {
            let id = service.request(ty, params);
            self.pending.push((id, Pending::Other));
        }
    }

    /// Apply a `render.job` event: the service queue's row (or, for the app's own jobs, a job event
    /// for the host). Hostile input: anything malformed is ignored.
    pub fn apply_event(&mut self, ev: &Value) {
        if ev.get("event").and_then(Value::as_str) != Some("render.job") {
            return;
        }
        let Some(row) = ev.get("data").and_then(service_row) else { return };
        if self.own_service_ids.contains(&row.id) {
            if self.job_events.len() < 256 {
                self.job_events.push(row);
            }
            return;
        }
        if let Some(r) = self.service_jobs.iter_mut().find(|r| r.id == row.id) {
            *r = row;
        } else if self.service_jobs.len() < 200 {
            self.service_jobs.push(row);
        }
    }

    /// Read the replies this page asked for and the service's `render.job` events.
    pub fn poll_service(&mut self, service: &mut crate::Service) {
        let pending = std::mem::take(&mut self.pending);
        for (id, what) in pending {
            let Some(r) = service.take_reply(id) else {
                self.pending.push((id, what));
                continue;
            };
            let message =
                || -> String { r.error.get("message").and_then(Value::as_str).unwrap_or("the Mortiflix service said no").chars().take(300).collect() };
            match what {
                Pending::Jobs => self.service_jobs = if r.ok { service_rows(&r.result, &self.own_service_ids) } else { Vec::new() },
                Pending::Add(i) => {
                    let row = if r.ok { service_row(&r.result) } else { None };
                    if let Some(row) = &row {
                        self.own_service_ids.push(row.id);
                        if self.own_service_ids.len() > 500 {
                            self.own_service_ids.remove(0);
                        }
                    }
                    self.added.push((i, row));
                }
                Pending::PublishSetup => {
                    let msg = message();
                    let (service_key, platforms): (String, Vec<String>) = if r.ok {
                        let svc = r.result.get("service").and_then(Value::as_str).unwrap_or("set").chars().take(20).collect();
                        (
                            svc,
                            r.result
                                .get("platforms")
                                .and_then(Value::as_array)
                                .map(|a| a.iter().filter_map(Value::as_str).take(20).map(|s| s.chars().take(30).collect()).collect())
                                .unwrap_or_default(),
                        )
                    } else if r.error.get("code").and_then(Value::as_str) == Some("no_project") {
                        ("noProject".into(), Vec::new())
                    } else if msg.contains("isn't set up") || msg.contains("not set up") {
                        ("none".into(), Vec::new())
                    } else {
                        // set up; it only needs to know where to (the accounts are picked here)
                        ("set".into(), Vec::new())
                    };
                    self.post.service_name = match service_key.as_str() {
                        "uploadpost" => "Upload-Post".into(),
                        "postiz" => "Postiz".into(),
                        _ => String::new(),
                    };
                    self.post.service = service_key;
                    self.post.platforms = POST_PLATFORMS.iter().map(|s| s.to_string()).collect();
                    if self.post.picked.is_empty() {
                        self.post.picked = platforms.into_iter().filter(|p| self.post.platforms.contains(p)).collect();
                    }
                }
                Pending::Post => {
                    self.post.sending = false;
                    self.post.results = if r.ok {
                        let list = r
                            .result
                            .get("platforms")
                            .and_then(Value::as_array)
                            .map(|a| a.iter().filter_map(Value::as_str).take(20).map(str::to_string).collect::<Vec<_>>());
                        list.unwrap_or_else(|| self.post.picked.clone())
                            .iter()
                            .map(|p| format!("Posted to {}", p.chars().take(30).collect::<String>()))
                            .collect()
                    } else {
                        vec![message()]
                    };
                }
                Pending::Other => {
                    if !r.ok && r.error.get("code").and_then(Value::as_str) != Some("not_running") {
                        self.note = message();
                    }
                }
            }
        }
        let events: Vec<(u64, Value)> = service.events_since(self.event_seq).take(512).cloned().collect();
        for (seq, ev) in events {
            self.event_seq = self.event_seq.max(seq);
            self.apply_event(&ev);
        }
        // a reply that never comes (a stuck service) must not grow this list forever
        if self.pending.len() > 64 {
            let extra = self.pending.len() - 64;
            self.pending.drain(..extra);
        }
    }

    /// Ask the service for its render queue every few seconds (`now`: seconds).
    pub fn poll_jobs(&mut self, service: &mut crate::Service, now: f64) {
        if now - self.jobs_polled_at < JOBS_POLL_SECONDS && now >= self.jobs_polled_at {
            return;
        }
        self.jobs_polled_at = now;
        if !service.status().is_connected() {
            self.service_jobs.clear();
            return;
        }
        if self.pending.iter().any(|p| p.1 == Pending::Jobs) {
            return;
        }
        let id = service.request("render.list", json!({}));
        self.pending.push((id, Pending::Jobs));
    }
}

// ---------------------------------------------------------------------------------------------
// what the host gives the page, and what the page asks the host to do
// ---------------------------------------------------------------------------------------------

/// What the host knows about the timeline and FilmCraft this frame.
#[derive(Clone, Debug, Default)]
pub struct TabView {
    /// Something on the timeline to render.
    pub has_sequence: bool,
    pub project: String,
    /// The timeline's length and the In/Out length (None = no In/Out).
    pub seconds: f64,
    pub in_out_seconds: Option<f64>,
    pub seq_width: u32,
    pub seq_height: u32,
    /// Estimated bytes for each of the four presets ([`Preset::ALL`] order), and for the pick.
    pub estimates: [u64; 4],
    pub estimate: u64,
    /// Output size of the pick (0 = audio only).
    pub out_width: u32,
    pub out_height: u32,
    /// "H.264 · 16 Mbps · AAC 384 kbps · −14 LUFS"
    pub summary: String,
    /// The app's FilmCraft export queue as rows.
    pub queue: Vec<QueueRow>,
    pub default_folder: String,
    pub os: Os,
    /// FilmCraft's own presets: (category, name).
    pub all_formats: Vec<(String, String)>,
    /// The timeline's current hash (renders made from another are stale).
    pub timeline: u64,
    /// Audio track names and the stem each feeds.
    pub audio_tracks: Vec<(String, Stem)>,
    /// Media the timeline uses that is offline ("Shot 9's file is missing").
    pub offline: Vec<String>,
}

/// What the page asks the host to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Render the current choices.
    Render,
    /// Render again with a remembered render's choices.
    RenderAgain(usize),
    /// Stop (cancel) a FilmCraft queue item; the half-written file is deleted.
    Stop(u64),
    Remove(u64),
    Retry(u64),
    Open(String),
    Reveal(String),
    SaveCopy(String),
    ChangeFolder,
    /// Advanced › All FilmCraft settings…: FilmCraft's full Export page.
    FullSettings,
    /// Change something: the Edit tab (stage 5).
    EditTab,
    NewProject,
    /// Post the video on the Rendered page (after the confirm dialog).
    Post(usize),
    SetupPosting,
}

/// What [`show_tab`] drew and wants.
#[derive(Default)]
pub struct TabOut {
    pub actions: Vec<Action>,
    /// Where the host draws the timeline picture, and whether it fills (crops) the frame.
    pub preview: Option<(Rect, bool)>,
}

// ---------------------------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------------------------

/// What PanelKind::MfxStage shows on stage 6 (when that panel is docked somewhere): stage 6 itself
/// is FilmCraft's Export tab, so this says where the renders are and offers to go there.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let t = ctx.theme;
    ui.label(egui::RichText::new("Render").size(15.0).color(t.text));
    ui.label(egui::RichText::new("Pick where the video goes and render it in the Export tab.").color(t.text_dim));
    let r = ui.button("Open the Export tab");
    ctx.auto("mfx.export.openTab", &r, "Open the Export tab");
    if r.clicked() {
        ctx.command("mode.export", json!({}));
    }
}

const LEFT_W: f32 = 320.0;
const RIGHT_W: f32 = 420.0;
const QUEUE_H: f32 = 232.0;
const GAP: f32 = 4.0;

/// The Mortiflix Export tab in `rect` (39a, or 39b when a render finished).
pub fn show_tab(ui: &mut egui::Ui, rect: Rect, ctx: &mut MfxCtx, st: &mut ExportState, v: &TabView) -> TabOut {
    let mut out = TabOut::default();
    let t = ctx.theme;
    ui.painter().rect_filled(rect, 0.0, t.app_bg);
    if st.page == Page::Rendered && st.renders.get(st.shown).is_some() {
        rendered_page(ui, rect, ctx, st, v, &mut out);
    } else {
        st.page = Page::Settings;
        settings_page(ui, rect, ctx, st, v, &mut out);
    }
    confirm_dialog(ui, ctx, st, &mut out);
    out
}

fn panel(ui: &egui::Ui, r: Rect, t: &MfxTheme) {
    ui.painter().rect_filled(r, 4.0, t.panel_bg);
}

/// A child ui for a panel body with a heading (and an optional faint note after it).
fn body(ui: &mut egui::Ui, r: Rect, t: &MfxTheme, title: &str, note: &str, salt: &str) -> egui::Ui {
    panel(ui, r, t);
    let p = ui.painter();
    p.text(r.min + vec2(14.0, 18.0), Align2::LEFT_CENTER, title, FontId::proportional(13.0), t.text);
    if !note.is_empty() {
        let w = p.layout_no_wrap(title.to_string(), FontId::proportional(13.0), t.text).size().x;
        p.text(r.min + vec2(24.0 + w, 18.0), Align2::LEFT_CENTER, note, FontId::proportional(12.0), t.text_faint);
    }
    let inner = Rect::from_min_max(r.min + vec2(12.0, 34.0), r.max - vec2(12.0, 8.0));
    let mut c = ui.new_child(egui::UiBuilder::new().max_rect(inner).id_salt(salt).layout(egui::Layout::top_down(egui::Align::Min)));
    c.set_clip_rect(inner.intersect(ui.clip_rect()));
    c
}

fn settings_page(ui: &mut egui::Ui, rect: Rect, ctx: &mut MfxCtx, st: &mut ExportState, v: &TabView, out: &mut TabOut) {
    let narrow = rect.width() < LEFT_W + RIGHT_W + 360.0;
    let (lw, rw) = if narrow { (rect.width() * 0.26, rect.width() * 0.34) } else { (LEFT_W, RIGHT_W) };
    let left = Rect::from_min_size(rect.min, vec2(lw, rect.height()));
    let right = Rect::from_min_max(pos2(rect.max.x - rw, rect.min.y), rect.max);
    let mid = Rect::from_min_max(pos2(left.max.x + GAP, rect.min.y), pos2(right.min.x - GAP, rect.max.y));
    let qh = QUEUE_H.min(mid.height() * 0.45);
    let preview = Rect::from_min_max(mid.min, pos2(mid.max.x, mid.max.y - qh - GAP));
    let queue = Rect::from_min_max(pos2(mid.min.x, preview.max.y + GAP), mid.max);

    where_it_goes(ui, left, ctx, st, v);
    preview_panel(ui, preview, ctx, st, v, out);
    queue_panel(ui, queue, ctx, st, v, out);
    settings_panel(ui, right, ctx, st, v, out);
}

fn where_it_goes(ui: &mut egui::Ui, r: Rect, ctx: &mut MfxCtx, st: &mut ExportState, v: &TabView) {
    let t = ctx.theme;
    let mut c = body(ui, r, &t, "Where it goes", "", "mfx-export-where");
    for (i, p) in Preset::ALL.into_iter().enumerate() {
        let on = st.choices.filmcraft_preset.is_none() && st.choices.preset == p;
        let dims = recipe(&Choices { preset: p, filmcraft_preset: None, ..st.choices.clone() }).frame_size.map(|(w, h)| format!("{w}×{h}")).unwrap_or_default();
        let (line, sub) = match p {
            Preset::Youtube => (format!("{dims} · MP4"), ""),
            Preset::Social => (format!("{dims} · MP4"), "TikTok, Reels, Shorts"),
            Preset::Master => ("ProRes 422 HQ · MOV".to_string(), "to keep, or for another editor"),
            Preset::Stems => ("Voice, Music, Effects · 3 WAV files".to_string(), ""),
        };
        let h = if sub.is_empty() { 58.0 } else { 74.0 };
        let (row, resp) = c.allocate_exact_size(vec2(c.available_width(), h), Sense::click());
        let p_ = c.painter();
        if on {
            p_.rect_filled(row, 0.0, t.hover);
            p_.line_segment([row.left_top(), row.left_bottom()], Stroke::new(2.0, t.text));
        } else if resp.hovered() {
            p_.rect_filled(row, 0.0, t.hover.gamma_multiply(0.6));
        }
        let dot = pos2(row.min.x + 18.0, row.min.y + 20.0);
        p_.circle_stroke(dot, 7.5, Stroke::new(1.0, if on { t.text } else { t.text_faint }));
        if on {
            p_.circle_filled(dot, 4.0, t.text);
        }
        let x = row.min.x + 36.0;
        p_.text(pos2(x, row.min.y + 20.0), Align2::LEFT_CENTER, p.label(), FontId::proportional(13.5), t.text);
        p_.text(pos2(x, row.min.y + 38.0), Align2::LEFT_CENTER, line, FontId::proportional(12.0), t.text_dim);
        let mut y = row.min.y + 38.0;
        if !sub.is_empty() {
            y += 16.0;
            p_.text(pos2(x, y), Align2::LEFT_CENTER, sub, FontId::proportional(12.0), t.text_dim);
        }
        if v.has_sequence {
            let est = v.estimates.get(i).copied().unwrap_or(0);
            if est > 0 {
                p_.text(
                    pos2(row.max.x - 8.0, row.min.y + 20.0),
                    Align2::RIGHT_CENTER,
                    format!("about {}", bytes_label(est)),
                    FontId::monospace(11.0),
                    t.text_faint,
                );
            }
        }
        ctx.auto(&format!("mfx.export.preset.{}", p.key()), &resp, p.label());
        if resp.clicked() {
            st.choices.preset = p;
            st.choices.filmcraft_preset = None;
        }
    }
    c.add_space(6.0);
    c.separator();
    let n = v.all_formats.len();
    let label = format!("All FilmCraft formats ({n}) {}", if st.all_formats_open { "⌄" } else { "›" });
    let r = c.add(egui::Button::new(egui::RichText::new(label).color(t.text_dim).size(12.0)).frame(false));
    ctx.auto("mfx.export.allFormats", &r, "All FilmCraft formats");
    if r.clicked() {
        st.all_formats_open = !st.all_formats_open;
    }
    if st.all_formats_open {
        egui::ScrollArea::vertical().id_salt("mfx-export-formats").auto_shrink([false, false]).show(&mut c, |ui| {
            let mut last = "";
            for (cat, name) in &v.all_formats {
                if cat != last {
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(cat).size(11.5).color(t.text_faint));
                    last = cat;
                }
                let on = st.choices.filmcraft_preset.as_deref() == Some(name.as_str());
                let r = ui.selectable_label(on, egui::RichText::new(name).size(12.0));
                ctx.auto(&format!("mfx.export.format.{name}"), &r, name);
                if r.clicked() {
                    st.choices.filmcraft_preset = Some(name.clone());
                }
            }
        });
    }
}

fn preview_panel(ui: &mut egui::Ui, r: Rect, ctx: &mut MfxCtx, st: &mut ExportState, v: &TabView, out: &mut TabOut) {
    let t = ctx.theme;
    let what = if v.out_width > 0 {
        let crop = if st.choices.filmcraft_preset.is_none() && st.choices.preset == Preset::Social {
            match st.choices.framing {
                Framing::Fill => ", sides cropped",
                Framing::Fit => ", bars top and bottom",
            }
        } else {
            ""
        };
        format!("What {} gets: {}×{}{crop}", st.choices.title(), v.out_width, v.out_height)
    } else if st.choices.filmcraft_preset.is_none() && st.choices.preset == Preset::Stems {
        "What Audio stems gets: 3 files, same length".to_string()
    } else {
        format!("What {} gets: sound only", st.choices.title())
    };
    let mut c = body(ui, r, &t, "Preview", &what, "mfx-export-preview");
    let area = c.max_rect();
    if !v.has_sequence {
        c.painter().text(area.center(), Align2::CENTER_CENTER, "Nothing to render yet. Finish the shots first.", FontId::proportional(14.0), t.text_dim);
        let b = c.put(Rect::from_center_size(area.center() + vec2(0.0, 30.0), vec2(220.0, 26.0)), egui::Button::new("Back to the stages"));
        ctx.auto("mfx.export.backToStages", &b, "Back to the stages");
        if b.clicked() {
            ctx.command("stages.voiceMusic", json!({}));
        }
        return;
    }
    let pic_area = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - 40.0));
    if v.out_width > 0 && v.out_height > 0 {
        let aspect = v.out_width as f32 / v.out_height as f32;
        let (mut w, mut h) = (pic_area.width(), pic_area.width() / aspect.max(0.01));
        if h > pic_area.height() {
            h = pic_area.height();
            w = h * aspect;
        }
        let pic = Rect::from_center_size(pic_area.center(), vec2(w.max(1.0), h.max(1.0)));
        c.painter().rect_filled(pic, 0.0, Color32::BLACK);
        let fill = st.choices.filmcraft_preset.is_some() || st.choices.preset != Preset::Social || st.choices.framing == Framing::Fill;
        out.preview = Some((pic, fill));
    } else {
        // Audio stems / sound only: one lane per stem with the tracks feeding it
        let lane_h = 48.0;
        let top = pic_area.center().y - lane_h * 1.5 - 8.0;
        for (i, s) in Stem::ALL.into_iter().enumerate() {
            let y = top + i as f32 * (lane_h + 8.0);
            let lane = Rect::from_min_size(pos2(pic_area.min.x + 8.0, y), vec2(pic_area.width() - 16.0, lane_h));
            c.painter().rect_filled(lane, 2.0, t.field_bg);
            c.painter().text(lane.left_center() + vec2(12.0, 0.0), Align2::LEFT_CENTER, s.file(), FontId::proportional(12.5), t.text);
            let names: Vec<&str> = v.audio_tracks.iter().filter(|(_, x)| *x == s).map(|(n, _)| n.as_str()).collect();
            let text = if names.is_empty() { "silent: no track feeds it".to_string() } else { names.join(", ") };
            c.painter().text(lane.left_center() + vec2(92.0, 0.0), Align2::LEFT_CENTER, text, FontId::proportional(12.0), t.text_dim);
        }
    }
    let foot = Rect::from_min_max(pos2(area.min.x, area.max.y - 34.0), area.max);
    let secs = match st.choices.how_much {
        HowMuch::InOut => v.in_out_seconds.unwrap_or(v.seconds),
        HowMuch::Whole => v.seconds,
    };
    c.painter().text(foot.left_top() + vec2(0.0, 8.0), Align2::LEFT_CENTER, "0:00", FontId::monospace(12.0), t.text_dim);
    c.painter().text(pos2(foot.max.x, foot.min.y + 8.0), Align2::RIGHT_CENTER, clock(secs), FontId::monospace(12.0), t.text_dim);
    c.painter().text(
        pos2(foot.center().x, foot.min.y + 26.0),
        Align2::CENTER_CENTER,
        "Plays exactly the timeline from the Edit tab. Nothing is re-made.",
        FontId::proportional(12.0),
        t.text_faint,
    );
}

fn state_word(r: &QueueRow) -> (String, Color32, bool) {
    match r.state {
        RowState::Rendering => (format!("Rendering {:.0}%", (r.progress * 100.0).clamp(0.0, 100.0)), Color32::from_rgb(80, 190, 110), true),
        RowState::Waiting if r.ahead == 0 => ("Waiting".into(), Color32::from_gray(150), false),
        RowState::Waiting => (format!("Waiting · {} ahead", r.ahead), Color32::from_gray(150), false),
        RowState::Done => (if r.finished.is_empty() { "Done".into() } else { format!("Done {}", r.finished) }, Color32::from_gray(150), false),
        RowState::Failed => (format!("Failed: {}", if r.error.is_empty() { "see the log" } else { &r.error }), Color32::from_rgb(230, 80, 70), false),
        RowState::Cancelled => ("Stopped".into(), Color32::from_gray(120), false),
    }
}

fn queue_panel(ui: &mut egui::Ui, r: Rect, ctx: &mut MfxCtx, st: &mut ExportState, v: &TabView, out: &mut TabOut) {
    let t = ctx.theme;
    let rows = merge_queue(&v.queue, &st.service_jobs);
    let mut c = body(ui, r, &t, "Render queue", "one at a time on this PC", "mfx-export-queue");
    if rows.is_empty() {
        c.add_space(8.0);
        c.label(egui::RichText::new("Nothing rendering. Pick where it goes, then Render.").color(t.text));
        c.label(egui::RichText::new("Renders on this PC run one at a time, so they never fight for memory.").color(t.text_dim).size(12.0));
        return;
    }
    egui::ScrollArea::vertical().id_salt("mfx-export-queue-rows").auto_shrink([false, false]).show(&mut c, |ui| {
        for (i, row) in rows.iter().enumerate() {
            let fill = if i % 2 == 1 { t.field_bg } else { Color32::TRANSPARENT };
            egui::Frame::NONE.fill(fill).inner_margin(egui::Margin::symmetric(8, 6)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let label_w = (ui.available_width() * 0.36).max(120.0);
                    left_label(ui, label_w, egui::RichText::new(&row.label).color(t.text).size(12.5));
                    let (word, col, bar) = state_word(row);
                    let (dot, _) = ui.allocate_exact_size(vec2(10.0, 18.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, col);
                    ui.label(egui::RichText::new(word).size(12.0).color(if row.state == RowState::Failed { col } else { t.text_dim }));
                    if bar {
                        ui.add(egui::ProgressBar::new(row.progress.clamp(0.0, 1.0)).desired_width(120.0).desired_height(4.0));
                    }
                    if row.stale {
                        ui.label(egui::RichText::new("made before your last edit").size(12.0).color(t.warn));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let key = |a: &str| format!("mfx.export.queue.{}{}.{a}", if row.service { "s" } else { "" }, row.id);
                        if !row.service {
                            match row.state {
                                RowState::Rendering => {
                                    let b = ui.small_button("Stop");
                                    ctx.auto(&key("stop"), &b, "Stop");
                                    if b.clicked() {
                                        st.confirm = Some(Confirm::Stop { id: row.id, label: row.label.clone() });
                                    }
                                }
                                RowState::Waiting => {
                                    let b = ui.small_button("Remove");
                                    ctx.auto(&key("remove"), &b, "Remove");
                                    if b.clicked() {
                                        out.actions.push(Action::Remove(row.id));
                                    }
                                }
                                RowState::Done => {
                                    let b = ui.small_button(v.os.reveal_label());
                                    ctx.auto(&key("show"), &b, v.os.reveal_label());
                                    if b.clicked() {
                                        out.actions.push(Action::Reveal(row.path.clone()));
                                    }
                                    let b = ui.small_button("Open");
                                    ctx.auto(&key("open"), &b, "Open");
                                    if b.clicked() {
                                        out.actions.push(Action::Open(row.path.clone()));
                                    }
                                }
                                RowState::Failed | RowState::Cancelled => {
                                    let b = ui.small_button("Try again");
                                    ctx.auto(&key("retry"), &b, "Try again");
                                    if b.clicked() {
                                        out.actions.push(Action::Retry(row.id));
                                    }
                                    let b = ui.small_button("Remove");
                                    ctx.auto(&key("remove"), &b, "Remove");
                                    if b.clicked() {
                                        out.actions.push(Action::Remove(row.id));
                                    }
                                }
                            }
                        }
                        let tail = match (row.state, row.seconds_left) {
                            (RowState::Rendering, Some(s)) => format!("{} left", clock(s)),
                            (RowState::Waiting, Some(s)) => format!("about {}", clock(s)),
                            _ => String::new(),
                        };
                        if !tail.is_empty() {
                            ui.label(egui::RichText::new(tail).monospace().size(12.0).color(t.text_dim));
                        }
                    });
                });
            });
        }
    });
}

/// A label of width `w`, left-aligned, cut with … when longer.
fn left_label(ui: &mut egui::Ui, w: f32, text: egui::RichText) {
    ui.allocate_ui_with_layout(vec2(w, 18.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(w);
        ui.set_max_width(w);
        ui.add(egui::Label::new(text).truncate());
    });
}

/// A segmented control: one of `opts` (key, label). Returns the picked index when clicked.
fn seg(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str, opts: &[(&str, &str)], on: usize) -> Option<usize> {
    let t = ctx.theme;
    let w = ui.available_width();
    let n = opts.len().max(1) as f32;
    let (r, _) = ui.allocate_exact_size(vec2(w, 28.0), Sense::hover());
    ui.painter().rect(r, 4.0, t.field_bg, Stroke::new(1.0, t.separator), egui::StrokeKind::Inside);
    let mut picked = None;
    for (i, (key, label)) in opts.iter().enumerate() {
        let cell = Rect::from_min_size(pos2(r.min.x + i as f32 * r.width() / n, r.min.y), vec2(r.width() / n, r.height()));
        let resp = ui.interact(cell, egui::Id::new((id, i)), Sense::click());
        if i == on {
            ui.painter().rect_filled(cell.shrink(1.0), 3.0, t.hover);
        } else if resp.hovered() {
            ui.painter().rect_filled(cell.shrink(1.0), 3.0, t.hover.gamma_multiply(0.5));
        }
        if i > 0 {
            ui.painter().line_segment([cell.left_top(), cell.left_bottom()], Stroke::new(1.0, t.separator));
        }
        ui.painter().with_clip_rect(cell).text(
            cell.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(12.5),
            if i == on { t.text } else { t.text_dim },
        );
        ctx.auto(&format!("{id}.{key}"), &resp, label);
        if resp.clicked() {
            picked = Some(i);
        }
    }
    picked
}

fn field_label(ui: &mut egui::Ui, t: &MfxTheme, s: &str) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(s).size(12.0).color(t.text_dim));
}

fn settings_panel(ui: &mut egui::Ui, r: Rect, ctx: &mut MfxCtx, st: &mut ExportState, v: &TabView, out: &mut TabOut) {
    let t = ctx.theme;
    let foot_h = 150.0;
    let top = Rect::from_min_max(r.min, pos2(r.max.x, r.max.y - foot_h));
    let foot = Rect::from_min_max(pos2(r.min.x, r.max.y - foot_h), r.max);
    panel(ui, r, &t);
    let title = st.choices.title();
    let mut c = body(ui, top, &t, "Settings", &title, "mfx-export-settings");
    let own = st.choices.filmcraft_preset.is_none();
    let p = st.choices.preset;
    egui::ScrollArea::vertical().id_salt("mfx-export-settings-scroll").auto_shrink([false, false]).show(&mut c, |ui| {
        ui.set_width(ui.available_width() - 6.0);
        // File name
        field_label(ui, &t, "File name");
        let ext = if own { p.extension().to_string() } else { String::new() };
        ui.horizontal(|ui| {
            let mut name = if st.choices.file_name.is_empty() { st.choices.file_stem(&v.project) } else { st.choices.file_name.clone() };
            let w = (ui.available_width() - 56.0).max(80.0);
            let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(w));
            ctx.auto("mfx.export.fileName", &r, "File name");
            if r.changed() {
                st.choices.file_name = name.chars().take(200).collect();
            }
            let e = if own && p == Preset::Stems { "folder".to_string() } else if ext.is_empty() { String::new() } else { format!(".{ext}") };
            ui.label(egui::RichText::new(e).size(12.0).color(t.text_faint));
        });
        // Save to
        field_label(ui, &t, "Save to");
        ui.horizontal(|ui| {
            let folder = st.choices.folder_or(&v.default_folder).to_string();
            let w = (ui.available_width() - 90.0).max(80.0);
            left_label(ui, w, egui::RichText::new(&folder).size(12.0).color(t.text));
            let b = ui.button("Change…");
            ctx.auto("mfx.export.folder.change", &b, "Change…");
            if b.clicked() {
                out.actions.push(Action::ChangeFolder);
            }
        });
        ui.label(egui::RichText::new("Change… opens your computer's folder picker.").size(11.5).color(t.text_faint));
        if own && p.has_video() {
            field_label(ui, &t, "Size");
            let opts: Vec<(&str, &str)> = Size::ALL.iter().map(|s| (s.key(), s.label())).collect();
            let on = Size::ALL.iter().position(|s| *s == st.choices.size).unwrap_or(1);
            if let Some(i) = seg(ui, ctx, "mfx.export.size", &opts, on) {
                st.choices.size = Size::ALL.get(i).copied().unwrap_or_default();
            }
            let short = v.seq_width.min(v.seq_height);
            if short > 0 && st.choices.size.lines() > short {
                ui.label(egui::RichText::new(format!("Your shots are {short}p. {} makes a bigger file, not a sharper one.", st.choices.size.label())).size(11.5).color(t.text_faint));
            }
        }
        if own && p == Preset::Social {
            field_label(ui, &t, "Framing");
            let on = usize::from(st.choices.framing == Framing::Fit);
            if let Some(i) = seg(ui, ctx, "mfx.export.framing", &[("fill", "Fill (crop the sides)"), ("fit", "Fit (bars top and bottom)")], on) {
                st.choices.framing = if i == 0 { Framing::Fill } else { Framing::Fit };
            }
        }
        if own && p.has_quality() {
            field_label(ui, &t, "Quality");
            let opts: Vec<(&str, &str)> = Quality::ALL.iter().map(|q| (q.key(), q.label())).collect();
            let on = Quality::ALL.iter().position(|q| *q == st.choices.quality).unwrap_or(1);
            if let Some(i) = seg(ui, ctx, "mfx.export.quality", &opts, on) {
                st.choices.quality = Quality::ALL.get(i).copied().unwrap_or_default();
            }
        }
        if own && p == Preset::Master {
            field_label(ui, &t, "What's in it");
            ui.label(egui::RichText::new("The best-quality copy: Apple ProRes 422 HQ with 24-bit sound. Big, made to keep or to open in another editor.").size(12.0).color(t.text_dim));
        }
        if own && p == Preset::Stems {
            field_label(ui, &t, "What's in it");
            ui.label(
                egui::RichText::new(format!(
                    "Voice.wav, Music.wav, Effects.wav: 24-bit, 48 kHz, each the full {}, all starting together. Played together at full volume they are the video's sound.",
                    clock(if st.choices.how_much == HowMuch::InOut { v.in_out_seconds.unwrap_or(v.seconds) } else { v.seconds })
                ))
                .size(12.0)
                .color(t.text_dim),
            );
        }
        field_label(ui, &t, "How much");
        let whole = format!("Whole video · {}", clock(v.seconds));
        let inout = match v.in_out_seconds {
            Some(s) => format!("In to Out · {}", clock(s)),
            None => "In to Out (none set)".to_string(),
        };
        let on = usize::from(st.choices.how_much == HowMuch::InOut);
        if let Some(i) = seg(ui, ctx, "mfx.export.howMuch", &[("whole", whole.as_str()), ("inOut", inout.as_str())], on) {
            st.choices.how_much = if i == 1 && v.in_out_seconds.is_some() { HowMuch::InOut } else { HowMuch::Whole };
        }
        // Advanced
        ui.add_space(10.0);
        ui.separator();
        let head = ui.horizontal(|ui| {
            let r = ui.add(egui::Button::new(egui::RichText::new(format!("{} Advanced", if st.advanced_open { "⌄" } else { "›" })).color(t.text)).frame(false));
            if !st.advanced_open {
                ui.add(egui::Label::new(egui::RichText::new(&v.summary).size(11.5).color(t.text_faint)).truncate());
            }
            r
        });
        ctx.auto("mfx.export.advanced", &head.inner, "Advanced");
        if head.inner.clicked() {
            st.advanced_open = !st.advanced_open;
        }
        if st.advanced_open {
            ui.label(egui::RichText::new(&v.summary).size(12.0).color(t.text_dim));
            if !own || p != Preset::Stems {
                field_label(ui, &t, "Loudness");
                let on = usize::from(st.choices.loudness == Loudness::AsMixed);
                if let Some(i) = seg(ui, ctx, "mfx.export.advanced.loudness", &[("platform", "−14 LUFS, peaks ≤ −1 dB"), ("asMixed", "Leave as mixed")], on) {
                    st.choices.loudness = if i == 0 { Loudness::Platform } else { Loudness::AsMixed };
                }
                if own && p == Preset::Master {
                    ui.label(egui::RichText::new("The master file always keeps the sound as mixed.").size(11.5).color(t.text_faint));
                }
                field_label(ui, &t, "Captions");
                let on = match st.choices.captions {
                    Captions::Off => 0,
                    Captions::Burned => 1,
                    Captions::Srt => 2,
                };
                if let Some(i) = seg(ui, ctx, "mfx.export.advanced.captions", &[("off", "Off"), ("burned", "Burned in"), ("srt", "Separate file (.srt)")], on) {
                    st.choices.captions = [Captions::Off, Captions::Burned, Captions::Srt].get(i).copied().unwrap_or_default();
                }
                field_label(ui, &t, "Hardware encoding");
                if let Some(i) =
                    seg(ui, ctx, "mfx.export.advanced.hardware", &[("off", "Off (same file on every PC)"), ("on", "On when the PC has one")], usize::from(st.choices.hardware))
                {
                    st.choices.hardware = i == 1;
                }
            }
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let b = ui.link("All FilmCraft settings…");
                ctx.auto("mfx.export.advanced.full", &b, "All FilmCraft settings…");
                if b.clicked() {
                    out.actions.push(Action::FullSettings);
                }
                ui.label(egui::RichText::new("·").color(t.text_faint));
                let b = ui.link("Reset");
                ctx.auto("mfx.export.advanced.reset", &b, "Reset");
                if b.clicked() {
                    let keep = (st.choices.preset, st.choices.filmcraft_preset.clone(), st.choices.folder.clone());
                    st.choices = Choices { preset: keep.0, filmcraft_preset: keep.1, folder: keep.2, ..Choices::default() };
                }
            });
        }
    });

    // foot: You'll get · Render · the queue note
    let mut f = ui
        .new_child(egui::UiBuilder::new().max_rect(foot.shrink2(vec2(14.0, 10.0))).id_salt("mfx-export-foot").layout(egui::Layout::top_down(egui::Align::Min)));
    ui.painter().line_segment([foot.left_top(), foot.right_top()], Stroke::new(1.0, t.separator));
    let secs = match st.choices.how_much {
        HowMuch::InOut => v.in_out_seconds.unwrap_or(v.seconds),
        HowMuch::Whole => v.seconds,
    };
    let what = if own && p == Preset::Stems {
        "a folder with 3 WAV files".to_string()
    } else if v.out_width > 0 {
        format!("one {}, {}×{}", if own { p.extension().to_uppercase() } else { "file".into() }, v.out_width, v.out_height)
    } else {
        "one sound file".to_string()
    };
    let size = if v.estimate > 0 { format!(", about {}", bytes_label(v.estimate)) } else { String::new() };
    let time = st.time_estimate(secs).map(|s| format!(" About {} on this PC.", clock(s.max(1.0)))).unwrap_or_default();
    f.label(egui::RichText::new(format!("You'll get {what}{size}.{time}")).color(t.text_dim).size(12.5));
    f.add_space(6.0);
    let reason = if !v.has_sequence {
        Some("Nothing on the timeline yet.".to_string())
    } else if secs <= 0.0 {
        Some("The timeline is empty.".to_string())
    } else if !v.offline.is_empty() {
        Some(format!("{} is missing: relink it in the Edit tab first.", v.offline.first().map_or("A file", String::as_str)))
    } else {
        None
    };
    let btn = egui::Button::new(egui::RichText::new("Render").color(Color32::WHITE).size(14.0)).fill(t.accent);
    let w = f.available_width();
    let b = f.add_enabled_ui(reason.is_none(), |ui| ui.add_sized(vec2(w, 34.0), btn)).inner;
    ctx.auto("mfx.export.render", &b, "Render");
    if b.clicked() {
        out.actions.push(Action::Render);
    }
    let busy = merge_queue(&v.queue, &st.service_jobs).iter().any(|r| r.state == RowState::Rendering);
    let note = if let Some(r) = reason {
        r
    } else if !st.note.is_empty() {
        st.note.clone()
    } else if busy {
        "One render is running: this one starts after it.".to_string()
    } else {
        "Renders run one at a time on this PC. You can keep editing while it renders.".to_string()
    };
    f.label(egui::RichText::new(note).size(11.5).color(t.text_faint));
}

fn rendered_page(ui: &mut egui::Ui, rect: Rect, ctx: &mut MfxCtx, st: &mut ExportState, v: &TabView, out: &mut TabOut) {
    let t = ctx.theme;
    let right_w = RIGHT_W.min(rect.width() * 0.4);
    let left = Rect::from_min_max(rect.min, pos2(rect.max.x - right_w - GAP, rect.max.y));
    let right = Rect::from_min_max(pos2(rect.max.x - right_w, rect.min.y), rect.max);
    let list_h = 220.0_f32.min(left.height() * 0.4);
    let video = Rect::from_min_max(left.min, pos2(left.max.x, left.max.y - list_h - GAP));
    let list = Rect::from_min_max(pos2(left.min.x, video.max.y + GAP), left.max);
    let post_h = (right.height() * 0.6).max(200.0).min(right.height() - 140.0);
    let post = Rect::from_min_max(right.min, pos2(right.max.x, right.min.y + post_h));
    let next = Rect::from_min_max(pos2(right.min.x, post.max.y + GAP), right.max);
    let Some(rec) = st.renders.get(st.shown).cloned() else { return };

    // your video
    let name = file_name_of(&rec.path).to_string();
    let mut c = body(ui, video, &t, "Your video", &name, "mfx-export-video");
    let back = ui
        .put(Rect::from_min_size(pos2(video.max.x - 210.0, video.min.y + 6.0), vec2(196.0, 22.0)), egui::Button::new("‹ Back to Export settings").frame(false));
    ctx.auto("mfx.export.rendered.back", &back, "Back to Export settings");
    if back.clicked() {
        st.page = Page::Settings;
    }
    let area = c.max_rect();
    let tile_h = 160.0;
    let pic_area = Rect::from_min_max(area.min, pos2(area.max.x, area.max.y - tile_h));
    if rec.width > 0 && rec.height > 0 && !rec.missing {
        let aspect = rec.width as f32 / rec.height as f32;
        let (mut w, mut h) = (pic_area.width(), pic_area.width() / aspect.max(0.01));
        if h > pic_area.height() {
            h = pic_area.height();
            w = h * aspect;
        }
        let pic = Rect::from_center_size(pic_area.center(), vec2(w.max(1.0), h.max(1.0)));
        c.painter().rect_filled(pic, 0.0, Color32::BLACK);
        // the host draws the timeline frame the file was made from (same picture, same framing)
        out.preview = Some((pic, rec.choices.framing == Framing::Fill || rec.choices.preset != Preset::Social));
    } else {
        c.painter().text(
            pic_area.center(),
            Align2::CENTER_CENTER,
            if rec.missing { "The file is missing." } else { "Sound only" },
            FontId::proportional(14.0),
            t.text_dim,
        );
    }
    let mut tile =
        c.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(area.min.x, area.max.y - tile_h + 8.0), area.max)).id_salt("mfx-export-tile"));
    egui::Frame::NONE.fill(t.field_bg).corner_radius(4.0).inner_margin(egui::Margin::same(10)).show(&mut tile, |ui| {
        ui.set_width(ui.available_width());
        let kind = file_name_of(&rec.path).rsplit('.').next().filter(|e| e.len() <= 4 && *e != file_name_of(&rec.path)).unwrap_or("folder").to_uppercase();
        let dims = if rec.width > 0 { format!(" · {}×{}", rec.width, rec.height) } else { String::new() };
        ui.label(egui::RichText::new(format!("{kind}  {name}")).color(t.text).size(13.0));
        ui.label(egui::RichText::new(format!("{} · {}{dims}", bytes_label(rec.bytes), clock(rec.seconds))).color(t.text_dim).size(12.0));
    });
    tile.add_space(6.0);
    tile.horizontal(|ui| {
        let b = ui.button("Open");
        ctx.auto("mfx.export.rendered.open", &b, "Open");
        if b.clicked() {
            out.actions.push(Action::Open(rec.path.clone()));
        }
        let b = ui.button(v.os.reveal_label());
        ctx.auto("mfx.export.rendered.show", &b, v.os.reveal_label());
        if b.clicked() {
            out.actions.push(Action::Reveal(rec.files.first().cloned().unwrap_or_else(|| rec.path.clone())));
        }
        let b = ui.add_enabled(!rec.files.is_empty() && rec.files.len() == 1, egui::Button::new("Save a copy…"));
        ctx.auto("mfx.export.rendered.saveCopy", &b, "Save a copy…");
        if b.clicked() {
            out.actions.push(Action::SaveCopy(rec.path.clone()));
        }
    });
    tile.horizontal_wrapped(|ui| {
        if rec.checks.is_empty() {
            ui.label(egui::RichText::new("Checking the file…").size(12.0).color(t.text_faint));
        }
        for ch in &rec.checks {
            let (mark, col) = if ch.ok { ("✓", t.ok) } else { ("!", t.warn) };
            ui.label(egui::RichText::new(format!("{mark} {}", ch.text)).size(12.0).color(col));
        }
    });

    // renders of this project
    let folder = st.choices.folder_or(&v.default_folder).to_string();
    let n = st.renders.iter().filter(|r| r.done()).count();
    let note = format!("{n} {} · {folder}", if n == 1 { "file" } else { "files" });
    let mut c = body(ui, list, &t, "Renders of this project", &note, "mfx-export-renders");
    let mut pick = None;
    let mut again = None;
    egui::ScrollArea::vertical().id_salt("mfx-export-renders-rows").auto_shrink([false, false]).show(&mut c, |ui| {
        for (i, r) in st.renders.iter().enumerate().rev() {
            ui.horizontal(|ui| {
                let label = file_name_of(&r.path);
                left_label(ui, (ui.available_width() * 0.34).max(100.0), egui::RichText::new(label).size(12.5).color(t.text));
                let when = if !r.error.is_empty() {
                    format!("Failed: {}", r.error)
                } else if r.finished_at == 0 {
                    "Rendering…".into()
                } else if r.missing {
                    "Missing".into()
                } else {
                    let mut s = r.finished_label.clone();
                    if i == st.shown {
                        s.push_str("  this one");
                    }
                    s
                };
                ui.label(egui::RichText::new(when).size(12.0).color(t.text_dim));
                if r.stale(v.timeline) {
                    ui.label(egui::RichText::new("made before your last edit").size(12.0).color(t.warn));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if r.stale(v.timeline) || r.missing || !r.error.is_empty() {
                        let b = ui.small_button("Render again");
                        ctx.auto(&format!("mfx.export.renders.{i}.again"), &b, "Render again");
                        if b.clicked() {
                            again = Some(i);
                        }
                    }
                    if r.done() && !r.missing {
                        let b = ui.small_button(v.os.reveal_label());
                        ctx.auto(&format!("mfx.export.renders.{i}.reveal"), &b, v.os.reveal_label());
                        if b.clicked() {
                            out.actions.push(Action::Reveal(r.files.first().cloned().unwrap_or_else(|| r.path.clone())));
                        }
                        let b = ui.small_button("Open");
                        ctx.auto(&format!("mfx.export.renders.{i}.open"), &b, "Open");
                        if b.clicked() {
                            out.actions.push(Action::Open(r.path.clone()));
                        }
                        let b = ui.small_button("Show");
                        ctx.auto(&format!("mfx.export.renders.{i}.show"), &b, "Show");
                        if b.clicked() {
                            pick = Some(i);
                        }
                    }
                    if r.bytes > 0 {
                        ui.label(egui::RichText::new(bytes_label(r.bytes)).monospace().size(11.5).color(t.text_faint));
                    }
                });
            });
        }
    });
    if let Some(i) = pick {
        st.shown = i;
    }
    if let Some(i) = again {
        out.actions.push(Action::RenderAgain(i));
    }

    post_panel(ui, post, ctx, st, &rec, out);

    // what next
    let mut c = body(ui, next, &t, "What next", "", "mfx-export-next");
    for (key, label, act) in [
        ("edit", "Change something (Edit tab)", Some(Action::EditTab)),
        ("another", "Render another format", None),
        ("newProject", "New project", Some(Action::NewProject)),
    ] {
        let b = c.add(egui::Button::new(egui::RichText::new(label).color(t.text)).frame(false));
        ctx.auto(&format!("mfx.export.rendered.{key}"), &b, label);
        if b.clicked() {
            match act {
                Some(a) => out.actions.push(a),
                None => st.page = Page::Settings,
            }
        }
    }
}

fn post_panel(ui: &mut egui::Ui, r: Rect, ctx: &mut MfxCtx, st: &mut ExportState, rec: &RenderRecord, out: &mut TabOut) {
    let t = ctx.theme;
    let via = if st.post.service_name.is_empty() { String::new() } else { format!("through your {}", st.post.service_name) };
    let mut c = body(ui, r, &t, "Post it", &via, "mfx-export-post");
    if st.post.service.is_empty() {
        c.label(egui::RichText::new("Checking how posting is set up…").size(12.0).color(t.text_faint));
        return;
    }
    if st.post.service == "noProject" {
        c.label(egui::RichText::new("Posting goes through a Mortiflix project: make or open one first.").color(t.text_dim));
        return;
    }
    if st.post.service == "none" || st.post.platforms.is_empty() {
        c.label(egui::RichText::new("Posting isn't set up. Set up Upload-Post or your own Postiz.").color(t.text_dim));
        let b = c.button("Set up posting…");
        ctx.auto("mfx.export.post.setup", &b, "Set up posting…");
        if b.clicked() {
            out.actions.push(Action::SetupPosting);
        }
        return;
    }
    if !rec.done() || rec.missing || rec.files.len() != 1 {
        c.label(egui::RichText::new("Only a finished video file can be posted.").color(t.text_dim));
        return;
    }
    field_label(&mut c, &t, "Accounts");
    let vertical = rec.choices.preset == Preset::Social && rec.choices.filmcraft_preset.is_none();
    c.horizontal_wrapped(|ui| {
        for p in st.post.platforms.clone() {
            let on = st.post.picked.contains(&p);
            let needs_vertical = matches!(p.as_str(), "tiktok" | "instagram") && !vertical;
            let label = if needs_vertical { format!("{p} (needs Social vertical)") } else { p.clone() };
            let b = ui.add_enabled(!needs_vertical, egui::Button::selectable(on, label));
            ctx.auto(&format!("mfx.export.post.{p}"), &b, &p);
            if b.clicked() {
                if on {
                    st.post.picked.retain(|x| *x != p);
                } else {
                    st.post.picked.push(p.clone());
                }
            }
        }
    });
    field_label(&mut c, &t, "Title");
    if st.post.title.is_empty() {
        st.post.title = rec.title.clone();
    }
    let r = c.add(egui::TextEdit::singleline(&mut st.post.title).desired_width(f32::INFINITY));
    ctx.auto("mfx.export.post.title", &r, "Title");
    field_label(&mut c, &t, "Description");
    let r = c.add(egui::TextEdit::multiline(&mut st.post.description).desired_rows(2).desired_width(f32::INFINITY));
    ctx.auto("mfx.export.post.description", &r, "Description");
    field_label(&mut c, &t, "When");
    if let Some(i) = seg(&mut c, ctx, "mfx.export.post.when", &[("now", "Now"), ("later", "Pick a time…")], usize::from(st.post.later)) {
        st.post.later = i == 1;
    }
    if st.post.later {
        let r = c.add(egui::TextEdit::singleline(&mut st.post.at).hint_text("2026-10-20T18:00").desired_width(f32::INFINITY));
        ctx.auto("mfx.export.post.at", &r, "When");
    }
    c.add_space(8.0);
    let ready = !st.post.picked.is_empty() && !st.post.title.trim().is_empty() && !st.post.sending && (!st.post.later || !st.post.at.trim().is_empty());
    c.horizontal(|ui| {
        let b = ui.add_enabled(ready, egui::Button::new("Post…"));
        ctx.auto("mfx.export.post.go", &b, "Post…");
        if b.clicked() {
            st.confirm = Some(Confirm::Post);
        }
        ui.label(egui::RichText::new(if st.post.sending { "posting…" } else { "asks first" }).size(11.5).color(t.text_faint));
    });
    for line in &st.post.results {
        c.label(egui::RichText::new(line).size(12.0).color(t.text_dim));
    }
}

fn confirm_dialog(ui: &mut egui::Ui, ctx: &mut MfxCtx, st: &mut ExportState, out: &mut TabOut) {
    let Some(c) = st.confirm.clone() else { return };
    let t = ctx.theme;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::new("mfx-export-confirm")).show(ui.ctx(), |ui| {
        ui.set_width(420.0);
        let (heading, lines, yes): (String, Vec<String>, &str) = match &c {
            Confirm::Stop { label, .. } => (format!("Stop {label}?"), vec!["The half-written file is deleted.".into()], "Stop it"),
            Confirm::Post => {
                let rec = st.renders.get(st.shown);
                let when = if st.post.later && !st.post.at.trim().is_empty() { st.post.at.trim().to_string() } else { "now".into() };
                (
                    format!("Post to {}?", st.post.picked.join(", ")),
                    vec![
                        format!("File: {}", rec.map_or("", |r| file_name_of(&r.path))),
                        format!("Title: {}", st.post.title.trim()),
                        format!("When: {when}"),
                        "This goes out on your accounts.".into(),
                    ],
                    "Post it",
                )
            }
            Confirm::Note { text } => (text.clone(), Vec::new(), "OK"),
        };
        ui.heading(heading);
        for l in lines {
            ui.label(egui::RichText::new(l).color(t.text_dim));
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if !matches!(c, Confirm::Note { .. }) {
                let no = ui.button("Cancel");
                ctx.auto("mfx.export.confirm.no", &no, "Cancel");
                if no.clicked() {
                    close = true;
                }
            }
            let go = ui.add(egui::Button::new(egui::RichText::new(yes).color(Color32::WHITE)).fill(t.accent));
            ctx.auto("mfx.export.confirm.yes", &go, yes);
            if go.clicked() {
                close = true;
                match &c {
                    Confirm::Stop { id, .. } => out.actions.push(Action::Stop(*id)),
                    Confirm::Post => out.actions.push(Action::Post(st.shown)),
                    Confirm::Note { .. } => {}
                }
            }
        });
    });
    if close || modal.should_close() {
        st.confirm = None;
    }
}

/// The `publish.send` request for the Post dialog's "Post it" (doc 39): only the person's click
/// sends it (`confirm: true`); the service refuses posts by the AI.
pub fn post_params(st: &ExportState, i: usize) -> Option<Value> {
    let r = st.renders.get(i)?;
    r.files.first()?;
    let at = if st.post.later { st.post.at.trim().to_string() } else { String::new() };
    let mut p = json!({
        "to": st.post.picked.join(","),
        "title": st.post.title.trim(),
        "confirm": true,
    });
    if !st.post.description.trim().is_empty() {
        p["description"] = json!(st.post.description.trim());
    }
    if !at.is_empty() {
        p["at"] = json!(at);
    }
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_map_to_filmcraft_presets_with_platform_loudness() {
        let mut c = Choices::default();
        let r = recipe(&c);
        assert_eq!(r.filmcraft_preset, "YouTube 1080p Full HD");
        assert_eq!(r.frame_size, Some((1920, 1080)));
        assert_eq!(r.bitrate_kbps, Some(16_000));
        assert_eq!(r.audio_kbps, Some(384));
        assert_eq!(r.sample_rate, Some(48_000));
        assert_eq!(r.loudness, Some((-14.0, -1.0)));
        assert_eq!(r.range, "entire");
        c.quality = Quality::Small;
        c.size = Size::P2160;
        assert_eq!(recipe(&c).bitrate_kbps, Some(22_500));
        c.preset = Preset::Social;
        c.framing = Framing::Fit;
        let r = recipe(&c);
        assert_eq!((r.filmcraft_preset.as_str(), r.frame_size, r.fill), ("Social Vertical 1080×1920", Some((2160, 3840)), Some(false)));
        c.preset = Preset::Master;
        c.captions = Captions::Burned;
        let r = recipe(&c);
        assert_eq!((r.filmcraft_preset.as_str(), r.loudness, r.burn_captions), ("Apple ProRes 422 HQ", None, false));
        c.preset = Preset::Stems;
        c.how_much = HowMuch::InOut;
        let r = recipe(&c);
        assert_eq!((r.filmcraft_preset.as_str(), r.loudness, r.range.as_str()), ("Waveform Audio 48 kHz 24-bit", None, "inOut"));
        c.filmcraft_preset = Some("Vimeo 1080p Full HD".into());
        c.captions = Captions::Srt;
        let r = recipe(&c);
        assert_eq!((r.filmcraft_preset.as_str(), r.frame_size, r.caption_sidecar.as_deref()), ("Vimeo 1080p Full HD", None, Some("srt")));
    }

    #[test]
    fn choices_take_control_channel_patches_and_refuse_bad_values() {
        let mut c = Choices::default();
        c.apply(&json!({"preset": "social", "size": "720p", "quality": "best", "folder": "/tmp/x", "bogus": 1})).unwrap();
        assert_eq!((c.preset, c.size, c.quality, c.folder.as_str()), (Preset::Social, Size::P720, Quality::Best, "/tmp/x"));
        c.apply(&json!({"preset": "Apple ProRes 422"})).unwrap();
        assert_eq!(c.filmcraft_preset.as_deref(), Some("Apple ProRes 422"));
        c.apply(&json!({"preset": "youtube"})).unwrap();
        assert_eq!((c.preset, c.filmcraft_preset.clone()), (Preset::Youtube, None));
        assert!(c.apply(&json!({"size": "8k"})).is_err());
        assert!(c.apply(&json!([1])).is_err());
        assert!(c.apply(&json!({"size": 5})).is_err());
        assert_eq!(c.size, Size::P720, "a refused patch changes nothing");
    }

    #[test]
    fn file_names_are_safe_and_never_overwrite() {
        let c = Choices::default();
        assert_eq!(c.file_stem("The Rise of AI"), "The Rise of AI – YouTube");
        assert_eq!(c.file_stem("a/b:c"), "a-b-c – YouTube");
        assert_eq!(safe_file_name("  ..  "), "Video");
        let taken = |p: &str| p.ends_with("x.mp4") || p.ends_with("x (2).mp4");
        assert_eq!(unique_path("/v/", "x", "mp4", &taken), "/v/x (3).mp4");
        assert_eq!(unique_path("C:\\V", "y", "mp4", &|_| false), "C:\\V\\y.mp4");
        assert_eq!(unique_path("", "z", "", &|_| false), "z");
        assert_eq!(file_name_of("/a/b/c.mp4"), "c.mp4");
        assert_eq!(file_name_of("C:\\a\\c.mov"), "c.mov");
    }

    #[test]
    fn default_folders_follow_the_os() {
        assert_eq!(default_folder(Os::Linux, "/home/u", None, "The Rise of AI"), "/home/u/Videos/Mortiflix/The Rise of AI");
        assert_eq!(default_folder(Os::Linux, "/home/u", Some("/home/u/Vids"), "P"), "/home/u/Vids/Mortiflix/P");
        // XDG_VIDEOS_DIR=$HOME means "not set" (the user-dirs convention)
        assert_eq!(default_folder(Os::Linux, "/home/u", Some("/home/u/"), "P"), "/home/u/Videos/Mortiflix/P");
        assert_eq!(default_folder(Os::Mac, "/Users/u", None, ""), "/Users/u/Movies/Mortiflix/Untitled");
        assert_eq!(default_folder(Os::Windows, "C:\\Users\\u", None, "P"), "C:\\Users\\u\\Videos\\Mortiflix\\P");
        let dirs = "# comment\nXDG_DESKTOP_DIR=\"$HOME/Desktop\"\nXDG_VIDEOS_DIR=\"$HOME/Filme\"\n";
        assert_eq!(xdg_videos_from_user_dirs(dirs, "/home/u").as_deref(), Some("/home/u/Filme"));
        assert_eq!(xdg_videos_from_user_dirs("XDG_VIDEOS_DIR=\"../x\"", "/h"), None);
        assert_eq!(Os::Mac.reveal_label(), "Show in Finder");
    }

    #[test]
    fn clock_and_sizes_survive_hostile_numbers() {
        assert_eq!(clock(92.0), "1:32");
        assert_eq!(clock(3723.4), "1:02:03");
        assert_eq!(clock(f64::NAN), "0:00");
        assert_eq!(clock(-5.0), "0:00");
        assert_eq!(clock(f64::INFINITY), "0:00");
        assert_eq!(bytes_label(184_000_000), "184 MB");
        assert_eq!(bytes_label(2_500_000_000), "2.5 GB");
        assert_eq!(bytes_label(12), "12 bytes");
    }

    #[test]
    fn one_queue_for_the_pc_counts_whats_ahead() {
        let svc = service_rows(
            &json!([
                {"id": 3, "label": "Shot 7 v3 · new version", "state": "running", "progress": {"percent": 42, "seconds_left": 70}},
                {"id": 4, "label": "Music", "state": "waiting", "renders_ahead": 1},
                {"id": 9, "label": "my export", "state": "waiting", "external": true},
                {"id": "x", "state": "running"},
                {"id": 5, "state": "exploded"},
                {"id": 6, "state": "done", "progress": {"percent": 1e300}},
                {"id": 8, "state": "failed", "cancelled": true},
                7,
            ]),
            &[9],
        );
        assert_eq!(svc.len(), 4);
        assert!(service_busy(&svc));
        assert_eq!((svc[0].progress, svc[0].seconds_left), (0.42, Some(70.0)));
        assert_eq!(svc[3].state, RowState::Cancelled);
        let app = vec![
            QueueRow { id: 1, label: "A.mp4".into(), state: RowState::Waiting, ..QueueRow::default() },
            QueueRow { id: 2, label: "B.mov".into(), state: RowState::Done, ..QueueRow::default() },
        ];
        let q = merge_queue(&app, &svc);
        let order: Vec<(bool, u64, u32)> = q.iter().map(|r| (r.service, r.id, r.ahead)).collect();
        assert_eq!(order, vec![(true, 3, 0), (false, 1, 1), (true, 4, 2), (false, 2, 0), (true, 6, 0), (true, 8, 0)]);
        assert!(service_rows(&json!("junk"), &[]).is_empty());
        assert!(!service_busy(&[]));
    }

    #[test]
    fn renders_are_bounded_stale_after_an_edit_and_teach_the_speed() {
        let mut st = ExportState::default();
        for i in 0..MAX_RENDERS + 3 {
            st.push_render(RenderRecord { path: i.to_string(), finished_at: 1, timeline: 7, ..RenderRecord::default() });
        }
        assert_eq!(st.renders.len(), MAX_RENDERS);
        assert_eq!(st.renders[0].path, "3");
        assert!(!st.renders[0].stale(7));
        assert!(st.renders[0].stale(8));
        assert!(!RenderRecord { error: "x".into(), finished_at: 1, ..RenderRecord::default() }.stale(8));
        assert_eq!(st.time_estimate(60.0), None);
        st.learn_speed(30.0, 60.0);
        assert_eq!(st.time_estimate(60.0), Some(30.0));
        st.learn_speed(f64::NAN, 1.0);
        st.learn_speed(1.0, 0.0);
        assert_eq!(st.time_estimate(60.0), Some(30.0));
    }

    #[test]
    fn stems_take_every_track_once() {
        assert_eq!(Stem::of_track("Voice"), Stem::Voice);
        assert_eq!(Stem::of_track("VO"), Stem::Voice);
        assert_eq!(Stem::of_track("Narration 2"), Stem::Voice);
        assert_eq!(Stem::of_track("Music"), Stem::Music);
        assert_eq!(Stem::of_track("Score"), Stem::Music);
        assert_eq!(Stem::of_track("Sound"), Stem::Effects);
        assert_eq!(Stem::of_track("Audio 3"), Stem::Effects);
        assert_eq!(Stem::of_track("Volume"), Stem::Effects);
    }

    #[test]
    fn service_replies_and_job_events_land_and_hostile_ones_are_ignored() {
        let mut svc = crate::Service::default();
        let mut st = ExportState::default();
        st.push_render(RenderRecord { files: vec!["/v/a.mp4".into()], path: "/v/a.mp4".into(), finished_at: 1, seconds: 3.0, ..RenderRecord::default() });
        // not running: every request is answered at once with an error, never left waiting
        st.poll_jobs(&mut svc, 10.0);
        st.send(&mut svc, "render.progress", json!({}));
        let id = svc.request("publish.plan", json!({}));
        st.pending.push((id, Pending::PublishSetup));
        let id = svc.request("render.add", json!({}));
        st.pending.push((id, Pending::Add(0)));
        st.poll_service(&mut svc);
        assert!(st.pending.is_empty());
        assert_eq!(st.post.service, "set", "not_running isn't 'not set up'");
        assert_eq!(st.added, vec![(0, None)], "a refused render.add lets the app render alone");
        // render.job: the app's own job goes to the host, others to the queue rows
        st.own_service_ids = vec![5];
        st.apply_event(&json!({"event": "render.job", "data": {"id": 5, "state": "running", "label": "mine"}}));
        st.apply_event(&json!({"event": "render.job", "data": {"id": 6, "state": "waiting", "label": "Shot 2"}}));
        st.apply_event(&json!({"event": "render.job", "data": {"id": 6, "state": "running", "label": "Shot 2"}}));
        st.apply_event(&json!({"event": "render.job", "data": {"id": "6"}}));
        st.apply_event(&json!({"event": "render.job"}));
        st.apply_event(&json!({"event": "chat.message", "data": {"id": 7, "state": "running"}}));
        assert_eq!(st.job_events.len(), 1);
        assert_eq!(st.service_jobs.len(), 1);
        assert_eq!(st.service_jobs[0].state, RowState::Rendering);
        st.post.picked = vec!["youtube".into(), "tiktok".into()];
        st.post.title = "T".into();
        let p = post_params(&st, 0).unwrap();
        assert_eq!((p["to"].as_str(), p["confirm"].as_bool()), (Some("youtube,tiktok"), Some(true)));
        assert!(p.get("at").is_none());
        assert!(post_params(&st, 5).is_none());
    }
}
