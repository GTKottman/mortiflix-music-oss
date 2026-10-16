//! New project window and preset creator (docs 31, 32a attach assets, 33a music theme).
//!
//! Owned by part 31/32a/33a. Stub from the foundation: fill it in; keep `pub fn show(ui, ctx)`.

use crate::MfxCtx;

/// The New project window's content (drawn inside the window the host opens with `mortiflix.newProject`).
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "new_project", "New project: Name, Preset, The idea, Assets, Music (docs 31, 32a, 33a)");
}
