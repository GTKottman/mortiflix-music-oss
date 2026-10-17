# Mortiflix Studio: the node area (part 03, doc 35)

The bottom panel on stages 1-4 (`PanelKind::MfxNodes`, "Mortiflix Nodes"). The whole film as a noodle graph:
shot nodes chained by noodles with a transition pill on every cut, voice lines under the shots they're said over
(dashed noodles), the music cues under them, and (Show ▸ Look and your assets) the looks and your own files as
input nodes (dotted noodles, drawn only for the selected shot).

**Free placement like Blender** (owner, 2026-10-16): nodes stay where you drop them, noodles follow, and the film
order is the chain, never the positions. A node nobody moved sits in its own place (shots in film order, lines and
music under their shots, a transition half way along its noodle) and follows its shot.

Code: `crates/mortiflix-ui/src/nodes.rs` (panel + `nodes.*` commands) and `nodes/` (`model.rs` typed graph and
film chain, `area.rs` view, layout, selection, undo, the studio requests, `canvas.rs` drawing and input,
`panel.rs` header, toolbar, Bin, inspector, overview strip, questions, `stills.rs` picture thumbnails).
State: `MfxState.nodes` (serde: `mortiflix.state` shows view, selection, positions, outbox, undo).

## Doing things

| Doing | Mouse / keys |
|---|---|
| Pan | drag empty space; middle-drag anywhere; trackpad two fingers; Shift + wheel sideways |
| Zoom (20-150 %, at the pointer) | mouse wheel notch; pinch; Ctrl/⌘ + scroll; `=` / `-`; toolbar − / + |
| Select | click; Ctrl/⌘ + click adds; Shift + drag on empty space = box select |
| Move nodes | drag (all selected move together); dropped = saved (`node.move`) |
| Change the film order | pull a noodle from a shot's out socket to another shot (that shot comes next), or from its in socket; or drop a shot onto a noodle ("Drop to put it here", Alt = just place it); or right-click › Move earlier / later |
| Move a beat | drag its band (the 3 px line over its shots): its shots, lines and cue move together |
| OK / Fix / Cut it | buttons on the node (near zoom), the inspector, Enter (OK and on), Shift + Enter (Fix box), Delete / Backspace |
| Walk the film | ← / → previous / next node (shots and the cuts between), ↑ / ↓ shot ↔ voice line ↔ music cue |
| Frame | F selected, `\` all, 1-9 a beat, double-click a beat band; the overview strip under the canvas jumps |
| Find | Ctrl/⌘ + F: words, `#7` / `7`, beat names, state words; Enter frames the next; others dim |
| Undo / redo here | Ctrl/⌘ + Z, Ctrl/⌘ + Shift + Z (moves, order, OK / Cut) |
| Maximise / Restore | the header button or ` (Mortiflix Studio's Maximize Frame); the divider above is Mortiflix Studio's dock split |

The keys are panel-scoped (engine `MFX_STUDIO_PANEL`, scope "Mortiflix Nodes"): with the node area focused
← / → walk nodes; anywhere else they stay Mortiflix Studio's one-frame steps.

## Talking to the studio (part 08's protocol)

`graph.get` (refreshed on `node.changed` / `status` events, every 2 s while something is being made, else 6 s),
`node.edit` (ok / fix / cut / clear), `node.move {positions: {id: [x, y] | null}}`, `node.reorder {order}`,
`node.pick {node, v}` (Use this version), `node.duplicate`, `node.insert {after, text}`, `assets.use {asset, node}`
(a Bin asset dropped on a shot), `chat.action` (a note; a transition picked on a cut with no transition node).
While the studio is away, edits and moves wait in `outbox` and go when it's back.

## Every command (`nodes.*`)

`prev next up down enter fixKey delete frameSelected frameAll frameBeat1-9 zoomIn zoomOut zoomTo{zoom} duplicate
find{text?} selectAll escape undo redo addShot select{node, add?, frame?} ok/cut/bringBack{node|nodes} fix{node|nodes,
text} move{moves: [{node, x, y} | {node, reset}]} reorder{node, after} moveInFilm{node, by} pickTransition{cut, kind,
sec} add{after?, text} useVersion{node, v} note{node, text} use{node, input} play{node, scope: shot|from|cut} show{voice,
music, inputs, bin} highlight{what} wheel{mode} tidy refresh state`. A Cut of something OK'd, a Fix of something
OK'd and a move into another beat ask first (`force: true` skips the question).

## For the other parts

- `state.selected_node` is the node area's selection (both ways: the chat selecting a node frames it here).
- Play: double-click a node, ▶ on a voice line, Play from here / Play across the cut in the inspector send part 04's
  `mortiflix.preview.play {node, scope}` (a cut plays from the shot before it; a line plays its shot; a cue its beat).
  "Follow the playhead" sends `mortiflix.preview.follow {on}`; panning by hand turns it off.
- The blue outline (`#5895ec`) is the shot under the playhead. `nodes::sync_preview` copies
  `state.preview.shot_at_playhead`, `.player.playing` and `.follow_playhead` in; its body is three lines the
  merge with part 04 fills in (written in its doc comment), because `MfxState::preview` is on part 04's branch.
- An asset dragged from part 02's Assets tab (`AssetDrag`) onto a shot sends `assets.use {asset, node}` through
  `nodes.use`; `canvas::hovering_input` / `dropped_input` get one `or` each at the merge (written in their docs).
- `state.graph` holds the last `graph.get` result.

## Not done / open

- Stills: shot versions are video and the demo's looks are SVG, so nodes show an empty 16:9 frame. PNG / JPEG / WebP
  stills (a version's `still`, or a picture `path`) are decoded and shown. The service could add a `still` (a PNG
  frame) per shot version.
- A transition picked on a cut that has no transition node yet goes to the chat history (`chat.action`), not to a
  dedicated message; a look dropped onto a shot is a Fix ("Use the look SF02"). The protocol has no `transition.set`
  or look-swap message.
- Asset positions: `graph.get` does not send a `pos` for assets, so a moved asset node keeps its place only until the
  app closes (shots, lines, cues and transitions are saved).
- Desktop files dropped onto the canvas: part 32 (Assets tab) owns adding files.
