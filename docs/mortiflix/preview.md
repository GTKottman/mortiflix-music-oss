# Part 04: the loose preview (doc 36), as built

The **Mortiflix Preview** panel plays everything made so far as one video, in film order, at the
planned timing, on stages 1-4. Nothing is rendered: it is FilmCraft's own Program monitor playing a
sequence called **Loose preview** that the app keeps in step with the graph.

## Files

| File | What |
|---|---|
| `crates/mortiflix-ui/src/preview/plan.rs` | The edit list: `Plan::from_service(preview.plan result, graph.get result)`. The service picks the best media per shot; this adds beat names, each shot's look still (fallback), shots being remade, and checks the files exist. Pure data, hostile-input safe. |
| `crates/mortiflix-ui/src/preview.rs` | `PreviewState` (in `MfxState::preview`), `poll` (asks `preview.plan {scope: all}` + `graph.get` on project change, ~0.3 s after a `node.changed` / `status` / `project.changed` event, and every 10 s), `layout`, and the panel: header (Plays at, Fit), caption, Beats / Shots / Voice / Music bar, transport (⏮ Play ⏭, timecode, All / From here / This shot / This beat, Loop, legend). |
| `crates/ui-egui/src/mortiflix_preview.rs` | FilmCraft side: builds the sequence from the plan, draws the Program monitor's picture in the panel, Play's scope and Loop, follows the node selection, fills `PreviewState::player`, and the `mortiflix.preview.*` commands. |

Shared edits (one line or one hunk each): `state.rs` (`preview` field), `ui-egui/src/lib.rs` (mod),
`mortiflix.rs` (Host field, frame hook, picture call, command route), `menus.rs` (commands chained),
`panels/monitor.rs` (`show_picture`: the Program picture without its controls),
`engine/src/shortcut_presets.rs` (Ctrl+L / ⌘L = Loop in the Mortiflix Preview panel),
`i18n/es.tsv`, `i18n/ja.tsv`.

## How it works

1. `preview::poll` keeps `state.preview.plan` fresh from the service's `preview.plan` and
   `graph.get` (part 08's protocol; no message of its own). Media paths must be absolute without
   `..`; anything else is refused.
2. The service gives each shot the best media it has (its piece of the animatic, else its look's
   still, else a beat card) and its start and length. A card is a colour matte in the beat's colour
   dimmed to 30 % on #0e0e0e. A file that is gone falls back to the look's still (caption:
   "⚠ Shot 5's motion won't play. Showing its still frame." with **Remake shot 5** and
   **Show the file**).
4. When the plan's signature changes (stages 1-4 only), the host clears the sequence and places
   every clip again with ordinary commands (`file.import` into the bin "Loose preview media",
   `file.newColorMatte`, `timeline.place`, `edit.clear`), folded into one undo step
   "Mortiflix: Loose preview". V1 = pictures, A1 = voice lines, A2 = music. Imports are cached, so
   a rebuild after a new version re-imports only the new file.
5. On stages 1-4 the Loose preview sequence is the active sequence. From stage 5 on, FilmCraft's
   own sequence takes the monitor; the Loose preview stays in the project bin.

## Play, scope, Loop

- **Play** plays the scope: **All** (from the start), **From here** (default, playhead to end),
  **This shot** / **This beat** (the selected node's, else the one under the playhead). Space
  (FilmCraft's Play) does the same.
- At the end of the scope Play stops; with **Loop** (Ctrl+L / ⌘L in the panel) it starts over. With
  In / Out marked, Loop loops that range (FilmCraft's own loop).
- FilmCraft's keys work on it: J/K/L, ←/→ one frame, ↑/↓ previous / next shot (edit), Home/End, I/O.
- Selecting a node (any part setting `state.selected_node`) moves the playhead to it; while playing
  it keeps playing from there. Clicking a shot in the bar selects it.

## For other parts

- `state.preview.shot_at_playhead`: the shot under the playhead (node area: blue outline while
  `state.preview.player.playing`).
- `state.preview.follow_playhead` + `mortiflix.preview.follow {on?}`: the node area's "Follow the
  playhead" toggle.
- Double-click a node: `ctx.command("mortiflix.preview.play", json!({"node": id, "scope": "shot"}))`.
- The chat / AI: `mortiflix.preview.play {scope?, node?, loop?}` ("Play the bet again" =
  `{"node": "b02", "scope": "beat", "loop": true}`).

## Commands

`mortiflix.preview.play {scope?, node?, loop?}`, `.toggle`, `.pause`, `.seek {node | seconds}`,
`.prevShot`, `.nextShot`, `.scope {scope: all|fromHere|shot|beat}`, `.loop {on?}`,
`.follow {on?}`, `.refresh`, `.reveal {node}`, `.state`. Each returns
`{scope, loop, follow, player, shots, shotAtPlayhead}`.

Automation ids: `mfx.preview.play`, `.prev`, `.next`, `.scope.<all|fromHere|shot|beat>`, `.loop`,
`.res`, `.zoom`, `.bar`, `.beats`, `.remake`, `.reveal`.

## Not done

- No per-shot swap without touching the rest: a new version rebuilds the whole sequence (one undo
  step, imports cached, the playhead kept). Fast for tens of shots; 100+ shots untested.
- Voice / music "stand-in" dotting reads a `stand_in` flag the service does not send yet.
- Transitions play as plain cuts.
- No proxies made for heavy pieces; FilmCraft's Half playback resolution is the default.
