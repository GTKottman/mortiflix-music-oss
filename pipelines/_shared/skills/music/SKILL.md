---
name: music
description: The music engine. Strudel writes the notes, every part on its own MIDI channel; the engine checks them (harmony lock, the story's intensity curve, clean loops for a loop kit), captures them as MIDI (every channel, the arrangement, a clip per section, the cue sheet with the owner's DAW steps), then an instrument is chosen for every channel from its real notes and auditioned, and the piece is rendered and mastered, or finished by the owner in their own DAW from the MIDI pack. Use for any music step, a song, a MIDI pack, a loop kit, or a change to music.
---

# Music

One engine makes all of Mortiflix Music's music: a song, a MIDI pack or a loop kit. It works in stages; each stage writes a file,
the engine checks it, and nothing moves on until the check passes. **The notes and the sounds are separate**: the
score is only music (notes on channels), its MIDI is captured, and only then is an instrument chosen for every channel.
So the same MIDI can be played by Strudel or taken into any DAW.

```
genre ─▶ blueprint ─▶ score ─▶ MIDI ─▶ instruments ─▶ audition ─▶ master
           the plan    notes    every     a sound per     each sound     Strudel's render, mastered
                       only     channel   channel, from   across its     ─ or ─
                                          its real notes  range          the owner's own, from the MIDI pack
```

| Stage | You write | The engine (`node $M <command>`) | It writes |
|---|---|---|---|
| Genre | `music/genre.json` | `genre`: every field, 5+ sources, hook and beat elements | `music/GENRE.md` |
| Blueprint | `music/blueprint.json` | `plan`: sections, chart, roles/channels, genre signature, length | `music/BLUEPRINT.md` |
| Score | `music/score.strudel.js` | `check`: harmony lock, intensity curve, nothing past the end, clean loops (loop kits) | `music/check.json` |
| MIDI | (nothing: it's captured) | `check`, once it passes | `music/notes.json`, `out/music/midi/` (+ `sections/`, `CUE-SHEET.md`) |
| Instruments | `music/instruments.json` | `instruments`: every channel, register clashes, palette rules | `music/instruments-check.json` |
| Audition | (nothing) | `audition`: lowest, middle, highest note of every part | `out/music/audition/*.wav` |
| Master | (nothing) | `master` (the studio's finish) or `render` (a sketch to listen to) | `out/music/master.wav`, `.mp3`, `stems/` |
| Owner's master | (the owner imports it) | `own-master`: is it there, does it fit | `music/own-master/` |

After every command the engine rewrites **`music/MUSIC-SHEET.md`**: every channel with its range, instrument, level,
audition result and why, the story measured against the plan, the master's numbers. It's how the owner (and the next
session) sees exactly how the music was made. `node $M sheet` rewrites it on demand.

```
M=.claude/skills/music/strudel.mjs
```

`render`, `master` and `audition` render audio, so they go through the studio's render queue:
`mfx render --label "master" -- node $M master`, then `mfx render-wait <id>` until it's done (`.mortiflix/GATES.md`).
The other commands run directly.

Read `craft/genre-research.md` before a genre, `craft/melody-craft.md` before writing the motif, `craft/harmony-lock.md` before the score, and
`craft/rich-voicing.md` once the lock passes.

## 0. What the owner set up

Strudel is at `$MFX_STRUDEL`, and runs in the Chrome at `$MFX_CHROME`. `$MFX_DAW` names the owner's DAW (`ableton`,
`fl`, `logic`, `reaper`, `bitwig`, `cubase`, `studioone`, `garageband`, `protools`), when they named one: the cue sheet's
import steps are written for it. If either is missing: `mfx needs-you "Music isn't set up: run mortiflix-music setup music,
then resume."` Never improvise music any other way, and never use another engine.

**Who finishes the music** comes from the pipeline and the brief (the `finish` answer):
- **The owner, from the MIDI**: the studio writes and checks the score, chooses sketch instruments so it can be
  heard, and hands over the MIDI pack; the owner makes the final sound in their own DAW (and, in a song, imports the
  master: section 9).
- **The studio, in Strudel**: instruments, audition, render, master.

## 1. The genre

Research the genre into `music/genre.json` (the format and the method: `craft/genre-research.md`), then
`node $M genre` until it passes. A song is written **from** its genre: its tempo range, lineup, beat grids, harmony,
hook traits and, above all, its unforgettable elements.

## 2. Read the drama

Write it at the top of the blueprint:

- **The dramatic reading** in one sentence, as a transformation: `confusion -> recognition -> confidence`: the topic
  translated into audible behaviour, what the listener feels change from the first bar to the last.
- **What the music must not do**: copy a reference, pile up unrelated ideas, stay at one level.

## 3. The blueprint: `music/blueprint.json`

```json
{
  "title": "a real title", "bpm": 96, "meter": "4/4", "bars": 48, "scale": "D:dorian",
  "dramatic_reading": "confusion -> recognition -> confidence",
  "style": "one dense line of descriptors, under 1000 characters: genre, instruments and their roles, groove, mood",
  "genre_signature": [ { "id": "dusty-breakbeat", "carries": "beat", "how": "half-time break, ghosted snares", "where": "from bar 9" } ],
  "motif": { "rhythm": "...", "contour": "...", "length_bars": 2, "role": "Glass hook", "returns": ["bar 17, an octave up", "bar 41, augmented"] },
  "sections": [
    { "name": "Headlights on wet glass", "start_bar": 1, "end_bar": 8, "function": "establish: pad and sub only, the motif withheld", "intensity": 2 }
  ],
  "chart": [ { "bars": [1, 8], "chords": ["Dm9", "Cmaj7", ["Gm7", "A7sus4"], "Dm9"] } ],
  "roles": [ { "channel": 1, "harmony": "bass", "name": "Sub", "job": "roots, the floor", "instrument_idea": "sine, low-passed" } ],
  "ending": "how it ends and why (a held chord that fades, or a question left unresolved)"
}
```

A loop kit adds `"loops": true` and `"length_fixed": true`: every section is then one loop (see section 6).

Decide in this order: the motif and hook (melody craft) → tempo, meter and scale → the chart → the roles → the story.

- **Sections** cover bars 1..bars, contiguous. Names are specific and story-bearing, never "Intro/Verse/Chorus/A/B"
  (a returning idea is "Return of …"). Each `function` says what happens: establish, develop, destabilise,
  withhold, release, reinterpret, resolve, and what rises or is held back.
- **`intensity` (0-10) is the energy curve**, section by section: the check measures
  each section (parts playing, notes per beat, velocity, range) and fails if the order disagrees. Builds really build;
  breakdowns really thin out. Don't raise everything at once: contrast makes intensity legible.
- **The chart** is built from the scale's own chords; borrowed colours where they serve the story. Vary the harmony
  across sections, and slow the harmonic rhythm to suspend time, speed it up to drive.
- **Roles**: 3 to 10, each on **its own MIDI channel** 1–16 (drums on 10), with a vivid name, a job in the story, and
  an `instrument_idea` (the instruments stage chooses the real sound).
- **The `genre_signature`** names which of the genre's unforgettable elements carry the hook and the beat; the
  length comes from the genre's `length` unless the brief fixed it (then `"length_fixed": true`).

`node $M plan` checks it and writes `music/BLUEPRINT.md`, the readable version (it's what the owner reviews).

## 4. The score: `music/score.strudel.js`, notes only

```js
setcpm(96 / 4)                                      // BPM / beats per bar: one cycle is one bar
const BARS = 48
const cue = (p) => p.filterWhen((t) => t < BARS)    // every part ends with the piece (Strudel loops otherwise)
const lift = "<0!12 1!20 0!4 1!12>"                  // a section mask, one value per bar: off 1-12, on 13-32, off 33-36, on 37-48

sub:   cue(note("<d2 c2 [g1 a1] d2>").velocity(.8)).midichan(1)
pad:   cue(note("<[d3,f3,a3,c4,e4] ...>").velocity(.6)).midichan(2)
hook:  cue(note("<[a4 ~ c5 d5] ...>").velocity("<.7 .85>").mask(lift)).midichan(3)
drums: cue(note("[36 ~ ~ 36 38 ~ 36 ~], [42*8]").velocity("<.8 .95>").mask(lift)).midichan(10)
```

- **Every part** is a labelled block, wrapped in `cue(…)`, with `.midichan(n)` = its role's channel.
- **Only music goes in the score**: pitches, rhythm, `.velocity()` (the playing dynamic: it goes into the MIDI),
  structure (masks, `<…>` sequences, `arrange()`). No `.s()`, filters, effects or `.gain()`: those belong to the
  instruments, and the engine warns when the score sets them.
- **Drums** on channel 10 are General MIDI numbers (36 kick, 38 snare, 42 closed hat, 46 open hat, 49 crash, 39 clap,
  45 tom): the instruments stage gives each number a sound, and the MIDI pack maps them straight into any drum rack.
- **Sections** are masks or `<…>` sequences with one value per bar, aligned to the blueprint's bars. Name each mask
  after its section.

## 5. Check it, and capture the MIDI

```
node $M check
```

It reports the harmony lock, parts that play past the end, roles that never play, and **the intensity curve**: a
sparkline per section, asked vs measured. Iterate until **0 problems**. Then read the curve against the story: does it
rise where it should, does it thin out where it should?

When it passes, the score's notes become **the MIDI**: `music/notes.json` (every channel's notes in beats, with
velocity) and `out/music/midi/`:

- `<title>-all.mid`: the whole arrangement, one track per channel, tempo, meter and section names as markers;
- `NN-<part>.mid`: each channel on its own;
- `sections/NN-<section>/`: a clip per section, its notes moved to start at the section's own bar 1 (every channel
  alone, and all of them together), to drop into a DAW or loop;
- `CUE-SHEET.md`: tempo, key, sections, channels with their ranges, the chord chart, and the steps to bring it into
  the owner's DAW (`$MFX_DAW`);
- `out/music/midi.zip`: all of it as one file.

From here on the MIDI is the music: the instruments are chosen from it, and the owner can take it into any DAW. The
check also prints where each channel plays (lowest, middle, highest note).

## 6. Loop kits

With `"loops": true` every section is a loop the owner will repeat in their DAW, and the check adds what that needs:

- **No note rings past its loop's end.** A tail that crosses the loop point is cut in a DAW, or doubles on the next
  pass: end held notes before the boundary.
- **Every loop plays.** An empty loop fails.
- **Lengths that sit on the grid.** 1, 2, 4, 8 or 16 bars; anything else is a warning.

Each loop is still part of a story (its `intensity`, its `function`): a kit is a family of variations, e.g. the groove
bare, then with the hook, then the hook answered, then a breakdown, so the owner can arrange them into a track. The
clips in `sections/` are the kit.

## 7. Instruments: a sound for every channel

`node $M instruments` lists each channel's range from the MIDI and the built-in sounds. Write `music/instruments.json`:

```json
{
  "concept": "the sound of the whole piece in two sentences",
  "parts": [
    { "channel": 1, "role": "Sub", "sound": "sine", "set": ".lpf(300).release(.3)", "gain": 0.8, "pan": 0, "character": "dark", "why": "a pure floor under everything" },
    { "channel": 3, "role": "Glass hook", "sound": "sine", "set": ".fm(3).fmh(3.5).decay(.4).sustain(.2).delay(.25)", "gain": 0.3, "pan": 0.35, "character": "bright", "why": "glass, for the lamp" },
    { "channel": 10, "role": "Drums", "gain": 0.9, "why": "dry and close",
      "kit": { "36": { "sound": "sine", "set": ".penv(24).decay(.2).sustain(0)" },
               "38": { "sound": "pink", "set": ".decay(.12).sustain(0)", "gain": 0.5 },
               "42": { "sound": "white", "set": ".hpf(7000).decay(.03).sustain(0)", "gain": 0.25 } } }
  ]
}
```

- `sound`: Strudel's built-in synths need no downloads and carry no licence questions: `sine`, `triangle`, `square`,
  `sawtooth`, `supersaw`, `pulse`, and `white`/`pink`/`brown`/`crackle` noise. A sample or soundfont needs `"source"`
  (where it comes from and its licence, also in `assets/SOURCES.md`), and only when the licence allows the owner's use.
- `set`: the sound's shaping, as Strudel methods: filters (`.lpf .hpf .lpq .lpenv`), envelopes (`.attack .decay
  .sustain .release`), FM (`.fm .fmh`), `.vib`, `.penv` (pitch envelope, for drums), `.shape .distort`, `.crush
  .coarse` (lo-fi), `.room .size`, `.delay .delaytime .delayfeedback`. It can't change notes, timing or the channel.
- `gain` is the part's level in the mix (velocity from the score still shapes every note); `pan` runs -1 to 1; each
  channel gets its own effects bus, so one part's reverb never smears another's.
- `character`: `dark`, `warm`, `neutral`, `bright` or `lofi`; `why`: what the sound does for the part.

Choose for **this** genre and **this** piece, as a palette that sounds good **together**, not just good alone. The
check enforces what it can:
- parts that share a register must have clearly different timbres (different families: sine, saw, FM, noise…);
- at most one deliberately degraded (`lofi`) sound;
- the low end and the top both covered; never three bright parts playing at once;
- every channel that plays has an instrument, and every drum number the score uses has a kit sound.

And what it can't: every song should sound different from the last (read `TASTE.md` and earlier projects' palettes);
don't reach for the same sound everywhere; a held pad wants a slow attack, a pluck a fast decay.

Then **`node $M audition`**: every instrument plays its part's lowest, middle and highest note (every kit piece for
drums), one per bar, into `out/music/audition/`. A silent or clipping sound fails here in seconds, not in the final
mix. Fix and audition again until it passes.

## 8. Render and master

- **The studio's finish**: `node $M master` renders the score with its instruments (`out/music/mix.wav`), renders every channel's
  stem (`out/music/stems/`), checks no part is silent or clipping alone, measures the mix's balance (low end, mud,
  width), and masters to -14 LUFS with the true peak at or below -1 dBTP: `out/music/master.wav` and `.mp3`. Act on
  its balance notes (they come from measurements: a muddy mix, a thin low end, a narrow image) by changing
  `instruments.json` (levels, pans, filters), then master again.
- **A sketch** (when the owner finishes it from the MIDI): `node $M render [--stems]` writes `out/music/score.wav` with
  the instruments, so the owner can hear the notes before they take them into their DAW.

You can't hear the render. Look at it: `ffmpeg -i out/music/master.wav -lavfi showspectrumpic=s=1600x400
music/spectrum.png` (each section's density and register should be visible where the blueprint put them). Say in the
note that these are measurements, not listening.

## 9. The owner's own master

When the brief says the owner finishes the music from the MIDI:

1. The song goes to review as usual, with the sketch render (sections 7–8), so the owner judges the notes.
2. Once it's approved, `node $M own-master`. If there is no master yet: `mfx needs-you "The score is approved. On the
   project page, MIDI › Download MIDI: give each channel its sound in your DAW, mix, export from bar 1 to the end,
   then Import your master (or: mortiflix-music music <project> --import <file>)."` Importing resumes the project.
3. When it's there, `own-master` reports how it fits: its length against the score, and whether its first sound
   arrives with the MIDI's first note. Pass its warnings on in the next note. Use `music/own-master/master.wav` as the music from then on; never re-render over it.

## 10. Submitting

What each step sends is in its pipeline's `PIPELINE.md`. A `.mid` item shows as a piano roll in the review room; an
audio item as a player.

At most one question, with a default. Notes on music come back as times (on a piano roll too): the bar is
`floor(t / seconds_per_bar) + 1`. Change the blueprint first when the note is about the story, then
the score; run the check again (it recaptures the MIDI), then the instruments.

## 11. The MIDI pack

The MIDI is always written (section 5) and always delivered: `out/music/midi.zip` goes with every final, with the
stems when the studio rendered them. The owner downloads it from the project page (MIDI › Download MIDI) or with
`mortiflix-music midi <project>`.
