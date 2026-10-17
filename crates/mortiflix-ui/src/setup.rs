//! First-run setup: the Welcome window on first launch, and Edit ▸ Preferences ▸ Mortiflix… later.
//!
//! Three steps, each one skippable (Mortiflix Studio's editing works without any of them):
//! 1. **Claude Code.** The AI is the user's own Claude Code (their plan pays). Setup finds `claude`
//!    (PATH and the usual install folders), reads its version and whether it is signed in
//!    (`claude auth status --json`), and **Link Claude Code** runs `claude auth login` with its output
//!    shown in the window: a button opens the sign-in page, a field sends the code back. A test
//!    message through the Mortiflix service (`setup.claude.test`) proves the chat can talk to it. An
//!    Anthropic API key is the optional fallback.
//! 2. **Keys** (optional): ElevenLabs, Gemini, Upload-Post, Postiz, with what each is for. The service
//!    keeps them in the studio folder (`secrets.json` / `session.env`, mode 600); they never come back
//!    to the app and never go into project files.
//! 3. **Tools:** the Mortiflix service and its Node, ffmpeg, Chrome for Remotion, the sound engine, and
//!    the Whisper / Kokoro models. Downloads always ask first (size, source, licence), then run in the
//!    service with progress and SHA-256 checks.
//!
//! The window's own state lives in egui's temporary data (one [`Model`] per context), so no shared
//! state struct changes. `MFX_FAKE_DOWNLOADS=1` swaps the model files for tiny fake ones (tests).

use std::collections::HashMap;
use std::io::{Read, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::service::{SERVICE_SCRIPT, ServiceConfig, ServiceSettings};
use crate::{MfxCtx, MfxTheme, Mortiflix};

/// What setup remembers, beside Mortiflix Studio's `preferences.json` (the service path stays in
/// `mortiflix.json`, see [`ServiceSettings`]).
pub const SETUP_FILE: &str = "mortiflix-setup.json";
/// Where Claude Code's install page is.
pub const CLAUDE_INSTALL_URL: &str = "https://claude.com/claude-code";
/// Variables a Claude Code session sets for its own tools; a `claude` started from the app must not
/// inherit them (the same list as the service's `PARENT_SESSION_VARS`).
pub const PARENT_SESSION_VARS: [&str; 10] = [
    "CLAUDECODE",
    "CLAUDE_CODE_ENTRYPOINT",
    "CLAUDE_CODE_SESSION_ID",
    "CLAUDE_CODE_CHILD_SESSION",
    "CLAUDE_CODE_SESSION_ATTENDED",
    "CLAUDE_CODE_MESSAGING_SOCKET",
    "CLAUDE_CODE_MESSAGING_TOKEN",
    "CLAUDE_CODE_BRIDGE_SESSION_ID",
    "CLAUDE_CODE_EXECPATH",
    "CLAUDE_PID",
];
/// Size of each fake model file with `MFX_FAKE_DOWNLOADS=1`.
pub const FAKE_FILE_BYTES: u64 = 3_000_000;
const MAX_LOGIN_TEXT: usize = 64 * 1024;
const MAX_CAPTURE: usize = 1024 * 1024;

// ---------------------------------------------------------------------------------------------
// Settings

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SetupSettings {
    /// The Welcome window was shown once (it then only opens from the menu).
    pub welcome_done: bool,
    /// The `claude` the user linked (empty: look it up).
    pub claude_path: String,
}

impl SetupSettings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(SETUP_FILE)
    }
    /// Missing, too big or broken = defaults.
    pub fn load(data_dir: &Path) -> Self {
        const MAX_BYTES: u64 = 64 * 1024;
        let Ok(file) = std::fs::File::open(Self::path(data_dir)) else { return Self::default() };
        let mut bytes = Vec::new();
        if file.take(MAX_BYTES + 1).read_to_end(&mut bytes).is_err() || bytes.len() as u64 > MAX_BYTES {
            return Self::default();
        }
        serde_json::from_slice(&bytes).unwrap_or_default()
    }
    /// Write atomically (temp file + rename).
    pub fn save(&self, data_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(data_dir)?;
        let path = Self::path(data_dir);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)?;
        std::fs::rename(tmp, path)
    }
}

// ---------------------------------------------------------------------------------------------
// Finding things

fn home_dir() -> Option<PathBuf> {
    ["HOME", "USERPROFILE"].iter().find_map(|k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from))
}

/// A file we may run (on Unix: with an execute bit).
fn runnable(p: &Path) -> bool {
    let Ok(md) = std::fs::metadata(p) else { return false };
    if !md.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        md.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

fn claude_names() -> &'static [&'static str] {
    if cfg!(windows) { &["claude.exe", "claude.cmd", "claude.bat"] } else { &["claude"] }
}

/// Where `claude` is: the linked path, then the PATH, then the folders its installers use (a desktop
/// app started from the dock often has a shorter PATH than the user's shell).
pub fn find_claude(saved: &str, path_var: Option<&std::ffi::OsStr>, home: Option<&Path>) -> Option<PathBuf> {
    let saved = Path::new(saved);
    if !saved.as_os_str().is_empty() && saved.is_absolute() && runnable(saved) {
        return Some(saved.to_path_buf());
    }
    let mut dirs: Vec<PathBuf> = path_var.map(|p| std::env::split_paths(p).take(256).collect()).unwrap_or_default();
    if let Some(h) = home {
        dirs.extend([".local/bin", ".claude/local", ".npm-global/bin", ".bun/bin", "AppData/Roaming/npm"].iter().map(|d| h.join(d)));
    }
    dirs.extend(["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"].iter().map(PathBuf::from));
    dirs.iter().filter(|d| d.is_absolute()).flat_map(|d| claude_names().iter().map(move |n| d.join(n))).find(|p| runnable(p))
}

// ---------------------------------------------------------------------------------------------
// Running things

#[cfg(windows)]
fn no_window(c: &mut Command) {
    use std::os::windows::process::CommandExt as _;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    c.creation_flags(CREATE_NO_WINDOW);
}
#[cfg(not(windows))]
fn no_window(_: &mut Command) {}

/// `claude <args>` (through `cmd /c` for npm's `.cmd` shim on Windows), without the variables of
/// any Claude Code session the app itself was started from.
pub fn claude_command(bin: &Path, args: &[&str]) -> Command {
    let shim = bin.extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let mut c = if cfg!(windows) && shim {
        let mut c = Command::new("cmd");
        c.arg("/d").arg("/c").arg(bin);
        c
    } else {
        Command::new(bin)
    };
    c.args(args);
    for v in PARENT_SESSION_VARS {
        c.env_remove(v);
    }
    no_window(&mut c);
    c
}

fn spawn_thread<T: Send + 'static>(name: &str, f: impl FnOnce() -> T + Send + 'static) -> Option<std::thread::JoinHandle<Option<T>>> {
    std::thread::Builder::new().name(name.into()).spawn(move || std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).ok()).ok()
}

fn read_all(mut r: impl Read) -> String {
    let mut buf = Vec::new();
    let _ = (&mut r).take(MAX_CAPTURE as u64).read_to_end(&mut buf);
    // drain the rest so the child never blocks on a full pipe
    let _ = std::io::copy(&mut r, &mut std::io::sink());
    String::from_utf8_lossy(&buf).into_owned()
}

/// Run to the end (or `timeout`): (exited 0, stdout, stderr). Never waits on a grandchild that kept
/// the pipes open longer than a moment.
pub fn run_capture(mut cmd: Command, timeout: Duration) -> Result<(bool, String, String), String> {
    cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let (tx_out, rx_out) = std::sync::mpsc::channel();
    let (tx_err, rx_err) = std::sync::mpsc::channel();
    if let Some(o) = child.stdout.take() {
        let _ = spawn_thread("mfx-setup-out", move || tx_out.send(read_all(o)));
    }
    if let Some(e) = child.stderr.take() {
        let _ = spawn_thread("mfx-setup-err", move || tx_err.send(read_all(e)));
    }
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("it took too long".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(e.to_string());
            }
        }
    };
    let grace = Duration::from_secs(2);
    let o = rx_out.recv_timeout(grace).unwrap_or_default();
    let e = rx_err.recv_timeout(grace).unwrap_or_default();
    Ok((status.success(), o, e))
}

// ---------------------------------------------------------------------------------------------
// Claude Code

/// What setup knows about the user's Claude Code.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClaudeInfo {
    pub path: Option<PathBuf>,
    pub version: String,
    /// None: couldn't tell.
    pub logged_in: Option<bool>,
    /// `claude.ai`, `console`, `api key`… as Claude Code says it.
    pub method: String,
    /// The account's email and plan, when Claude Code says.
    pub account: String,
    pub problem: String,
}

impl ClaudeInfo {
    pub fn linked(&self) -> bool {
        self.path.is_some() && self.logged_in == Some(true)
    }
}

/// Read `claude auth status --json`: (signed in, method, account). Hostile text: None when it isn't
/// the JSON object it should be.
pub fn parse_auth_status(text: &str) -> Option<(bool, String, String)> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let v: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    let logged_in = v.get("loggedIn").and_then(Value::as_bool)?;
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(|s| s.chars().take(120).collect::<String>());
    let method = s(&v, "authMethod").unwrap_or_default();
    let email = s(&v, "email").or_else(|| s(&v, "emailAddress")).or_else(|| v.get("account").and_then(|a| s(a, "email"))).unwrap_or_default();
    let plan = s(&v, "subscriptionType").or_else(|| v.get("account").and_then(|a| s(a, "subscriptionType"))).unwrap_or_default();
    let account = [email, plan].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(", ");
    Some((logged_in, method, account))
}

/// Find Claude Code and ask it whether it is signed in. Blocks (up to ~35 s): run it on a thread.
pub fn probe_claude(saved: &str) -> ClaudeInfo {
    let path_var = std::env::var_os("PATH");
    let Some(path) = find_claude(saved, path_var.as_deref(), home_dir().as_deref()) else {
        return ClaudeInfo { problem: "Claude Code isn't installed here (or isn't on the PATH).".into(), ..ClaudeInfo::default() };
    };
    let mut info = ClaudeInfo { path: Some(path.clone()), ..ClaudeInfo::default() };
    match run_capture(claude_command(&path, &["--version"]), Duration::from_secs(15)) {
        Ok((true, out, _)) => info.version = out.lines().next().unwrap_or("").trim().chars().take(80).collect(),
        Ok((false, _, err)) => info.problem = format!("`claude --version` failed: {}", first_line(&err)),
        Err(e) => info.problem = format!("couldn't run Claude Code: {e}"),
    }
    if !info.problem.is_empty() {
        return info;
    }
    match run_capture(claude_command(&path, &["auth", "status", "--json"]), Duration::from_secs(20)) {
        Ok((_, out, err)) => match parse_auth_status(&out) {
            Some((logged_in, method, account)) => {
                info.logged_in = Some(logged_in);
                info.method = method;
                info.account = account;
            }
            None => info.problem = format!("couldn't read whether it is signed in: {}", first_line(if out.trim().is_empty() { &err } else { &out })),
        },
        Err(e) => info.problem = format!("couldn't ask whether it is signed in: {e}"),
    }
    info
}

fn first_line(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no output").chars().take(200).collect()
}

/// Terminal escape sequences out (colours, cursor moves), `\r` progress lines folded.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC: up to BEL or ESC \
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {
                    chars.next();
                }
            },
            '\r' if chars.peek() != Some(&'\n') => out.push('\n'),
            c if c.is_control() && c != '\n' && c != '\t' => {}
            c => out.push(c),
        }
    }
    out
}

/// The last `https://` address in the text (the sign-in page Claude Code prints).
pub fn last_url(text: &str) -> Option<String> {
    let at = text.rfind("https://")?;
    let url: String = text.get(at..)?.chars().take_while(|c| !c.is_whitespace() && *c != '"' && *c != '\'' && *c != '>').take(4096).collect();
    (url.len() > "https://".len()).then_some(url)
}

/// `claude auth login` running with its output shown in the window.
pub struct LoginRun {
    child: Child,
    stdin: Option<ChildStdin>,
    text: Arc<Mutex<String>>,
    /// Set when it ended: (signed in, what to say).
    pub ended: Option<(bool, String)>,
    pub code: String,
}

impl LoginRun {
    pub fn start(bin: &Path) -> Result<Self, String> {
        let mut cmd = claude_command(bin, &["auth", "login"]);
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| format!("couldn't start Claude Code's sign-in: {e}"))?;
        let text = Arc::new(Mutex::new(String::new()));
        for r in [child.stdout.take().map(|o| Box::new(o) as Box<dyn Read + Send>), child.stderr.take().map(|e| Box::new(e) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
        {
            let t = Arc::clone(&text);
            let _ = spawn_thread("mfx-setup-login", move || pump(r, &t));
        }
        let stdin = child.stdin.take();
        Ok(Self { child, stdin, text, ended: None, code: String::new() })
    }
    /// What it printed so far, cleaned.
    pub fn text(&self) -> String {
        strip_ansi(&self.text.lock().unwrap_or_else(PoisonError::into_inner))
    }
    /// Send the code from the browser (one line).
    pub fn send_code(&mut self) -> Result<(), String> {
        let code: String = self.code.trim().chars().filter(|c| !c.is_control()).take(4096).collect();
        if code.is_empty() {
            return Err("paste the code from the browser first".into());
        }
        let stdin = self.stdin.as_mut().ok_or("the sign-in isn't asking for a code any more")?;
        stdin.write_all(format!("{code}\n").as_bytes()).and_then(|()| stdin.flush()).map_err(|e| format!("couldn't send it: {e}"))?;
        self.code.clear();
        Ok(())
    }
    /// True once it has ended (sets [`Self::ended`]).
    pub fn poll(&mut self) -> bool {
        if self.ended.is_some() {
            return true;
        }
        match self.child.try_wait() {
            Ok(Some(st)) => {
                self.stdin = None;
                self.ended =
                    Some(if st.success() { (true, "Claude Code says you're signed in.".into()) } else { (false, format!("The sign-in stopped ({st}).")) });
                true
            }
            Ok(None) => false,
            Err(e) => {
                self.ended = Some((false, format!("lost the sign-in: {e}")));
                true
            }
        }
    }
    pub fn cancel(&mut self) {
        self.stdin = None;
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.ended = Some((false, "Cancelled.".into()));
    }
}

impl Drop for LoginRun {
    fn drop(&mut self) {
        if self.ended.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

fn pump(mut r: Box<dyn Read + Send>, text: &Mutex<String>) {
    let mut buf = [0u8; 4096];
    loop {
        match r.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                let mut t = text.lock().unwrap_or_else(PoisonError::into_inner);
                t.push_str(&String::from_utf8_lossy(buf.get(..n).unwrap_or(&[])));
                if t.len() > MAX_LOGIN_TEXT {
                    let cut = t.len() - MAX_LOGIN_TEXT / 2;
                    let cut = (cut..t.len()).find(|i| t.is_char_boundary(*i)).unwrap_or(t.len());
                    t.drain(..cut);
                }
            }
        }
    }
}

/// Open the user's terminal running `claude auth login` (when they'd rather sign in there).
pub fn open_terminal(bin: &Path) -> Result<String, String> {
    let spawn = |mut c: Command| -> Result<(), String> {
        c.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        for v in PARENT_SESSION_VARS {
            c.env_remove(v);
        }
        let mut child = c.spawn().map_err(|e| e.to_string())?;
        let _ = spawn_thread("mfx-setup-terminal", move || child.wait());
        Ok(())
    };
    #[cfg(target_os = "macos")]
    {
        let line = format!("'{}' auth login", bin.display().to_string().replace('\'', "'\\''"));
        let script = format!("tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell", line.replace('\\', "\\\\").replace('"', "\\\""));
        let mut c = Command::new("osascript");
        c.arg("-e").arg(script);
        spawn(c).map(|()| "Terminal".to_string())
    }
    #[cfg(windows)]
    {
        let mut c = Command::new("cmd");
        c.args(["/c", "start", "Claude Code sign-in", "cmd", "/k"]).arg(bin).args(["auth", "login"]);
        spawn(c).map(|()| "Command Prompt".to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let path_var = std::env::var_os("PATH");
        let on_path = |name: &str| -> Option<PathBuf> {
            if name.contains('/') {
                return runnable(Path::new(name)).then(|| PathBuf::from(name));
            }
            path_var.as_ref().and_then(|p| std::env::split_paths(p).map(|d| d.join(name)).find(|f| runnable(f)))
        };
        let user = std::env::var("TERMINAL").ok().filter(|t| !t.is_empty() && !t.contains(' '));
        let mut terms: Vec<(String, &[&str])> = Vec::new();
        if let Some(t) = user {
            terms.push((t, &["-e"]));
        }
        for (t, flag) in [
            ("x-terminal-emulator", &["-e"][..]),
            ("gnome-terminal", &["--"][..]),
            ("konsole", &["-e"][..]),
            ("kitty", &[][..]),
            ("alacritty", &["-e"][..]),
            ("foot", &[][..]),
            ("wezterm", &["start", "--"][..]),
            ("xterm", &["-e"][..]),
        ] {
            terms.push((t.to_string(), flag));
        }
        for (t, flag) in terms {
            if let Some(exe) = on_path(&t) {
                let mut c = Command::new(exe);
                c.args(flag).arg(bin).args(["auth", "login"]);
                if spawn(c).is_ok() {
                    return Ok(t);
                }
            }
        }
        Err("no terminal found: run `claude auth login` in yours".into())
    }
}

// ---------------------------------------------------------------------------------------------
// Model packs (Whisper, Kokoro): the host fills them in from Mortiflix Studio's own catalogues

/// One file of a model pack.
#[derive(Clone, Debug, PartialEq)]
pub struct PackFile {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

/// A model Mortiflix Studio can use (Whisper for captions, Kokoro for natural voices).
#[derive(Clone, Debug, PartialEq)]
pub struct ModelPack {
    pub id: String,
    pub name: String,
    /// What it is for, in a few words.
    pub what: String,
    pub license: String,
    pub source: String,
    /// Where its files go.
    pub dir: PathBuf,
    /// This build can use it (Mortiflix Studio built with the `whisper` / `neural-voices` features).
    pub usable: bool,
    pub files: Vec<PackFile>,
}

impl ModelPack {
    pub fn size(&self) -> u64 {
        self.files.iter().fold(0u64, |a, f| a.saturating_add(f.size))
    }
    /// Files with a plain name only (a hostile catalogue can't write outside `dir`).
    fn safe_files(&self) -> impl Iterator<Item = &PackFile> {
        self.files.iter().filter(|f| !f.name.is_empty() && f.name != "." && f.name != ".." && !f.name.contains(['/', '\\', ':']))
    }
    /// Every file present with its size.
    pub fn installed(&self) -> bool {
        !self.files.is_empty()
            && self.safe_files().count() == self.files.len()
            && self.safe_files().all(|f| std::fs::metadata(self.dir.join(&f.name)).is_ok_and(|m| m.len() == f.size))
    }
    /// The `setup.download` file list.
    pub fn download_files(&self) -> Vec<Value> {
        self.safe_files().map(|f| json!({"url": f.url, "dest": self.dir.join(&f.name), "size": f.size, "sha256": f.sha256})).collect()
    }
    /// The same pack with one tiny fake file (tests: `MFX_FAKE_DOWNLOADS=1`).
    pub fn faked(mut self) -> Self {
        self.files =
            vec![PackFile { name: format!("fake-{}.bin", self.id), url: format!("fake:{FAKE_FILE_BYTES}"), sha256: String::new(), size: FAKE_FILE_BYTES }];
        self.usable = true;
        self
    }
}

/// The host (Mortiflix Studio's side) hands over its model catalogues once.
pub fn set_model_packs(egui_ctx: &egui::Context, packs: Vec<ModelPack>) {
    let fake = std::env::var("MFX_FAKE_DOWNLOADS").is_ok_and(|v| v == "1");
    let packs = if fake { packs.into_iter().map(ModelPack::faked).collect() } else { packs };
    let shared = model(egui_ctx);
    let mut m = shared.lock().unwrap_or_else(PoisonError::into_inner);
    m.packs = packs;
    m.packs_set = true;
    m.installed_at = None;
}

pub fn has_model_packs(egui_ctx: &egui::Context) -> bool {
    model(egui_ctx).lock().unwrap_or_else(PoisonError::into_inner).packs_set
}

// ---------------------------------------------------------------------------------------------
// The window's state

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Page {
    #[default]
    Claude,
    Keys,
    Tools,
    /// Settings only: which voice narrates (not one of the three first-run steps).
    Narration,
}

const PAGES: [(Page, &str, &str); 4] =
    [(Page::Claude, "claude", "Claude Code"), (Page::Keys, "keys", "Keys"), (Page::Tools, "tools", "Tools"), (Page::Narration, "narration", "Narration")];

/// The narration engines Settings › Narration offers (`voice.set`), with what each means.
const ENGINES: [(&str, &str, &str); 4] = [
    ("qwen", "This computer", "Qwen3-TTS on your own GPU (ComfyUI). Free, nothing leaves this computer."),
    ("elevenlabs", "ElevenLabs", "Online voices; every line is a paid call on your ElevenLabs plan (needs its key under Keys)."),
    ("own", "Your own voice", "You record each line in the booth (stage 4)."),
    ("none", "No narration", "On-screen text and music only."),
];

/// A service job (download, install, test message) as its `setup.progress` events tell it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Job {
    pub label: String,
    /// running | done | failed | cancelled
    pub state: String,
    pub line: String,
    pub done: u64,
    pub total: u64,
    pub data: Value,
}

impl Job {
    pub fn running(&self) -> bool {
        self.state == "running" || self.state.is_empty()
    }
}

/// Which keys.* request a key row waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KeyStep {
    Set,
    Check,
    Remove,
}

/// Everything the Setup window keeps between frames.
#[derive(Default)]
pub struct Model {
    pub data_dir: Option<PathBuf>,
    pub settings: SetupSettings,
    /// Opened by itself on the first launch: says Welcome.
    pub first_run: bool,
    pub page: Page,
    claude: Arc<Mutex<Option<ClaudeInfo>>>,
    probing: Arc<AtomicBool>,
    probe_started: bool,
    pub login: Option<LoginRun>,
    pub note: String,
    node: Arc<Mutex<Option<String>>>,
    node_for: Option<PathBuf>,
    pub service_path: String,
    service_loaded: bool,
    was_connected: bool,
    /// The last `setup.status` result.
    pub status: Value,
    status_req: Option<u64>,
    key_inputs: HashMap<String, String>,
    key_reqs: HashMap<String, (u64, KeyStep)>,
    key_lines: HashMap<String, (Option<bool>, String)>,
    backend_req: Option<u64>,
    pushed_claude: String,
    /// row id → request id (the reply carries the job id)
    row_reqs: HashMap<String, u64>,
    /// row id → job id
    row_jobs: HashMap<String, u64>,
    pub jobs: HashMap<u64, Job>,
    event_seq: u64,
    /// The row whose download is waiting for the user's yes.
    pub ask: Option<String>,
    /// The ask just opened: scroll it into view once.
    ask_new: bool,
    /// A job just started from this row: scroll its progress line into view once.
    scroll_job: Option<String>,
    packs: Vec<ModelPack>,
    packs_set: bool,
    installed: HashMap<String, bool>,
    installed_at: Option<Instant>,
    /// The last `voice.list` / `voice.set` result (Settings › Narration).
    pub voice: Value,
    voice_req: Option<u64>,
}

fn model(egui_ctx: &egui::Context) -> Arc<Mutex<Model>> {
    egui_ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("mfx-setup-model"), || Arc::new(Mutex::new(Model::default()))).clone())
}

/// At start (desktop app): remember the data folder and open the Welcome window on the first launch
/// (`welcome`: false for `--demo` runs). A packaged app's own service is found by `ServiceConfig`.
pub fn init(egui_ctx: &egui::Context, mfx: &mut Mortiflix, data_dir: Option<PathBuf>, welcome: bool) {
    let shared = model(egui_ctx);
    let mut m = shared.lock().unwrap_or_else(PoisonError::into_inner);
    m.settings = data_dir.as_deref().map(SetupSettings::load).unwrap_or_default();
    m.data_dir = data_dir.clone();
    if welcome && !m.settings.welcome_done {
        mfx.state.show_setup = true;
        m.first_run = true;
        m.settings.welcome_done = true;
        if let Some(dir) = data_dir.as_deref()
            && let Err(e) = m.settings.save(dir)
        {
            log::warn!("mortiflix setup: couldn't save {SETUP_FILE}: {e}");
        }
    }
}

impl Model {
    pub fn claude(&self) -> Option<ClaudeInfo> {
        self.claude.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
    pub fn probing(&self) -> bool {
        self.probing.load(Ordering::Relaxed)
    }
    /// Look for Claude Code again (on a thread).
    pub fn probe(&mut self, egui_ctx: &egui::Context) {
        if self.probing.swap(true, Ordering::Relaxed) {
            return;
        }
        self.probe_started = true;
        let (slot, flag, saved, waker) = (Arc::clone(&self.claude), Arc::clone(&self.probing), self.settings.claude_path.clone(), egui_ctx.clone());
        let spawned = spawn_thread("mfx-setup-claude", move || {
            let info = probe_claude(&saved);
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(info);
            flag.store(false, Ordering::Relaxed);
            waker.request_repaint();
        });
        if spawned.is_none() {
            self.probing.store(false, Ordering::Relaxed);
        }
    }

    fn save_settings(&mut self) {
        if let Some(dir) = self.data_dir.as_deref()
            && let Err(e) = self.settings.save(dir)
        {
            self.note = format!("Couldn't save setup: {e}");
        }
    }

    /// Read threads, replies and events. Never blocks.
    pub fn tick(&mut self, egui_ctx: &egui::Context, ctx: &mut MfxCtx) {
        if !self.probe_started {
            self.probe(egui_ctx);
        }
        if !self.service_loaded {
            self.service_loaded = true;
            let s = self.data_dir.as_deref().map(ServiceSettings::load).unwrap_or_default();
            self.service_path = s.service_path;
        }
        // remember the claude we found, and tell the service where it is
        if let Some(info) = self.claude()
            && let Some(p) = info.path.as_ref()
        {
            let p = p.display().to_string();
            if info.logged_in == Some(true) && self.settings.claude_path != p {
                self.settings.claude_path = p.clone();
                self.save_settings();
            }
            if ctx.service.status().is_connected() && self.pushed_claude != p {
                self.pushed_claude = p.clone();
                self.backend_req = Some(ctx.service.request("setup.backend", json!({"claudeBin": p})));
            }
        }
        if let Some(run) = self.login.as_mut()
            && run.ended.is_none()
            && run.poll()
        {
            self.probe(egui_ctx);
        }
        // signed in and Claude Code agrees: the sign-in box has done its job
        if self.login.as_ref().is_some_and(|l| l.ended.as_ref().is_some_and(|e| e.0)) && !self.probing() && self.claude().is_some_and(|i| i.linked()) {
            self.login = None;
            self.note = "Claude Code is linked.".into();
        }
        let node = ServiceConfig::resolve(self.data_dir.as_deref()).node;
        if self.node_for.as_ref() != Some(&node) {
            self.node_for = Some(node.clone());
            let (slot, waker) = (Arc::clone(&self.node), egui_ctx.clone());
            *slot.lock().unwrap_or_else(PoisonError::into_inner) = None;
            let _ = spawn_thread("mfx-setup-node", move || {
                let mut c = Command::new(&node);
                c.arg("--version");
                no_window(&mut c);
                let line = match run_capture(c, Duration::from_secs(10)) {
                    Ok((true, out, _)) => format!("Node {}", first_line(&out)),
                    Ok((false, _, err)) => format!("not working: {}", first_line(&err)),
                    Err(e) => format!("not found ({}: {e})", node.display()),
                };
                *slot.lock().unwrap_or_else(PoisonError::into_inner) = Some(line);
                waker.request_repaint();
            });
        }
        let connected = ctx.service.status().is_connected();
        if connected && !self.was_connected {
            self.refresh(ctx);
        }
        if !connected {
            self.pushed_claude.clear();
        }
        self.was_connected = connected;
        self.take_replies(ctx);
        let events: Vec<(u64, Value)> = ctx.service.events_since(self.event_seq).map(|(s, v)| (*s, v.clone())).collect();
        for (seq, ev) in events {
            self.event_seq = self.event_seq.max(seq);
            if ev.get("event").and_then(Value::as_str) == Some("setup.progress") {
                self.on_progress(ev.get("data").unwrap_or(&Value::Null), ctx);
            }
        }
        if self.installed_at.is_none_or(|t| t.elapsed() > Duration::from_secs(3)) {
            self.installed_at = Some(Instant::now());
            self.installed = self.packs.iter().map(|p| (p.id.clone(), p.installed())).collect();
        }
        let busy =
            self.probing() || self.login.as_ref().is_some_and(|l| l.ended.is_none()) || self.jobs.values().any(Job::running) || !self.row_reqs.is_empty();
        if busy || matches!(ctx.service.status(), crate::ServiceStatus::Starting) {
            egui_ctx.request_repaint_after(Duration::from_millis(250));
        }
    }

    /// Ask the service for `setup.status` again.
    pub fn refresh(&mut self, ctx: &mut MfxCtx) {
        self.status_req = Some(ctx.service.request("setup.status", json!({})));
    }

    fn take_replies(&mut self, ctx: &mut MfxCtx) {
        if let Some(id) = self.status_req
            && let Some(r) = ctx.service.take_reply(id)
        {
            self.status_req = None;
            if r.ok {
                self.status = r.result;
            }
        }
        if let Some(id) = self.voice_req
            && let Some(r) = ctx.service.take_reply(id)
        {
            self.voice_req = None;
            if r.ok {
                self.voice = r.result;
            } else {
                self.note = format!("Couldn't change the voice: {}", error_text(&r.error));
            }
        }
        if let Some(id) = self.backend_req
            && let Some(r) = ctx.service.take_reply(id)
        {
            self.backend_req = None;
            if r.ok {
                if let Some(o) = self.status.as_object_mut() {
                    for k in ["backend", "claudeBin"] {
                        if let Some(v) = r.result.get(k) {
                            o.insert(k.into(), v.clone());
                        }
                    }
                }
            } else {
                self.note = error_text(&r.error);
            }
        }
        let keys: Vec<(String, (u64, KeyStep))> = self.key_reqs.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for (key, (id, step)) in keys {
            let Some(r) = ctx.service.take_reply(id) else { continue };
            self.key_reqs.remove(&key);
            let line = match step {
                _ if !r.ok => (Some(false), error_text(&r.error)),
                KeyStep::Remove => (None, "Removed.".into()),
                KeyStep::Set => {
                    // saved: now the free check (keys.check), which says whether the service accepts it
                    self.key_reqs.insert(key.clone(), (ctx.service.request("keys.check", json!({"id": key})), KeyStep::Check));
                    (None, "Saved. Checking it…".into())
                }
                KeyStep::Check => {
                    let detail: String = r.result.get("detail").and_then(Value::as_str).unwrap_or("").chars().take(300).collect();
                    match r.result.get("ok").and_then(Value::as_bool) {
                        Some(true) => (Some(true), format!("Saved and working: {detail}")),
                        Some(false) => (Some(false), format!("Saved, but {detail}")),
                        None => (None, format!("Saved ({detail})")),
                    }
                }
            };
            self.key_lines.insert(key, line);
            self.refresh(ctx);
        }
        let rows: Vec<(String, u64)> = self.row_reqs.iter().map(|(k, v)| (k.clone(), *v)).collect();
        for (row, id) in rows {
            let Some(r) = ctx.service.take_reply(id) else { continue };
            self.row_reqs.remove(&row);
            if row == "test" {
                // setup.claude.test answers directly: {ok, reply, seconds}
                let job = u64::MAX - self.jobs.len() as u64;
                self.row_jobs.insert(row, job);
                let (state, data) = if r.ok { ("done", r.result) } else { ("failed", json!({"ok": false, "detail": error_text(&r.error)})) };
                self.jobs.insert(job, Job { state: state.into(), data, ..Job::default() });
                continue;
            }
            match r.result.get("job").and_then(Value::as_u64) {
                Some(job) if r.ok => {
                    self.row_jobs.insert(row, job);
                    self.jobs.entry(job).or_insert_with(|| Job { state: "running".into(), ..Job::default() });
                }
                _ => {
                    let job = u64::MAX - self.jobs.len() as u64;
                    self.row_jobs.insert(row, job);
                    self.jobs.insert(job, Job { state: "failed".into(), line: error_text(&r.error), ..Job::default() });
                }
            }
        }
    }

    fn on_progress(&mut self, d: &Value, ctx: &mut MfxCtx) {
        let Some(job) = d.get("job").and_then(Value::as_u64) else { return };
        let s = |k: &str| d.get(k).and_then(Value::as_str).unwrap_or("").chars().take(500).collect::<String>();
        let entry = self.jobs.entry(job).or_default();
        entry.label = s("label");
        entry.state = s("state");
        if !s("line").is_empty() {
            entry.line = s("line");
        }
        if let Some(n) = d.get("done").and_then(Value::as_u64) {
            entry.done = n;
        }
        if let Some(n) = d.get("total").and_then(Value::as_u64) {
            entry.total = n;
        }
        entry.data = d.clone();
        if !entry.running() {
            self.installed_at = None;
            self.refresh(ctx);
        }
    }

    fn row_job(&self, row: &str) -> Option<&Job> {
        self.row_jobs.get(row).and_then(|j| self.jobs.get(j))
    }

    fn start_row(&mut self, ctx: &mut MfxCtx, row: &str, ty: &str, params: Value) {
        self.row_jobs.remove(row);
        self.row_reqs.insert(row.to_string(), ctx.service.request(ty, params));
        self.scroll_job = Some(row.to_string());
    }

    /// The step's short state for the left rail: (done, text).
    fn page_state(&self, page: Page) -> (bool, String) {
        match page {
            Page::Claude => match self.claude() {
                _ if self.probing() => (false, "Checking…".into()),
                Some(i) if i.linked() => (true, "Linked".into()),
                Some(i) if i.path.is_some() => (false, "Not signed in".into()),
                Some(_) => (false, "Not installed".into()),
                None => (false, "Checking…".into()),
            },
            Page::Keys => {
                let n = self.keys().iter().filter(|k| k.get("source").is_some_and(|s| !s.is_null())).count();
                (n > 0, if n == 0 { "Optional".into() } else { format!("{n} added") })
            }
            Page::Narration => {
                let engine = self.voice.get("engine").and_then(Value::as_str).unwrap_or("");
                match ENGINES.iter().find(|e| e.0 == engine) {
                    Some((_, name, _)) => (true, (*name).to_string()),
                    None => (false, "Checking…".into()),
                }
            }
            Page::Tools => {
                let Some(t) = self.status.get("tools") else { return (false, "Optional".into()) };
                let found = [t.get("ffmpeg"), t.get("studio").and_then(|s| s.get("chrome")), t.get("engine")];
                let n = found.iter().filter(|v| v.and_then(|v| v.get("ok")) == Some(&Value::Bool(true))).count();
                (n == found.len(), format!("{n} of {} ready", found.len()))
            }
        }
    }

    fn keys(&self) -> Vec<Value> {
        self.status.get("keys").and_then(Value::as_array).cloned().unwrap_or_default()
    }
}

/// A long path as `…/<last two parts>` (the full path is in the tooltip-free settings file).
fn short_path(p: &Path) -> String {
    let parts: Vec<String> = p.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.len() <= 4 {
        return p.display().to_string();
    }
    format!("…/{}", parts.iter().rev().take(2).rev().cloned().collect::<Vec<_>>().join("/"))
}

fn error_text(e: &Value) -> String {
    e.get("message").and_then(Value::as_str).unwrap_or("it didn't work").chars().take(300).collect()
}

fn mb(bytes: u64) -> String {
    let m = bytes as f64 / 1_000_000.0;
    if m >= 10.0 { format!("{m:.0} MB") } else { format!("{m:.1} MB") }
}

fn host_of(url: &str) -> String {
    url.split("://").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url).chars().take(80).collect()
}

// ---------------------------------------------------------------------------------------------
// Drawing

const ROW_GAP: f32 = 16.0;
/// The step list on the left, the step's own area, and its height.
const RAIL_W: f32 = 184.0;
const LABEL_W: f32 = 160.0;
const STATE_W: f32 = 180.0;
const BODY_W: f32 = 620.0;
const BODY_H: f32 = 440.0;
const CONTROL_H: f32 = 28.0;

fn primary(ui: &mut egui::Ui, theme: &MfxTheme, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(theme.accent).min_size(egui::vec2(0.0, CONTROL_H)))
}

fn secondary(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(egui::vec2(0.0, CONTROL_H)))
}

fn tertiary(ui: &mut egui::Ui, theme: &MfxTheme, text: &str) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(theme.text_dim)).frame(false).min_size(egui::vec2(0.0, CONTROL_H)))
}

/// A state with its word (never colour alone).
fn state_label(ui: &mut egui::Ui, theme: &MfxTheme, ok: Option<bool>, text: &str) {
    let (mark, color) = match ok {
        Some(true) => ("✔", theme.ok),
        Some(false) => ("○", theme.warn),
        None => ("…", theme.text_faint),
    };
    ui.label(RichText::new(mark).color(color));
    ui.add(egui::Label::new(RichText::new(text).color(theme.text)).truncate());
}

fn heading(ui: &mut egui::Ui, theme: &MfxTheme, title: &str, lead: &str) {
    ui.label(RichText::new(title).size(18.0).strong().color(theme.text));
    ui.add_space(4.0);
    ui.label(RichText::new(lead).color(theme.text_dim));
    ui.add_space(ROW_GAP);
}

/// The left column of a settings grid: left-aligned, one width, centred on the control height.
fn field_label(ui: &mut egui::Ui, theme: &MfxTheme, text: &str) {
    ui.allocate_ui_with_layout(egui::vec2(LABEL_W, CONTROL_H), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_size(egui::vec2(LABEL_W, CONTROL_H));
        ui.add(egui::Label::new(RichText::new(text).color(theme.text_dim)).truncate());
    });
}

/// The Setup window's content (opened with `mortiflix.setup`, from every "service not running" line
/// and by itself on the first launch).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let shared = model(ui.ctx());
    let mut m = shared.lock().unwrap_or_else(PoisonError::into_inner);
    // opened on a step (Link Claude Code: step 1)
    if let Some(page) = ctx.state.setup_page.take() {
        m.page = match page.as_str() {
            "keys" => Page::Keys,
            "tools" => Page::Tools,
            "narration" => Page::Narration,
            _ => Page::Claude,
        };
    }
    let egui_ctx = ui.ctx().clone();
    m.tick(&egui_ctx, ctx);
    draw(&mut m, ui, ctx);
}

fn draw(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let theme = ctx.theme;
    if m.first_run {
        ui.label(RichText::new("Welcome to Mortiflix Studio").size(22.0).strong().color(theme.text));
        ui.add_space(4.0);
        ui.label(
            RichText::new("Three quick steps to let the AI make videos with you. Skip any of them: Mortiflix Studio's editing works without all of this.")
                .color(theme.text_dim),
        );
        ui.add_space(ROW_GAP);
    }
    // a fixed body (the window never grows past the screen; the step scrolls inside it)
    let body_h = if m.first_run { BODY_H - 64.0 } else { BODY_H };
    ui.allocate_ui_with_layout(egui::vec2(RAIL_W + BODY_W + 16.0, body_h), egui::Layout::left_to_right(egui::Align::Min), |ui| {
        ui.set_max_height(body_h);
        ui.allocate_ui_with_layout(egui::vec2(RAIL_W, body_h), egui::Layout::top_down(egui::Align::Min), |ui| {
            ui.set_width(RAIL_W);
            ui.set_min_height(body_h);
            // the first launch is three steps; Settings (the menu, later) also has Narration
            let pages = if m.first_run { &PAGES[..3] } else { &PAGES[..] };
            for (i, (page, key, name)) in pages.iter().enumerate() {
                let (done, state) = m.page_state(*page);
                let selected = m.page == *page;
                // one row: number + name, the step's state under it; selected = a lighter surface and a left rule
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(RAIL_W - 8.0, 48.0), egui::Sense::click());
                let fill = if selected {
                    theme.hover
                } else if resp.hovered() {
                    theme.panel_bg
                } else {
                    Color32::TRANSPARENT
                };
                let p = ui.painter();
                p.rect_filled(rect, 4.0, fill);
                if selected {
                    p.rect_filled(egui::Rect::from_min_size(rect.min, egui::vec2(3.0, rect.height())), 0.0, theme.accent);
                }
                let x = rect.left() + 14.0;
                p.text(
                    egui::pos2(x, rect.top() + 8.0),
                    egui::Align2::LEFT_TOP,
                    format!("{}  {name}", i + 1),
                    egui::FontId::proportional(14.0),
                    if selected { theme.tab_text_active } else { theme.tab_text },
                );
                p.text(
                    egui::pos2(x + 16.0, rect.top() + 28.0),
                    egui::Align2::LEFT_TOP,
                    if done { format!("✔ {state}") } else { state },
                    egui::FontId::proportional(12.0),
                    if done { theme.ok } else { theme.text_faint },
                );
                ui.add_space(4.0);
                if resp.clicked() {
                    m.page = *page;
                }
                ctx.auto(&format!("mfx.setup.page.{key}"), &resp, name);
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            ui.set_width(BODY_W);
            egui::ScrollArea::vertical().id_salt("mfx-setup-body").max_height(body_h).min_scrolled_height(body_h).auto_shrink([false, false]).show(ui, |ui| {
                ui.set_max_width(BODY_W - 16.0);
                match m.page {
                    Page::Claude => claude_page(m, ui, ctx),
                    Page::Keys => keys_page(m, ui, ctx),
                    Page::Tools => tools_page(m, ui, ctx),
                    Page::Narration => narration_page(m, ui, ctx),
                }
            });
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(match (m.note.is_empty(), m.first_run) {
                (false, _) => m.note.as_str(),
                (true, true) => "Change any of this later: Edit › Preferences › Mortiflix…",
                (true, false) => "Changes are saved as you make them.",
            })
            .small()
            .color(theme.text_faint),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Settings (opened from the menu or a link, not the first launch): change any page, then Done.
            // It used to be the first-run wizard again: Welcome steps, Next and "Skip setup for now".
            if !m.first_run {
                let done = primary(ui, &theme, "Done");
                if done.clicked() {
                    finish(m, ctx);
                }
                ctx.auto("mfx.setup.done", &done, "Done");
                return;
            }
            let last = m.page == Page::Tools;
            let next = if last { primary(ui, &theme, "Finish") } else { secondary(ui, "Next ›") };
            if next.clicked() {
                if last {
                    finish(m, ctx);
                } else {
                    m.page = if m.page == Page::Claude { Page::Keys } else { Page::Tools };
                }
            }
            ctx.auto(if last { "mfx.setup.finish" } else { "mfx.setup.next" }, &next, if last { "Finish" } else { "Next" });
            let skip = tertiary(ui, &theme, "Skip setup for now");
            if skip.clicked() {
                finish(m, ctx);
            }
            ctx.auto("mfx.setup.skip", &skip, "Skip setup for now");
        });
    });
}

/// Close the window; the service remembers that first-run setup is done (`setup.done`).
fn finish(m: &mut Model, ctx: &mut MfxCtx) {
    ctx.state.show_setup = false;
    m.first_run = false;
    if ctx.service.status().is_connected() {
        let _ = ctx.service.request("setup.done", json!({}));
    }
}

fn needs_service(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx, what: &str) -> bool {
    if ctx.service.status().is_connected() {
        return false;
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(format!("{what} needs the Mortiflix service, which isn't running.")).color(ctx.theme.warn));
        let go = ui.link("Set it up in Tools");
        if go.clicked() {
            m.page = Page::Tools;
        }
        ctx.auto("mfx.setup.goTools", &go, "Set it up in Tools");
    });
    true
}

fn claude_page(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let theme = ctx.theme;
    heading(ui, &theme, "Link your Claude Code", "The AI in Mortiflix is your own Claude Code: it works on your Claude plan, and the chat talks to it.");
    let info = m.claude();
    let egui_ctx = ui.ctx().clone();
    egui::Grid::new("mfx-setup-claude").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
        field_label(ui, &theme, "Installed");
        ui.horizontal(|ui| match &info {
            _ if m.probing() => state_label(ui, &theme, None, "Looking for Claude Code…"),
            Some(i) if i.path.is_some() => {
                let path = i.path.as_ref().map(|p| short_path(p)).unwrap_or_default();
                state_label(ui, &theme, Some(true), &format!("{} · {path}", if i.version.is_empty() { "Claude Code" } else { &i.version }));
            }
            Some(_) => state_label(ui, &theme, Some(false), "Not found"),
            None => state_label(ui, &theme, None, "Looking for Claude Code…"),
        });
        ui.end_row();
        field_label(ui, &theme, "Signed in");
        ui.horizontal(|ui| match &info {
            Some(i) if i.logged_in == Some(true) => {
                let how = [i.method.as_str(), i.account.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" · ");
                state_label(ui, &theme, Some(true), &if how.is_empty() { "Yes".to_string() } else { format!("Yes ({how})") });
            }
            Some(i) if i.logged_in == Some(false) => state_label(ui, &theme, Some(false), "Not yet"),
            _ => state_label(ui, &theme, None, "—"),
        });
        ui.end_row();
    });
    if let Some(p) = info.as_ref().map(|i| i.problem.clone()).filter(|p| !p.is_empty()) {
        ui.add_space(8.0);
        ui.label(RichText::new(p).color(theme.warn));
    }
    ui.add_space(ROW_GAP);

    let path = info.as_ref().and_then(|i| i.path.clone());
    let signed_in = info.as_ref().is_some_and(ClaudeInfo::linked);
    let login_running = m.login.as_ref().is_some_and(|l| l.ended.is_none());
    ui.horizontal(|ui| {
        match &path {
            None => {
                let install = primary(ui, &theme, "How to install Claude Code");
                if install.clicked() {
                    egui_ctx.open_url(egui::OpenUrl::new_tab(CLAUDE_INSTALL_URL));
                }
                ctx.auto("mfx.setup.claude.install", &install, "How to install Claude Code");
            }
            Some(bin) if !signed_in && !login_running => {
                let link = primary(ui, &theme, "Link Claude Code");
                if link.clicked() {
                    match LoginRun::start(bin) {
                        Ok(run) => m.login = Some(run),
                        Err(e) => m.note = e,
                    }
                }
                ctx.auto("mfx.setup.claude.link", &link, "Link Claude Code");
            }
            _ => {}
        }
        let again = secondary(ui, "Check again");
        if again.clicked() {
            m.probe(&egui_ctx);
        }
        ctx.auto("mfx.setup.claude.check", &again, "Check again");
        if let Some(bin) = path.as_ref().filter(|_| !signed_in) {
            let term = tertiary(ui, &theme, "Sign in from a terminal instead");
            if term.clicked() {
                m.note = match open_terminal(bin) {
                    Ok(t) => format!("Opened {t}: sign in there, then press Check again."),
                    Err(e) => e,
                };
            }
            ctx.auto("mfx.setup.claude.terminal", &term, "Sign in from a terminal instead");
        }
    });

    if m.login.is_some() {
        ui.add_space(ROW_GAP);
        login_box(m, ui, ctx);
    }

    if signed_in {
        ui.add_space(ROW_GAP);
        ui.label(RichText::new("Test the chat").strong().color(theme.text));
        ui.add_space(4.0);
        ui.label(RichText::new("Sends one short message through the Mortiflix service to your Claude Code, the way the chat will.").color(theme.text_dim));
        ui.add_space(8.0);
        if !needs_service(m, ui, ctx, "The test message") {
            let job = m.row_job("test").cloned();
            let busy = m.row_reqs.contains_key("test") || job.as_ref().is_some_and(Job::running);
            ui.horizontal(|ui| {
                let test = ui.add_enabled(!busy, egui::Button::new("Send a test message").min_size(egui::vec2(0.0, CONTROL_H)));
                if test.clicked() {
                    m.start_row(ctx, "test", "setup.claude.test", json!({}));
                }
                ctx.auto("mfx.setup.claude.test", &test, "Send a test message");
                if busy {
                    ui.spinner();
                    ui.label(RichText::new("Waiting for Claude Code…").color(theme.text_dim));
                }
            });
            if let Some(j) = job.filter(|j| !j.running()) {
                ui.add_space(8.0);
                let ok = j.data.get("ok").and_then(Value::as_bool).unwrap_or(j.state == "done");
                let text = if ok {
                    format!("Claude Code answered: “{}”", j.data.get("reply").and_then(Value::as_str).unwrap_or(""))
                } else {
                    format!("No answer: {}", j.data.get("detail").and_then(Value::as_str).unwrap_or(&j.line))
                };
                ui.horizontal_wrapped(|ui| state_label(ui, &theme, Some(ok), &text));
            }
        }
    }

    ui.add_space(ROW_GAP);
    egui::CollapsingHeader::new(RichText::new("No Claude Code? Use an Anthropic API key instead (optional)").color(theme.text_dim))
        .id_salt("mfx-setup-api")
        .show(ui, |ui| {
            ui.label(RichText::new("Only if you can't use Claude Code: the Claude API bills your key per token.").color(theme.text_dim));
            ui.add_space(8.0);
            if needs_service(m, ui, ctx, "Saving a key") {
                return;
            }
            if let Some(k) = m.keys().into_iter().find(|k| k.get("id").and_then(Value::as_str) == Some("anthropic")) {
                key_row(m, ui, ctx, &k);
            }
            ui.add_space(8.0);
            let backend = m.status.get("backend").and_then(Value::as_str).unwrap_or("claude-code").to_string();
            ui.horizontal(|ui| {
                field_label(ui, &theme, "The AI runs on");
                for (id, label) in [("claude-code", "Claude Code (your plan)"), ("anthropic-api", "Claude API (your key)")] {
                    let r = ui.radio(backend == id, label);
                    if r.clicked() && backend != id {
                        m.backend_req = Some(ctx.service.request("setup.backend", json!({"backend": id})));
                    }
                    ctx.auto(&format!("mfx.setup.backend.{id}"), &r, label);
                }
            });
        });
}

fn login_box(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let theme = ctx.theme;
    let egui_ctx = ui.ctx().clone();
    let Some((text, ended)) = m.login.as_ref().map(|r| (r.text(), r.ended.clone())) else { return };
    let url = last_url(&text);
    ui.label(RichText::new("Signing in").strong().color(theme.text));
    ui.add_space(4.0);
    ui.label(RichText::new("Claude Code opens its sign-in page in your browser. Sign in there; if it shows a code, paste it below.").color(theme.text_dim));
    ui.add_space(8.0);
    egui::Frame::new().fill(theme.field_bg).inner_margin(egui::Margin::same(8)).corner_radius(4.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt("mfx-setup-login-log").max_height(120.0).stick_to_bottom(true).show(ui, |ui| {
            let shown = if text.trim().is_empty() { "Starting Claude Code's sign-in…".to_string() } else { text.trim_end().to_string() };
            ui.add(egui::Label::new(RichText::new(shown).monospace().size(12.0).color(theme.text_dim)).wrap());
        });
    });
    ui.add_space(8.0);
    match ended {
        None => {
            ui.horizontal(|ui| {
                if let Some(u) = url.as_ref() {
                    let open = secondary(ui, "Open the sign-in page");
                    if open.clicked() {
                        egui_ctx.open_url(egui::OpenUrl::new_tab(u.clone()));
                    }
                    ctx.auto("mfx.setup.login.open", &open, "Open the sign-in page");
                }
                let Some(run) = m.login.as_mut() else { return };
                let field = ui
                    .add(egui::TextEdit::singleline(&mut run.code).hint_text("Paste the code here").desired_width(260.0).min_size(egui::vec2(0.0, CONTROL_H)));
                ctx.auto("mfx.setup.login.code", &field, "Paste the code here");
                let send = primary(ui, &theme, "Send code");
                if (send.clicked() || (field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))))
                    && let Err(e) = run.send_code()
                {
                    m.note = e;
                }
                ctx.auto("mfx.setup.login.send", &send, "Send code");
                let cancel = tertiary(ui, &theme, "Cancel");
                if cancel.clicked()
                    && let Some(run) = m.login.as_mut()
                {
                    run.cancel();
                }
                ctx.auto("mfx.setup.login.cancel", &cancel, "Cancel");
            });
        }
        Some((ok, what)) => {
            ui.horizontal(|ui| {
                state_label(ui, &theme, Some(ok), &what);
                let close = tertiary(ui, &theme, "Hide");
                if close.clicked() {
                    m.login = None;
                }
                ctx.auto("mfx.setup.login.hide", &close, "Hide");
            });
        }
    }
}

fn key_row(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx, k: &Value) {
    let theme = ctx.theme;
    let s = |f: &str| k.get(f).and_then(Value::as_str).unwrap_or("").chars().take(400).collect::<String>();
    let id = s("id");
    if id.is_empty() {
        return;
    }
    let source = k.get("source").and_then(Value::as_str).map(str::to_string);
    ui.horizontal(|ui| {
        ui.label(RichText::new(s("name")).strong().color(theme.text));
        ui.add_space(8.0);
        match source.as_deref() {
            Some("saved" | "session.env") => state_label(ui, &theme, Some(true), "Saved"),
            Some(_) => state_label(ui, &theme, Some(true), "From your environment"),
            None => state_label(ui, &theme, Some(false), "Not added"),
        }
    });
    ui.add_space(2.0);
    ui.label(RichText::new(format!("For {}", s("for"))).color(theme.text_dim));
    ui.label(RichText::new(format!("Get one: {}", s("get"))).small().color(theme.text_faint));
    ui.add_space(6.0);
    let busy = m.key_reqs.contains_key(&id);
    ui.horizontal(|ui| {
        let input = m.key_inputs.entry(id.clone()).or_default();
        let field = ui.add(
            egui::TextEdit::singleline(input)
                .password(true)
                .hint_text(if source.is_some() { "Paste a new key to replace it" } else { "Paste the key" })
                .desired_width(300.0)
                .min_size(egui::vec2(0.0, CONTROL_H)),
        );
        ctx.auto(&format!("mfx.setup.key.{id}.input"), &field, &format!("{} field", s("name")));
        let has_text = !input.trim().is_empty();
        let save = ui.add_enabled(has_text && !busy, egui::Button::new("Save and check").min_size(egui::vec2(0.0, CONTROL_H)));
        if save.clicked() || (has_text && field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
            let value = std::mem::take(input);
            m.key_reqs.insert(id.clone(), (ctx.service.request("keys.set", json!({"id": id, "value": value.trim()})), KeyStep::Set));
            m.key_lines.remove(&id);
        }
        ctx.auto(&format!("mfx.setup.key.{id}.save"), &save, "Save and check");
        if matches!(source.as_deref(), Some("saved" | "session.env")) {
            let remove = tertiary(ui, &theme, "Remove");
            if remove.clicked() {
                m.key_reqs.insert(id.clone(), (ctx.service.request("keys.remove", json!({"id": id})), KeyStep::Remove));
            }
            ctx.auto(&format!("mfx.setup.key.{id}.remove"), &remove, "Remove");
        }
        if busy {
            ui.spinner();
        }
    });
    if let Some((ok, line)) = m.key_lines.get(&id).filter(|(_, l)| !l.is_empty()) {
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| state_label(ui, &theme, *ok, line));
    }
}

fn keys_page(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let theme = ctx.theme;
    heading(
        ui,
        &theme,
        "Keys (optional)",
        "Add only the services you use. Keys stay on this computer, in your studio folder, readable only by you; they are never shown again or saved in projects.",
    );
    if needs_service(m, ui, ctx, "Saving keys") {
        return;
    }
    let keys: Vec<Value> = m.keys().into_iter().filter(|k| k.get("id").and_then(Value::as_str) != Some("anthropic")).collect();
    if keys.is_empty() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Asking the service…").color(theme.text_dim));
        });
        if m.status_req.is_none() {
            m.refresh(ctx);
        }
        return;
    }
    for (i, k) in keys.iter().enumerate() {
        if i > 0 {
            ui.add_space(ROW_GAP);
        }
        key_row(m, ui, ctx, k);
    }
}

/// Settings › Narration: which engine reads the script (`voice.list`, `voice.set`).
fn narration_page(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let theme = ctx.theme;
    heading(ui, &theme, "Narration", "Which voice reads the script. New lines and remakes use it; lines already made keep theirs.");
    if needs_service(m, ui, ctx, "Choosing a voice") {
        return;
    }
    if m.voice.is_null() {
        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(RichText::new("Asking the service…").color(theme.text_dim));
        });
        if m.voice_req.is_none() {
            m.voice_req = Some(ctx.service.request("voice.list", json!({})));
        }
        return;
    }
    let current = m.voice.get("engine").and_then(Value::as_str).unwrap_or("").to_string();
    for (id, name, what) in ENGINES {
        let r = ui.radio(current == id, RichText::new(name).color(theme.text));
        ui.indent(("mfx-setup-engine", id), |ui| {
            ui.label(RichText::new(what).small().color(theme.text_dim));
        });
        ui.add_space(6.0);
        if r.clicked() && current != id && m.voice_req.is_none() {
            m.voice_req = Some(ctx.service.request("voice.set", json!({"engine": id})));
        }
        ctx.auto(&format!("mfx.setup.voice.{id}"), &r, name);
    }
    if current == "elevenlabs" && m.voice.get("elevenlabs_key").is_none_or(Value::is_null) {
        state_label(ui, &theme, Some(false), "No ElevenLabs key yet: add it under Keys.");
    }
}

/// The window's title: Setup on the first launch, Settings after that (Edit › Preferences › Mortiflix…).
pub fn window_title(egui_ctx: &egui::Context) -> &'static str {
    let shared = model(egui_ctx);
    let m = shared.lock().unwrap_or_else(PoisonError::into_inner);
    if m.first_run { "Mortiflix Setup" } else { "Mortiflix Settings" }
}

fn tools_page(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let theme = ctx.theme;
    heading(ui, &theme, "Tools", "What making videos uses. Downloads always ask first and show how big they are.");
    service_block(m, ui, ctx);
    ui.add_space(ROW_GAP);
    let tools = m.status.get("tools").cloned().unwrap_or(Value::Null);
    let connected = ctx.service.status().is_connected();
    let tool = |k: &str| tools.get(k).cloned().unwrap_or(Value::Null);
    let line = |v: &Value| {
        let text = v.get("detail").or_else(|| v.get("path")).and_then(Value::as_str).unwrap_or("");
        let text = if text.is_empty() { if v.get("ok") == Some(&Value::Bool(true)) { "Ready" } else { "Not found" } } else { text };
        text.chars().take(200).collect::<String>()
    };
    let ok = |v: &Value| v.get("ok").and_then(Value::as_bool);
    egui::Grid::new("mfx-setup-tools").num_columns(3).spacing([12.0, 12.0]).min_col_width(110.0).show(ui, |ui| {
        field_label(ui, &theme, "ffmpeg");
        let v = tool("ffmpeg");
        ui.horizontal(|ui| {
            ui.set_min_width(STATE_W);
            state_label(ui, &theme, if connected { ok(&v) } else { None }, &if connected { line(&v) } else { "—".into() });
        });
        ui.label(RichText::new("audio and video conversion").small().color(theme.text_faint));
        ui.end_row();

        // the components the installers offer (setup.mjs TOOLS): installed, skipped or failed, and Install… for
        // the ones skipped (the same installers, through `setup.install`)
        let parts = tools.get("installable").and_then(Value::as_array).cloned().unwrap_or_default();
        for c in parts.iter().take(40) {
            let s = |k: &str| c.get(k).and_then(Value::as_str).unwrap_or("").chars().take(120).collect::<String>();
            let id = s("id");
            if id.is_empty() || id.contains(['.', ':', ' ']) {
                continue;
            }
            let name = if s("name").is_empty() { id.clone() } else { s("name") };
            let v = if id == "daw-engine" { tool("engine") } else { tools.get("studio").and_then(|t| t.get(&id)).cloned().unwrap_or(Value::Null) };
            let failed = m.row_job(&id).filter(|j| j.state == "failed").map(|j| j.line.clone());
            let (state, text) = match ok(&v) {
                Some(true) => (Some(true), "Installed".to_string()),
                _ if failed.is_some() => (Some(false), "Install failed".to_string()),
                Some(false) => (Some(false), "Not installed".to_string()),
                None => (None, "Not checked here".to_string()),
            };
            field_label(ui, &theme, &name);
            ui.horizontal(|ui| {
                ui.set_min_width(STATE_W);
                state_label(ui, &theme, if connected { state } else { None }, &if connected { text } else { "—".into() });
            });
            if connected && state != Some(true) {
                download_button(m, ui, ctx, &id, &if failed.is_some() { "Try again…".to_string() } else { "Install…".to_string() });
            } else {
                ui.label(RichText::new(s("size")).small().color(theme.text_faint));
            }
            ui.end_row();
        }

        let packs = m.packs.clone();
        for p in &packs {
            field_label(ui, &theme, &p.name);
            let installed = m.installed.get(&p.id).copied().unwrap_or(false);
            let state = if installed {
                (Some(true), "Downloaded".to_string())
            } else if !p.usable {
                (None, "This build can't use it".to_string())
            } else {
                (Some(false), "Not downloaded".to_string())
            };
            ui.horizontal(|ui| {
                ui.set_min_width(STATE_W);
                state_label(ui, &theme, state.0, &state.1);
            });
            let row = format!("pack:{}", p.id);
            if !installed && p.usable && connected {
                download_button(m, ui, ctx, &row, &format!("Download ({})…", mb(p.size())));
            } else {
                ui.label(RichText::new(&p.what).small().color(theme.text_faint));
            }
            ui.end_row();
        }
    });
    // the ask, and running downloads, under the table
    if let Some(row) = m.ask.clone() {
        ui.add_space(ROW_GAP);
        ask_box(m, ui, ctx, &row);
    }
    let rows: Vec<String> = m.row_jobs.keys().filter(|r| r.as_str() != "test").cloned().collect();
    for row in rows {
        if let Some(j) = m.row_job(&row).cloned() {
            ui.add_space(8.0);
            let shown = ui.scope(|ui| job_line(m, ui, ctx, &row, &j)).response;
            if m.scroll_job.as_deref() == Some(row.as_str()) {
                m.scroll_job = None;
                shown.scroll_to_me(Some(egui::Align::Center));
            }
        }
    }
}

fn service_block(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    let theme = ctx.theme;
    let status = ctx.service.status().clone();
    egui::Grid::new("mfx-setup-service").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
        field_label(ui, &theme, "Mortiflix service");
        ui.horizontal_wrapped(|ui| {
            let ok = if status.is_connected() {
                Some(true)
            } else if matches!(status, crate::ServiceStatus::Starting) {
                None
            } else {
                Some(false)
            };
            state_label(ui, &theme, ok, &status.line());
        });
        ui.end_row();
        field_label(ui, &theme, "Node");
        let node = m.node.lock().unwrap_or_else(PoisonError::into_inner).clone();
        ui.horizontal(|ui| match node {
            Some(n) => state_label(ui, &theme, Some(n.starts_with("Node ")), &n),
            None => state_label(ui, &theme, None, "Checking…"),
        });
        ui.end_row();
    });
    if std::env::var_os("MFX_SERVICE").is_some_and(|v| !v.is_empty()) {
        ui.add_space(6.0);
        ui.label(RichText::new("Set by MFX_SERVICE for this run.").small().color(theme.text_faint));
        return;
    }
    if status.is_connected() {
        return;
    }
    ui.add_space(8.0);
    ui.label(RichText::new("Where is it? The mortiflix-oss folder, or its bin/mortiflix-service.mjs.").color(theme.text_dim));
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        let field = ui
            .add(egui::TextEdit::singleline(&mut m.service_path).hint_text("/path/to/mortiflix-oss").desired_width(330.0).min_size(egui::vec2(0.0, CONTROL_H)));
        ctx.auto("mfx.setup.service.path", &field, "Service folder");
        let use_it = primary(ui, &theme, "Use this");
        if use_it.clicked() {
            m.note = use_service(m, ctx);
        }
        ctx.auto("mfx.setup.service.use", &use_it, "Use this");
    });
}

/// Save the typed service path and restart the service with it.
fn use_service(m: &mut Model, ctx: &mut MfxCtx) -> String {
    let typed = m.service_path.trim();
    let p = PathBuf::from(typed);
    let script = if p.is_dir() { p.join(SERVICE_SCRIPT) } else { p.clone() };
    if typed.is_empty() || !script.is_file() {
        return format!("No Mortiflix service at {}.", if typed.is_empty() { "an empty path" } else { typed });
    }
    let Some(dir) = m.data_dir.clone() else { return "No data folder to save it in.".into() };
    let mut settings = ServiceSettings::load(&dir);
    settings.service_path = typed.to_string();
    match settings.save(&dir) {
        Ok(()) => {
            ctx.service.start(ServiceConfig::resolve(Some(&dir)));
            m.node_for = None;
            "Starting the Mortiflix service…".into()
        }
        Err(e) => format!("Couldn't save it: {e}"),
    }
}

fn download_button(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx, row: &str, label: &str) {
    let busy = m.row_reqs.contains_key(row) || m.row_job(row).is_some_and(Job::running);
    let b = ui.add_enabled(!busy && m.ask.as_deref() != Some(row), egui::Button::new(label).min_size(egui::vec2(0.0, CONTROL_H)));
    if b.clicked() {
        m.ask = Some(row.to_string());
        m.ask_new = true;
    }
    let id = row.replace(':', ".");
    ctx.auto(&format!("mfx.setup.tool.{id}.download"), &b, label);
}

/// What a download or install is, before it starts: its name and (label, value) facts.
fn ask_facts(m: &Model, row: &str) -> Option<(String, Vec<(&'static str, String)>)> {
    if !row.starts_with("pack:") {
        let info = m
            .status
            .get("tools")
            .and_then(|t| t.get("installable"))
            .and_then(Value::as_array)
            .and_then(|l| l.iter().find(|t| t.get("id").and_then(Value::as_str) == Some(row)).cloned())?;
        let s = |k: &str| info.get(k).and_then(Value::as_str).unwrap_or("").chars().take(400).collect::<String>();
        let name = if s("name").is_empty() { row.to_string() } else { s("name") };
        let at = if s("where").is_empty() { String::new() } else { format!("the studio's {}", s("where")) };
        return Some((name, vec![("Size", s("size")), ("What", s("what")), ("Goes in", at)]));
    }
    let id = row.strip_prefix("pack:")?;
    let p = m.packs.iter().find(|p| p.id == id)?;
    let from = p.files.first().map(|f| host_of(&f.url)).unwrap_or_default();
    Some((p.name.clone(), vec![("Size", mb(p.size())), ("From", from), ("Licence", p.license.clone()), ("Goes in", p.dir.display().to_string())]))
}

fn ask_box(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx, row: &str) {
    let theme = ctx.theme;
    let Some((title, facts)) = ask_facts(m, row) else {
        m.ask = None;
        return;
    };
    let shown = egui::Frame::new().fill(theme.header_bg).inner_margin(egui::Margin::same(12)).corner_radius(4.0).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let verb = if row.starts_with("pack:") { "Download" } else { "Install" };
        ui.label(RichText::new(format!("{verb} {title}?")).strong().color(theme.text));
        ui.add_space(6.0);
        for (k, v) in &facts {
            if !v.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_sized([70.0, 18.0], egui::Label::new(RichText::new(*k).small().color(theme.text_faint)));
                    ui.add(egui::Label::new(RichText::new(v).color(theme.text_dim)).wrap());
                });
            }
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let yes = primary(ui, &theme, verb);
            if yes.clicked() {
                m.ask = None;
                if !row.starts_with("pack:") {
                    m.start_row(ctx, row, "setup.install", json!({"tool": row}));
                } else if let Some(p) = row.strip_prefix("pack:").and_then(|id| m.packs.iter().find(|p| p.id == id)).cloned() {
                    m.start_row(ctx, row, "setup.download", json!({"label": p.name, "files": p.download_files()}));
                }
            }
            ctx.auto("mfx.setup.ask.yes", &yes, verb);
            let no = secondary(ui, "Not now");
            if no.clicked() {
                m.ask = None;
            }
            ctx.auto("mfx.setup.ask.no", &no, "Not now");
        });
    });
    if m.ask_new {
        m.ask_new = false;
        shown.response.scroll_to_me(Some(egui::Align::Center));
    }
}

fn job_line(m: &mut Model, ui: &mut egui::Ui, ctx: &mut MfxCtx, row: &str, j: &Job) {
    let theme = ctx.theme;
    let title = if j.label.is_empty() { "Download".to_string() } else { j.label.clone() };
    if j.running() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(&title).color(theme.text));
            let frac = if j.total > 0 { (j.done as f32 / j.total as f32).clamp(0.0, 1.0) } else { 0.0 };
            let text = if j.total > 0 { format!("{} of {}", mb(j.done), mb(j.total)) } else { j.line.clone() };
            ui.add(egui::ProgressBar::new(frac).desired_width(240.0).text(text));
            let cancel = tertiary(ui, &theme, "Cancel");
            if cancel.clicked()
                && let Some(job) = m.row_jobs.get(row)
            {
                let _ = ctx.service.request("setup.cancel", json!({"job": job}));
            }
            ctx.auto(&format!("mfx.setup.job.{}.cancel", row.replace(':', ".")), &cancel, "Cancel");
        });
    } else {
        ui.horizontal_wrapped(|ui| state_label(ui, &theme, Some(j.state == "done"), &format!("{title}: {}", j.line)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mfx-setup-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[cfg(unix)]
    fn script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn settings_round_trip_and_hostile_files() {
        let d = temp("settings");
        assert_eq!(SetupSettings::load(&d), SetupSettings::default());
        let s = SetupSettings { welcome_done: true, claude_path: "/x/claude".into() };
        s.save(&d).unwrap();
        assert_eq!(SetupSettings::load(&d), s);
        std::fs::write(SetupSettings::path(&d), b"{not json").unwrap();
        assert_eq!(SetupSettings::load(&d), SetupSettings::default());
        std::fs::write(SetupSettings::path(&d), vec![b' '; 70 * 1024]).unwrap();
        assert_eq!(SetupSettings::load(&d), SetupSettings::default());
        std::fs::write(SetupSettings::path(&d), br#"{"welcomeDone": 5, "claudePath": []}"#).unwrap();
        assert_eq!(SetupSettings::load(&d), SetupSettings::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn auth_status_is_read_and_hostile_output_is_none() {
        let out = r#"{"loggedIn": true, "authMethod": "claude.ai", "email": "a@b.c", "subscriptionType": "max"}"#;
        assert_eq!(parse_auth_status(out), Some((true, "claude.ai".into(), "a@b.c, max".into())));
        let out = "warning\n{\n \"loggedIn\": false,\n \"authMethod\": \"none\"\n}\n";
        assert_eq!(parse_auth_status(out), Some((false, "none".into(), String::new())));
        assert_eq!(parse_auth_status(r#"{"account": {"email": "x@y"}, "loggedIn": true}"#).map(|r| r.2), Some("x@y".into()));
        for bad in ["", "Not logged in", "{", "}{", r#"{"loggedIn": "yes"}"#, "[1,2]", "{\"loggedIn\": tru"] {
            assert_eq!(parse_auth_status(bad), None, "{bad}");
        }
    }

    #[test]
    fn ansi_and_urls() {
        let raw = "\u{1b}[1mOpening\u{1b}[0m browser…\r\nIf it didn't open, visit: https://claude.com/oauth?code=true&x=1\n\u{1b}]8;;https://evil\u{7}link\u{1b}]8;;\u{7}\rPaste code here > ";
        let clean = strip_ansi(raw);
        assert!(clean.starts_with("Opening browser…\n"));
        assert!(!clean.contains('\u{1b}'));
        assert_eq!(last_url(&clean).as_deref(), Some("https://claude.com/oauth?code=true&x=1"));
        assert_eq!(last_url("no address"), None);
        assert_eq!(last_url("https:// "), None);
        // a lone escape at the end and odd bytes never panic
        let _ = strip_ansi("\u{1b}");
        let _ = strip_ansi("\u{1b}[");
        let _ = strip_ansi("\u{1b}]");
        let _ = strip_ansi("ok\u{1b}");
    }

    #[cfg(unix)]
    #[test]
    fn claude_is_found_on_the_path_then_in_install_folders() {
        let d = temp("find");
        let bin = d.join("bin");
        let home = d.join("home");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::create_dir_all(home.join(".local/bin")).unwrap();
        // not executable: skipped
        std::fs::write(bin.join("claude"), "x").unwrap();
        script(&home.join(".local/bin/claude"), "echo hi");
        let path = std::env::join_paths([&bin]).unwrap();
        assert_eq!(find_claude("", Some(&path), Some(&home)), Some(home.join(".local/bin/claude")));
        script(&bin.join("claude"), "echo hi");
        assert_eq!(find_claude("", Some(&path), Some(&home)), Some(bin.join("claude")));
        // the linked one first; a relative or missing saved path is ignored
        assert_eq!(find_claude(&home.join(".local/bin/claude").display().to_string(), Some(&path), Some(&home)), Some(home.join(".local/bin/claude")));
        assert_eq!(find_claude("claude", Some(&path), Some(&home)), Some(bin.join("claude")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn model_packs_never_write_outside_their_folder() {
        let d = temp("packs");
        let f = |name: &str, size| PackFile { name: name.into(), url: "https://h/x".into(), sha256: "a".repeat(64), size };
        let p = ModelPack {
            id: "m".into(),
            name: "M".into(),
            what: String::new(),
            license: String::new(),
            source: String::new(),
            dir: d.clone(),
            usable: true,
            files: vec![f("a.bin", 3), f("../evil", 1), f("x/y", 1), f("", 1)],
        };
        assert_eq!(p.download_files().len(), 1);
        std::fs::write(d.join("a.bin"), b"abc").unwrap();
        // the hostile names keep it from ever counting as installed
        assert!(!p.installed());
        let ok = ModelPack { files: vec![f("a.bin", 3)], ..p.clone() };
        assert!(ok.installed());
        assert_eq!(ok.size(), 3);
        let fake = ok.faked();
        assert_eq!(fake.files.len(), 1);
        assert!(fake.files[0].url.starts_with("fake:"));
        assert!(!fake.installed());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn probe_and_link_with_a_fake_claude() {
        let d = temp("fake-claude");
        let state = d.join("signed-in");
        let bin = d.join("claude");
        script(
            &bin,
            &format!(
                r#"case "$1 $2" in
  "--version "*) echo "9.9.9 (Claude Code)";;
  "auth status") if [ -f '{s}' ]; then echo '{{"loggedIn": true, "authMethod": "claude.ai", "email": "test@example.com"}}'; else echo '{{"loggedIn": false, "authMethod": "none"}}'; exit 1; fi;;
  "auth login") echo "Opening browser to sign in…"; echo "If the browser didn't open, visit: https://example.invalid/oauth?code=true"; printf "Paste code here if prompted > "; read code; if [ "$code" = "good" ]; then touch '{s}'; echo "Login successful."; else echo "Invalid code"; exit 1; fi;;
esac"#,
                s = state.display()
            ),
        );
        let saved = bin.display().to_string();
        let info = probe_claude(&saved);
        assert_eq!(info.path.as_deref(), Some(bin.as_path()));
        assert_eq!(info.version, "9.9.9 (Claude Code)");
        assert_eq!(info.logged_in, Some(false));
        assert!(!info.linked());

        let mut run = LoginRun::start(&bin).unwrap();
        let t = Instant::now();
        while last_url(&run.text()).is_none() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(last_url(&run.text()).as_deref(), Some("https://example.invalid/oauth?code=true"));
        assert!(run.send_code().is_err(), "empty code");
        run.code = "good\n".into();
        run.send_code().unwrap();
        while !run.poll() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(run.ended.as_ref().map(|e| e.0), Some(true));
        assert!(run.text().contains("Login successful."));
        let info = probe_claude(&saved);
        assert!(info.linked());
        assert_eq!(info.account, "test@example.com");

        // a wrong code ends it, and cancel kills a waiting one
        std::fs::remove_file(&state).unwrap();
        let mut bad = LoginRun::start(&bin).unwrap();
        bad.code = "nope".into();
        bad.send_code().unwrap();
        let t = Instant::now();
        while !bad.poll() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(bad.ended.as_ref().map(|e| e.0), Some(false));
        let mut waiting = LoginRun::start(&bin).unwrap();
        waiting.cancel();
        assert!(waiting.poll());
        assert!(waiting.send_code().is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_command_times_out() {
        let d = temp("hang");
        let bin = d.join("claude");
        script(&bin, "sleep 30");
        let t = Instant::now();
        assert!(run_capture(claude_command(&bin, &["--version"]), Duration::from_millis(300)).is_err());
        assert!(t.elapsed() < Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn welcome_shows_once_and_not_for_demo_runs() {
        let d = temp("welcome");
        let ctx = egui::Context::default();
        let mut mfx = Mortiflix::default();
        init(&ctx, &mut mfx, Some(d.clone()), false);
        assert!(!mfx.state.show_setup);
        init(&ctx, &mut mfx, Some(d.clone()), true);
        assert!(mfx.state.show_setup);
        assert!(SetupSettings::load(&d).welcome_done);
        let ctx2 = egui::Context::default();
        let mut again = Mortiflix::default();
        init(&ctx2, &mut again, Some(d.clone()), true);
        assert!(!again.state.show_setup);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_window_draws_without_a_service_and_registers_ids() {
        let ctx = egui::Context::default();
        let mut mfx = Mortiflix::default();
        init(&ctx, &mut mfx, None, true);
        let mut ids = Vec::new();
        for page in [Page::Claude, Page::Keys, Page::Tools] {
            model(&ctx).lock().unwrap().page = page;
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                let mut x = mfx.ctx(MfxTheme::default());
                show(ui, &mut x);
                ids.extend(x.auto.iter().map(|a| a.0.clone()));
            });
            out.textures_delta.clear();
        }
        for id in [
            "mfx.setup.page.claude",
            "mfx.setup.page.keys",
            "mfx.setup.page.tools",
            "mfx.setup.next",
            "mfx.setup.finish",
            "mfx.setup.skip",
            "mfx.setup.claude.check",
        ] {
            assert!(ids.iter().any(|i| i == id), "{id} in {ids:?}");
        }
        assert!(!ids.iter().any(|i| i == "mfx.setup.page.narration"), "the first launch is three steps");
        assert_eq!(window_title(&ctx), "Mortiflix Setup");
    }

    #[test]
    fn settings_later_is_not_the_wizard_again() {
        // Edit › Preferences › Mortiflix… after the first launch: Settings, every page plus Narration,
        // one Done; no Welcome steps, Next or "Skip setup for now" (run 2: it reopened Setup)
        let ctx = egui::Context::default();
        let mut mfx = Mortiflix::default();
        init(&ctx, &mut mfx, None, false);
        assert_eq!(window_title(&ctx), "Mortiflix Settings");
        let mut ids = Vec::new();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            let mut x = mfx.ctx(MfxTheme::default());
            show(ui, &mut x);
            ids.extend(x.auto.iter().map(|a| a.0.clone()));
        });
        out.textures_delta.clear();
        for id in ["mfx.setup.page.narration", "mfx.setup.done"] {
            assert!(ids.iter().any(|i| i == id), "{id} in {ids:?}");
        }
        for id in ["mfx.setup.next", "mfx.setup.skip", "mfx.setup.finish"] {
            assert!(!ids.iter().any(|i| i == id), "{id} in {ids:?}");
        }
        // the Narration page lists the engines once the service answers
        model(&ctx).lock().unwrap().page = Page::Narration;
        model(&ctx).lock().unwrap().voice = serde_json::json!({"engine": "qwen", "elevenlabs_key": null});
        assert_eq!(model(&ctx).lock().unwrap().page_state(Page::Narration), (true, "This computer".to_string()));
    }
}
