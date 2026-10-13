<p align="center"><img src="web/img/mark.png" width="96" alt="Mortiflix Music"></p>

<h1 align="center">Mortiflix Music</h1>

<p align="center"><b>Claude composes. You decide.</b><br>
A music studio on your own machine that writes music the way a composer does, and stops for you at every turn.</p>

---

Most "AI music" tools are one prompt and a slot machine: you get a finished audio file, and if it's wrong you pull
the lever again. Mortiflix Music never takes the music out of your hands. Claude researches the genre, plans the
piece in words, writes it note by note, and checks every note. **Nothing moves forward until you say so.** What you
get at the end is **the MIDI**: every part on its own channel, ready for your DAW, or a mastered track if you want
the studio to finish it.

```
  you: the brief
      │
      ▼
   genre ──▶ blueprint ──▶ score ──▶ check ──▶ MIDI ──▶ sounds ──▶ master ──▶ final
 researched   in words     a layer    every     every             (or your     │
                  │        at a time  note      channel            own DAW)    │
                  ▼           │                                                ▼
              YOU APPROVE   YOU WATCH IT GROW                    YOU APPROVE, NOTE A BAR,
              or ask for                                         OR CHANGE THE NOTES YOURSELF
              changes
```

## How a song gets composed

Every piece follows the same method. Each stage leaves a file you can read, and a machine check refuses it until it's
right.

1. **You write the brief.** The genre, what the piece is about, how long, who finishes the sound (the studio, or you
   in your DAW), and the flow: the shape the music takes (a slow burn, a rondo, an inverted arch…), or let the studio
   pick one from what you've loved before.
2. **The genre is researched first.** A session reads 5+ sources and writes down what makes the genre itself: its
   tempo range, lineup, beat grids, harmony, hook traits and its unforgettable elements. The song is written *from*
   that profile, not from a vague idea of the style.
3. **The drama is read.** The topic becomes one transformation the listener should feel from the first bar to the
   last, like `confusion -> recognition -> confidence`, and a list of what the music must not do.
4. **The piece is planned in words: the blueprint.** The hook, tempo, key and meter, the chord chart for every bar,
   at least 6 parts (most pieces have 7 to 10), each with a name, a job and its own MIDI channel, and the story as
   named sections, each with an energy level from 0 to 10. It also names the techniques the piece will use, from 144
   (call and response, a pedal tone, augmentation…), with where and how. For example:

   | Bars | Section | What happens | Energy |
   |---|---|---|---|
   | 1–8 | Headlights on wet glass | Pad and sub only; the hook is held back | 2 |
   | 9–24 | The overpass | The beat arrives; the glass hook asks, the guitar answers a bar later | 6 |
   | 25–40 | Return of the headlights | The hook comes back an octave up; every part plays | 8 |
   | 41–48 | Home | The hook at half speed as the city lets go; the sub finally moves | 4 |

   **This is your first gate.** You read the blueprint as plain paragraphs, comment on any of them, and approve it
   or ask for changes. Once you approve it, it's the contract: its tempo, key, chords, parts and sections are kept,
   and changing any of them comes back to you.
5. **The score is written a layer at a time, while you watch.** The beat, then the bass, the chords, the hook, then
   the rest (counter-lines, answers, risers). Each layer appears on the project page as it lands, and you can play it.
   The score is only notes: pitches, rhythm, velocity and structure. No sounds yet.
6. **Every note is checked.** The engine plays the score and refuses it until every note fits the chord of the moment,
   each section's measured energy follows the blueprint's curve, every planned technique can really be heard in the
   notes (a call and response has to trade phrases), the flow's shape holds and nothing runs past the end. The session
   fixes and checks again until there are 0 problems. Only then is the score captured as **MIDI**.
7. **Sounds come last.** Each channel gets a sound chosen from the notes it really plays, and every sound is
   auditioned at its part's lowest, middle and highest note. A sound can never change a note, so the MIDI you take
   into your DAW is exactly what you heard.
8. **It's finished, by the studio or by you.** The studio renders and masters it (-14 LUFS), or hands you the MIDI to
   finish in your own DAW, and checks your master against the score when you import it.
9. **Your final gate.** You hear it, see it as a piano roll, and leave notes on exact bars. In a MIDI pack or a
   loop kit you can also change the notes yourself, right there (below). Approve it, and it's delivered. Rate how it
   landed, and the next piece leans your way.

The whole method, stage by stage: [docs/MUSIC.md](docs/MUSIC.md).

## You stay in the loop

Claude does the work; you keep the say. That's built into the code, not left to a prompt:

- **Only you cross a gate.** A session can finish its own internal steps, but it can't approve a step you review.
  It submits, writes a handoff and stops. When you answer, a fresh session picks up from the journal, so waiting on
  you costs nothing, for an hour or a week.
- **Every note you leave gets an answer.** A new version is refused unless it says, note by note, exactly what it
  changed ("moved the hook's first entry to bar 17", never "addressed"), or why it didn't.
- **What you reviewed can't change behind your back.** Everything you're shown is copied out of the session's reach
  the moment it's submitted.
- **Your words outrank the defaults.** Your brief, notes and answers come before the pipeline's own craft rules. A
  session that disagrees does what you asked and says so.
- **You're asked, not assumed.** When a choice is yours, the session asks it at the gate. A question with a sensible
  default lets you just approve; one without a default waits for your answer.
- **You can put your hands on the notes.** In a MIDI review, drag across the bars you mean, say what you want ("a
  different drum beat") and press **Change it now**: Claude writes only those bars, the studio forces them in and
  proves nothing else moved, and it plays back within seconds. Keep it, undo it, or try another. Or edit the notes
  yourself in the piano roll. Your edits are put back in on every check, so no later rewrite can lose them.
- **It learns your taste, with your permission.** What you rate, keep and undo shapes the next pieces. When you point
  out a real mistake, the session proposes a new check ("the bass never flams with the kick"); approve it once and
  every future piece runs it.
- **It never burns tokens behind your back.** Two sessions in a row that move nothing forward pause the project and
  tell you why.

## Who it's for

It's built for **one person**: you write the briefs, you review every stage, and the studio runs on your machine. The
work is done by Claude through **your own Claude Code login or your own Anthropic API key**. Mortiflix Music is the
harness around it: the pipelines, the gates, the music engine that checks every note, the review room, and the memory
that carries a project across sessions and days.

Pipelines are folders: a `pipeline.json` (the steps, how each is reviewed, the error checks), a `PIPELINE.md` (the
craft) and skills. Anyone can write one.

## What it makes

| Pipeline | You review | You get |
|---|---|---|
| `song` | blueprint → the finished song | a mastered instrumental (-14 LUFS), the stems, the music sheet and the MIDI. Or you finish it in your DAW and import your master |
| `midi-pack` | blueprint → the MIDI | MIDI for your DAW, written from a researched genre (and around a `.mid` of yours, if you bring one), with a sketch render to hear it by |
| `loop-kit` | kit plan → the loops | a family of loops in one genre, each checked to repeat cleanly, as MIDI clips to arrange |

Every one of them delivers the **MIDI pack**:

```
midi/
  night-drive-all.mid          the whole arrangement, one track per channel, tempo, meter and section markers
  01-sub.mid  03-hook.mid …    each channel on its own
  sections/
    01-headlights/             a clip per section, from its own bar 1: every channel alone, and all together
    02-the-overpass/
  CUE-SHEET.md                 tempo, key, sections, each channel's range, the chords, and your DAW's import steps
MUSIC-SHEET.md                 every decision and measurement, from the genre to the master
```

## Flows: the shape it takes

Without them every piece drifts toward the same arc. With them the arc is a choice, from four shapes:

| Shape | Flows for songs and MIDI packs | For loop kits |
|---|---|---|
| **Builds** (the energy climbs to somewhere) | Slow burn · Staircase · Drop | Energy ladder · Layer stack |
| **Cycles** (something keeps coming back) | Rondo · Pendulum · Ostinato trance | A/B pairs |
| **Stories** (something changes for good) | Two worlds · Inverted arch · False ending | Breaks and fills |
| **Forms** (a classic way of handling an idea) | Theme and variations · Call and response · Through-composed | One groove, many turns · Call and answer |

Or browse by feel (hypnotic, cinematic, danceable, intimate, restless, uplifting, dark, playful, epic, melancholic,
warm). Each flow names the techniques that make it, from 144 (arpeggio, hocket, tresillo, inversion, stop-time…), and
the check hears each one in the MIDI: a planned call and response has to really trade phrases. Rate what you get and
the studio's picks follow your taste, so two studios that start the same grow apart. The whole story:
[docs/FLOWS.md](docs/FLOWS.md).

Name your DAW in setup (Ableton Live, FL Studio, Logic Pro, REAPER, Bitwig, Cubase, Studio One, GarageBand, Pro
Tools) and the cue sheet says how to bring it in there. Drums are on channel 10 in General MIDI numbers, so they map
straight onto a drum rack.

## Quick start

> **Tested on Linux.** macOS should mostly work; the Windows installer is here (`install.ps1`), but sessions on
> Windows are still being tested in Mortiflix, which this studio shares its engine with.

You need **Node 20+** and **ffmpeg**. For real music you also need one of:
- [Claude Code](https://claude.com/claude-code), logged in (your plan pays), or
- an [Anthropic API key](https://console.anthropic.com/) (you pay per token).

```sh
git clone https://github.com/GTKottman/mortiflix-music-oss.git && cd mortiflix-music-oss
npm install
npm link                       # puts `mortiflix-music` on your PATH (or use: node bin/mortiflix-music)

mortiflix-music init           # makes the studio folder (~/MortiflixMusic) and picks a backend it finds
mortiflix-music setup          # the walkthrough: Claude, the music engine (Strudel), your DAW
mortiflix-music demo           # a full walk-through with placeholder work: free, no Claude needed
```

Then either:

**A. In the terminal**

```sh
mortiflix-music new midi-pack        # asks the brief's questions, the flow among them (or --flow rondo)
mortiflix-music run                  # sessions run until something waits on you
mortiflix-music review               # read the note, answer questions, leave notes, approve or ask for changes
mortiflix-music midi <project>       # the MIDI pack as one zip
mortiflix-music rate <project> love  # how it landed: the studio's next picks follow it
```

**B. In the browser**

```sh
mortiflix-music serve                # http://127.0.0.1:4747
```

The web studio opens on your **library**: everything you've made, each with a sketch of its notes and a play button, and
what's waiting on you on top. Press play and you hear the MIDI right in the browser on a small built-in synth and drum
kit (a sketch of the music; the real sounds come in your DAW), or your own master once you've imported it. The player
bar stays at the bottom across pages, shows the section you're in, and Space plays and pauses. The studio also shows a
live log of what Claude is doing, and the review room: MIDI shows as a
**piano roll** you can play (pick a part to bring it forward; click a bar, or press *Note at the playhead* while it plays, to leave a note there), audio as a player you can note a
moment on, a blueprint as paragraphs you can comment on.

**Change it right there.** On a MIDI review, drag across the bars you mean (pick a part first to change only that part), write
what you want ("a different drum beat"), and press **Change it now**. Strudel finds what plays in those bars and the code
behind it, Claude writes only the new bars, and the studio forces them in: everything outside them stays exactly as it was,
and it checks that. The roll redraws and plays from just before the change within seconds. Then **Keep**, **Undo** or
**Try another**. Before it writes, Claude is briefed by the studio's context engine: the section and what it's for, its
chords, the bars around it, what the other parts play, the genre's beat, and the edits you've kept before. What you keep is
saved as a measured difference (`taste.json`) and shapes the next edits and the next pieces you make; what you undo
counts against that change. Quick edits use Sonnet through your own Claude Code or API key (`config quickModel` to change
it).

**Made for learning, with your hands on the notes.** Above every piece's MIDI sits a row of its **instruments**, each in
its own colour with a line saying what it is ("The low notes: the bass line", "A tune: the melody you could hum").
While it plays, each one **lights up** as it sounds, so you can tell what you're hearing and which part to change. Click
one to read what it does in the piece, **hear it alone** or **mute it**, and (in a review) tell the studio about it with
one click ("Too loud", "Plays too much", "Wrong sound for it"…). Below, the **notes of the whole piece** in the
instruments' colours, following the music as it plays: pick an instrument, click to add a note, drag to move it, drag
its end to make it longer, double-click to remove it; pick several and make them softer or louder, move them an octave,
repeat them, or **give them to another instrument**. Your changes play straight away; **Put it in the song** writes them
into the score exactly (checked note for note, nothing else moves), with Keep and Undo. You can change the notes in a
MIDI review, and on the project page whenever the piece waits for you (paused for your master, or delivered), never
while a session is writing it. A "How to read and change this" note under the grid explains it all in plain words.

The **Flows** page shows every flow by shape or by feel, with how each has gone for you. The project page has the MIDI panel: the arrangement as a
piano roll, **Download MIDI**, and **Import your master** when you finish a song in your own DAW. Sessions start by
themselves while `serve` runs. To use it from your phone or another computer at home, run `mortiflix-music serve --network`
(or `mortiflix-music config network true` once): it prints the addresses to open, no token, and anything on your network
can use the studio. On a network you don't trust, run `mortiflix-music serve --host 0.0.0.0` instead: you then get a
private link with an access token (put it behind HTTPS if it leaves your network).

**C. In Claude Code**

```
/plugin marketplace add GTKottman/mortiflix-music-oss
/plugin install mortiflix-music@mortiflix-music
/mortiflix-music
```

`/mortiflix-music` walks you through it in the conversation: the install, setup, the demo, a new piece's brief, running
the studio, and every gate. It turns "bar 17 is too busy" into a note at that moment and reads your notes back before
sending them. Your Claude Code conversation is the guide, not the studio: the music is still written by the studio's
own sessions, behind the same gates, and the guide never decides a gate for you. Keys never go into the chat. From a
clone, `claude --plugin-dir ./plugin` loads it without installing.

## The music engine

[Strudel](https://strudel.cc) (AGPL-3.0) is the engine: it plays the score so it can be checked, captured as MIDI and
rendered. Sounds are Strudel's built-in synths shaped in code (nothing to download, no licence questions); a sample is
used only with a source that names a licence allowing your use. Each stage is one command that refuses to pass until
its stage is right:

```sh
node $M genre        # the genre profile: 5+ sources read, the elements that make it this genre
node $M plan         # the blueprint: sections, chords, one channel per part, the story's intensity curve
node $M check        # the score: harmony lock, the curve, clean loops; captures the MIDI when it passes
node $M instruments  # a sound for every channel, from the notes it really plays
node $M audition     # every sound across its part's range: nothing silent, nothing clipping
node $M master       # render, stems, balance, -14 LUFS (or `own-master` for yours)
```

The whole method: [docs/MUSIC.md](docs/MUSIC.md).

## Backends

| Backend | What runs | Who pays |
|---|---|---|
| `claude-code` | `claude -p` in the project folder, with your login | your Claude plan |
| `anthropic-api` | the studio's own agent loop on the Claude API: a persistent shell, a file editor, web search, prompt caching, compaction for long sessions | your API key, per token |
| `demo` | a scripted stand-in that walks every gate with a placeholder blueprint, a four-bar MIDI sketch and a test tone | nobody |

Pick one with `mortiflix-music init --backend …`, `mortiflix-music config backend …`, or Settings in the web studio.
Each project shows what its Claude work cost: on a Claude plan, what it **would have cost** at API prices (you didn't
pay that); with an API key, about what you spent.

## Your keys

The studio runs on your own accounts: there's nothing to sign up for. `mortiflix-music keys` sets them (Settings ›
Keys in the web studio does the same):

```
Anthropic API key   only for the anthropic-api backend (Claude Code uses your own login)
Other keys          anything a pipeline's tools read from the environment, e.g. GEMINI_API_KEY
```

Typing is hidden, the key is checked with a free API call before it's saved, and keys are written to the studio
folder only (`secrets.json` and `session.env`, mode 600).

## Make your own pipeline

```sh
cp -r pipelines/midi-pack ~/MortiflixMusic/pipelines/my-pipeline   # the studio's own pipelines override built-ins
$EDITOR ~/MortiflixMusic/pipelines/my-pipeline/pipeline.json
mortiflix-music pipelines                                         # validates it
mortiflix-music new my-pipeline --backend demo                    # walk its gates for free
```

Read **[docs/PIPELINES.md](docs/PIPELINES.md)**. Help is most wanted here: an EP, drum programming from a reference
groove, harmonising a melody you bring, game-music loops with intensity layers, a string arrangement. If you can
describe how a good composer makes it, it can be a pipeline. `mortiflix-music pipelines export <slug>` packs one with
its skills to share.

## How it works

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md): the runner, the bridge (`mfx`), the gates, the session brief, the backends
- [docs/MUSIC.md](docs/MUSIC.md): the music engine, stage by stage, and the MIDI pack
- [docs/FLOWS.md](docs/FLOWS.md): flows, taste, the 144 techniques and how each is heard
- [docs/PIPELINES.md](docs/PIPELINES.md): the pipeline format and how to write a good one
- [harness/GATES.md](harness/GATES.md): the protocol every session follows
- [docs/SECURITY.md](docs/SECURITY.md): what a session can and can't reach, and the web server's guards
- [docs/SETUP.md](docs/SETUP.md): what setup installs and where

## Where it came from

Mortiflix Music is a sister of [Mortiflix](https://github.com/GTKottman/mortiflix-oss), the open-source motion design
studio: the same studio, gates and music engine, turned entirely toward music and MIDI. The music method comes from
Mortiflix's song system: the genre researched first, the piece planned in words, notes before sounds, and a machine
that checks each stage before the next one starts.

## License

[AGPL-3.0](LICENSE), like Mortiflix and Strudel. One person's studio, open so it can get better than one person
could make it.
