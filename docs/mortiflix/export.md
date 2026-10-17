# Stage 6 Render: the Export page (part 07, doc 39)

Stage 6 is Mortiflix Studio's Export tab. While Mortiflix is on, the tab shows the simple Mortiflix page
instead of Mortiflix Studio's full settings (mockups `UI Layout/39a Export.html` and `39b Rendered.html`).
Every file is written by Mortiflix Studio's own export queue, so it is exactly what Mortiflix Studio's Export
writes.

## What is where

| File | What |
|---|---|
| `crates/mortiflix-ui/src/export.rs` | the page itself (no Mortiflix Studio dependency): choices, presets → Mortiflix Studio settings recipe, default folders per OS, safe and unique file names, the one queue, renders of this project, Post it, the drawing |
| `crates/ui-egui/src/mortiflix_export.rs` | the host: fills the page from the engine, draws the timeline picture, runs `export.queue.*`, Audio stems, the checks, the shared queue with the service, Get the video, `mortiflix.renderVideo` |

Shared files touched (one line each): `ui-egui/src/lib.rs` (module + Export-mode dispatch),
`ui-egui/src/mortiflix.rs` (`Host.export`, `frame_begin` hook, `mortiflix.renderVideo` route,
`stages.getVideo` hook). Service messages used (part 08's protocol, no new ones): `render.add`,
`render.progress`, `render.finish`, `render.list` + `render.job` events, `export.done`,
`publish.plan`, `publish.send`.

## The page

- **Where it goes**: YouTube (default), Social vertical, Master file, Audio stems, and
  **All Mortiflix Studio formats (N) ›** (every Mortiflix Studio preset by category).

  | Pick | Mortiflix Studio preset | The page's overrides |
  |---|---|---|
  | YouTube | `YouTube 1080p Full HD` | size 720p/1080p/4K, bitrate by Quality (Small ×0.5, Good, Best ×1.5), AAC 384 kbps 48 kHz, −14 LUFS / −1 dBTP |
  | Social vertical | `Social Vertical 1080×1920` | size, Quality, Framing Fill (crop the sides) / Fit (bars), AAC 320 kbps, −14 LUFS / −1 dBTP |
  | Master file | `Apple ProRes 422 HQ` | size; sound left as mixed |
  | Audio stems | `Waveform Audio 48 kHz 24-bit` ×3 | a folder `<Project> – Stems/` with Voice.wav, Music.wav, Effects.wav, each the full length; every audio track feeds one stem by its name (voice/dialog/narration/VO → Voice, music/song/score/theme → Music, the rest → Effects), so together they are the mix; never normalised |

- **Settings**: File name (`<Project> – <Preset>`), Save to (default
  `$XDG_VIDEOS_DIR` or `~/Videos` / `~/Movies` / `%USERPROFILE%\Videos` + `Mortiflix/<Project>`,
  Change… = OS folder dialog), Size, Framing, Quality, How much (Whole video / In to Out), Advanced
  (summary when folded; Loudness, Captions off / burned in / .srt, Hardware encoding off by default,
  All Mortiflix Studio settings…, Reset). **You'll get** line with size and (after the first render) the
  time on this PC. **Render**; the reason is written under it when it can't.
- **Render queue**: one queue for the PC, owned by the service. Each Render is a job there
  (`render.add`, kind export); Mortiflix Studio encodes it only once its `render.job` says running, sends
  `render.progress` about once a second and `render.finish` at the end, then `export.done` (the
  project keeps the delivered final). The service's own heavy jobs (`render.list`) show as rows
  beside the app's; a waiting row says how many are ahead. Without the service it renders at once.
  Stop asks first and deletes the half-written file. Failed rows say why, with Try again.
- Names never collide (" (2)"); the folder is checked writable before anything starts.
- **Rendered** (when the render you started finishes): the file tile (name, size, length, frame
  size), Open, Show in Files / Finder / Explorer, Save a copy…, the checks (it opens, its length
  is the timeline's), **Renders of this project** (kept in
  `<data dir>/mortiflix-renders.json`; "made before your last edit" when the timeline changed, Render
  again, Missing when the file was moved), **Post it** (`publish.plan` says whether Upload-Post / Postiz
  is set up; the confirm dialog shows file, title, accounts and time; only its "Post it" sends
  `export.done` for the shown file and `publish.send {confirm: true}`; the AI can't press it) and **What next**
  (Change something → Edit tab, Render another format, New project).
- **Get the video** on stage 4 goes to Render and renders the YouTube video at once (as soon as a
  timeline is there).

## Commands

`mortiflix.renderVideo` (the stage bar's "Render the video", and the control channel):

```json
{"set": {"preset": "youtube|social|master|stems|<Mortiflix Studio preset name>", "size": "720p|1080p|4k",
         "quality": "small|good|best", "framing": "fill|fit", "howMuch": "whole|inOut",
         "loudness": "platform|asMixed", "captions": "off|burned|srt", "hardware": false,
         "fileName": "", "folder": ""},
 "render": true, "page": "settings|rendered|full|simple"}
```

Returns `{started, export, view}`. Without params it renders the current choices.

Automation ids: see the header of `crates/mortiflix-ui/src/export.rs` (`mfx.export.*`).

## Not done

- The Rendered page shows the timeline frame the file was made from, not the file itself playing
  (no in-app player of an outside file without importing it into the project); Open plays it in the
  OS player.
- Drag-out of the file tile (egui has no OS drag source), taskbar/Dock progress, OS notification,
  keep-awake, tray-on-quit, the header's progress bar on other tabs.
- Free-space check before rendering (no pure-Rust disk-space call in the workspace); a full disk
  fails the render with Mortiflix Studio's error.
- "Make a version 2" (needs a service-side project copy).
- One beat… in How much (needs the beat list from the node graph).
- Loudness / true-peak lines on the Rendered page (part 08's protocol has no check message; the
  render itself is normalised to −14 LUFS / −1 dBTP by Mortiflix Studio).
- Posting was only checked up to "not set up" / "needs a project" (no real post was sent).
