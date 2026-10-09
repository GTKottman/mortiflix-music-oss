# /mortiflix-music reference

## Commands

| Read (JSON) | |
|---|---|
| `mortiflix-music list --json` | `{ studio, backend, runner: {pid, web} \| null, projects: [{ id, title, pipeline, state, label, status, needs_you, working, in_review: [step], unanswered }] }` |
| `mortiflix-music status <id> --json` | `{ id, title, pipeline, state, status, needs_you, working, steps: [{ key, name, review, state, version }], deliverables: [{ label, kind, path }], usage, cost, log: [{ t, event, step, version, details }] }` |
| `mortiflix-music review [<id>] --json` | an array, one per project waiting on the owner (below) |
| `mortiflix-music pipelines --json` | `[{ slug, name, description, steps: [{ key, name, review, after }], intake: [{ id, label, type, required, choices, default, help }] }]` |
| `mortiflix-music flows --json` | `{ buckets, feels, ratings, flows: [{ slug, name, for: song\|kit, bucket, feels, summary, shape, techniques: { required, avoid, free }, source, stats: { used, love, fine, no } \| null }] }` |
| `mortiflix-music flows show <flow> --json` | one flow with its `flow_md` |
| `mortiflix-music setup status --json` | `{ backend, backends, tools, parts, tool_info, daws }` |
| `mortiflix-music keys --json` | which keys are set and where they come from (never their values) |

| Act | |
|---|---|
| `mortiflix-music init --yes [--backend …]` | create the studio |
| `mortiflix-music new <slug> --title "…" [--flow …] --set id=value … --file id=path … --yes` | create and queue a project; `--flow` is a flow's slug, `studio` (the default), `shape:<bucket>`, `feel:<feel>` or `none` |
| `mortiflix-music rate <id> <love \| fine \| no>` | how a piece landed (the studio's flow picks follow it) |
| `mortiflix-music flows save <id> --name "…"` · `flows export <flow>` · `flows add <file \| https://…>` | a piece as a flow of the owner's own; share flows |
| `mortiflix-music run` | run sessions until everything waits on the owner (run it in the background) |
| `mortiflix-music respond <id> <step> --approve \| --changes [--note "…"]… [--overall "…"] [--answer qid=value]… [--rate love\|fine\|no]` | decide a step in review (`--version N` to name the version; default is the one in review) |
| `mortiflix-music answer <id> <qid> ["answer"]` | answer a project question (no answer takes the default) |
| `mortiflix-music reopen <id> <step> "what to change"` | send an approved step back |
| `mortiflix-music pause \| resume \| cancel <id>` | |
| `mortiflix-music checks` · `checks approve \| reject <check-id>` | checks that sessions proposed |
| `mortiflix-music setup install <tool>…` | strudel, chrome |
| `mortiflix-music setup daw <id \| none>` | the owner's DAW: ableton, fl, logic, reaper, bitwig, cubase, studioone, garageband, protools, other |
| `mortiflix-music midi <id> [--out <file>.zip]` | save the MIDI pack (every channel, the arrangement, a clip per section, the cue sheet) |
| `mortiflix-music music <id>` | where its music stands: length and tempo, the MIDI, the owner's master |
| `mortiflix-music music <id> --import <file>` | the owner's own master (wav, aiff, flac, mp3, m4a, ogg): checked against the score, then the project resumes |
| `mortiflix-music pipelines export <slug> [--out <file>.zip]` · `pipelines add <file \| https://…> [--replace]` | share a pipeline, or add someone else's (it brings scripts: read them first) |
| `mortiflix-music config [key [value]]` | e.g. `config backend claude-code`, `config sandbox true` |
| `mortiflix-music serve` | the web studio on http://127.0.0.1:4747 (also runs sessions by itself) |
| `mortiflix-music doctor` | what's installed |

The owner runs `mortiflix-music keys` in their own terminal, not you, because they type keys.

A project id can be any unique part of it (`mortiflix-music status smoke`). If only one project is live, it can be left
out.

## What `review --json` returns

```json
{
  "id": "261008-night-drive-c7b0", "title": "Night drive", "pipeline": "midi-pack",
  "state": "waiting",               // or "paused": then needs_you says why
  "needs_you": null,
  "questions": [ { "id": "q1", "step": "blueprint", "text": "…", "choices": ["…"], "default": "…" } ],
  "reviews": [ {
    "step": "final", "name": "Your MIDI", "review": "midi", "version": 2,
    "note": "what the session says it sent and what to look at",
    "pin_changes": [ { "note": 1, "change": "what it changed", "status": "done|partly|not_done" } ],
    "questions": [ { "id": "tempo", "text": "…", "choices": ["…"], "default": "…" } ],
    "items": [
      { "index": 0, "label": "Arrangement", "kind": "midi", "path": "/…/state/<id>/reviews/final/v2/files/01-night-drive-all.mid" },
      { "index": 1, "label": "Sketch", "kind": "audio", "path": "/…/state/<id>/reviews/final/v2/files/02-score.wav" },
      { "index": 2, "label": "Cue sheet", "kind": "text", "paragraphs": ["¶0 text", "¶1 text"] }
    ],
    "web": "#/p/<id>/review/final"
  } ]
}
```

Item kinds: `text`, `midi`, `audio`, `image`, `pdf`, `file`. Paths point into the studio's record (`state/`).
Those files never change after they're submitted. Open them, but don't edit them.

## Notes

Each `--note` is one note. A prefix points it at something:

| Note | Points at |
|---|---|
| `"too busy overall"` | the whole version |
| `"2: too dark"` | item 2 |
| `"1@12.5s: the hook drops out here"` | 12.5 seconds into item 1 (audio or MIDI) |
| `"0¶3: reword this"` | paragraph 3 of text item 0 |

Turn the owner's words into these. "Bar 17 of the sketch" at 96 BPM in 4/4 is `(17 - 1) × 4 × 60 / 96 = 40` seconds:
`"1@40s: …"`. Say the bar you converted when you read the notes back. For exact moments, use the web review room
(a click on the piano roll notes that bar). `--overall "…"` is one comment on the whole version. A changes
response needs at least one note or an overall comment.

Quoting: put each note in single quotes. Write an apostrophe inside one as `'\''`.

## When a project is paused (`needs_you`)

| It says | What to do |
|---|---|
| "This project needs your … key" | The owner adds it in their terminal (`mortiflix-music keys`) or Settings › Keys. Then `mortiflix-music resume <id>`. |
| "The last two sessions stopped without moving the project forward (…)" | Show the error and the last `log` events. Fix what it names if the owner agrees (a missing tool: `mortiflix-music doctor`), then resume. |
| "8 sessions in a row ended without submitting…" | The work is stuck in a loop. Show the log. A note through `reopen`, or a resume after a fix, usually gets it moving. |
| "Paused by you" | `mortiflix-music resume <id>` when they want it back. |
| "The score is approved. On the project page, MIDI › Download MIDI…" | The owner finishes the music in their DAW. Save the pack (`mortiflix-music midi <id> --out <file>.zip`), then import their master (`mortiflix-music music <id> --import <file>`). Importing resumes the project, so no `resume` is needed. |
| anything else | A session asked for something only the owner can do. Read it out, and resume once it's handled. |

## Errors and what they mean

- `another Mortiflix Music runner (pid N) is already running this studio`: `serve` or another `run` is already running
  sessions. Leave it alone, because sessions start by themselves.
- `only the owner does that: …`: the command ran inside a studio session (`MFX_TOKEN` is set). It never applies
  in the owner's own Claude Code.
- `X vN isn't waiting for review`: someone already answered it, maybe in the web studio. Read `review --json` again.
- `"…" needs an answer`: a submission question without a default. Ask the owner.
- `pin_changes …`, `error_checks …`: these refuse a session's submission. You won't see them, because they go to the
  session.
