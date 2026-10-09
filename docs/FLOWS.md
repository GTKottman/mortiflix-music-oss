# Flows

A pipeline decides **what you get and where you review it** (a song, a MIDI pack, a loop kit). A flow decides **the
shape the music takes inside it**: how the energy moves, which techniques give it its character, what it never does.
Every piece gets one. Pick it in the brief, or let the studio choose: it learns from how you rate what you get.

Without flows every piece drifts toward the same arc: quiet start, build, breakdown, climax, resolve. Flows make the
arc a choice.

## Choosing one

In the web studio's brief (**Flow**), or `mortiflix-music new <pipeline> --flow <choice>`:

| Choice | What happens |
|---|---|
| Let the studio choose (`studio`, the default) | The studio picks, leaning toward flows you've loved, away from the ones you just had, and now and then trying one you haven't heard |
| From a shape (`shape:builds`) | The studio picks within a bucket: **builds** (the energy climbs to somewhere), **cycles** (something keeps coming back), **stories** (something changes for good), **forms** (a classic way of handling an idea) |
| From a feel (`feel:hypnotic`) | The studio picks among flows with that feel: hypnotic, cinematic, danceable, intimate, restless, uplifting, dark, playful, epic, melancholic, warm |
| Pick one (`rondo`) | That flow |
| `none` | No flow (the piece's shape is left to the session) |

`mortiflix-music flows` lists them (`--by feel` for the other view), `flows show <flow>` prints one. The **Flows** page in
the web studio does the same.

## The built-in flows

For songs and MIDI packs:

| Shape | Flow | In one line |
|---|---|---|
| builds | **Drop** (`drop`) | A build that pulls everything tight, a moment of near-silence, then everything lands at once. |
| builds | **Slow burn** (`slow-burn`) | One long rise with no breakdown: energy is only ever added, never taken away, until the piece arrives where it was always heading. |
| builds | **Staircase** (`staircase`) | Every section is one clear step up: one new layer, one new register, never a step back, until the top step. |
| cycles | **Ostinato trance** (`ostinato-trance`) | One pattern that never stops; everything around it slowly changes, so the same notes keep meaning something new. |
| cycles | **Pendulum** (`pendulum`) | Calm and storm in turns, each swing wider than the last, until the last storm never comes back. |
| cycles | **Rondo** (`rondo`) | A theme that keeps coming home between detours: A, then B, A again, then C, A again. |
| stories | **False ending** (`false-ending`) | It resolves and stops, long enough to believe it is over, then comes back changed and goes further. |
| stories | **Inverted arch** (`inverted-arch`) | It starts at the peak and comes apart: the loudest moment is the first, and the piece is what remains after. |
| stories | **Two worlds** (`two-worlds`) | Two separate ideas, each with its own sound and space, that meet late and play together. |
| forms | **Call and response** (`call-and-response`) | Two voices trading phrases, one asking and one answering, until they finish each other's sentences. |
| forms | **Theme and variations** (`theme-and-variations`) | One motif, rewritten in every section: upside down, slower, faster, in a new register, never the same twice. |
| forms | **Through-composed** (`through-composed`) | It never repeats, only travels: every section is new ground, held together by how each one grows out of the last. |

For loop kits (each section is a loop):

| Shape | Flow | In one line |
|---|---|---|
| builds | **Energy ladder** (`energy-ladder`) | Loops ordered from bare to full: each one keeps the last and adds a layer, so the kit is an arrangement waiting to be dragged in order. |
| builds | **Layer stack** (`layer-stack`) | One groove taken apart: every loop is one or two parts, and any of them stack together in a DAW. |
| cycles | **A/B pairs** (`ab-pairs`) | Every idea comes as twins: a calm loop and a busy one, made to switch between in a DAW. |
| stories | **Breaks and fills** (`breaks-and-fills`) | The main loops plus the pieces that join them: fills, breaks and stops, everything needed to tell a track's story. |
| forms | **Call and answer** (`call-and-answer`) | Loops built as conversations: in each, two parts trade phrases, and the loops answer each other. |
| forms | **One groove, many turns** (`one-groove`) | The drums never change: every loop keeps the same beat while the harmony, bass and hook turn around it. |

## Taste: how the studio learns yours

When a piece is delivered you're asked **how it landed**: *Love it*, *It's fine* or *Not for me* (on the final review,
on the project page, or `mortiflix-music rate <project> love|fine|no`). The studio keeps it in `taste.json` with the
piece's flow and the techniques it planned. From then on:

- **Picking a flow**: a flow you loved weighs more (each *love* +2, *fine* +0.5, *not for me* −1.5), and so do flows
  that share its feels; a flow you've never heard gets a curiosity bonus; the flow of your last two pieces is held well
  back, the four before that half back. So the studio settles toward your taste without repeating itself.
- **Picking techniques**: every session brief lists the techniques in pieces you loved, the ones in pieces that weren't
  for you, and the ones used in your last three pieces (reach past those), for the techniques the piece picks itself.

Two studios that start from the same flows grow apart as their owners rate what they get.

## A flow of your own

A piece you love can become a flow: on its project page, **Make this a flow of your own**, or
`mortiflix-music flows save <project> --name "…"`. Its sections' rise and fall becomes the curve, the techniques the check
heard become the flow's own, and its motif and ending are written into its `FLOW.md`. It lives in your studio's
`flows/` folder, you can pick it for new pieces, and the studio may pick it too.

Share one with `mortiflix-music flows export <flow>` (a zip of `flow.json` and `FLOW.md`); anyone adds it with
`mortiflix-music flows add <file.zip | https://…>`.

## The format

A flow is a folder: `flow.json` and `FLOW.md` (the craft a session reads before planning).

```json
{
  "slug": "slow-burn", "name": "Slow burn", "for": "song",
  "bucket": "builds", "feels": ["hypnotic", "cinematic"],
  "summary": "One long rise with no breakdown…",
  "shape": { "curve": [[0, 1.5], [0.35, 4], [0.7, 7], [0.9, 9], [1, 8]], "rules": ["no_breakdown", "peak_late"] },
  "sections": { "min": 4, "max": 7 },
  "techniques": { "required": ["Ostinato", "Crescendo", "Pedal Tone"], "avoid": ["Drop", "Breakdown", "Stop-Time"], "free": 3 },
  "motif": "…", "surprise": "…", "ending": "…"
}
```

- `for`: `song` (songs and MIDI packs) or `kit` (loop kits). A pipeline says which flows it takes (`"flows": "song"`).
- `shape.curve`: points from 0 (the start) to 1 (the end), intensity 0-10. The plan's section intensities, taken at
  each section's middle (each loop's place in a kit), must follow it with a correlation of 0.6 or more. Optional:
  some flows are better described by their rules alone.
- `sections` (songs): how many sections the piece has.
- `techniques`: `required` (the flow's identity: every one must be planned and heard), `avoid` (never planned; a
  warning if the notes show it anyway), `free` (how many more the piece picks).

Built-in flows are in `flows/`; your studio's `flows/` folder adds to them (the same slug there wins). A project
keeps a copy of its flow from the moment it's created.

## The rules

| Rule | What it asks | For |
|---|---|---|
| `rising` | every section at least as intense as the one before (no dips) | both |
| `no_breakdown` | no section drops below the one before it, until the last | songs |
| `peak_first` | the most intense section starts in the first 30% of the piece | songs |
| `peak_late` | the most intense section starts in the last 40% of the piece | songs |
| `alternating` | intensity swings up and down from section to section (4+ sections) | both |
| `widening` | each swing is at least as wide as the one before | songs |
| `quiet_before_peak` | the section right before the peak is quiet (intensity 3 or less) | songs |
| `ends_low` | the last section is quiet (3 or less) and well below the peak | songs |
| `rondo` | sections marked "material": "A" return 3+ times, never back to back, between 2+ other materials | songs |
| `merge` | an "A" world and a "B" world apart first, then a section marked "A+B" that plays both | songs |
| `false_stop` | a near-silent section of 1-2 bars (intensity 1 or less) after the music has built (5+), with more after it | songs |
| `through` | no section repeats an earlier one: every section is new material | songs |
| `pairs` | loops in pairs: each odd loop (1st, 3rd…) a calm twin of the busier loop after it | kits |
| `shared_drums` | every loop keeps the first loop's drum pattern (75%+ of its hits) | kits |
| `layers` | every loop is one or two parts: layers that stack on each other | kits |

The plan is held to each rule with the intensities it asks for; the score with what the engine measures (each
section's level: notes per beat, parts playing, velocity, range), and for `rondo`, `merge`, `through` and
`shared_drums` with how much of one section's notes another keeps.

## The techniques

All 144 of them, the owner's list from the music prompt. A blueprint names each technique it uses with its part
(a channel; two for techniques between parts), where (section names, or `all`) and how; the check listens for it
there in the captured MIDI. `node .claude/skills/music/strudel.mjs techniques` prints this list in a session, and with
a captured score, what it plays.

| Technique | Heard | What counts |
|---|---|---|
| Arpeggio | on a part | chord tones one at a time: 4 in a row, a 3rd or more apart, at least twice |
| Arpeggiate | on a part | as Arpeggio |
| Staccato | on a part | notes sound at most 55% of the time to the next (the median) |
| Legato | on a part | each note runs into the next: 95%+ of the time to the next (the median) |
| Tenuto | on a part | notes held their full value: 85-100% of the time to the next |
| Marcato | on a part | 3+ notes both accented (0.15+ velocity over the part) and short |
| Accent | on a part | 3+ notes 0.15+ velocity over the part's median, at most half of them |
| Portamento | not in the notes: the instruments stage | a glide between notes: the instrument's glide or pitch envelope |
| Glissando | on a part | 6+ notes by step at a 32nd or faster, one way |
| Tremolo | on a part | 6+ fast repeats (16th or faster) of one note, or of two notes a 3rd or more apart |
| Trill | on a part | 6+ fast alternations (16th or faster) between two notes a step apart |
| Mordent | on a part | main note, quick neighbour, main note held: twice or more |
| Turn | on a part | above, main, below, main (or the reverse), quick: twice or more |
| Grace Note | on a part | a very short note (≤ 0.15 beat) leaning into the next, a step or two away: twice or more |
| Appoggiatura | on a part | a leaning note resolving by step (the harmony lock keeps it short: as Grace Note) |
| Acciaccatura | on a part | as Grace Note: crushed in just before the main note |
| Ostinato | on a part | one bar repeated exactly 4+ times in a row, or two bars 3+ times |
| Riff | on a part | a melodic 1- or 2-bar figure (4+ notes, 3+ pitches) played twice or more in a row |
| Motif | on the motif | the motif ("motif.at") comes back twice or more, in any recognisable form |
| Sequence | on a part | a 3-5 note figure played 3+ times in a row, moved by the same interval each time |
| Pedal Tone | on a part | one pitch held or repeated for 60%+ of the part, under 2+ different chords |
| Drone | on a part | a note held 2 bars or longer |
| Alberti Bass | on a part | low, high, middle, high: 3+ times |
| Walking Bass | on a part | 8+ quarter notes in a row, a new pitch each beat, mostly by step |
| Broken Chord | on a part | as Arpeggio |
| Block Chord | on a part | 3+ notes struck together, 4+ times |
| Power Chord | on a part | root and 5th (and octaves), no 3rd: 4+ times |
| Chord Stab | on a part | short struck chords (≤ ½ beat) with space after: 4+ |
| Chord Pulse | on a part | 4+ chords in a row on an even pulse of a beat or faster |
| Strumming | on a part | 3+ chord notes a hair apart (≤ 0.15 beat) in pitch order: 4+ times |
| Rolling Chord | on a part | 3+ notes spread upward over 0.1-0.75 beat, each held: twice or more |
| Arpeggio Sweep | on a part | 5+ chord leaps one way at a 16th or faster |
| Scalar Run | on a part | 5+ notes by step (1-2 semitones) one way |
| Chromatic Run | on a part | 5+ notes by semitone one way |
| Diatonic Run | on a part | 5+ notes of the scale by step one way |
| Pentatonic Run | on a part | 5+ notes one way in whole steps and minor 3rds |
| Octave Run | on a part | 4+ steps doubled an octave apart |
| Thirds | on a part | 4+ two-note chords a 3rd apart in a row (or, between two parts, 4+ shared attacks a 3rd apart) |
| Sixths | on a part | 4+ two-note chords a 6th apart in a row (or, between two parts, 4+ shared attacks a 6th apart) |
| Contrary Motion | between two parts | two parts: 3+ shared moves in a row going opposite ways |
| Parallel Motion | between two parts | two parts: 4+ shared moves in a row the same way, the same distance |
| Oblique Motion | between two parts | two parts: one holds a note while the other moves through 3+ pitches, twice or more |
| Call and Response | between two parts | two parts: 3+ exchanges where one plays a phrase, then the other answers |
| Question-and-Answer Phrase | on a part | a phrase left open (not on the tonic) answered by one landing on the tonic, twice or more (with two parts: as Call and Response) |
| Antiphonal Pattern | between two parts | two parts: as Call and Response |
| Hocket | between two parts | two parts interlocking, never on the same 16th: 2+ bars |
| Syncopation | on a part | 4+ attacks (20%+) off the beat and held across it, or leaving the next beat empty |
| Backbeat | on a part | snare or clap (GM 37-40): 70%+ of the hits on beats 2 and 4, in 60%+ of the bars |
| Offbeat | on a part | 40%+ of the attacks on the "and", 30% or less on the beat |
| Polyrhythm | between two parts | two parts with steady pulses in a 3:2, 4:3, 5:4, 5:3 or 5:2 ratio |
| Cross-Rhythm | on a part | 5+ even attacks every ¾, 1½, ⅔ or 1⅓ beats, against the beat |
| Tuplet | on a part | 6+ attacks (25%+) on thirds, fifths or sixths of a beat |
| Triplet | on a part | 6+ attacks (25%+) on triplet positions |
| Swing | on a part | 6+ off-beat attacks (60%+) late, 58-72% through the beat |
| Shuffle | on a part | as Swing |
| Dotted Rhythm | on a part | long-short in 3:1 (dotted 8th + 16th, or dotted quarter + 8th): 4+ times |
| Gallop | on a part | an 8th and two 16ths (or the reverse): 4+ times |
| Tresillo | on a part | bars on the 3+3+2 grid: twice or more |
| Clave | on a part | two-bar clave cycles (3-2 or 2-3, son or rumba): twice or more |
| Euclidean Rhythm | on a part | bars whose k hits are spread as evenly as the 16th grid allows (k not dividing 16): twice or more |
| Polymeter | on a part | a pattern repeating 3+ times on a length the bar doesn't divide (3 beats in 4/4) |
| Rubato | on a part | 30%+ of the attacks pulled off the grid by 0.02-0.2 beat |
| Ritardando | on a part | 6+ notes in a phrase whose gaps keep growing, 1.5× by the end |
| Accelerando | on a part | 6+ notes in a phrase whose gaps keep shrinking, 1.5× faster by the end |
| Fermata | on sections | every part stops attacking for 2+ beats while a note rings, after a busy passage |
| Pickup | on a part | a phrase starting in the last beat of a bar and leading into the downbeat: twice or more |
| Anacrusis | on a part | as Pickup |
| Anticipation | on a part | attacks a 16th to an 8th before the bar line, held across it, nothing on the downbeat: 3+ |
| Delay | on a part | phrases coming in a 16th to an 8th after the bar line: 3+ |
| Push | on a part | 40%+ of the attacks early by 0.02-0.12 beat |
| Drag | on a part | 40%+ of the attacks late by 0.02-0.12 beat |
| Ghost Note | on a part | 4+ notes at velocity 0.35 or under, in a part around 0.55 or louder |
| Dead Note | on a part | 4+ very short (≤ 0.1 beat), soft (≤ 0.5) notes |
| Muted Note | on a part | as Dead Note |
| Palm Mute | on a part | 8+ short (≤ 0.3 beat), low (≤ C4) notes in a row |
| Pizzicato | on a part | plucked: short and detached (as Staccato); the instrument gives the pluck |
| Spiccato | on a part | bounced: short and detached (as Staccato) |
| Col Legno | not in the notes: the instruments stage | struck with the wood of the bow: the instrument's attack |
| Harmonic | not in the notes: the instruments stage | a glassy overtone: the instrument's timbre |
| Vibrato | not in the notes: the instruments stage | a pitch wobble on held notes: the instrument (.vib) |
| Pitch Bend | not in the notes: the instruments stage | a bend into or out of a note: the instrument (pitch envelope) |
| Scoop | not in the notes: the instruments stage | a bend up into the note: the instrument |
| Fall | not in the notes: the instruments stage | a bend down off the note: the instrument |
| Doit | not in the notes: the instruments stage | a quick bend up off the note: the instrument |
| Rip | not in the notes: the instruments stage | a fast smeared run up into a note: the instrument |
| Flutter Tongue | not in the notes: the instruments stage | a rolled, buzzing tone: the instrument |
| Growl | not in the notes: the instruments stage | a rough, distorted tone: the instrument (.shape, .distort) |
| Swell | on a part | a crescendo turning straight into a diminuendo |
| Crescendo | on a part | velocity rising 0.15+ over 3+ bars in a row |
| Diminuendo | on a part | velocity falling 0.15+ over 3+ bars in a row |
| Sforzando | on a part | a sudden loud attack (0.25+ over the part) dropping straight back to soft |
| Fade-In | on a part | velocity rising from 0.45 or less at the start of the part |
| Fade-Out | on a part | velocity falling to 0.45 or less at the end of the part |
| Sidechain Pumping | not in the notes: the instruments stage | the mix ducking with the kick: the instruments stage (sidechain) |
| Gate Rhythm | on a part | 8+ chopped chords (≤ 0.2 beat) at a 16th or faster |
| Chopping | not in the notes: the instruments stage | a sample cut into rhythmic slices: the instruments stage |
| Stutter | on a part | 3+ repeats at a 32nd or faster |
| Retrigger | on a part | as Stutter |
| Ratchet | on a part | as Stutter |
| Roll | on a part | 6+ hits at a 32nd or faster on one sound |
| Flam | on a part | a softer grace hit 0.02-0.08 beat before the main one, on the same sound: twice or more |
| Percussion Drag | on a part | two quick grace hits into a main one: twice or more |
| Paradiddle | on a part | two sounds in the pattern A B A A, B A B B |
| Fill | on sections | a section's last bar (or a one-bar loop) with 1.5× the drum hits of the bars before, bringing in toms or new sounds |
| Break | on sections | a bar with one part left alone, next to bars with 3+ parts |
| Drop | on sections | a section landing 1.5× busier than the bar before, with 2+ more parts |
| Buildup | on sections | a section rising bar by bar, ending 30%+ busier than it began |
| Breakdown | on sections | a section 35%+ quieter than the one before, with fewer parts |
| Stop-Time | on sections | bars where every part hits together, then a beat or more of silence: twice or more |
| Repetition | on sections | a section that replays 80%+ of an earlier one of the same length |
| Variation | on the motif | the motif with its rhythm or most of its intervals kept and the rest changed (without a motif: a section keeping 40-80% of an earlier one) |
| Inversion | on the motif | the motif upside down (its intervals mirrored) |
| Retrograde | on the motif | the motif backwards |
| Retrograde Inversion | on the motif | the motif backwards and upside down |
| Augmentation | on the motif | the motif slowed 1.5× or more |
| Diminution | on the motif | the motif sped up 1.5× or more |
| Fragmentation | on the motif | 3+ notes of the motif played on their own, twice or more |
| Development | on the motif | the motif comes back 3+ different ways |
| Transposition | on the motif | the motif moved to another pitch, rhythm kept |
| Octave Displacement | on the motif | the motif (or some of its notes) moved by an octave |
| Suspension | on a part | a sus chord's 4th in the part, resolved to the 3rd on the same root |
| Anticipation Tone | on a part | the next chord's note sounded just before it arrives: twice or more |
| Passing Tone | on a part | a non-chord note stepping between two chord tones: twice or more |
| Neighbor Tone | on a part | a step away from a chord tone and back: twice or more |
| Chromatic Enclosure | on a part | a chord tone on the beat approached from a semitone above and below: twice or more |
| Diatonic Enclosure | on a part | a chord tone on the beat approached by scale steps above and below: twice or more |
| Walking Enclosure | on a part | enclosures by semitone or scale step, walking into the target: twice or more |
| Arpeggio Enclosure | on a part | a chord tone on the beat approached from chord tones above and below: twice or more |
| Contrary Arpeggio | between two parts | two parts arpeggiating opposite ways: as Contrary Motion |
| Cascade | on a part | 6+ fast notes (16th or faster) tumbling down a 5th or more |
| Ascending Staircase | on a part | a figure repeated 3+ times, 1-4 semitones higher each time |
| Descending Staircase | on a part | a figure repeated 3+ times, 1-4 semitones lower each time |
| Pendulum | on a part | 6+ notes turning every note, a 3rd or more each way |
| Zigzag | on a part | 6+ notes turning every note while climbing or falling 4+ semitones |
| Arch | on a part | phrases rising to a peak in the middle and falling back: twice or more |
| Inverted Arch | on a part | phrases falling to a low point in the middle and rising back: twice or more |
| Spiral | on a part | a turning figure repeated 3+ times at new heights |
| Wave | on a part | a phrase rising and falling twice or more, by a 3rd or more each time |
| Leap-and-Step | on a part | a leap (5+ semitones) answered by a step back: 3+ times |
| Step-and-Leap | on a part | two steps one way springing into a leap: 3+ times |
| Repeated-Note Motif | on a part | one pitch 4+ times in a row: twice or more |
| Neighbor Oscillation | on a part | 5+ slower alternations (longer than a 16th) between two notes a step apart |
| Chord-Tone Targeting | the harmony lock | strong beats land on chord tones: the harmony lock already guarantees it |
| Approach-Note Targeting | on a part | a non-chord note stepping into a chord tone on the beat: 3+ times |
