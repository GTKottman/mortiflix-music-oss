//! Talking with the AI: one chat across all stages, the "talking about" bar, a button for every move
//! (doc 37), and the Assets tab beside it (doc 32b).
//!
//! Owned by part 37 (+ 32b assets tab). Stub from the foundation: fill it in; keep the two `show`s.
//! Send with `ctx.service.request("chat.send", json!({"text": ..}))`; replies arrive as `chat.message`
//! events, which the host already appends to `ctx.state.chat`.

use crate::MfxCtx;

/// The chat panel (PanelKind::MfxChat): left on stages 1-4, docked right in FilmCraft on stage 5.
pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::service_notice(ui, ctx);
    crate::placeholder(ui, ctx, "chat", "Chat with the AI (doc 37)");
}

/// The Assets tab beside the chat (PanelKind::MfxAssets, doc 32b).
pub fn show_assets(ui: &mut egui::Ui, ctx: &mut MfxCtx) {
    crate::placeholder(ui, ctx, "assets", "Your assets (doc 32b)");
}
