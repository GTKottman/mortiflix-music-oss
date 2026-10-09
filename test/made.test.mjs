// What every step made, readable by the owner (src/made.mjs), and the note a session leaves when it finishes a step.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, writeFileSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { request } from 'node:http';
import { tmpdir } from 'node:os';
import { randomBytes } from 'node:crypto';
import { tempStudio } from './helpers.mjs';
import { createProject, startProject, projectPaths, loadProject } from '../src/projects.mjs';
import * as gates from '../src/gates.mjs';
import { madeBy, madeFile } from '../src/made.mjs';

const pass = (root, id, step) => gates.stepChecks(root, id, step).map((c) => ({ id: c.id, result: 'pass' }));

function songWithWork(root) {
  const p = createProject(root, { pipeline: 'song', title: 'Harbour', answers: { genre: 'trip hop', topic: 'fog' } });
  startProject(root, p.id);
  const w = projectPaths(root, p.id).work;
  const put = (f, v) => { mkdirSync(join(w, f, '..'), { recursive: true }); writeFileSync(join(w, f), typeof v === 'string' ? v : JSON.stringify(v)); };
  put('music/genre.json', { name: 'Trip hop', summary: 'Slow, moody hip hop beats from Bristol.', tempo: { min: 70, max: 100, typical: 88 }, meters: ['4/4'], length: { min_sec: 200, max_sec: 360 },
    lineup: [{ role: 'Breakbeat drums' }, { role: 'Dub bass' }, { role: 'Strings' }], unforgettable: [{ element: 'a dusty break' }, { element: 'a sad string phrase' }],
    sources: [{ url: 'https://example.org/a', title: 'A history of trip hop' }, { url: 'javascript:alert(1)', title: 'bad' }] });
  put('music/GENRE.md', '# Trip hop\n\n## The beat\nA **slow** breakbeat.\n');
  put('music/blueprint.json', { title: 'Harbour', bars: 96, bpm: 88, meter: '4/4', scale: 'A minor', dramatic_reading: 'fog -> light', roles: [{ name: 'Sub' }, { name: 'Glass hook' }],
    sections: [{ name: 'Fog on the water' }, { name: 'The lamp turns' }], techniques: [{ name: 'Pedal Tone' }] });
  put('music/BLUEPRINT.md', '# Harbour\n');
  put('music/check.json', { ok: true, errors: [], warnings: ['the hook is quiet in bar 40'], parts: [{ name: 'Sub', notes: 96 }, { name: 'Glass hook', notes: 210 }],
    intensity: [{ name: 'Fog on the water', bars: [1, 48], asked: 2, measured: 0.21 }], techniques: [{ name: 'Pedal Tone', found: true }] });
  put('music/notes.json', { bpm: 88, tracks: [] });
  put('music/score.strudel.js', 'setcpm(88 / 4)\n');
  put('out/music/midi/harbour-all.mid', 'MThd'); put('out/music/midi/01-sub.mid', 'MThd'); put('out/music/midi/CUE-SHEET.md', '# Cue\n');
  put('out/music/midi/sections/01-fog/fog-all.mid', 'MThd'); put('out/music/midi.zip', 'PK');
  put('music/instruments.json', { concept: 'tape-worn and dark', parts: [{ channel: 1, role: 'Sub', sound: 'sine', gain: 0.9 }, { channel: 10, role: 'Drums', kit: { 36: { sound: 'kick' } } }] });
  put('out/music/audition/01-sub.wav', 'RIFF');
  put('music/master.json', { master: { lufs: -14.1, true_peak: -1.2, seconds: 262 } });
  put('out/music/master.mp3', 'ID3'); put('out/music/stems/01-sub.wav', 'RIFF');
  put('input/secret.wav', 'RIFF');
  return { id: p.id, w };
}

test('made: every kind of step says what it made, in words and in its work', (t) => {
  const root = tempStudio(t);
  const { id } = songWithWork(root);
  gates.stepDone(root, id, 'genre', pass(root, id, 'genre'), { note: 'Trip hop at 70-100 BPM, built on a lazy break.' });

  const g = madeBy(root, id, 'genre');
  assert.equal(g.step.note, 'Trip hop at 70-100 BPM, built on a lazy break.');
  assert.match(g.step.describe, /researched from 5\+ sources/);
  assert.deepEqual(g.summary.slice(0, 2), ['Trip hop: Slow, moody hip hop beats from Bristol.', '70-100 BPM (usually 88) · 4/4 · tracks run 3:20-6:00']);
  assert.ok(g.summary.includes('Its usual band: Breakbeat drums, Dub bass, Strings.'));
  assert.deepEqual(g.pieces.map((x) => x.kind), ['markdown', 'links']);
  assert.deepEqual(g.pieces[1].links, [{ url: 'https://example.org/a', title: 'A history of trip hop' }]); // only http(s) links
  assert.ok(g.step.checks.length && g.step.checks.every((c) => c.result === 'pass'));

  const b = madeBy(root, id, 'blueprint');
  assert.match(b.summary[0], /"Harbour": 96 bars at 88 BPM in 4\/4, A minor, 4:22 long/);
  assert.ok(b.summary.includes('2 parts: Sub, Glass hook.'));

  const s = madeBy(root, id, 'score');
  assert.match(s.summary[0], /passed its check/);
  assert.ok(s.summary.includes('2 parts, 306 notes: Sub (96), Glass hook (210).'));
  assert.deepEqual(s.pieces.map((x) => x.kind), ['table', 'facts', 'notes', 'code', 'files', 'markdown']);
  assert.deepEqual(s.pieces[0].rows, [['Fog on the water', '1-48', '2', '0.21']]);
  assert.ok(s.pieces[4].files.includes('out/music/midi/sections/01-fog/fog-all.mid'));
  assert.equal(s.pieces[4].zip, true);

  const i = madeBy(root, id, 'instruments');
  assert.equal(i.summary[0], 'The sound: tape-worn and dark');
  assert.deepEqual(i.pieces[0].rows[1], ['10', 'Drums', 'drum kit: 36 kick', '0.8']);
  assert.deepEqual(i.pieces.at(-1).files, ['out/music/audition/01-sub.wav']);

  const m = madeBy(root, id, 'mix');
  assert.equal(m.summary[0], 'Mastered: -14.1 LUFS, true peak -1.2 dBTP, 4:22.');
  assert.deepEqual(m.pieces.map((x) => [x.title, x.files]), [['The master', ['out/music/master.mp3']], ['Stems: every channel on its own', ['out/music/stems/01-sub.wav']]]);
});

test('made: only readable music outputs are served from the project folder', (t) => {
  const root = tempStudio(t);
  const { id, w } = songWithWork(root);
  assert.ok(madeFile(root, id, 'out/music/master.mp3'));
  assert.ok(madeFile(root, id, 'music/GENRE.md'));
  for (const bad of ['input/secret.wav', 'music/genre.json', 'music/score.strudel.js', '../../etc/passwd.md', 'music/../input/secret.wav', 'CLAUDE.md', 'out/music/nope.mp3']) assert.equal(madeFile(root, id, bad), null, bad);
  symlinkSync('/etc/hostname', join(w, 'music', 'link.md'));
  assert.equal(madeFile(root, id, 'music/link.md'), null, 'a link out of the folder');
});

test('made: a session finishing a step must say what it made', async (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'song', title: 'Note', answers: { genre: 'house', topic: 'x' } });
  startProject(root, p.id);
  const { openBridge } = await import('../src/bridge.mjs');
  const { RenderQueue } = await import('../src/renderq.mjs');
  const { callStudio } = await import('../src/mfx.mjs');
  const bridge = await openBridge({ root, projectId: p.id, sessionId: 's1', workdir: projectPaths(root, p.id).work, renders: new RenderQueue({ logDir: join(root, 'run') }), sessionEnv: {} });
  t.after(() => bridge.close());
  await assert.rejects(callStudio('step-done', { step: 'genre', checks: pass(root, p.id, 'genre') }, bridge.env), /add --note/);
  await callStudio('step-done', { step: 'genre', checks: pass(root, p.id, 'genre'), note: 'House at 124 BPM.' }, bridge.env);
  assert.equal(loadProject(root, p.id).steps.genre.note, 'House at 124 BPM.');
});

test('made: the web server answers for a step and serves its files', async (t) => {
  const root = tempStudio(t);
  const { id } = songWithWork(root);
  const { startServer } = await import('../src/web/server.mjs');
  const socketPath = join(tmpdir(), `mfx-made-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true, runner: false });
  t.after(() => srv.close());
  const get = (path) => new Promise((ok, fail) => {
    const req = request({ socketPath, path, headers: { host: 'localhost' } }, (res) => { let d = ''; res.on('data', (c) => { d += c; }); res.on('end', () => ok({ status: res.statusCode, headers: res.headers, body: d })); });
    req.on('error', fail); req.end();
  });
  const r = await get(`/api/projects/${id}/steps/genre/made`);
  assert.equal(r.status, 200); assert.equal(JSON.parse(r.body).pieces[0].title, 'The genre research');
  const f = await get(`/made/${id}/out/music/master.mp3`);
  assert.equal(f.status, 200); assert.equal(f.headers['content-type'], 'audio/mpeg'); assert.match(f.headers['content-security-policy'], /sandbox/);
  assert.equal((await get(`/made/${id}/input/secret.wav`)).status, 404);
  assert.equal((await get(`/made/${id}/out%2fmusic%2f..%2f..%2finput%2fsecret.wav`)).status, 404);
});
