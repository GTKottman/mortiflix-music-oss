# Into Edit (doc 38): how "Edit the video" works in the app

Part 06. Code: `crates/mortiflix-ui/src/into_edit.rs` (the plan, the build, the stage-5 panel) and
`crates/ui-egui/src/mortiflix_into.rs` (runs it on Mortiflix Studio's `Session`).

## The hand-off

1. Entering stage 5 (Edit the video, Next from Voice & music, Ctrl+Alt+5, or `mortiflix.importEdit`)
   docks the **Mortiflix Stage** panel beside the Mortiflix chat and, the first time, asks the service for
   `handoff.plan`.
2. The plan is read and bounded (`Plan::from_handoff`, every value hostile), then built in-process, all in
   one undo step **"Mortiflix: Bring <title> into Edit"**:
   - a fresh Mortiflix Studio project when the open one has nothing unsaved (never loses work), one sequence at
     the project's size and rate; tracks named V1 Shots, A1 Voice, A2 Music (A3 Sounds), only those
     with something on them;
   - bins as the plan names them (Shots › `N beat`, Voice, Music, Your assets/…, Older versions, Cut on
     the board), every piece and version imported once, items named like their clips;
   - shots on V1 back to back; a shot cut from the animatic gets the item In/Out of its own range
     (`project.setMarks`) and plays only that range; a shot with no picture yet is a colour matte;
   - voice lines at their times; the music cut at its cues (one clip per cue);
   - the board's native transitions (Cross Dissolve, Dip to Black) as Mortiflix Studio transitions;
   - item and clip labels = the beat's colour (`edit.label`), a Chapter marker per beat, a Comment
     marker per music cue, a caption per voice line, the project notes;
   - saved as the plan's `.fcproj`; then `handoff.done {map}` tells the service what came in.
   A piece that fails is listed on its row with **Make it again in Mortiflix** / **Leave it out**; the
   rest still comes in. Only a sequence that can't be made fails the hand-off (Try again).
3. Done once. After that, `node.changed` (and a slow timer) asks `handoff.swaps`; each newer version is
   imported, the old item moves to Older versions, `clip.replaceFromBin` puts it in the same spot
   (one undo step "Mortiflix: S04 v3"), then `handoff.swapped`. File ▸ **Start the Edit Again from
   Mortiflix…** asks first, then builds a new sequence (the old one stays in the project).

## The Mortiflix panel on stage 5

The Import summary (rows with ○ ◐ ✓ !, progress, How it's placed, Bins) folds away once the edit is
open. Below it, the clicked timeline clip: name, beat (colour + number + name), times and track, its
words, versions newest first (On the timeline / **Use this one** = `mortiflix.useVersion`, no AI), the
talk, chips (New version, Trim it, Show it in Mortiflix) and the ask box. **Send**: a trim in plain
words ("trim 0.5 s off the start", "cut 12 frames from the end") is done at once as
`mortiflix.timelineEdit` (one undo step "Mortiflix: Trim …"); anything else is a Fix on that node
(`node.edit`). Changes (folded): making · done · needs you.

## Commands

| Id | Params | |
|---|---|---|
| `mortiflix.importEdit` | `{again?}` | go to stage 5 and bring the video in (again: start over) |
| `mortiflix.import.run` | `{open_edit?: true, again?}` | the same import; `open_edit: false` = Get the video (part 07): no stage change, no toast; Export waits for the timeline |
| `mortiflix.restartEdit` | `{}` | File menu; shows the confirm in the panel |
| `mortiflix.useVersion` | `{node, version}` | a version already in the bin onto the timeline |
| `mortiflix.timelineEdit` | `{label, steps: [{id, params}]}` | engine commands as one undo step "Mortiflix: …" |

Automation ids: `mfx.into.*` (bringIn, goToEdit, back, summaryFold, use.v<N>, chip.newVersion|trim|show,
ask, send, changes, restart.go|keep, failed.<node>.again). State: `mortiflix.state` → `state.handoff`.

Demo project for trying it (no AI): `src/demo-handoff.mjs` in the service, branch
`mfx-service/integration` (`makeHandoffDemo(studioRoot)`).
