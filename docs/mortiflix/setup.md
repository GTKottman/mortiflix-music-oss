# Mortiflix FilmCraft: first-run setup (part 09)

On the first launch the app opens the **Setup** window by itself ("Welcome to Mortiflix FilmCraft"). Later it opens
from **Edit ▸ Preferences ▸ Mortiflix…** (`mortiflix.setup`) and from every "Mortiflix service not running" line.
Every step can be skipped: FilmCraft's own editing works without any of it.

Code: `crates/mortiflix-ui/src/setup.rs` (the window), `crates/ui-egui/src/mortiflix.rs` (`model_packs`: FilmCraft's
Whisper and Kokoro catalogues), one line in `apps/filmcraft/src/main.rs` (`setup::init`). Service side:
part 08's `src/svc/setup.mjs` plus `src/service-setup.mjs` (downloads). Messages: [protocol.md](protocol.md).

## The three steps

| Step | What it does |
|---|---|
| 1 Claude Code | Finds `claude` (the linked path, the PATH, then `~/.local/bin`, `~/.claude/local`, npm and Homebrew folders: a desktop app's PATH is often shorter than the shell's), shows its version and whether it is signed in (`claude auth status --json`). **Link Claude Code** runs `claude auth login` inside the window: its output is shown, **Open the sign-in page** opens the address it prints, and a field sends the code from the browser back. **Sign in from a terminal instead** opens the user's terminal with the same command. **Send a test message** asks the service (`setup.claude.test`) to send one short `claude -p`, the way the chat will. The app tells the service where `claude` is (`setup.backend {claudeBin}`). Under a fold: an Anthropic API key and the choice Claude Code / Claude API (`setup.backend {backend}`), as a fallback only. |
| 2 Keys | ElevenLabs, Gemini, Upload-Post, Postiz: what each is for and where to get it. **Save and check** = `keys.set`, then `keys.check` (a free read-only call). The service keeps them in the studio (`secrets.json`, `session.env` for Gemini; mode 600). The app never gets a key back and never writes one into a project. |
| 3 Tools | The Mortiflix service and its Node (with a field for where the service is when it isn't found), ffmpeg, then every **component** the installers offer (setup.mjs `TOOLS`: Chrome, Strudel, transitions, Blender and the 3D toolkits, browser-harness, ComfyUI, the sound engine, the Craft apps) as Installed / Not installed / Install failed, with **Install…** / **Try again…** for the ones skipped: `setup.install` runs setup.mjs's own installer (no second install logic). Then Whisper (captions) and Natural voices (Kokoro). Downloads **ask first** (size, where from, licence, where it goes), then run as service jobs with a progress bar and Cancel (`setup.download`, `setup.install`, `setup.progress`). Model files are checked against FilmCraft's pinned sizes and SHA-256 and land in `<data dir>/models`, where FilmCraft looks for them. A model this build can't use (built without `whisper` / `neural-voices`) says so and offers no download. |

**Finish** or **Skip setup for now** closes it and tells the service (`setup.done`).

## Files

- `<data dir>/mortiflix-setup.json`: `{welcomeDone, claudePath}`. The Welcome opens by itself once; an explicit
  `--demo` run (tests) never opens it.
- `<data dir>/mortiflix.json`: the service path (`ServiceSettings`), written by the Tools step's **Use this**.

## Testing

- `cargo test -p filmcraft-mortiflix-ui setup`: settings, hostile `auth status` output, ANSI/URL parsing, finding
  `claude`, model packs that can't write outside their folder, the whole link flow with a fake `claude` (wrong code,
  cancel, hanging command), the Welcome once, and the window drawing with no service.
- Service: `node --test test/service-setup.test.mjs` (downloads, checksums, cancel, hostile requests).
- In the app: a temporary `HOME`, a fake `claude` first on the PATH, `MORTIFLIX_STUDIO` and `FILMCRAFT_DATA_DIR` in
  a temp folder, `MFX_FAKE_DOWNLOADS=1`, run without `--demo`. Never with the real login or real keys.
