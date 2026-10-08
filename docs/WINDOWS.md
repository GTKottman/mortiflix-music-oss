# Windows

> **Work in progress.** Installing works as described below, but a session (the part where Claude writes the music)
> doesn't run on Windows yet: see [What's left](#whats-left). Mortiflix Music shares this work with
> [Mortiflix](https://github.com/GTKottman/mortiflix-oss), where the Windows sessions are being finished.

## Install

In **PowerShell** (not CMD; you don't need to run it as Administrator):

```powershell
irm https://raw.githubusercontent.com/GTKottman/mortiflix-music-oss/main/install.ps1 | iex
```

It checks for what the studio needs and offers to install what's missing, asking first each time:

| Needs | Why | Installed with |
|---|---|---|
| Windows 10 1809 or newer | what Node 20, Claude Code and winget need | |
| Node.js 20+ | runs Mortiflix Music | `winget install OpenJS.NodeJS.LTS` |
| Git for Windows | downloads Mortiflix Music; its Git Bash runs the sessions' shell commands | `winget install Git.Git` |
| ffmpeg | masters, measures loudness, imports your own masters | `winget install Gyan.FFmpeg` |
| Claude Code (optional) | does the work on your Claude plan; an Anthropic API key works instead | Anthropic's installer, `irm https://claude.ai/install.ps1 \| iex` |

Then it clones the repository into `%USERPROFILE%\mortiflix-music-oss` (or updates the clone that's there), runs
`npm install` and `npm link` (which puts `mortiflix-music` in `%APPDATA%\npm`, already on your PATH), and starts
`mortiflix-music init`.

Choices, as environment variables (set them before the line above):

```powershell
$env:MORTIFLIX_MUSIC_DIR = 'D:\mortiflix-music-oss'   # where the code goes
$env:MORTIFLIX_MUSIC_BRANCH = 'main'                  # which branch to clone
$env:MORTIFLIX_MUSIC_YES = '1'                        # yes to every question
$env:MORTIFLIX_MUSIC_SKIP_INIT = '1'                  # stop before mortiflix-music init
```

From a clone: `powershell -ExecutionPolicy Bypass -File install.ps1 -Dir D:\mortiflix-music-oss -Yes -SkipInit`.

## What `mortiflix-music setup` does on Windows

- **The music engine** runs Strudel in your Chrome, or in **Microsoft Edge** if there's no Chrome (Edge is Chromium
  and comes with Windows), so nothing is downloaded.
- **npm and npx** (`.cmd` programs on Windows) are run through `cmd.exe` with each argument quoted, so a studio under
  a folder with spaces (`C:\Users\Ada Lovelace\MortiflixMusic`) works.

## Removing it

```powershell
npm uninstall -g mortiflix-music
Remove-Item -Recurse -Force "$env:USERPROFILE\mortiflix-music-oss", "$env:USERPROFILE\MortiflixMusic\tools"
```

`%USERPROFILE%\MortiflixMusic` is your studio (projects, keys): delete it only if you mean to.

## What's left

**Running sessions**
- [ ] The bridge listens on a Unix socket path (`src/bridge.mjs`); on Windows it has to be a named pipe
- [ ] Sessions' `PATH` is joined with `:` (`src/backends/claude-code.mjs`, `src/backends/anthropic-api.mjs`); use
      `pathDelimiter()`
- [ ] `bin/mfx` is a Node script with a shebang: Git Bash runs it, but Claude Code's PowerShell tool needs an `mfx.cmd`
- [ ] Stopping a session kills its process group (`process.kill(-pid)`); Windows needs `taskkill /T /F /PID`
- [ ] The API backend's shell is `bash`; on Windows, Git Bash from `findGitBash()`
- [ ] `secrets.json` relies on file mode 600, which Windows ignores; the user profile's permissions protect it
- [ ] Run the installer, setup and a song on a real Windows 10 and 11 machine
