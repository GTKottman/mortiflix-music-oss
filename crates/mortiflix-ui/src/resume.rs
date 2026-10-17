//! When the AI stops (owner's first run: "Needs you: The last two sessions stopped… then resume",
//! and nothing to press): the chat and the status line get **Resume** (`project.resume`). When the
//! cause is a Claude Code login that expired or failed ("OAuth session expired"), it says so plainly
//! and offers **Link Claude Code** (setup step 1) instead, then resumes on its own once signed in.

use std::time::{Duration, Instant};

use egui::{Color32, RichText};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{MfxCtx, MfxState, Service};

/// How often the sign-in is checked while waiting to resume after Link Claude Code.
const CHECK_EVERY: Duration = Duration::from_secs(4);

/// The project's stop, as the `status` event says it (`state`, `needs_you`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stop {
    /// The service's words ("The last two sessions stopped without moving the project forward (…)").
    pub text: String,
    /// The cause is Claude Code's login (expired, signed out, refused).
    pub login: bool,
}

/// Resume after Link Claude Code: waiting for the sign-in, and the check in flight.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResumeState {
    pub after_login: bool,
    pub last_check: Option<Instant>,
    pub inflight: Option<u64>,
}

/// A login problem in the AI's own words ("OAuth session expired", "401", "Please run /login").
pub fn is_login(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    [
        "oauth",
        "authenticat",
        "401",
        "/login",
        "log in",
        "login",
        "logged out",
        "signed out",
        "sign in",
        "not logged",
        "credential",
        "unauthorized",
        "token expired",
        "session expired",
    ]
    .iter()
    .any(|w| t.contains(w))
}

/// Read the stop from a `status` event's data: a project paused by the AI with `needs_you` (not one
/// you paused yourself).
pub fn stop_of(data: &Value) -> Option<Stop> {
    if data.get("state").and_then(Value::as_str) != Some("paused") {
        return None;
    }
    let n = data.get("needs_you").filter(|n| n.is_object())?;
    if n.get("by_you").and_then(Value::as_bool).unwrap_or(false) {
        return None;
    }
    let text: String = n.get("text").and_then(Value::as_str).unwrap_or("").chars().take(600).collect();
    let login = n.get("cause").and_then(Value::as_str) == Some("login") || is_login(&text);
    Some(Stop { text, login })
}

/// What the bar says.
pub fn words(stop: &Stop) -> String {
    if stop.login {
        "The AI stopped: Claude Code is signed out (its login expired). Link Claude Code again and it carries on by itself.".into()
    } else if stop.text.is_empty() {
        "The AI stopped. Press Resume when you're ready.".into()
    } else {
        format!("The AI stopped: {}", stop.text.trim_end_matches("Check the activity log, then resume.").trim())
    }
}

/// Send Resume.
pub fn resume(state: &mut MfxState, service: &mut Service) {
    let params = state.project_id.as_ref().map_or_else(|| json!({}), |p| json!({"project": p}));
    service.request("project.resume", params);
    state.stop = None;
    state.resume.after_login = false;
}

/// Each frame: after Link Claude Code, check the sign-in every few seconds and resume once it's in.
pub fn tick(state: &mut MfxState, service: &mut Service) {
    if let Some(id) = state.resume.inflight
        && let Some(r) = service.take_reply(id)
    {
        state.resume.inflight = None;
        let signed_in = r.ok && r.result.pointer("/claude/logged_in").and_then(Value::as_bool) == Some(true);
        if signed_in && state.resume.after_login {
            resume(state, service);
        }
    }
    if !state.resume.after_login || state.resume.inflight.is_some() || !service.status().is_connected() {
        return;
    }
    if state.stop.is_none() {
        state.resume.after_login = false;
        return;
    }
    if state.resume.last_check.is_none_or(|t| t.elapsed() >= CHECK_EVERY) {
        state.resume.last_check = Some(Instant::now());
        state.resume.inflight = Some(service.request("setup.status", json!({})));
    }
}

/// The stop bar: what happened and the one button (Resume, or Link Claude Code). `id` keeps the
/// automation ids of the chat's and the stage bar's apart. Returns true when it drew.
pub fn bar(ui: &mut egui::Ui, ctx: &mut MfxCtx, id: &str, compact: bool) -> bool {
    let Some(stop) = ctx.state.stop.clone() else { return false };
    let t = ctx.theme;
    let label = if stop.login { "Link Claude Code" } else { "Resume" };
    let draw = |ui: &mut egui::Ui, ctx: &mut MfxCtx| {
        if !compact {
            // two lines at most (the whole story on hover and in the thread): a long stop ran over the chat's
            // Moves row and hid Resume under it (run 2)
            let mut job = egui::text::LayoutJob::simple(format!("! {}", words(&stop)), egui::FontId::proportional(12.0), t.warn, ui.available_width());
            job.wrap.max_rows = 2;
            job.wrap.overflow_character = Some('…');
            ui.add(egui::Label::new(job)).on_hover_text(words(&stop));
            ui.end_row();
        }
        let waiting = ctx.state.resume.after_login;
        let text = if waiting { "Waiting for Claude Code…".to_string() } else { label.to_string() };
        let b = ui.add(egui::Button::new(RichText::new(&text).size(12.0).color(Color32::WHITE)).fill(t.accent).corner_radius(4.0)).on_hover_text(words(&stop));
        ctx.auto(&format!("mfx.{id}.resume"), &b, label);
        if b.clicked() {
            if stop.login {
                ctx.state.resume.after_login = true;
                ctx.state.resume.last_check = None;
                ctx.command("mortiflix.setup", json!({"step": "claude"}));
            } else {
                resume(ctx.state, ctx.service);
            }
        }
        if stop.login && !waiting {
            // signed in some other way: Resume works too
            let r = ui.add(egui::Button::new(RichText::new("Resume").size(12.0)).fill(t.hover).corner_radius(4.0));
            ctx.auto(&format!("mfx.{id}.resumeAnyway"), &r, "Resume");
            if r.clicked() {
                resume(ctx.state, ctx.service);
            }
        }
    };
    if compact {
        draw(ui, ctx);
    } else {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| draw(ui, ctx));
        });
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_a_stop_and_tells_a_login_from_the_rest() {
        let s = stop_of(&json!({"state": "paused", "needs_you": {"text": "The last two sessions stopped without moving the project forward (OAuth session expired). Check the activity log, then resume."}})).unwrap();
        assert!(s.login);
        assert!(words(&s).contains("Claude Code is signed out"));
        let s = stop_of(&json!({"state": "paused", "needs_you": {"text": "5 sessions in a row ended without submitting"}})).unwrap();
        assert!(!s.login);
        assert_eq!(words(&s), "The AI stopped: 5 sessions in a row ended without submitting");
        assert!(stop_of(&json!({"state": "paused", "needs_you": {"text": "x", "by_you": true}})).is_none());
        assert!(stop_of(&json!({"state": "queued", "needs_you": null})).is_none());
        assert!(stop_of(&json!({"state": "paused", "needs_you": {"text": "x", "cause": "login"}})).is_some_and(|s| s.login));
    }

    #[test]
    fn resume_asks_the_service_and_clears_the_stop() {
        let mut state = MfxState::default();
        let mut service = Service::default();
        state.stop = Some(Stop { text: "x".into(), login: false });
        resume(&mut state, &mut service);
        assert!(state.stop.is_none());
        // not running: answered at once with not_running, never left waiting
        assert!(service.take_reply(1).is_some_and(|r| !r.ok));
    }
}
