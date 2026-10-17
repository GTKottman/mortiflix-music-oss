# Voice & music (stage 4) — how it is built

Part 05 of the Mortiflix FilmCraft build (docs 34 and 33b in Morti Drive › Design Docs › Mortiflix FilmCraft).

## Where it lives

| What | File |
|---|---|
| Stage 4's panel (`show`): header + transport, the selected line / cue, the lanes, the booth | `crates/mortiflix-ui/src/voice_music.rs` |
| The data on one time axis (pure, tested) | `crates/mortiflix-ui/src/voice_music/model.rs` |
| The lanes (Shots, Music, Voice; zoom; drag a line; drop a recording) | `crates/mortiflix-ui/src/voice_music/lanes.rs` |
| Doc 33b cards for the chat: `show_theme_card`, `show_cue_plan` | `crates/mortiflix-ui/src/voice_music/theme.rs` |
| `mortiflix.pickFile` (OS file dialog → a service request) | `crates/ui-egui/src/mortiflix.rs` |
| Service side | part 08's `voicemusic.view`, `mix.set`, `voice.line.*`, `voice.booth.*`, `music.*` (`mfx-service/core`) |

## Data (part 08's protocol)

- `graph.get`: every piece with its state, the owner's OK / Cut, its versions and which one plays.
- `voicemusic.view`: the shots, voice lines and cues at their seconds (a hand-set start wins), each line's mix
  (level, voice, timing lock) and your booth takes, the cue plan before the score (doc 33b) and the scored cues
  with level, ducking and length.
- `voice.list` (the voices a line can use) and `music.theme.get` (the theme in words).
- Read again on `node.changed` / `status`, every 3 s while something is being made, else every 15 s.
- Without `voicemusic.view` the stage still lays everything out from the graph; levels, timing and takes are then
  read-only.

## What each control sends

| Control | Message |
|---|---|
| OK / Cut it / Take back / Put it back | `node.edit {node, action: ok \| cut \| clear}` |
| Fix (inline note) · Try again · Make shot N longer | `node.edit {node, action: fix, text}` |
| New take · Say it faster | `voice.line.take {line, text?}` |
| Remake line | `voice.line.words {line, text}` |
| Voice (this line / all lines without an OK) | `mix.set {node, voice}` per line |
| An AI take chip · a cue version chip | `node.pick {node, v}` |
| One of your takes · Keep & next | `voice.booth.keep {line, take}` |
| −0.1 s / +0.1 s · drag the line · Back to the script's timing | `mix.set {node, nudge_sec \| start_sec \| start_sec: null}` |
| Level · Quieter under the voice · Shorter / Longer | `mix.set {node, gain_db \| duck_db \| len_bars}` |
| Import a recording… · drop a file on a line | `mortiflix.pickFile` → `voice.booth.import {line, path}` |
| Swap: try another | `music.cue.set {cue, text}` |
| A planned cue's feel / energy (stage 4 and the chat card) | `music.cue.set {cue, feel, energy}` |
| Using this (theme card) | `music.theme.use {sample}` |
| Play all / Play beat / Play line / click / ruler | FilmCraft's player: `playhead.set`, `playback.toggle \| stop \| forward` |

The selection is shared (`state.selected_node`), so the chat's "talking about" bar and the node area follow it.
The move-on buttons are the stage bar's (Edit the video / Get the video, owner answer 1). When the service reports
nothing for stage 4, the stage fills its status (making / needs you / "N of M OK") so Get the video asks first only
when something failed.

## Not done yet

- Recording from the microphone in the booth (Record is shown, disabled): the app has to capture the take
  (FilmCraft's voice-over recorder) and hand it over with `voice.booth.import {source: booth}`.
- Lane M / S / faders: the protocol has no lane mix yet (per-line and per-cue levels work).
- Playing a sample or a single take's audio on its own (the app plays through FilmCraft's sequence only).
- The doc 34 keys (← →, Enter, R) work only where FilmCraft's own keymap doesn't take them first; panel-scoped
  bindings would need `menus.rs`.
- The loudness meter and the playhead position come from the preview (part 04); this stage shows where it last put
  the playhead.
- "Open the notes" waits for the mini DAW.
