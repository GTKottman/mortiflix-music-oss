//! The Voice & music stage: hear it all before FilmCraft, per line / per cue (docs 34, 33b).
//!
//! Owned by part 34/33b. Stub from the foundation: fill it in; keep `pub fn show(ui, ctx)`.

use crate::MfxCtx;

/// Stage 4's main panel (PanelKind::MfxStage on stage 4).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "voice_music", "Voice & music (docs 34, 33b)");
}
