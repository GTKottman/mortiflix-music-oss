// "Edit the video": the final as a FilmCraft project. Every shot is its own clip on V1 (so one can be swapped at a time),
// every narration line its own clip on A1 at its time, the music on A2, a marker per beat. It is built with FilmCraft's
// own headless engine (`filmcraft-cli`), so the project is always in the format the installed FilmCraft writes.
// out/filmcraft/map.json ties every clip back to its canvas piece: click a clip in FilmCraft, and the studio knows
// which shot, line or cue it is.
import { spawnSync, spawn } from 'node:child_process';
import { existsSync, mkdirSync, writeFileSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { projectPaths, loadProject } from './projects.mjs';
import { canvasView } from './canvas.mjs';
import { installed } from './craft.mjs';
import { UserError } from './studio.mjs';

export const CONTROL_PORT = 9876;
const TPS = 254016000000; // FilmCraft ticks per second

export function filmcraftPaths(root, id) {
  const dir = join(projectPaths(root, id).work, 'out', 'filmcraft');
  return { dir, media: join(dir, 'media'), map: join(dir, 'map.json') };
}

export function filmcraftCli(root) {
  const fc = installed(root, 'filmcraft');
  if (!fc) throw new UserError('FilmCraft isn\'t installed: mortiflix craft install filmcraft (Settings › Setup › Craft apps)');
  return fc;
}

const run = (cmd, args, what) => {
  const r = spawnSync(cmd, args, { encoding: 'utf8', timeout: 600_000 });
  if (r.status !== 0) throw new Error(`${what} failed: ${(r.stderr || r.stdout || '').trim().split('\n').slice(-3).join(' ')}`);
  return r.stdout;
};

// FilmCraft reads media best re-encoded: no B-frames (else frames show late), constant 48 kHz audio from time zero.
function prepVideo(src, dest, { from = null, to = null, audio = true } = {}) {
  const args = ['-y', '-loglevel', 'error'];
  if (from !== null) args.push('-ss', String(from));
  args.push('-i', src);
  if (to !== null && from !== null) args.push('-t', String(+(to - from).toFixed(3)));
  args.push('-c:v', 'libx264', '-preset', 'veryfast', '-crf', '16', '-bf', '0', '-pix_fmt', 'yuv420p');
  if (audio) args.push('-c:a', 'aac', '-ar', '48000', '-af', 'asetpts=N/SR/TB'); else args.push('-an');
  args.push(dest);
  run('ffmpeg', args, `preparing ${dest}`);
}
const prepAudio = (src, dest) => run('ffmpeg', ['-y', '-loglevel', 'error', '-i', src, '-ar', '48000', '-c:a', 'pcm_s16le', dest], `preparing ${dest}`);

export function makeFilmcraftProject(root, id, { log = () => {} } = {}) {
  const fc = filmcraftCli(root);
  const project = loadProject(root, id);
  const state = projectPaths(root, id).state;
  const g = canvasView(root, id);
  const fp = filmcraftPaths(root, id);
  rmSync(fp.dir, { recursive: true, force: true });
  mkdirSync(fp.media, { recursive: true });
  const n = g.nodes;
  const shots = g.order.shots.map((s) => n[s]).filter((s) => s && s.state !== 'cut' && s.versions?.length);
  if (!shots.length) throw new UserError('nothing to edit yet: the shots come with the animatic');
  const lines = g.order.lines.map((l) => n[l]).filter((l) => l && l.state !== 'cut' && l.versions?.find((v) => v.v === l.current)?.file);
  const music = n.music && !n.music.missing ? n.music : null;
  const current = (node) => node.versions.find((v) => v.v === node.current);

  // 1. Media, one file per piece and version, named so a person can read the bin.
  log(`Preparing ${shots.length} shot${shots.length === 1 ? '' : 's'}${lines.length ? `, ${lines.length} voice lines` : ''}${music ? ' and the music' : ''}`);
  const files = [];
  let t = 0;
  for (const s of shots) {
    const v = current(s);
    const dest = join(fp.media, `${s.label}-v${s.current}.mp4`);
    // Without separate voice lines, a shot keeps its own sound (narration and music are in it).
    prepVideo(join(state, v.file), dest, { from: v.from_sec ?? null, to: v.to_sec ?? null, audio: !lines.length && !music });
    const len = +(v.to_sec - (v.from_sec || 0)).toFixed(3);
    files.push({ node: s.id, kind: 'shot', version: s.current, beat: s.beat || null, file: dest, label: `${s.label} · ${s.title || ''}`.trim().slice(0, 80), start_sec: +t.toFixed(3), seconds: len });
    t += len;
  }
  const shotStart = Object.fromEntries(files.map((f) => [f.beat, f.start_sec]));
  let lineCursor = {};
  for (const l of lines) {
    const dest = join(fp.media, `${l.label}-v${l.current}.wav`);
    prepAudio(join(state, current(l).file), dest);
    // At its narration time; without one, at the start of its beat's shot, after the beat's earlier lines.
    const at = l.start_sec ?? ((shotStart[l.beat] ?? 0) + (lineCursor[l.beat] || 0));
    const len = l.end_sec !== null && l.start_sec !== null ? l.end_sec - l.start_sec : 0;
    lineCursor[l.beat] = (lineCursor[l.beat] || 0) + len;
    files.push({ node: l.id, kind: 'line', version: l.current, beat: l.beat || null, file: dest, label: `${l.label} · ${String(l.title).slice(0, 60)}`, start_sec: +at.toFixed(3) });
  }
  if (music) {
    const dest = join(fp.media, `Music-v${music.current}.wav`);
    prepAudio(join(state, current(music).file), dest);
    files.push({ node: 'music', kind: 'music', version: music.current, beat: null, file: dest, label: 'Music', start_sec: 0 });
  }

  // 2. The project, through FilmCraft's own engine.
  const fcproj = join(fp.dir, `${project.title.replace(/[^\w .-]+/g, '').trim() || 'Mortiflix'}.fcproj`);
  const cli = (args, what) => JSON.parse(run(fc.cli, ['--compact', ...args], what).trim() || 'null');
  const exec = (cmd, params) => cli(['--project', fcproj, '--save', 'exec', cmd, JSON.stringify(params)], cmd);
  cli(['--save-as', fcproj, 'exec', 'file.newProject', JSON.stringify({ name: project.title })], 'file.newProject');
  const imported = exec('file.import', { paths: files.map((f) => f.file) });
  if (imported.errors?.length) throw new Error(`FilmCraft couldn't import: ${JSON.stringify(imported.errors).slice(0, 300)}`);
  files.forEach((f, i) => { f.item = imported.items[i]; });
  // The sequence takes the shots' own frame rate and size.
  const probe = JSON.parse(run('ffprobe', ['-v', 'error', '-select_streams', 'v:0', '-show_entries', 'stream=width,height,r_frame_rate', '-of', 'json', files[0].file], 'ffprobe')).streams[0];
  const [num, den] = String(probe.r_frame_rate).split('/').map(Number);
  const fps = +(num / (den || 1)).toFixed(3);
  const seq = exec('file.newSequence', { name: project.title, width: probe.width, height: probe.height, fps, video: 2, audio: 3 }).sequence;
  const placed = exec('timeline.place', { items: files.filter((f) => f.kind === 'shot').map((f) => f.item), track: 'V1', seconds: 0, insert: false, audio: !lines.length && !music });
  files.filter((f) => f.kind === 'shot').forEach((f, i) => { f.clip = placed.placed?.[i]?.[0] ?? placed.clips[i]; });
  for (const f of files.filter((x) => x.kind !== 'shot')) {
    const r = exec('timeline.place', { item: f.item, track: f.kind === 'music' ? 'A2' : 'A1', seconds: f.start_sec, insert: false, video: false });
    f.clip = r.clips[0];
  }
  // Names and beat markers in one run (no result needed from them).
  const script = [
    ...files.map((f) => ({ id: 'clip.rename', params: { clip: f.clip, name: f.label } })),
    ...files.filter((f) => f.kind === 'shot' && f.beat).map((f) => ({ id: 'markers.add', params: { target: 'program', time: Math.round(f.start_sec * TPS), name: `${n[f.beat]?.label || ''} ${String(n[f.beat]?.title || '').slice(0, 60)}`.trim(), ...(n[f.beat]?.colour?.label ? { color: n[f.beat].colour.label } : {}) } })),
  ];
  const jsonl = join(fp.dir, '.names.jsonl');
  writeFileSync(jsonl, script.map((x) => JSON.stringify(x)).join('\n'));
  spawnSync(fc.cli, ['--project', fcproj, '--save', '--keep-going', 'run', jsonl], { encoding: 'utf8', timeout: 120_000 });
  rmSync(jsonl, { force: true });

  const map = {
    format: 'mortiflix.filmcraft', version: 1, project: id, fcproj, sequence: seq, fps, made_at: new Date().toISOString(), filmcraft: fc.version,
    clips: files.map(({ node, kind, version, beat, item, clip, label, start_sec, seconds, file }) => ({ node, kind, version, beat, item, clip, label, start_sec, seconds: seconds ?? null, file })),
  };
  writeFileSync(fp.map, JSON.stringify(map, null, 2));
  log(`FilmCraft project: ${fcproj} (${files.length} clips)`);
  return map;
}

export function loadMap(root, id) {
  try { return JSON.parse(readFileSync(filmcraftPaths(root, id).map, 'utf8')); } catch { return null; }
}

// Open the project in desktop FilmCraft with its control channel on (loopback only), so the studio can see what the
// owner clicks. On Hyprland it opens on workspace 9, out of the way of whatever the owner has open.
export function openInFilmcraft(root, id, { port = CONTROL_PORT } = {}) {
  const fc = filmcraftCli(root);
  const map = loadMap(root, id);
  if (!map || !existsSync(map.fcproj)) throw new UserError('make the FilmCraft project first (Export › Edit the video)');
  if (!fc.app) throw new UserError('this FilmCraft install has no desktop app');
  const argv = [fc.app, '--control', String(port), map.fcproj];
  if (process.env.HYPRLAND_INSTANCE_SIGNATURE) {
    // On a workspace that isn't on screen FilmCraft would stop drawing, and it answers its control channel only as it
    // draws: let its window keep drawing there (FilmCraft's window only; gone at the next Hyprland reload).
    spawnSync('hyprctl', ['keyword', 'windowrule', 'render_unfocused on, match:class ^(ai\\.storyteller\\.filmcraft)$']);
    spawnSync('hyprctl', ['dispatch', 'exec', `[workspace 9 silent] ${argv.map((a) => `'${a.replace(/'/g, "'\\''")}'`).join(' ')}`]);
  } else {
    spawn(argv[0], argv.slice(1), { detached: true, stdio: 'ignore' }).unref();
  }
  return { opened: map.fcproj, port };
}
