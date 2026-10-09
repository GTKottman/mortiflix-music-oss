# Writing a pipeline

A pipeline is how one kind of piece gets made: the steps, which ones you review and how, what a session must check
before it shows you anything, and the craft. It's a folder:

```
my-pipeline/
  pipeline.json      the machine-readable part (below)
  PIPELINE.md        the craft: what each step does and the rules that don't bend (every session reads it)
  checklist.md       optional: a production checklist the session copies and ticks off (how it resumes)
  skills/<name>/     optional: skills only this pipeline uses (SKILL.md + scripts)
```

Built-in pipelines are in `pipelines/`; skills several pipelines share are in `pipelines/_shared/skills/`. Your own go
in your studio's `pipelines/` folder (a pipeline with the same slug as a built-in one replaces it there).

## pipeline.json

```json
{
  "slug": "game-loops",
  "name": "Game music loops",
  "description": "One line people see when they pick what to make.",
  "makes": "midi",
  "flows": "kit",
  "shared_skills": ["music"],
  "intake": [
    { "id": "game", "label": "What's the game?", "type": "long", "required": true, "help": "Shown under the field." },
    { "id": "layers", "label": "Intensity layers", "type": "choice", "choices": ["2", "3", "4"], "default": "3" },
    { "id": "theme", "label": "A theme to build on", "type": "files" }
  ],
  "steps": [
    { "key": "genre", "name": "Genre", "review": "internal", "work": ["research"] },
    { "key": "plan", "name": "Plan", "review": "document", "work": ["plan"] },
    { "key": "score", "name": "Loops and MIDI", "review": "internal", "work": ["music", "midi"] },
    { "key": "sketch", "name": "Sketch", "review": "internal", "work": ["instruments", "audio"] },
    { "key": "final", "name": "Your loops", "review": "midi", "work": ["midi", "audio"], "delivers": true }
  ],
  "status_lines": { "genre": "Researching the genre", "plan": "Planning the layers", "score": "Writing the loops" },
  "checks": [
    { "id": "layers-stack", "title": "Every layer fits on the one below", "applies_to": ["music"],
      "how": "Each intensity layer adds parts on new channels over the same chart and bars; no layer changes a note of the one below." }
  ]
}
```

`makes` is what the studio calls the result (`music`, `midi`, or anything you like). Steps use the music engine
through the shared `music` skill ([MUSIC.md](MUSIC.md)).

`flows` says which flows its pieces take: `"song"` (one piece, in sections) or `"kit"` (loops). The brief then gets a
**Flow** question and every project pins one ([FLOWS.md](FLOWS.md)); leave it out for a pipeline that doesn't plan its
music in sections.

### Intake

What you're asked when you start a project. `type`: `text`, `long`, `choice` (needs `choices`, may have `default`),
`files` (uploads land in the project's `input/<id>/`). `required: true` blocks the start until it's answered. Ask only
what the studio can't decide well on its own; a reviewed step can ask the rest, with defaults.

### Steps

| Field | Meaning |
|---|---|
| `key` | lowercase id; sessions use it with `mfx` |
| `name` | what you see |
| `review` | how you review it: `questions` (a note + questions), `document` (text, commented by paragraph), `audio` (noted at a moment), `midi` (a `.mid` as a piano roll, noted at a bar), or `internal` (no review: the session finishes it with its checks) |
| `after` | the steps it waits for. Leave it out and a step waits for the one before it; give `[]` for none. Steps with no path between them run side by side |
| `work` | what the step makes: `research`, `plan` (a written plan, like a blueprint), `music` (a score), `midi` (the MIDI pack), `instruments` (sounds for a score), `audio` (a render or a master). Decides which checks it runs |
| `delivers` | the step whose approved files are the deliverables (defaults to the last reviewed step) |
| `describe` | optional one-liner shown on the Pipelines page |

### Checks: the error checklist

Each check is a mistake you should never have to point out. `applies_to` lists the kinds of work it covers (leave it
empty for all); a step runs every check that matches its `work`. Sessions must report each one (`pass`, `fixed` with
a note, or `n/a` with a reason) or the submission is refused.

Write checks about **errors** (objectively wrong: a note against the chord, a part past the end, a note over a loop
point, clipping, the wrong tempo), never about taste (sounds, groove, harmony choices). Make `how` concrete: what to
look at and what counts as wrong. The best checks have a tool behind them (`strudel.mjs check` proves the harmony
lock; you can't eyeball LUFS).

### Status lines

The only sentences you see while a session works (`mfx status <key>`). Keep them short and plain.

## PIPELINE.md

This is the craft, and it's what makes a pipeline good. Every session reads it first. A strong one has:

1. **What it makes and for whom**, in two lines.
2. **Core rules** that don't bend (five or so). "The approved blueprint is the brief." "The owner's MIDI is kept note
   for note." Rules come from mistakes; add one every time something goes wrong twice.
3. **A table of steps**: for each, what happens and exactly what gets submitted.
4. **Which skill to read** before which step.
5. **Status line keys.**

Keep studio-mechanics out of it: `.mortiflix/GATES.md` (the gate protocol) is given to every session already.

## Skills

A skill is a folder with a `SKILL.md` (front matter `name` and `description`, then instructions) and any scripts it
needs. Claude Code loads skills from `.claude/skills/` in the project folder, which is where the studio puts them; the
API backend reads them with its editor. Good skills hold the *how*: commands that work, numbers that were measured,
the order to do things in, and the checks that prove it's right. Put tools in the skill folder and call them by path
(`node .claude/skills/music/strudel.mjs …`).

Shared skills (`_shared/skills/`, listed in `shared_skills`) are copied into each project's pinned snapshot, so a
project keeps the version it started with.

## Try it without spending anything

```sh
mortiflix-music pipelines                                   # validates every pipeline, shows errors
mortiflix-music new my-pipeline --backend demo              # the demo backend walks every gate with placeholder work
mortiflix-music run && mortiflix-music review
```

The demo checks the plumbing (steps, dependencies, review modes, checks, notes), not the craft. For the craft, run a
real session on a small brief and read its transcript (`state/<id>/sessions/*.jsonl`) and journal.

## Sharing one

```sh
mortiflix-music pipelines export my-pipeline              # my-pipeline.mortiflix-pipeline.zip: the folder + every skill it uses
mortiflix-music pipelines add my-pipeline.mortiflix-pipeline.zip   # (or an https:// link) into your studio's pipelines/
```

The zip is self-contained: shared skills are copied into its own `skills/`, the same shape a project pins. Adding one
checks every path in it, validates it, refuses to replace a studio pipeline unless you pass `--replace`, and lists the
scripts it brings: a pipeline's skills run in your sessions, so read them before you start a project with one.

## Sending one upstream

A pull request with the folder in `pipelines/` (and any new shared skill in `pipelines/_shared/skills/`). Include a
line in the README table, and say in the PR what you made with it. MIDI or audio in examples must be yours or clearly
licensed.
