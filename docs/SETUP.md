# Setup

`mortiflix-music setup` (or **Settings › Setup** in the web studio) walks through what the studio uses, says why each
part is needed and what it would install, and asks before installing anything. `mortiflix-music setup <part>` does one
part again; `mortiflix-music setup status` shows where things stand.

Everything installs **into the studio folder** (`~/MortiflixMusic/tools/`), never system-wide and never as root.
Deleting `tools/` removes it all.

| Part | Why | What it installs | Size |
|---|---|---|---|
| **Claude** | Claude does the work: research, the plan, the score, the sounds | nothing (your Claude Code login), or your Anthropic API key in `secrets.json` | |
| **The music engine** | the score is played to be checked, captured as MIDI and rendered | Strudel (`@strudel/web` from npm) and a headless Chrome (yours is reused) | ~20 MB (+~100 MB without Chrome) |
| **Your DAW** | every MIDI pack comes with the steps to bring it into your DAW | nothing | |

## Claude

Claude Code, logged in, is used if it's installed (your plan pays). Otherwise an Anthropic API key (you pay per
token); the key is checked with a free call and kept in `secrets.json` (mode 600). See `mortiflix-music keys`.

## The music engine

Strudel 1.3 (AGPL-3.0) is installed from npm into `tools/strudel`. The engine runs Strudel in a headless Chrome to
read the score's notes, and renders audio with Strudel's own offline renderer there. Your Chrome, Chromium or (on
Windows) Edge is used if it's there; otherwise Chrome for Testing's headless shell is downloaded into
`tools/browsers`. Without the engine, sessions can research and plan but stop before the score and ask you to run
`mortiflix-music setup music`.

## Your DAW

Pick the DAW you finish music in: Ableton Live, FL Studio, Logic Pro, REAPER, Bitwig Studio, Cubase, Studio One,
GarageBand, Pro Tools, or another. The cue sheet in every MIDI pack then has its import steps (`setup daw none` writes
general ones). Without a terminal: `mortiflix-music setup daw <ableton|fl|logic|reaper|bitwig|cubase|studioone|garageband|protools|other|none>`.

## Without a terminal

```sh
mortiflix-music setup install strudel chrome
mortiflix-music setup daw reaper
mortiflix-music setup status --json
```
