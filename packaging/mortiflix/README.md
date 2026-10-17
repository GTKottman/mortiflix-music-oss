# Mortiflix FilmCraft packaging

Every Mortiflix FilmCraft package carries the app, `filmcraft-cli` and the **Mortiflix bundle**: the
Mortiflix service with its own Node runtime, ffmpeg and the sound engine. Someone new installs one
file and the app works; Claude Code is not bundled (first-run setup links it).

## The bundle

`packaging/mortiflix/bundle.sh --os linux|windows|macos --arch x64|arm64 --out DIR` makes it:

| Folder | What |
|---|---|
| `service/` | the Mortiflix service (`mortiflix-oss`: `bin/mortiflix-service.mjs`, `src`, `pipelines`, `harness`, `web`, `vendor`, `docs`, `plugin`, `engine` source) from a git ref, with its production `node_modules` (`npm ci --omit=dev`) |
| `node/` | Node.js (default 24 LTS), checked against nodejs.org's `SHASUMS256.txt` |
| `bin/` | `ffmpeg`, `ffprobe` (Linux/Windows: BtbN GPL builds, checked against the release's `checksums.sha256`; macOS: martin-riedl.de builds, pin with `FFMPEG_SHA256`) and `mortiflix-engine` |
| `LICENSES/` | Node, ffmpeg, the service (AGPL-3.0) and the engine |
| `BUNDLE.json` | what went in: versions, service commit, ffmpeg URL, engine version |

Env: `MFX_SERVICE_SRC` (the mortiflix-oss checkout; default `../mortiflix-oss-filmcraft`),
`MFX_SERVICE_REF` (default `HEAD`; `worktree` = the files as they are), `NODE_VERSION`,
`MFX_ENGINE_BIN` (a built engine; or `--build-engine` builds one with the service's own
`mortiflix setup daw-engine --build`), `FFMPEG_URL` / `FFMPEG_SHA256`, `MFX_CACHE` (downloads).
When the bundle is for this machine, it ends by sending the service `hello` with the bundled Node.

## Where the app finds it

`crates/mortiflix-ui/src/service.rs` (`bundled_dir`), after `MFX_SERVICE` and the `servicePath` in
`<data dir>/mortiflix.json`:

| OS | Binary | Bundle |
|---|---|---|
| Linux (tarball, deb, rpm, AppImage, Flatpak) | `<prefix>/bin/filmcraft` | `<prefix>/lib/mortiflix-filmcraft` |
| Windows (MSI, portable zip) | `<install dir>\filmcraft.exe` | `<install dir>\mortiflix` |
| macOS | `FilmCraft.app/Contents/MacOS/FilmCraft` | `FilmCraft.app/Contents/Resources/mortiflix` |

`MFX_BUNDLE` points at a bundle anywhere. The bundled Node is used unless `MFX_NODE` or `nodePath` says
otherwise. The service starts with the bundle's `bin/` and Node first on its `PATH` (so `ffmpeg`,
`ffprobe`, `node`, `npx` are the bundled ones), `MFX_BUNDLE` set, and `MFX_DAW_ENGINE` naming the
bundled engine unless the user set one.

## Per OS

- **Linux:** `packaging/linux/package.sh [--formats "tar deb rpm appimage"] [--no-mortiflix]`
  stages the bundle at `usr/lib/mortiflix-filmcraft` for every format; `install.sh` copies
  `lib/mortiflix-filmcraft` fresh on each install. Flatpak: `flatpak-bundle.sh` repackages the
  tarball (`ai.storyteller.filmcraft.bundle.yml` installs the bundle to `/app/lib`; both manifests
  now grant `--share=network`, which the AI needs).
- **Windows:** `pwsh packaging/windows/package.ps1 -Arch x64|arm64 [-NoMortiflix]` runs `bundle.sh`
  under Git Bash into `mortiflix\`, signs its `.exe` files with `sign.ps1`, and the MSI harvests the
  folder with WiX v5's `<Files>` (`-d MfxDir=...`). The portable zip carries it too. No x86 bundle.
- **macOS:** `packaging/macos/package.sh [--arch universal|aarch64|x86_64] [--no-mortiflix]` puts it
  in `Contents/Resources/mortiflix`; universal builds `lipo` each tool from its arm64 and x86_64
  builds (`MFX_ENGINE_BIN_X64` for the engine). Every Mach-O is signed with the hardened runtime,
  Node with `node-entitlements.plist` (JIT), so notarization covers it.
- **CI:** `.github/workflows/mortiflix-release.yml` (run by hand) builds all three, optionally the
  sound engine per OS, installs the MSI and checks the service answers, mounts the DMG and checks
  the same, and runs `test_install.py` on Linux. Signing secrets are the same as FilmCraft's; without
  them the builds are unsigned (Windows) / ad-hoc (macOS).

## The Mortiflix tools (components step)

Every installer also offers the tools Mortiflix uses, downloaded during the install for the user who installs
(into their own studio, `~/Mortiflix`; never system-wide, never the user's own Blender profile):

| Component | For | Size | Default |
|---|---|---|---|
| `blender` | 3D shots (newest official build, checked against Blender's sha256 list; an existing Blender is reused) | ~350 MB | on |
| `blender-addons` | MoBlend, Nova FX, Camera, Animate, Math, Circuits, Camera Flight in the studio's own Blender profile | ~15 MB | on |
| `chrome` | headless Chrome for Strudel and Remotion (an existing Chrome/Chromium is reused) | ~100 MB | on |
| `strudel` | the music step (`@strudel/web` from npm) | ~20 MB | on |
| `transitions` | the remotion-transitions library | ~5 MB | on |
| `uv` | Python tools (browser-harness, ComfyUI) | ~40 MB | on |
| `comfyui` | ComfyUI + Qwen3-TTS local voice, NVIDIA only | ~10 GB | off |

No install logic lives here: each is one of the service's own installers (`src/setup.mjs`), run by
`service/bin/mortiflix-components.mjs` (`list [--json]`, `install default|all|none|id,...`, `choose`) with the
bundled Node; `components.sh` (Linux/macOS) and `components.cmd` (Windows) put the bundle's tools first on PATH.
A failed or skipped tool never fails the install, and Mortiflix setup can install it later.

- **Linux tarball:** `install.sh --components default|all|none|id,...` (or `MORTIFLIX_COMPONENTS`); with a
  terminal and no flag it asks one by one; with neither it skips and says how. Under sudo the tools go to
  `$SUDO_USER`. `bin/mortiflix-components` links to the picker.
- **.deb/.rpm:** the post-install links `/usr/bin/mortiflix-components` and, on a fresh install from a
  terminal under sudo, runs the picker as `$SUDO_USER`; otherwise it prints how to run it.
- **AppImage:** `AppRun` asks once on a first run from a terminal (or installs `MORTIFLIX_COMPONENTS`); started
  from a desktop, the app's first-run setup offers them.
- **Windows MSI:** a "Mortiflix tools" feature tree (WixUI_FeatureTree, ComfyUI unticked); the ticked ones are
  installed at the end by `components.cmd` through WixQuietExec, deferred and impersonated (the installing user,
  not SYSTEM), `Return="ignore"`; output in the MSI log. Untested on Windows.
- **macOS:** the new `.pkg` has the tools as Customize choices; each runs `macos-tool-postinstall.sh` as the
  logged-in user (log: `~/Library/Logs/Mortiflix tools.log`). Untested on a Mac.
- **First-run setup (app):** the service's `components.status` (each tool: name, for, size, default, needs,
  installed, detail) and `components.install {ids}` (progress as `components.progress` events, then
  `components.done`).

Checksums: Blender against its release sha256 list; Nova FX pinned to a commit and its archive's sha256; Strudel
through npm's registry integrity hashes; uv through astral.sh's installer. The other toolkits, transitions and
Chrome for Testing come over HTTPS from their GitHub/Google sources without a published checksum.

## Proving a fresh install

`xvfb-run -a -s "-screen 0 1920x1080x24" python3 packaging/mortiflix/test_install.py dist/release/filmcraft-<v>-linux-x86_64.tar.gz --shots DIR`
extracts the tarball, installs it into a temporary prefix with `install.sh`, starts the installed app
as a new user (temporary `HOME`/XDG folders, no `MFX_*`) on the control channel, waits for the
bundled service to connect, checks the first-run setup window is on screen and takes screenshots.
`--setup-optional` opens setup with `mortiflix.setup` when the build does not open it by itself yet.

## Licences

The app stays MIT/Apache-2.0. The bundle is separate programs it starts as child processes, each with
its own licence (`LICENSES/README.txt`): the service and engine AGPL-3.0 (their source ships in
`service/`), ffmpeg GPL-3.0, Node MIT.
