//! The node area: free placement like Blender, noodles, OK / Fix / Cut it, inspector, minimap (doc 35).
//!
//! Owned by part 35. Stub from the foundation: fill it in; keep `pub fn show(ui, ctx)`.

use crate::MfxCtx;

/// The node area panel (PanelKind::MfxNodes), along the bottom on stages 1-4.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "nodes", "Node area (doc 35)");
}

/// Stage 3 Shots' main panel (PanelKind::MfxStage on stage 3): the shots themselves are in the node
/// area; this slot can show the selected shot or the "need you" list.
pub fn show_stage(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "shots", "Shots: OK, Fix or Cut each shot in the node area (doc 35)");
}
