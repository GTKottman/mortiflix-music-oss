//! Stage 1 Script: the script by beat (simple).
//!
//! Owned by part script. Stub from the foundation: fill it in; keep `pub fn show(ui, ctx)`.

use crate::MfxCtx;

/// Stage 1's main panel (PanelKind::MfxStage on stage 1).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "script", "Script (stage 1)");
}
