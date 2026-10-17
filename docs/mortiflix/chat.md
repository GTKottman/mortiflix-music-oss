# Part 02: the Mortiflix chat and the Assets tab

Docs 37 (Talking with the AI) and 32b (Assets tab). Code: `crates/mortiflix-ui/src/chat.rs` (the panel),
`chat/model.rs` (messages, graph lookups, rows, links: no drawing, tested headless), `chat/cards.rs` (inline cards),
`chat/assets.rs` (the Assets tab). Service messages are part 08's (`docs/mortiflix/protocol.md` on
`mortiflix/service-protocol`); the chat adds none of its own.

## What it does

- **One panel, one history.** Left on stages 1-4 (tab group with Assets), docked right in FilmCraft on stage 5, same
  thread. On connect it loads `chat.history` and `questions.list` once per project; new rows arrive as
  `chat.message` events.
- **Talking about.** Crumbs `The whole video › beat › node` follow `MfxState::selected_node` both ways: the node area
  (or a FilmCraft clip) sets it, a link / crumb / folded row / History entry in the chat sets it, and the chat tells the
  service (`chat.context`); a `chat.context` event from elsewhere moves the selection. ✕ or Esc = the whole video.
- **Thread.** Stage dividers (click = go to that stage), your messages as pills, Mortiflix's messages with links
  (refs, "shot N", "line N"), cards, one question at a time (accent rule, default ticked, "Going with X unless you
  say", Custom…), grey action lines; OK / Cut it presses in a row merge into one line; in the whole video a node's
  messages fold into one row, in a node's context only its own thread shows.
- **Cards** (part 08 shapes): script, looks (pick = OK on that look), shots / transitions (links), versions (from
  the graph; Use vN = `node.pick`), themes (Hear the music themes = `music.theme.samples`, Use it =
  `music.theme.use`), defaults / submission (each question's chips = `chat.answer`). Stills are not drawn yet (the
  frame shows the beat colour): the crate has no image loader.
- **Moves row.** Whole video: Play everything + the next stage's move (Hear the voice and music · Edit the video /
  Get the video · Go to Export). A node: OK · Fix · Cut it · Play it (line: New take; transition: Change it; cue: OK ·
  Fix). Fix focuses the composer.
- **Composer.** Enter sends, Shift+Enter new line, + attaches (OS dialog), an asset row dropped on it attaches the
  asset. Words go to `chat.send` with the context: on a node the service makes them its Fix and asks first on an OK'd
  piece ("Remake it" re-sends with `confirm: true`); about the whole video the user's Claude Code answers
  (`chat.typing` shows "Mortiflix is replying"). Attached files go to `assets.add`; an asset on a node is
  `assets.use`. While the service is down messages wait ("Sends when Mortiflix is back") and go when it is back.
- **Live line + Details.** The AI's line from `status`; Details: the six stages with counts, three to-do rows around
  the current step (`activity` todo events), the running render (`render.job`).
- **Assets tab.** `assets.list` (`yours` + `found`), filter (All / Not used yet / Must use), rows with what happened
  to each, copy progress (`assets.progress`), How to use it / Where (beat chips) / Note (`assets.update`), Used in +
  Show (selects the node), Replace… (`assets.replace`, then "Remake shots 1 and 2" = a Fix per shot), Show in folder,
  Remove from project (loud confirm when shots use it; they get a Fix to drop it).

## Shared pieces other parts use

- Play buttons in the chat (Play everything, Play it, a voice line's ▶) run part 04's
  `mortiflix.preview.play {scope: all | shot | beat, node?}`; a music theme sample opens its file in the system
  player (`mortiflix.reveal {open: true}`).
- `MfxState::files_in` + `push_files` / `take_files(purpose)`: files from the OS dialog or a desktop drop.
  `mortiflix.pickFiles {purpose, multiple}` opens the dialog (`paths` skips it, for agents and tests). On stages 1-4
  (or with New project open) files dropped on the window arrive with purpose `drop` instead of going to FilmCraft's
  bin: **part 01** takes `drop` while its window is open; otherwise the chat (node context: attach; whole video:
  Assets) or the Assets tab takes them.
- `chat::AssetDrag {id, name}`: the egui drag payload of an asset row. **Part 03 (nodes)**: on drop over a shot, send
  `assets.use {asset, node}`.
- `chat::model::set_local_offset(fn)`: `apps/filmcraft` gives the chat local time for message times.
- `mortiflix.reveal {path, open?}`: Show in folder / open a file.

## Layout fix (stages 1-4 show only Mortiflix panels)

Window ▸ Timeline (Shift+3) or a sequence opened on stages 1-4 put FilmCraft's Timeline ("Main Edit") into the chat's
tab group (`restore_timeline` falls back to the first group when the workspace has no Project panel).
`mortiflix::keep_mortiflix_only` closes any FilmCraft panel in the Mortiflix workspace (status line: "FilmCraft's own
panels open on stage 5 FilmCraft."), run when a stage 1-4 is applied and every frame; `frame_begin` also puts stages
1-4 back in the Mortiflix workspace if another one was picked.
