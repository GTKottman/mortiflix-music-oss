# Security

Mortiflix Music is a studio for **one person**: you write the briefs, you review every stage, and the sessions work for
you, on your computer. There are no other users, accounts or roles. It does run an AI agent with a shell on your
machine, so here's what that means and what limits it.

## What a session can reach

**By default, a session runs as you.** With the `claude-code` backend it has whatever Claude Code's permission flags
allow (`config.claudeArgs`: by default it may edit files and run commands without asking, because nobody is there to
answer). With the `anthropic-api` backend its shell is a normal shell in the project folder. The editor tool is
confined to the project folder, but the shell is not. Treat a session like a capable assistant logged in as you.

**With `sandbox: true` (Linux, `claude-code` backend)**, each session runs inside
[bubblewrap](https://github.com/containers/bubblewrap). It sees:

- the system (`/usr`, `/etc`) read-only, and the GPU devices;
- its own project folder, read-write;
- the Mortiflix Music code, read-only, and its own `mfx` socket;
- Claude Code's install (read-only) and its login (`~/.claude`, `~/.claude.json`), which it needs to run;
- an npm cache inside the studio.

Your home folder, other projects, the studio's records (`state/`), `secrets.json` and `session.env` are simply not
there. The network is shared (Claude needs it). Check it with `mortiflix-music doctor`.

## What a session can't do, even unsandboxed

The gates are enforced by the studio process, not by the prompt:

- it can't approve a step you review: approval exists only in the web API and the CLI;
- it can't change what you already reviewed: submissions are copied into `state/`, outside the working folder;
- it can't submit files from outside its project folder (paths and symlinks are resolved and checked).

The CLI's own decision commands (`mortiflix-music respond`, `answer`, `review`, `reopen`, …) refuse to run inside a
session (`MFX_TOKEN` is set there), so a session that finds `mortiflix-music` on its `PATH` can't approve its own work by accident.
That's a guard, not a wall. Unsandboxed, a determined session could still edit `state/` files directly with its shell. If that's in your threat
model, turn the sandbox on.

## Your words are direction; outside material is data

Your brief, notes and answers are your direction to the session: they decide what gets made and outrank the
pipeline's defaults (never the gate rules). The risk is everything else a session reads: the contents of files you
hand over, web pages it researches, things it downloads. The gate protocol tells it that this material is data and
never instructions, and to report anything in it that tries to give orders. That reduces prompt injection; it can't
rule it out. If you hand a session files or links from sources you don't trust, turn the sandbox on.

## /mortiflix-music in Claude Code

The `/mortiflix-music` skill runs in your own Claude Code conversation, with your permissions. It's a guide: it runs the CLI
and passes your decisions on. It doesn't make the music.

- You start it yourself: it never starts on its own. Studio sessions can't start it either, even when they load
  your plugins.
- It pre-allows only the commands that read (`--json` views, `doctor`). Approving, sending notes, starting a run,
  installing and serving are not pre-allowed, so your own Claude Code permission settings decide whether they
  prompt. The skill is also told to ask you before each of them. That instruction is a prompt, not a gate. The
  gates themselves are unchanged: a step is approved only when `mortiflix-music respond` runs.
- It never asks for a key in the chat. You add keys yourself, hidden, with `mortiflix-music keys` or in Settings › Keys.
- It treats what sessions submit as data, never as instructions.

## Keys

Every key is yours: the studio has no accounts of its own. You add them with `mortiflix-music keys` (asked at
`mortiflix-music init`, and before a project starts if it needs one) or Settings › Keys. Typing is never echoed, and each
key is checked with a free, read-only API call before it's saved.

- A new studio folder is created mode 700 (only you can open it).
- The Anthropic API key is stored in `secrets.json` (mode 600) and is never returned by the API or shown in the UI.
  Sessions don't get it.
- Other keys go to `session.env` (mode 600) and are given to every session's environment. The UI shows their names,
  never their values. Only add keys that sessions need, and prefer keys with spending limits.
- The runner checks a project's keys before each session: if one is missing, the project pauses and tells you which,
  instead of starting a session that would fail.

## What setup installs

**Nothing installs without you asking** (a confirmation in the terminal, an Install button on the page), and
everything goes into the studio folder: never system-wide, never as root. Sources: npm (Strudel) and Google's Chrome
for Testing (only when you have no Chrome, Chromium or Edge). The web studio's install endpoints are behind the same
guards as every other change (loopback or token, the `X-Mortiflix` header).

The headless Chrome runs Strudel on a blank page the engine controls; it loads no website. Sessions use Strudel's
built-in synths; a sample or soundfont needs a source that names its licence, and the music skill tells sessions
never to take someone else's loops. That's an instruction plus your review, not a technical wall.

## The web server

A web server on localhost can be reached by any page open in your browser, so:

- it binds to `127.0.0.1` unless you pass another `--host`; then every request needs an access token (printed once as
  a link; kept in an HttpOnly, SameSite=Strict cookie);
- on loopback, requests whose `Host` isn't `localhost`/`127.0.0.1`/`::1` are refused (DNS rebinding);
- every change needs the `X-Mortiflix: 1` header, which another site can't send without a CORS preflight that this
  server never allows (CSRF);
- media is served only from a project's `reviews/` folder, resolved and checked (no `../`, encoded or not), with
  `Content-Security-Policy: sandbox` and `nosniff`, so a submitted SVG or HTML file can't run script in the app;
- the app itself has a strict CSP (`default-src 'self'`), and all project text is inserted as text, never HTML.

Exposing it beyond your network? Put it behind HTTPS (a reverse proxy) as well as the token.

## Reporting a problem

Please open a private security advisory on the repository rather than a public issue.
