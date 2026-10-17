# Mortiflix service protocol (v1)

The app (Rust, `crates/mortiflix-ui/src/service.rs`) runs the Mortiflix service (Node,
`mortiflix-oss/bin/mortiflix-service.mjs`, code in `src/service.mjs`) as a child process and talks
**JSON lines over its stdin/stdout**. One JSON object per line, UTF-8, `\n` at the end. No port.
Lines longer than 16 MB are dropped by both sides. The service writes nothing but protocol lines to
stdout (its logs go to stderr, which the app copies into its log).

## Envelopes

| Direction | Shape |
|---|---|
| app → service: request | `{"id": 7, "type": "stage.set", "params": {...}}` |
| service → app: reply | `{"id": 7, "ok": true, "result": {...}}` or `{"id": 7, "ok": false, "error": {"code": "bad_request", "message": "..."}}` |
| service → app: event | `{"event": "status", "data": {...}}` |

- `id` is a positive integer the app picks; every request gets exactly one reply. Id `0` is the
  supervisor's own `hello`.
- A reply with `"id": null` answers a line the service could not read (`bad_json`, `bad_request`).
- Error codes: `bad_json`, `bad_request`, `unknown_type`, `no_project`, `failed`, and from the app side
  `not_running` (the app answers itself while the service is down, so nothing waits forever).
- Unknown fields are ignored on both sides. Every value from the other side is hostile input.

## Life cycle

1. The app starts `node <service>` (path from `MFX_SERVICE`, else `servicePath` in
   `<data dir>/mortiflix.json`; Node from `MFX_NODE`, else `nodePath`, else `node` on the PATH).
2. The app sends `hello` (id 0). The reply turns the app's status to *connected*.
3. The service exits when its stdin closes (the app quit or restarts it).
4. If the service dies, the app restarts it with backoff (1 s, doubling, max 30 s; reset after 60 s up).
   A missing Node or service shows "Mortiflix service not running" with a Setup link, never a crash.

## Messages

| Type | Params | Result | Notes |
|---|---|---|---|
| `hello` | `{client, version, protocol}` | `{service, version, protocol, studio}` | then a `status` event |
| `state.get` | `{}` | `{project, stage, stages, line}` | |
| `project.list` | `{}` | `[{id, title, state, pipeline}]` | |
| `project.new` | `{pipeline?, title?, answers?}` | `{project}` | `createProject`; part 31 extends (preset, assets, music) |
| `project.open` | `{id}` | `{project}` | `loadProject` |
| `stage.set` | `{stage: 0-5, key?, from?, done?}` | `{stage, key}` | `done: true` marks `from` done (Next) |
| `chat.send` | `{text}` | `{queued: true}` | the user's message and the AI's answers come back as `chat.message` events (part 37) |
| `node.list` | `{project?}` | `{nodes: [...], open_edits}` | `canvasView`; current project when `project` is left out |
| `node.edit` | `{node, action: ok\|fix\|cut\|clear, text?, bars?, part?, project?}` | the canvas `nodeAction` result | OK / Fix / Cut it (doc 35) |

Stage keys: `script`, `look`, `shots`, `voiceMusic`, `filmcraft`, `render` (index 0-5).

## Events

| Event | Data | App does |
|---|---|---|
| `status` | `{line?, stage?, project?, stages?: [StageStatus × 6]}` | stage bar statuses, the AI's live line in the hint, `state.projectId` |
| `chat.message` | `{role: user\|ai\|system, text, at?}` | appends to `state.chat` (bounded to 2000) |

`StageStatus` = `{kind: notStarted|making|ready|needsYou|done|stopped, done, total, need, text?}`.

## Extending it (parts)

- Add a message: a handler in `src/service.mjs` (`handlers['x.y']`), the sender in your Rust module
  (`ctx.service.request("x.y", params)` returns the id; `ctx.service.take_reply(id)` later), and a row
  above, in the same commit. Use your part's prefix (`voice.*`, `music.*`, `preview.*`, `assets.*`,
  `export.*`, `setup.*`).
- Events the shared state doesn't apply are kept for the parts: `ctx.service.events_since(seq)`
  (newest 512, each with a sequence number).
- The control channel reaches all of it: `engine.execute {command: "mortiflix.request", params: {type,
  params}}` → `{id}`, then `mortiflix.reply {id}`; `mortiflix.state` returns the shared state and the
  service status.

## Service core messages (part 08)

Every message below is implemented in the Node service (`mortiflix-oss`, branch `mfx-service/integration`: `src/service.mjs`
over `src/svc/*.mjs`; full reference with the engine changes: `docs/service-protocol.md` there). Parts send them with
`ctx.service.request(type, params)`; events the shared state doesn't apply come through `ctx.service.events_since`.

- `project` in any params is optional: left out, the open project is used (`no_project` when none is open).
- Node ids: `b01` beats, `sf01` looks, `s01` shots, `t01` transitions, `b01-1` voice lines, `m01` music cues, `music`
  the mix; assets `a01`. Ids are forever.
- Paths in results are absolute (media to import or play). Paths the app sends (assets, recordings, renders) are files
  the person picked; the service copies them into the project.
- The service runs the studio's runner (the AI works ahead while the app is open); `hello` returns `runner` and
  `types` (every type it answers). No paid API calls: the AI is the user's own Claude Code; tests use the demo backend.
- Extra fields the foundation doesn't read yet: `status` also carries `state`, `needs_you`, `title`; `StageStatus`
  also `text`, `key`, `present` (false = the preset has no step there: hide the stage), `steps`; `chat.message` rows
  also `id`, `context`, `card`, `refs`, `edit`, `question`, `stage`, and roles `action` (grey button lines) and
  `stage` (dividers).

### Requests

#### Projects and stages (docs 30, 31)

| Type | Params | Result |
|---|---|---|
| `hello` | `{client, version, protocol}` | `{service, version, protocol, studio, runner, runner_note, types[]}`, then a `status` event |
| `state.get` | `{}` | `{project, stage, stages, line, context}` |
| `project.list` | `{}` | `[{id, title, state, pipeline, created_at, updated_at, stage, delivered}]` |
| `project.new` | `{preset, title, idea, answers?, assets?: [{path, use, where, note}], music?: {theme}, start?: true, backend?: "demo"}` | `{project, assets_pending}`; the preset's defaults fill the answers, `idea` is its first long answer; assets copy in the background, then the project starts (`start: false` keeps it a draft) |
| `project.open` | `{id}` | `{project, stage}` (the last stage looked at) |
| `project.close` | `{}` | `{closed}` |
| `project.start` / `.pause` / `.resume` / `.cancel` | `{}` | |
| `project.events` | `{limit?}` | the project's log rows |
| `stage.set` | `{stage: 0-5 \| key, from?, done?}` | `{stage, key, kept[], left[], handover?}`. `done: true` = the owner's Next on `from`: steps in review are approved as they are (pieces OK'd, questions take their defaults). From 3 to 4 (Edit the video) or 5 (Get the video), the session's build/final steps are skipped: the app builds and renders the edit |
| `stage.status` | `{}` | `[StageStatus × 6]` (also in every `status` event) |
| `stage.remakes` | `{stage}` | `{remade: [{step, name, stage, pieces}]}`: what is made again if that stage changes (the "going back" pop-up) |
| `stage.reopen` | `{stage, text}` | reopens that stage's last approved step with the owner's words |
| `stage.handover` | `{how: edit \| get}` | `{how, skipped[]}` (what `stage.set` does from 3) |

`StageStatus` = `{kind: notStarted|making|ready|needsYou|done|stopped, done, total, need, text, key, present, steps[]}`.
`present: false` = the preset has no step in that stage (hide it). `need` counts pieces waiting for an OK, open
questions and failed remakes.

#### Presets (doc 31)

| Type | Params | Result |
|---|---|---|
| `presets.list` | `{}` | `{built_in: [Preset], yours: [Preset]}`; Preset = `{slug, name, description, source, makes, based_on, length, aspect, voice, music, idea_field, stages[], steps: [{key, name, review, stage, off, waits, always}], error?}` |
| `presets.get` | `{slug}` | Preset + `{intake, skills[], look, voice_preset, music_theme, editable}` |
| `presets.save` | `{from, name, description?, defaults?, steps?: {key: {on, waits}}, skills?: [names kept], skill_folders?: [paths], look?: {words, frames: 3-5, pictures: [paths]}, voice?: {name, delivery}, music_theme?, replace?}` | `{slug, name, preset}`. A built-in is never changed (saving makes your copy). Brief, Script and the final always run |
| `presets.delete` | `{slug}` | your own presets only |
| `voice.list` | `{}` | `{engine, engines, presets, elevenlabs_key, local}` (Settings › Narration) |

#### Assets (doc 32)

| Type | Params | Result |
|---|---|---|
| `assets.list` | `{}` | `{yours: [Asset], found: [...], removed: [...]}`; Asset = `{id, name, kind, size, use: must\|fits\|reference, where: [beats]\|null, note, path, version, versions[], used_in: [{node, how}]}` |
| `assets.add` | `{paths: [...]}` or `{assets: [{path, use, where, note}]}` | `{queued}`; then `assets.progress` / `assets.added` / `assets.failed` events. Works at any stage |
| `assets.update` | `{asset, use?, where?, note?}` | `{asset}` |
| `assets.replace` | `{asset, path}` | `{queued}`; new version, the old one kept |
| `assets.remove` | `{asset}` | hidden, the file kept |
| `assets.use` | `{asset, node, how?, remove?, fix?: true}` | drag onto a piece: recorded as used there, and (unless `fix: false`) a Fix asks the AI to put it in |

#### Music theme and cues (doc 33)

| Type | Params | Result |
|---|---|---|
| `music.theme.get` | `{}` | `{theme}` (`music/theme.json`, doc 33's shape) |
| `music.theme.set` | `{theme}` | the owner's theme (`picked_by: you`) |
| `music.theme.samples` | `{theme?, project?}` | `{samples: [{id, name, desc, strudel, wav, bpm, key}], made, using}`; demo: three quiet 15 s placeholders at once (no project needed, for New project › Choose…); a real backend asks the next session (`made: false`, they arrive as a chat card) |
| `music.theme.use` | `{sample}` | picks one of the samples |
| `music.theme.check` | `{theme}` | the theme cleaned as it would be saved |
| `music.cues` | `{}` | one cue per beat: the plan before the score, the sections after (`scored`) |
| `music.cue.set` | `{cue, feel?, energy?, text?, bars?, part?}` | before the score: `{plan}`; once scored: a Fix on that cue, `{edit}` |

#### Script (stage 1, doc 11 §9.1)

| Type | Params | Result |
|---|---|---|
| `script.get` | `{}` | `{project, rev, writing, version, beats: [{id, no, name, name_auto, colour, visual, target_sec, state, lines: [{id, words, changed, changed_at, read_only, remaking: {shots, lines}, feeds: [{node, kind: line\|shot, label, state, current, made, remaking}]}], deleted: [{id, words, after, changed_at}]}]}`. Line ids = voice line ids (`b01-1`), forever; `feeds` = the line's voice line and its beat's shots (the graph's `says` edges); while `writing`, the newest line is `read_only` |
| `script.edit` | one edit or `{edits: [edit…]}`; edit = `{beat, beat_name}` \| `{beat, line, words}` \| `{beat, add_after: line id \| key \| null, words, key?}` \| `{beat, line, delete: true \| false}` | `{project, beats, changed[], added[], keys: {key: id}, deleted[], restored[], renamed[], remaking: [{node, kind, edit}]}` (also the `script.changed` event) |

The app (`crates/mortiflix-ui/src/script.rs`): the Script panel and the chat's script card share one editor and one
draft. Click a line: it becomes a text field in place; Enter or clicking away keeps it in the draft, Esc drops it.
Kept changes are marked until **Save** (or Ctrl/Cmd+S) sends them all in one `script.edit {edits}`; **Discard** drops
them; a stage command that would leave with a draft asks first (Save and go on / Discard / Keep editing). The service
writes them into `script/beats.json` and `voice/lines.json` and remakes only what used the changed words that already
exists (a Fix each: the line's voice line and its beat's shots; versions kept); a deleted line's voice line is cut,
`delete: false` brings it back. A delete stays undoable after Save: the line shows struck through in its place
("deleted · Undo", from the beat's `deleted` list) and Undo or Ctrl/Cmd+Z in the Script panel (`script.undo`,
panel-scoped) sends `delete: false`: the same line, words, links and voice line (its OK too), nothing remade. The panel reads the script again on `script.changed` / `node.changed` and shows
"changed · remaking 1 shot and 1 voice line" until the remakes are in. The chat card's **Change the words** opens
stage 1 with the first line's field open.

#### Voice & music stage (doc 34)

| Type | Params | Result |
|---|---|---|
| `voicemusic.view` | `{}` | `{shots, voice: [{node, words, path, start_sec, dur_sec, gain_db, voice, timing_locked, takes[]}], cues, music: {path}, bpm, beats_per_bar, length_sec}` |
| `voice.lines` | `{}` | just `voice` of the above |
| `mix.set` | `{node, gain_db?, nudge_sec?, start_sec?: n\|null, voice?, duck_db?, len_bars?}` | `{node, mix, edit}`; a new voice or length is also a Fix |
| `voice.line.words` | `{line, text}` | Remake line with new words (a Fix) |
| `voice.line.take` | `{line, text?}` | New take (a Fix) |
| `voice.booth.status` | `{}` | the booth's lines and takes |
| `voice.booth.import` | `{line, path, keep?: true, source?: booth}` | the app's own recording or a picked file as a take (any format via ffmpeg), kept so it plays |
| `voice.booth.keep` | `{line, take}` | |

#### Node area (doc 35)

| Type | Params | Result |
|---|---|---|
| `graph.get` | `{}` | `{rev, fps, order: {beats, shots, frames, lines, cues, transitions, film}, beats: [{id, no, title, colour}], nodes: [...], edges: [{from, to, kind: next\|says\|look\|uses\|under}], assets, open_edits, problems}`; each node: `id, kind, label, title, beat, beat_no, colour, state, owner, current, plays, versions[{v, path, from_sec, to_sec, still}], pending, failed, edits, pos, default_pos, mix, uses`. `still` = a PNG of a shot version's first frame (null until made; a picture version is its own still); `assets[]` carry `pos` |
| `node.list` | `{}` | the foundation's flat list (kept) |
| `node.edit` | `{node, action: ok\|fix\|cut\|clear, text?, bars?, part?}` | OK / Fix / Cut it; every press is a grey line in the chat |
| `node.move` | `{positions: {id: [x, y] \| null}}` | `{moved}`: free placement, saved per project |
| `node.reorder` | `{order: [shot ids]}` | `{film}`: reorder = rewire the chain; nothing is remade |
| `node.pick` | `{node, v \| null}` | `{node, plays}`: play an older version (null = newest) |
| `node.duplicate` | `{node, text?}` | `{edit, node}`: a new shot like it, right after it |
| `node.insert` | `{after, text}` | `{edit, node}`: a new shot after `after` |
| `node.find` | `{query}` | matching nodes |

#### Loose preview (doc 36)

| Type | Params | Result |
|---|---|---|
| `preview.plan` | `{scope?: all\|from\|shot\|beat, node?}` | `{rev, range: [from, to], length_sec, fps, shots: [{node, start_sec, dur_sec, version, colour, beat_no, media: {kind: video\|still\|card, path, from_sec, to_sec}}], voice[], music, cues[]}`: best media per shot (its piece of the animatic, else its look's still, else a beat card), in film order |

#### Into FilmCraft (doc 38)

| Type | Params | Result |
|---|---|---|
| `handoff.plan` | `{}` | `{fcproj, sequence: {name, width, height, fps, length_sec}, tracks, bins, clips: [{node, kind, track, bin, name, label, version, start_sec, dur_sec, media}], markers: [{kind: chapter\|comment, time_sec, name, label, comment}], captions, your_assets, older_versions, cut_on_board, notes, previews}`; `fcproj` = where the app saves the FilmCraft project (`<studio>/state/<id>/filmcraft/<title>.fcproj`, or the one saved before); transitions carry `native` + the FilmCraft transition name |
| `handoff.done` | `{map: {clips: [{node, version, clip, item}], fcproj?, rev?}}` | saves what the app imported (done once) |
| `handoff.get` | `{}` | `{map}` |
| `handoff.swaps` | `{}` | `{imported, swaps: [{node, version, clip, item, path, from_sec, to_sec}]}`: newer versions to swap in (`clip.replaceFromBin`) |
| `handoff.swapped` | `{node, version, clip?, item?}` | |

#### Chat (doc 37)

| Type | Params | Result |
|---|---|---|
| `chat.send` | `{text, context?: node id \| "video", confirm?}` | `{queued, move?}`. In a node context the text is a Fix there (an OK'd piece asks first; send again with `confirm: true` for "Remake it"). In the whole video the user's Claude Code answers (`chat.typing`, then `chat.message`) and may make one move (a Fix or an answer) |
| `chat.history` | `{limit?, before?}` | rows `{id, at, role: user\|ai\|system\|action\|stage, text, context?, card?, refs?, edit?, question?, stage?}` |
| `chat.action` | `{text, context?, refs?}` | a grey action line |
| `chat.context` | `{context}` | the "talking about" target, echoed as a `chat.context` event |
| `chat.answer` | `{question, value}` | answers an open question |
| `questions.list` | `{}` | the project's questions |
| `activity.list` | `{limit?}` | the session's recent activity (Details) |

Cards: `script {beats}`, `looks {frames, using}`, `shots`/`transitions {nodes}`, `submission {items, questions}`,
`question {id, choices, default}`, `defaults {questions}`, `versions {edit, refs}`, `themes`, `confirm {node, text}`.

#### Render queue and export (doc 39)

| Type | Params | Result |
|---|---|---|
| `render.list` | `{project?}` | every job in the one queue (session renders and app exports) |
| `render.add` | `{label, kind?: export}` | a job for the app's own FilmCraft export; start it when its `render.job` event says `running` |
| `render.progress` | `{id, done, total, phase?}` | |
| `render.finish` | `{id, ok, file?, error?}` | |
| `render.cancel` | `{id}` | waiting: never starts; running: stopped (an app job gets `state: failed, cancelled: true`) |
| `export.target` | `{ext?, name?, stems?, create?}` | `{dir, file, name, reveal}`: `<Videos>/Mortiflix/<Project>/`, never over an existing file |
| `export.done` | `{file, preset?, timeline_rev?}` | copies the render into the project's record and delivers the project |
| `publish.plan` | `{to, profile?, title?, description?, at?, timezone?}` | what would be posted |
| `publish.send` | same + `confirm: true` | posts (only with the owner's confirm; refused with `by: "ai"`) |

#### Setup (part 09)

| Type | Params | Result |
|---|---|---|
| `setup.status` | `{}` | `{backend, backends, claude: {found, path, version, logged_in, auth_method, subscription, login_command, detail}, keys, voice, tools: {node, ffmpeg, ffprobe, installable, studio}, first_run}` |
| `setup.backend` | `{backend?: claude-code\|anthropic-api\|demo, claudeBin?}` (`claude` or an absolute path; `backend` may then be left out) | `{backend, claudeBin}` |
| `setup.done` | `{done?: true}` | first run finished |
| `setup.claude.test` | `{}` | `{ok, reply, seconds}`: one short `claude -p` with no tools |
| `keys.list` | `{}` | `[{id, name, for, get, set, source}]`, never the values |
| `keys.set` / `keys.remove` | `{id, value}` / `{id}` | `anthropic`, `elevenlabs`, `uploadpost`, `postiz` (secrets.json) and `gemini` (session.env) |
| `keys.check` | `{id}` | a free check of a saved key (network, only on request) |
| `setup.download` | `{label?, files: [{url, dest, size, sha256}]}` | `{job}`: https only, `dest` absolute; each file streams to `<dest>.part`, size + SHA-256 checked, renamed into place; files already there are skipped. Sent only after the user said yes |
| `setup.install` | `{tool}` | `{job}`: one of setup.mjs's components (`tools.installable` ids) with its own installer, after the user said yes |
| `setup.cancel` | `{job}` | `{cancelled}` |
| `components.status` | `{}` | `{components: [{id, name, for, what, where, size, default, needs, installed, detail, path}], installing}`: the installer's components; ComfyUI counts as `installed` only when it is the studio's own (`tools/ComfyUI`) |
| `components.install` | `{ids?: [id] \| "default" \| "all"}` | `{started}`, then `components.progress` events and one `components.done` |

`setup.status` → `tools.engine` = `{ok, path}` (the sound engine). `MFX_FAKE_DOWNLOADS=1` (tests only): `fake:<bytes>`
URLs, a pretend Chrome install, and the app's model packs swapped for one small fake file each.

### Events

| Event | Data |
|---|---|
| `status` | `{line, stage, project, stages: [StageStatus × 6], state, needs_you, title}`: sent when anything in it changes |
| `project.changed` | `{id, state, needs_you, deliverables}` |
| `chat.message` | a chat row (see `chat.history`) |
| `chat.typing` | `{on}` while the AI writes a reply |
| `chat.context` | `{context}` |
| `script.changed` | `{project, beats, changed, added, keys, deleted, restored, renamed, remaking}`: script edits were saved (the `script.edit` result) |
| `node.changed` | `{project, rev, nodes: [{id, kind, current, state, owner, missing}]}` (`all: true` on the first look at a project) |
| `activity` | `{project, t, kind: text\|tool\|todo\|info\|error, text, todos?}` |
| `assets.progress` | `{project, asset, name, done, total}` |
| `assets.added` / `assets.failed` | `{project, asset, replaced?}` / `{project, path?, asset?, message}` |
| `setup.progress` | `{job, kind: download\|install, label, state: running\|done\|failed\|cancelled, line, done?, total?}` |
| `render.job` | a queue job `{id, label, state: waiting\|running\|done\|failed, project, external?, kind?, progress?, file?, error?, cancelled?}` |

### Engine changes behind it (Node side)

- `pipelines.mjs`: a step can be `off` (skipped at start) and name its `stage`; presets carry `look`, `voice`,
  `music_theme`, `based_on`.
- `canvas.mjs` `requestNewPiece`: an edit request for a new shot (duplicate / insert).
- `renderq.mjs`: app jobs (`addExternal`, `report`, `finish`), `cancel`, `list`.
- `torch.mjs`: the session's brief gets the owner's assets, the music theme, the preset's look and voice, and the
  chat since the last session (`svc/brief.mjs`).
- `backends/demo.mjs`: voice lines with clips and a quiet score (so every lane has something), new shots, line remakes.
- Policies in the service: questions never block (a questions step whose questions all have defaults goes on with
  them; `config.mfxQuestionsBlock: true` turns that off), and a stage the owner moved on from keeps what arrives.

## App-side commands (for agents and the control channel)

Not service messages, but how the parts reach the host: `mortiflix.request {type, params}` / `mortiflix.reply {id}`
(any message above), `mortiflix.chooseFiles {purpose, exts?, folder?, multiple?, paths?}` (OS dialog; files wait in
`state.files_in` under `purpose`; dropped files on stages 1-4 or on New project come as purpose `drop`),
`mortiflix.chooseFileAndSend {request: {type, params}, field, filter?, exts?}` (one file, then that request with the
path in `params[field]`), `mortiflix.reveal {path, open?}`, `mortiflix.preview.*` (doc 36), `nodes.*` (doc 35),
`mortiflix.importEdit` / `mortiflix.import.run {open_edit?}` (doc 38; Get the video runs `import.run {open_edit:
false}` before rendering), `mortiflix.renderVideo` (doc 39).
