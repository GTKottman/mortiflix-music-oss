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
