<p align="center"><img src="web/img/mark.png" width="96" alt="Mortiflix Music"></p>

<h1 align="center">Mortiflix Music</h1>

<p align="center"><b>A music studio on your own machine.</b><br>
Claude writes the music as MIDI, step by step. You approve every stage.</p>

---

Most "AI music" tools are one prompt and a slot machine, and what comes out is a finished audio file you can't
change. Mortiflix Music works like a composer you brief: it researches the genre, plans the piece in words for you to
approve, writes every part as notes on its own MIDI channel, checks them, and hands you **the MIDI**: every channel,
the whole arrangement, a clip per section, and the steps to bring it into your DAW. If you want, it also gives every
channel a sound and masters the track.

It's built for **one person**: you write the briefs, you review every stage, and the studio runs on your machine.
The work is done by Claude through **your own Claude Code login or your own Anthropic API key**. Mortiflix Music is
the harness around it: the pipelines, the gates, the music engine that checks every note, the review room, and the
memory that carries a project across sessions and days.

```
 genre ──▶ blueprint ──▶ score ──▶ MIDI ──▶ sounds ──▶ final ──▶ delivered
              ▲                                          ▲
              └──────────────── you ─────────────────────┘
       approve, note a bar on the piano roll, answer questions (the session stops at every gate)
```

## Why it's built this way

- **Notes first, sounds second.** The score holds only music: pitches, rhythm, velocity and structure, one part per
  MIDI channel. Its MIDI is captured the moment the score passes its check, and only then is a sound chosen for each
  channel. A sound can never change a note, so the MIDI you take into your DAW is exactly what you heard.
- **Every stage is checked by a machine.** The engine plays the score and refuses it until every note fits the chord
  of the moment (the harmony lock), the energy rises and falls where the blueprint said it would, every part plays and
  nothing runs past the end. In a loop kit, no note may ring over a loop point.
- **Sessions end at gates.** A Claude session works until it has something for you to review, submits it, writes a
  handoff and stops. When you respond, a fresh session picks up from the journal. Waiting on you costs nothing.
- **The rules live in code, not in the prompt.** Only you can approve a step you review. A submission is refused if
  it doesn't report every error check for its kind of work, or doesn't answer each note you left on the last version.
  What you reviewed is copied out of the session's reach, so it can't change afterwards.
- **Every piece has a shape, and it's chosen.** A flow decides how the music moves (a slow burn, a rondo, a pendulum,
  theme and variations, an inverted arch…), which techniques give it its character, and what it never does. Pick
  one, or let the studio pick: the check then listens for the flow's shape and every planned technique in the notes.
- **It learns your taste.** Rate what you get (love it, fine, not for me) and the studio's picks lean toward the
  flows and techniques you love and away from what you've just had; a piece you love can become a flow of your own.
  When you point out a real mistake, the session proposes a new check ("the bass never flams with the kick").
  Approve it once and every future piece runs it.
- **Pipelines are folders.** A pipeline is a `pipeline.json` (steps, how each is reviewed, the error checks), a
  `PIPELINE.md` (the craft), and skills. Anyone can write one.

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

The web studio shows what's waiting on you, a live log of what Claude is doing, and the review room: MIDI shows as a
**piano roll** (pick a part to bring it forward; click a bar to leave a note there), audio as a player you can note a
moment on, a blueprint as paragraphs you can comment on. The **Flows** page shows every flow by shape or by feel, with how each has gone for you. The project page has the MIDI panel: the arrangement as a
piano roll, **Download MIDI**, and **Import your master** when you finish a song in your own DAW. Sessions start by
themselves while `serve` runs. To use it from another device, run `mortiflix-music serve --host 0.0.0.0`: you then get
a private link with an access token (put it behind HTTPS if it leaves your network).

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
