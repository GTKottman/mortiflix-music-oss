//! The Mortiflix service supervisor: runs `node <path>/bin/mortiflix-service.mjs` as a child process
//! and talks JSON lines over its stdin/stdout (`docs/mortiflix/protocol.md`). No port.
//!
//! The UI thread never blocks: a background thread owns the child, writes requests, restarts it with
//! backoff when it dies, and answers requests itself ("not running") while it is down. A missing Node
//! or service is a calm state ([`ServiceStatus`]) with a Setup link, never an error dialog or crash.
//!
//! Where the service is: `MFX_SERVICE` (the `.mjs` file, or the mortiflix-oss folder), else
//! `servicePath` in `<data dir>/mortiflix.json` (written by first-run setup), else the copy a packaged
//! install ships ([`bundled_dir`]). Node: `MFX_NODE`, else `nodePath` there, else the packaged Node,
//! else `node` on the PATH.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Protocol version both sides speak (`hello`).
pub const PROTOCOL: u32 = 1;
/// The request id the supervisor uses for its own `hello` after each start.
pub const HELLO_ID: u64 = 0;
/// The settings file beside Mortiflix Studio's `preferences.json`.
pub const SETTINGS_FILE: &str = "mortiflix.json";
/// The service script inside the mortiflix-oss folder.
pub const SERVICE_SCRIPT: &str = "bin/mortiflix-service.mjs";
/// Longest line read from the service; longer lines are skipped (hostile input).
pub const MAX_LINE: usize = 16 * 1024 * 1024;
const MAX_REPLIES: usize = 256;
const MAX_EVENTS: usize = 512;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum ServiceStatus {
    /// Not started (tests, Mortiflix Studio alone).
    #[default]
    Off,
    /// No service path set: first-run setup is needed.
    NotConfigured {
        reason: String,
    },
    Starting,
    Connected {
        version: String,
    },
    /// Died or could not start; the supervisor tries again.
    Stopped {
        reason: String,
    },
    /// No child processes here (the web build).
    Unsupported,
}

impl ServiceStatus {
    pub fn is_connected(&self) -> bool {
        matches!(self, ServiceStatus::Connected { .. })
    }
    /// One calm line for the UI.
    pub fn line(&self) -> String {
        let line: String = match self {
            ServiceStatus::Off => "Mortiflix service not running.".into(),
            ServiceStatus::NotConfigured { .. } => "Mortiflix service not running: it isn't set up yet.".into(),
            ServiceStatus::Starting => "Starting the Mortiflix service…".into(),
            ServiceStatus::Connected { version } => format!("Mortiflix service {version} connected."),
            ServiceStatus::Stopped { reason } => format!("Mortiflix service not running ({reason}). Trying again."),
            ServiceStatus::Unsupported => "Mortiflix service not available here.".into(),
        };
        crate::branded(&line).into_owned()
    }
}

/// `<data dir>/mortiflix.json`: where first-run setup put things.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServiceSettings {
    /// The service `.mjs`, or the mortiflix-oss folder.
    pub service_path: String,
    /// The Node binary; empty = `node` on the PATH.
    pub node_path: String,
}

impl ServiceSettings {
    pub fn path(data_dir: &Path) -> PathBuf {
        data_dir.join(SETTINGS_FILE)
    }
    /// Read the settings; missing, too big or broken = defaults.
    pub fn load(data_dir: &Path) -> Self {
        use std::io::Read as _;
        const MAX_BYTES: u64 = 256 * 1024;
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

/// The service folder, the Node runtime and the tools (`bin/`: ffmpeg, ffprobe, mortiflix-engine) inside
/// a packaged install's bundle (see `packaging/mortiflix/README.md`).
pub const BUNDLE_SERVICE: &str = "service";
pub const BUNDLE_NODE: &str = if cfg!(windows) { "node/node.exe" } else { "node/bin/node" };
pub const BUNDLE_TOOLS: &str = "bin";

/// Where a packaged install keeps the Mortiflix service with its own Node, ffmpeg and sound engine:
/// `MFX_BUNDLE`, else beside the binary: `mortiflix/` (Windows), `../lib/mortiflix-studio/` (Linux,
/// AppImage), `../Resources/mortiflix/` (macOS app). None = no bundle (a development build).
pub fn bundled_dir() -> Option<PathBuf> {
    let has_service = |d: &Path| d.join(BUNDLE_SERVICE).join(SERVICE_SCRIPT).is_file();
    if let Some(d) = std::env::var_os("MFX_BUNDLE").filter(|v| !v.is_empty()).map(PathBuf::from) {
        return has_service(&d).then_some(d);
    }
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let up = dir.parent().unwrap_or(dir);
    [dir.join("mortiflix"), up.join("lib").join("mortiflix-studio"), up.join("Resources").join("mortiflix")].into_iter().find(|d| has_service(d))
}

/// How to start the service.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServiceConfig {
    pub node: PathBuf,
    /// The service script; None = not set up.
    pub script: Option<PathBuf>,
}

impl ServiceConfig {
    /// From the environment, then the settings file in `data_dir`.
    pub fn resolve(data_dir: Option<&Path>) -> Self {
        let settings = data_dir.map(ServiceSettings::load).unwrap_or_default();
        let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        let bundle = bundled_dir();
        let bundled_node = bundle.as_ref().map(|b| b.join(BUNDLE_NODE)).filter(|n| n.is_file());
        let node = env("MFX_NODE")
            .or_else(|| (!settings.node_path.is_empty()).then(|| PathBuf::from(&settings.node_path)))
            .or(bundled_node)
            .unwrap_or_else(|| PathBuf::from("node"));
        let script = env("MFX_SERVICE")
            .or_else(|| (!settings.service_path.is_empty()).then(|| PathBuf::from(&settings.service_path)))
            .or_else(|| bundle.map(|b| b.join(BUNDLE_SERVICE)))
            .map(|p| script_in(&p));
        Self { node, script }
    }
}

/// A folder means the mortiflix-oss checkout: its `bin/mortiflix-service.mjs`.
fn script_in(p: &Path) -> PathBuf {
    if p.is_dir() { p.join(SERVICE_SCRIPT) } else { p.to_path_buf() }
}

/// A reply to one request.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply {
    pub id: u64,
    pub ok: bool,
    pub result: Value,
    /// `{code, message}` when not ok.
    pub error: Value,
}

#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
enum Incoming {
    Status(ServiceStatus),
    Line(Value),
}

/// The supervisor's side of the channel: every message also wakes the UI (a repaint), so replies
/// show at once without the UI polling on a timer.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
struct Out {
    tx: Sender<Incoming>,
    waker: Arc<Mutex<Option<egui::Context>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl Out {
    fn send(&self, m: Incoming) -> Result<(), ()> {
        self.tx.send(m).map_err(|_| ())?;
        if let Some(ctx) = self.waker.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            ctx.request_repaint();
        }
        Ok(())
    }
}

/// The UI side of the connection. Cheap to poll every frame.
#[derive(Default)]
pub struct Service {
    status: ServiceStatus,
    tx: Option<Sender<Value>>,
    rx: Option<Receiver<Incoming>>,
    stop: Option<Arc<AtomicBool>>,
    next_id: u64,
    replies: VecDeque<Reply>,
    events: VecDeque<(u64, Value)>,
    event_seq: u64,
    waker: Arc<Mutex<Option<egui::Context>>>,
    /// How many times a `hello` was answered (each start of the service).
    connects: u64,
}

impl Drop for Service {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl Service {
    pub fn status(&self) -> &ServiceStatus {
        &self.status
    }

    /// How many times the service has connected (it is restarted after a crash).
    pub fn connects(&self) -> u64 {
        self.connects
    }

    /// Repaint this context whenever something arrives from the service.
    pub fn set_waker(&mut self, ctx: &egui::Context) {
        let mut w = self.waker.lock().unwrap_or_else(PoisonError::into_inner);
        if w.is_none() {
            *w = Some(ctx.clone());
        }
    }

    /// Start (or restart with a new config) the supervisor thread.
    pub fn start(&mut self, config: ServiceConfig) {
        self.shutdown();
        #[cfg(target_arch = "wasm32")]
        {
            let _ = config;
            self.status = ServiceStatus::Unsupported;
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            let (tx, cmd_rx) = std::sync::mpsc::channel::<Value>();
            let (out_tx, rx) = std::sync::mpsc::channel::<Incoming>();
            let out_tx = Out { tx: out_tx, waker: Arc::clone(&self.waker) };
            let stop = Arc::new(AtomicBool::new(false));
            let stop2 = Arc::clone(&stop);
            let spawned = std::thread::Builder::new().name("mortiflix-service".into()).spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| supervise(&config, &cmd_rx, &out_tx, &stop2)));
                if r.is_err() {
                    let _ = out_tx.send(Incoming::Status(ServiceStatus::Stopped { reason: "the supervisor failed".into() }));
                }
            });
            match spawned {
                Ok(_) => {
                    self.status = ServiceStatus::Starting;
                    self.tx = Some(tx);
                    self.rx = Some(rx);
                    self.stop = Some(stop);
                }
                Err(e) => self.status = ServiceStatus::Stopped { reason: format!("no thread: {e}") },
            }
        }
    }

    /// Stop the supervisor; it kills the child (which also exits when its stdin closes).
    pub fn shutdown(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        self.tx = None;
        self.rx = None;
        self.status = ServiceStatus::Off;
    }

    /// Send a request; returns its id. When the service is not running the reply is an error
    /// (`not_running`), never a request that waits forever.
    pub fn request(&mut self, ty: &str, params: Value) -> u64 {
        self.next_id = self.next_id.saturating_add(1);
        let id = self.next_id;
        let msg = json!({"id": id, "type": ty, "params": params});
        let sent = self.tx.as_ref().is_some_and(|tx| tx.send(msg).is_ok());
        if !sent {
            self.push_reply(Reply { id, ok: false, result: Value::Null, error: json!({"code": "not_running", "message": self.status.line()}) });
        }
        id
    }

    /// The reply to request `id`, once it arrived (taken: a second call returns None).
    pub fn take_reply(&mut self, id: u64) -> Option<Reply> {
        let at = self.replies.iter().position(|r| r.id == id)?;
        self.replies.remove(at)
    }

    /// Events received so far with a sequence number above `seq` (parts remember the last one they
    /// read). The newest [`MAX_EVENTS`] are kept.
    pub fn events_since(&self, seq: u64) -> impl Iterator<Item = &(u64, Value)> {
        self.events.iter().filter(move |(s, _)| *s > seq)
    }

    /// Drain what arrived; returns the events (`{event, data}`) of this poll.
    pub fn poll(&mut self) -> Vec<Value> {
        let mut new_events = Vec::new();
        let mut incoming = Vec::new();
        if let Some(rx) = &self.rx {
            while let Ok(m) = rx.try_recv() {
                incoming.push(m);
                if incoming.len() >= 1000 {
                    break;
                }
            }
        }
        for m in incoming {
            match m {
                Incoming::Status(s) => self.status = s,
                Incoming::Line(v) => {
                    if v.get("event").and_then(Value::as_str).is_some() {
                        self.event_seq = self.event_seq.saturating_add(1);
                        self.events.push_back((self.event_seq, v.clone()));
                        while self.events.len() > MAX_EVENTS {
                            self.events.pop_front();
                        }
                        new_events.push(v);
                    } else if let Some(id) = v.get("id").and_then(Value::as_u64) {
                        let ok = v.get("ok").and_then(Value::as_bool).unwrap_or(false);
                        let result = v.get("result").cloned().unwrap_or(Value::Null);
                        let error = v.get("error").cloned().unwrap_or(Value::Null);
                        if id == HELLO_ID {
                            self.status = if ok {
                                let version = result.get("version").and_then(Value::as_str).unwrap_or("?").chars().take(40).collect();
                                self.connects = self.connects.saturating_add(1);
                                ServiceStatus::Connected { version }
                            } else {
                                ServiceStatus::Stopped { reason: error_text(&error) }
                            };
                        } else {
                            self.push_reply(Reply { id, ok, result, error });
                        }
                    }
                }
            }
        }
        new_events
    }

    fn push_reply(&mut self, r: Reply) {
        self.replies.push_back(r);
        while self.replies.len() > MAX_REPLIES {
            self.replies.pop_front();
        }
    }
}

/// A connected service for other modules' tests: what the app sends comes out of the receiver,
/// [`Service::answer`] puts a reply in.
#[cfg(test)]
impl Service {
    pub(crate) fn fake() -> (Service, Receiver<Value>) {
        let (tx, rx) = std::sync::mpsc::channel();
        let mut s = Service::default();
        s.tx = Some(tx);
        s.status = ServiceStatus::Connected { version: "test".into() };
        (s, rx)
    }
    pub(crate) fn answer(&mut self, id: u64, ok: bool, result: Value) {
        self.push_reply(Reply { id, ok, result, error: if ok { Value::Null } else { json!({"code": "failed", "message": "no"}) } });
    }
}

fn error_text(error: &Value) -> String {
    let s = error.get("message").and_then(Value::as_str).or_else(|| error.as_str()).unwrap_or("error");
    s.chars().take(200).collect()
}

#[cfg(not(target_arch = "wasm32"))]
fn supervise(config: &ServiceConfig, cmd_rx: &Receiver<Value>, out: &Out, stop: &AtomicBool) {
    use std::time::{Duration, Instant};
    let status = |s: ServiceStatus| {
        let _ = out.send(Incoming::Status(s));
    };
    // While down: answer requests with not_running until `until` (or stop).
    let wait_down = |until: Instant, why: &str| -> bool {
        while Instant::now() < until {
            if stop.load(Ordering::Relaxed) {
                return false;
            }
            match cmd_rx.recv_timeout(Duration::from_millis(100)) {
                Ok(req) => answer_down(out, &req, why),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return false,
            }
        }
        true
    };
    let Some(script) = config.script.clone() else {
        let why = "set the Mortiflix service up in Setup".to_string();
        status(ServiceStatus::NotConfigured { reason: why.clone() });
        while wait_down(Instant::now() + Duration::from_secs(3600), &why) {}
        return;
    };
    let mut backoff = Duration::from_secs(1);
    loop {
        if stop.load(Ordering::Relaxed) {
            return;
        }
        status(ServiceStatus::Starting);
        let started = Instant::now();
        let why = match run_child(config, &script, cmd_rx, out, stop) {
            Ok(Exit::Stopped) => return,
            Ok(Exit::Died(why)) | Err(why) => why,
        };
        if started.elapsed() > Duration::from_secs(60) {
            backoff = Duration::from_secs(1);
        }
        log::warn!("mortiflix service: {why}; restarting in {}s", backoff.as_secs());
        status(ServiceStatus::Stopped { reason: why.clone() });
        if !wait_down(Instant::now() + backoff, &why) {
            return;
        }
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

#[cfg(not(target_arch = "wasm32"))]
enum Exit {
    /// The app stopped the service.
    Stopped,
    Died(String),
}

#[cfg(not(target_arch = "wasm32"))]
fn answer_down(out: &Out, req: &Value, why: &str) {
    if let Some(id) = req.get("id").and_then(Value::as_u64) {
        let _ = out.send(Incoming::Line(
            json!({"id": id, "ok": false, "error": {"code": "not_running", "message": format!("Mortiflix service not running ({why})")}}),
        ));
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run_child(config: &ServiceConfig, script: &Path, cmd_rx: &Receiver<Value>, out: &Out, stop: &AtomicBool) -> Result<Exit, String> {
    use std::io::Write as _;
    use std::process::{Command, Stdio};
    use std::time::Duration;
    if !script.is_file() {
        return Err(format!("no service at {}", script.display()));
    }
    let mut cmd = Command::new(&config.node);
    cmd.arg(script).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(dir) = script.parent() {
        cmd.current_dir(dir);
    }
    if let Some(bundle) = bundled_dir() {
        bundle_env(&mut cmd, &bundle);
    }
    // the app studio's service: its own stages, pipelines and studio folder (service src/product.mjs)
    if crate::apps() {
        cmd.env("MFX_PRODUCT", "apps");
    }
    // the MIDI studio's the same way (owner, 2026-10-23)
    if crate::midi() {
        cmd.env("MFX_PRODUCT", "midi");
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().map_err(|e| format!("could not start {}: {e}", config.node.display()))?;
    let (Some(mut stdin), Some(stdout), Some(stderr)) = (child.stdin.take(), child.stdout.take(), child.stderr.take()) else {
        let _ = child.kill();
        let _ = child.wait();
        return Err("no pipes to the service".into());
    };
    let reader_out = out.clone();
    let _ = std::thread::Builder::new().name("mortiflix-service-out".into()).spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_lines(stdout, |line| match serde_json::from_slice::<Value>(line) {
                Ok(v) => {
                    let _ = reader_out.send(Incoming::Line(v));
                }
                Err(e) => log::warn!("mortiflix service: not JSON ({e}): {}", String::from_utf8_lossy(line.get(..200).unwrap_or(line))),
            })
        }));
    });
    let _ = std::thread::Builder::new().name("mortiflix-service-err".into()).spawn(move || {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            read_lines(stderr, |line| log::info!("mortiflix service: {}", String::from_utf8_lossy(line.get(..500).unwrap_or(line))))
        }));
    });
    let hello = json!({"id": HELLO_ID, "type": "hello", "params": {"client": "mortiflix-studio", "version": env!("CARGO_PKG_VERSION"), "protocol": PROTOCOL}});
    let write = |stdin: &mut std::process::ChildStdin, v: &Value| -> std::io::Result<()> {
        let mut bytes = serde_json::to_vec(v).map_err(std::io::Error::other)?;
        bytes.push(b'\n');
        stdin.write_all(&bytes)?;
        stdin.flush()
    };
    let mut result = write(&mut stdin, &hello).map_err(|e| format!("could not talk to it: {e}")).map(|()| Exit::Stopped);
    while result.is_ok() {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        match child.try_wait() {
            Ok(Some(st)) => {
                result = Ok(Exit::Died(format!("it stopped ({st})")));
                break;
            }
            Ok(None) => {}
            Err(e) => {
                result = Err(format!("lost it: {e}"));
                break;
            }
        }
        match cmd_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(req) => {
                if let Err(e) = write(&mut stdin, &req) {
                    answer_down(out, &req, "it stopped reading");
                    result = Ok(Exit::Died(format!("it stopped reading: {e}")));
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stdin);
    if matches!(result, Ok(Exit::Stopped)) || child.try_wait().ok().flatten().is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    result
}

/// Call `f` with each line (without the newline). Lines longer than [`MAX_LINE`] are skipped.
#[cfg(not(target_arch = "wasm32"))]
fn read_lines(r: impl std::io::Read, mut f: impl FnMut(&[u8])) {
    use std::io::{BufRead as _, Read as _};
    let mut r = std::io::BufReader::new(r);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        let n = match (&mut r).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        if buf.last() != Some(&b'\n') && n > MAX_LINE {
            // too long: skip to the end of this line
            loop {
                buf.clear();
                match (&mut r).take(64 * 1024).read_until(b'\n', &mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(_) if buf.last() == Some(&b'\n') => break,
                    Ok(_) => {}
                }
            }
            log::warn!("mortiflix service: skipped a line over {MAX_LINE} bytes");
            continue;
        }
        let line = buf.strip_suffix(b"\n").unwrap_or(&buf);
        let line = line.strip_suffix(b"\r").unwrap_or(line);
        if !line.iter().all(u8::is_ascii_whitespace) {
            f(line);
        }
    }
}

/// A packaged install's tools come first on the service's PATH (ffmpeg, ffprobe, node, npx), and the
/// sound engine is named for it (`MFX_DAW_ENGINE`) unless the user set one.
#[cfg(not(target_arch = "wasm32"))]
fn bundle_env(cmd: &mut std::process::Command, bundle: &Path) {
    let node_dir = bundle.join(BUNDLE_NODE).parent().map(Path::to_path_buf);
    let mut dirs: Vec<PathBuf> = [Some(bundle.join(BUNDLE_TOOLS)), node_dir].into_iter().flatten().filter(|d| d.is_dir()).collect();
    if let Some(path) = std::env::var_os("PATH") {
        dirs.extend(std::env::split_paths(&path));
    }
    if let Ok(path) = std::env::join_paths(dirs) {
        cmd.env("PATH", path);
    }
    cmd.env("MFX_BUNDLE", bundle);
    let engine = bundle.join(BUNDLE_TOOLS).join(if cfg!(windows) { "mortiflix-engine.exe" } else { "mortiflix-engine" });
    if engine.is_file() && std::env::var_os("MFX_DAW_ENGINE").is_none() {
        cmd.env("MFX_DAW_ENGINE", engine);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_without_a_service_get_a_not_running_reply() {
        let mut s = Service::default();
        let id = s.request("state.get", json!({}));
        let r = s.take_reply(id).unwrap();
        assert!(!r.ok);
        assert_eq!(r.error["code"], "not_running");
        assert!(s.take_reply(id).is_none());
    }

    #[test]
    fn a_missing_service_is_a_calm_status_and_requests_are_answered() {
        let mut s = Service::default();
        s.start(ServiceConfig { node: PathBuf::from("node"), script: Some(PathBuf::from("/nonexistent/mortiflix-service.mjs")) });
        let id = s.request("hello", json!({}));
        let t = std::time::Instant::now();
        let mut reply = None;
        while reply.is_none() && t.elapsed() < std::time::Duration::from_secs(5) {
            s.poll();
            reply = s.take_reply(id);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(matches!(s.status(), ServiceStatus::Stopped { .. }), "{:?}", s.status());
        assert!(!reply.unwrap().ok);
        s.shutdown();
        assert_eq!(s.status(), &ServiceStatus::Off);
    }

    #[test]
    fn not_configured_without_a_path() {
        let mut s = Service::default();
        s.start(ServiceConfig { node: PathBuf::from("node"), script: None });
        let t = std::time::Instant::now();
        while !matches!(s.status(), ServiceStatus::NotConfigured { .. }) && t.elapsed() < std::time::Duration::from_secs(5) {
            s.poll();
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(matches!(s.status(), ServiceStatus::NotConfigured { .. }));
    }

    #[test]
    fn lines_are_split_and_huge_lines_skipped() {
        let mut data = b"{\"a\":1}\r\n\n  \n".to_vec();
        data.extend(std::iter::repeat_n(b'x', MAX_LINE + 10));
        data.extend(b"\n{\"b\":2}");
        let mut got = Vec::new();
        read_lines(&data[..], |l| got.push(String::from_utf8_lossy(l).into_owned()));
        assert_eq!(got, ["{\"a\":1}", "{\"b\":2}"]);
    }

    #[test]
    fn replies_and_events_are_routed() {
        let (out, rx) = std::sync::mpsc::channel();
        let mut s = Service::default();
        s.rx = Some(rx);
        out.send(Incoming::Line(json!({"id": 0, "ok": true, "result": {"version": "0.1.0"}}))).unwrap();
        out.send(Incoming::Line(json!({"id": 5, "ok": true, "result": {"x": 1}}))).unwrap();
        out.send(Incoming::Line(json!({"event": "status", "data": {"line": "hi"}}))).unwrap();
        out.send(Incoming::Line(json!({"id": "bad"}))).unwrap();
        let ev = s.poll();
        assert_eq!(ev.len(), 1);
        assert!(s.status().is_connected());
        assert_eq!(s.take_reply(5).unwrap().result["x"], 1);
        assert_eq!(s.events_since(0).count(), 1);
        assert_eq!(s.events_since(1).count(), 0);
    }

    /// A service restarted after a crash has no project open: the app opens its project again.
    #[test]
    fn a_restarted_service_gets_the_project_opened_again() {
        let (out, rx) = std::sync::mpsc::channel();
        let (sent_tx, sent) = std::sync::mpsc::channel();
        let mut m = crate::Mortiflix::default();
        m.service.rx = Some(rx);
        m.service.tx = Some(sent_tx);
        out.send(Incoming::Line(json!({"id": HELLO_ID, "ok": true, "result": {"version": "0.1.0"}}))).unwrap();
        m.poll();
        assert_eq!(m.service.connects(), 1);
        assert!(sent.try_recv().is_err(), "the first start opens nothing");
        m.state.project_id = Some("p-one".into());
        out.send(Incoming::Line(json!({"id": HELLO_ID, "ok": true, "result": {"version": "0.1.0"}}))).unwrap();
        m.poll();
        // (the Script panel's `script.get` goes too: only `project.open` matters here)
        let opens = |sent: &std::sync::mpsc::Receiver<Value>| sent.try_iter().filter(|m| m["type"] == "project.open").collect::<Vec<_>>();
        let open = opens(&sent);
        assert_eq!(open.len(), 1);
        assert_eq!(open[0]["params"]["id"].as_str(), Some("p-one"));
        m.poll();
        assert!(opens(&sent).is_empty(), "only once per restart");
    }

    #[test]
    fn settings_round_trip_and_resolve() {
        let dir = std::env::temp_dir().join(format!("mfx-settings-{}", std::process::id()));
        let st = ServiceSettings { service_path: dir.display().to_string(), node_path: String::new() };
        st.save(&dir).unwrap();
        assert_eq!(ServiceSettings::load(&dir), st);
        if std::env::var_os("MFX_SERVICE").is_none() {
            let cfg = ServiceConfig::resolve(Some(&dir));
            assert_eq!(cfg.script, Some(dir.join(SERVICE_SCRIPT)));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_bundle_tools_come_first_on_the_service_path() {
        let dir = std::env::temp_dir().join(format!("mfx-bundle-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(BUNDLE_TOOLS)).unwrap();
        let mut cmd = std::process::Command::new("node");
        bundle_env(&mut cmd, &dir);
        let env = |k: &str| cmd.get_envs().find(|(n, _)| *n == k).and_then(|(_, v)| v.map(PathBuf::from));
        assert_eq!(env("MFX_BUNDLE"), Some(dir.clone()));
        let path = env("PATH").unwrap();
        assert_eq!(std::env::split_paths(&path).next(), Some(dir.join(BUNDLE_TOOLS)));
        std::fs::remove_dir_all(&dir).ok();
    }
}
