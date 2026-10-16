//! First-run setup: link Claude Code, optional API keys, where the Mortiflix service is.
//!
//! Owned by part setup. Stub from the foundation: fill it in; keep `pub fn show(ui, ctx)`.

use crate::MfxCtx;

/// The Setup window's content (opened with `mortiflix.setup`, and from every "service not running" line). Save the service path with `service::ServiceSettings` and restart with `ctx.service.start(..)`.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "setup", "Setup: link Claude Code, keys, Mortiflix service");
}
