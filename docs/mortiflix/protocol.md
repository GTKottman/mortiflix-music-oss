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

## Setup additions (part 09)

Part 09's Setup window uses part 08's `setup.status`, `setup.backend`, `setup.done`, `setup.claude.test` and
`keys.set` / `keys.remove` / `keys.check`, plus these (service branch `mfx-service/setup`, `src/service-setup.mjs`):

| Type | Params | Result |
|---|---|---|
| `setup.backend` | also `{claudeBin?}` (`claude` or an absolute path; `backend` may then be left out) | `{backend, claudeBin}` |
| `setup.download` | `{label?, files: [{url, dest, size, sha256}]}` | `{job}`: https only, `dest` absolute; each file streams to `<dest>.part`, size + SHA-256 checked, renamed into place; files already there are skipped. Sent only after the user said yes |
| `setup.install` | `{tool}` | `{job}`: one of setup.mjs's components (`tools.installable` ids) with its own installer, after the user said yes |
| `setup.cancel` | `{job}` | `{cancelled}` |

| Event | Data |
|---|---|
| `setup.progress` | `{job, kind: download\|install, label, state: running\|done\|failed\|cancelled, line, done?, total?}` |

`setup.status` → `tools.engine` = `{ok, path}` (the sound engine). `MFX_FAKE_DOWNLOADS=1` (tests only): `fake:<bytes>`
URLs, a pretend Chrome install, and the app's model packs swapped for one small fake file each.
