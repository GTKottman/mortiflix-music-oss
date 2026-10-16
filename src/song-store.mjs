// Where a project's song lives and how it's saved (doc 18 §1): music/song.json and music/song.strudel.js, written only
// by the studio, atomically (temp file + rename) under one lock, and only when checkSong passes.
import { existsSync, readFileSync, writeFileSync, renameSync, mkdirSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { projectPaths } from './projects.mjs';
import { withLock, UserError } from './studio.mjs';
import { toolsDir } from './setup.mjs';
import { checkSong, writeSong, readSong } from './song.mjs';
import { importScore } from './song-import.mjs';
import { createEvaluator } from './strudel/evaluator.mjs';

export function songPaths(root, id) {
  const music = join(projectPaths(root, id).work, 'music');
  return { music, song: join(music, 'song.json'), strudel: join(music, 'song.strudel.js'), lock: join(music, '.song.lock'),
    blobs: join(music, 'daw', 'blobs'), versions: join(music, 'daw', 'versions'), audio: join(music, 'daw', 'audio'),
    blueprint: join(music, 'blueprint.json'), notes: join(music, 'notes.json'), instruments: join(music, 'instruments.json'), score: join(music, 'score.strudel.js') };
}

const atomic = (file, text) => { mkdirSync(dirname(file), { recursive: true }); const tmp = `${file}.${process.pid}.tmp`; writeFileSync(tmp, text); renameSync(tmp, file); };

export function loadSong(root, id) {
  const sp = songPaths(root, id);
  if (!existsSync(sp.song)) return null;
  return { song: readSong(readFileSync(sp.song, 'utf8')), text: existsSync(sp.strudel) ? readFileSync(sp.strudel, 'utf8') : null };
}

// Saves both files; the song's rev goes up by one. Refuses a song that fails its check.
export function saveSong(root, id, song, text) {
  const sp = songPaths(root, id);
  mkdirSync(sp.music, { recursive: true });
  return withLock(sp.lock, () => {
    const errors = checkSong(song, { dir: sp.music });
    if (errors.length) throw new UserError(`the song can't be saved: ${errors.slice(0, 3).join('; ')}`);
    const prev = existsSync(sp.song) ? readSong(readFileSync(sp.song, 'utf8')).rev : 0;
    song.rev = prev + 1;
    if (text !== undefined) atomic(sp.strudel, text);
    atomic(sp.song, writeSong(song));
    return song.rev;
  });
}

export function strudelDirOf(root) {
  const d = process.env.MFX_STRUDEL || join(toolsDir(root), 'strudel');
  if (!existsSync(join(d, 'node_modules', '@strudel', 'core'))) throw new UserError('Strudel isn\'t installed: mortiflix setup music');
  return d;
}

// The project's checked score → its song. `evaluator` may be shared (the server keeps one); otherwise one is made.
export async function importProjectSong(root, id, { evaluator = null } = {}) {
  const sp = songPaths(root, id);
  for (const [f, what] of [[sp.blueprint, 'the blueprint'], [sp.notes, 'the score\'s notes (they come when the score passes its check)']]) if (!existsSync(f)) throw new UserError(`no song yet: ${what} ${f === sp.notes ? '' : 'is missing'}`.trim());
  const rd = (f) => (existsSync(f) ? readFileSync(f, 'utf8') : null);
  const ev = evaluator || createEvaluator({ strudelDir: strudelDirOf(root) });
  try {
    const r = await importScore({ blueprint: JSON.parse(rd(sp.blueprint)), notes: JSON.parse(rd(sp.notes)), instruments: rd(sp.instruments) ? JSON.parse(rd(sp.instruments)) : null,
      evaluator: ev, sources: { score: rd(sp.score), notes: rd(sp.notes), blueprint: rd(sp.blueprint) } });
    const rev = saveSong(root, id, r.song, r.text);
    return { ...r, rev, paths: sp };
  } finally { if (!evaluator) ev.close(); }
}
