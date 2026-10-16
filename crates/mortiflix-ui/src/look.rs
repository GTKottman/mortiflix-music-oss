//! Stage 2 Look: the looks to pick from (simple).
//!
//! Owned by part look. Stub from the foundation: fill it in; keep `pub fn show(ui, ctx)`.

use crate::MfxCtx;

/// Stage 2's main panel (PanelKind::MfxStage on stage 2).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "look", "Look (stage 2)");
}
