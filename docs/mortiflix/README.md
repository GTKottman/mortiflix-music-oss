# Mortiflix FilmCraft: the foundation

Mortiflix FilmCraft is FilmCraft 0.8.0 plus the Mortiflix stages: **1 Script · 2 Look · 3 Shots ·
4 Voice & music · 5 FilmCraft · 6 Render** (docs 30-40 in Morti Drive ›
Design Docs › Mortiflix FilmCraft). The binary is still `filmcraft`; the window is "Mortiflix FilmCraft".

## How it fits together

- **Stages 1-4:** FilmCraft's Edit mode in the **Mortiflix** workspace: Mortiflix Chat (+ Assets tab)
  left, Preview and the stage's own panel (Mortiflix Stage) in the middle/right, Node area along the
  bottom. FilmCraft's Import/Edit/Export tabs are hidden.
- **Stage 5:** FilmCraft's Edit mode in the user's editing workspace, Mortiflix Chat docked right.
- **Stage 6:** FilmCraft's Export mode. Clicking FilmCraft's Export tab lights stage 6, Edit lights 5.
- **Stage bar:** 44 px under the header (`stagebar.rs`): Back · six stages with status · hint · Next; on
  4 Voice & music: Edit the video + **Get the video** (straight to Render).
- **Stages menu / commands:** `stages.back` Ctrl+Alt+Page Up, `stages.next` Ctrl+Alt+Page Down,
  `stages.script` … `stages.render` Ctrl+Alt+1-6 (Cmd+Option on Mac), `stages.getVideo`,
  `stages.editVideo`, `stages.set {stage}`. Ctrl+Alt+Left/Right were taken (Slip, Font Size).
  Also `mortiflix.state`, `mortiflix.newProject`, `mortiflix.setup`, `mortiflix.request {type, params}`,
  `mortiflix.reply {id}`, `mortiflix.renderVideo`, `mortiflix.showNeedsYou` (stubs where a part owns it).
  File dialogs are one pair: `mortiflix.chooseFiles {purpose, exts?, folder?, multiple?, paths?}` (an OS
  file/folder dialog; the paths wait in `state.files_in` under `purpose`, taken with
  `ctx.state.take_files(purpose)` / `take_paths(purpose)`; with `paths` no dialog opens: agents, tests) and
  `mortiflix.chooseFileAndSend {request: {type, params}, field, filter?, exts?}` (one file, then that service
  request with the path in `params[field]`). Files dropped on stages 1-4 (or on the New project window) wait
  under purpose `drop`.
- **Service:** the app runs `node <mortiflix-oss>/bin/mortiflix-service.mjs` (JSON lines, no port), see
  [protocol.md](protocol.md). Path: `MFX_SERVICE` (file or the mortiflix-oss folder) or `servicePath` in
  `<data dir>/mortiflix.json`.

## Who owns which file

All Mortiflix UI is in `crates/mortiflix-ui/src/` (layer L5, no FilmCraft dependency). Each part fills
its own file and keeps its `pub fn show(ui: &mut egui::Ui, ctx: &mut MfxCtx)`.

| Part | File | Shown in |
|---|---|---|
| 31 / 32a / 33a New project | `new_project.rs` | New project window (`mortiflix.newProject`, Back on Script) |
| 37 Chat + 32b Assets tab | `chat.rs` (`show`, `show_assets`) | Mortiflix Chat / Mortiflix Assets panels |
| 35 Node area | `nodes.rs` (`show`, `show_stage` for stage 3) | Mortiflix Nodes panel |
| 36 Preview | `preview.rs` | Mortiflix Preview panel |
| 34 / 33b Voice & music | `voice_music.rs` | Mortiflix Stage on stage 4 |
| 38 Into FilmCraft | `into_filmcraft.rs` | Mortiflix Stage on stage 5; the import hooks `stages.editVideo` |
| 39 Render / export | `export.rs` | Mortiflix Stage on stage 6; `mortiflix.renderVideo` |
| Setup | `setup.rs` | Setup window (`mortiflix.setup`, every "service not running" line) |
| Stage 1 Script / 2 Look | `script.rs` / `look.rs` | Mortiflix Stage on stages 1 / 2 |
| Foundation | `lib.rs`, `state.rs`, `stagebar.rs`, `service.rs` | shared: smallest edits only |

`MfxCtx` gives a part: `state` (`MfxState`: project id, stage, statuses, selected node, chat, `graph` /
`presets` JSON slots to type later), `service` (`request`, `take_reply`, `events_since`, `status`),
`theme` (FilmCraft's colours), `command(id, params)` (run any FilmCraft/Mortiflix command after drawing)
and `auto(id, &response, label)` (automation id; required for every interactive widget).

FilmCraft-side glue is one file, `crates/ui-egui/src/mortiflix.rs` (commands, layouts, panel dispatch,
windows). Small edits elsewhere: `dock.rs` (5 `Mfx*` PanelKinds + the "Mortiflix" layout),
`panels/mod.rs` (dispatch line), `menus.rs` (Stages menu, command chaining, route), `header.rs` (mode
tabs hidden on 1-4), `lib.rs` (field, stage bar row, frame hook, Timeline not forced into stages 1-4),
`i18n/*.tsv` (menu and panel names), `apps/filmcraft/src/main.rs` (title, activate, start service).

The Node side: `~/Documents/programs/mortiflix-oss-filmcraft`, branch `mfx-service/base`:
`bin/mortiflix-service.mjs`, `src/service.mjs` (one handler per message), `test/service.test.mjs`.

## Build, run, screenshot

```sh
cd ~/Documents/programs/mortiflix-filmcraft
export CARGO_TARGET_DIR=/mnt/storage/drive_1/mfc-targets/<part>
flock /tmp/mfc-cargo.lock cargo build -p filmcraft -j 6
flock /tmp/mfc-cargo.lock cargo test -p filmcraft-mortiflix-ui
# rustfmt/clippy: the rustup proxies have no components here; use the system ones
PATH=/usr/bin:$PATH /usr/bin/cargo-fmt --all -- --check
PATH=/usr/bin:$PATH flock /tmp/mfc-cargo.lock /usr/bin/cargo-clippy clippy -p filmcraft-mortiflix-ui --all-targets -- -D warnings
```

Run it headless (a hidden Hyprland special workspace gets no frames, so `ui.screenshot` times out;
Xvfb works and never shows on the owner's screen):

```sh
cat > run.sh <<'EOF'
#!/bin/sh
echo $$ > app.pid            # close it later with: kill $(cat app.pid)
unset WAYLAND_DISPLAY
export MFX_SERVICE=$HOME/Documents/programs/mortiflix-oss-filmcraft MORTIFLIX_STUDIO=$PWD/studio
exec $CARGO_TARGET_DIR/debug/filmcraft --control 198NN --demo --no-recover --data-dir $PWD/fcdata
EOF
xvfb-run -a -s "-screen 0 1920x1080x24" ./run.sh &
# control channel: one JSON line per request on 127.0.0.1:198NN
{"id":1,"method":"ui.resize","params":{"width":1920,"height":1080}}
{"id":2,"method":"engine.execute","params":{"command":"stages.voiceMusic"}}
{"id":3,"method":"ui.click","params":{"id":"mfx.stagebar.getVideo"}}
{"id":4,"method":"ui.screenshot","params":{"path":"/abs/path/shot.png"}}
{"id":5,"method":"engine.execute","params":{"command":"mortiflix.state"}}
```

Automation ids: `mfx.stagebar.back|next|editVideo|getVideo|<stage key>`, `mfx.stagebar.confirm.goOn|show`,
`mfx.<part>` for the stub placeholders, `mfx.service.setup`.
