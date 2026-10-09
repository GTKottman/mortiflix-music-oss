// Watching a piece being built: the status line names what exactly, the score goes down a layer at a time
// (strudel.mjs draft → music/draft.json), and the owner's page hears of every change the session makes at once.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync, readFileSync, mkdtempSync, rmSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { request } from 'node:http';
import { randomBytes } from 'node:crypto';
import { tempStudio, songAtBlueprint } from './helpers.mjs';
import * as gates from '../src/gates.mjs';
import * as setup from '../src/setup.mjs';

const REPO = resolve(import.meta.dirname, '..');

test('live build: the status line says what exactly; a draft layer needs a draft and names itself in the status', async (t) => {
  const root = tempStudio(t);
  const { id, work } = await songAtBlueprint(root);
  assert.equal(gates.status(root, id, 'score', 'working', 'the bass, bars 1-48').text, 'Writing the score: the bass, bars 1-48');
  assert.equal(gates.status(root, id, 'score').text, 'Writing the score');
  assert.throws(() => gates.status(root, id, 'score', 'working', 'x'.repeat(101)), /a few words/);

  assert.throws(() => gates.draftLayer(root, id, 'the beat'), /no music\/draft\.json/);
  writeFileSync(join(work, 'music', 'draft.json'), JSON.stringify({ bars: 8, tracks: [{ channel: 10, name: 'Drums', notes: [{ pitch: 36, start: 0, dur: 0.5, velocity: 0.9 }] }], layers: [{ name: 'the beat' }] }));
  assert.throws(() => gates.draftLayer(root, id, '  '), /name the layer/);
  gates.draftLayer(root, id, 'the beat');
  const { loadProject, readEvents } = await import('../src/projects.mjs');
  assert.equal(loadProject(root, id).status.text, 'Writing the score: the beat');
  assert.match(readEvents(root, id).at(-1).details, /the beat: 1 parts, 1 notes, 8 bars/);
});

test('live build: the demo writes the score in layers, and the page gets each one and the check', async (t) => {
  const root = tempStudio(t);
  const { createProject, startProject, readEvents } = await import('../src/projects.mjs');
  const { Runner } = await import('../src/runner.mjs');
  const { startServer } = await import('../src/web/server.mjs');
  const p = createProject(root, { pipeline: 'midi-pack', title: 'Harbour lights', answers: { genre: 'trip hop', topic: 'fog' } });
  startProject(root, p.id);
  const runner = new Runner(root); runner.acquire();
  let changes = 0; runner.on('change', () => { changes++; });
  await runner.loop();
  gates.respond(root, p.id, 'blueprint', 1, { verdict: 'approve' });
  const before = changes;
  await runner.loop();
  runner.release();
  const drafts = readEvents(root, p.id, { limit: 1000 }).filter((e) => e.event === 'DRAFT');
  assert.deepEqual(drafts.map((e) => e.details.split(':')[0]), ['the drums', 'the bass', 'the pad']);
  // Every bridge command reached the page: at least one change per draft, beyond the session's own start and end.
  assert.ok(changes - before >= drafts.length + 2, `${changes - before} changes`);

  const socketPath = join(tmpdir(), `mfx-live-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true, runner: false });
  t.after(() => srv.close());
  const get = (path) => new Promise((ok, fail) => {
    request({ socketPath, method: 'GET', path, headers: { host: 'localhost' } }, (res) => {
      let d = ''; res.on('data', (c) => { d += c; }); res.on('end', () => ok({ status: res.statusCode, json: JSON.parse(d || '{}') }));
    }).on('error', fail).end();
  });
  const ms = (await get(`/api/projects/${p.id}/music`)).json;
  assert.deepEqual(ms.draft.layers.map((l) => l.name), ['the drums', 'the bass', 'the pad']);
  assert.deepEqual(ms.draft.layers.map((l) => l.parts), [[10], [1], [2]]);
  assert.equal(ms.draft.check.ok, true);
  assert.equal(ms.draft.parts, 3);
  const dr = await get(`/api/projects/${p.id}/music/draft`);
  assert.equal(dr.status, 200);
  assert.equal(dr.json.tracks.length, 3);
  assert.ok(dr.json.sections.length);

  const other = createProject(root, { pipeline: 'song', title: 'Nothing yet', answers: { genre: 'house', topic: 'x' } });
  const none = await get(`/api/projects/${other.id}/music/draft`);
  assert.equal(none.status, 400);
  assert.match(none.json.error, /nothing drafted yet/);
});

// The engine's draft for real: layers add up in order, a layer drafted again is replaced, a passing check ends the
// list, the next version starts a new one, and a score that doesn't evaluate fails.
test('live build: strudel.mjs draft writes the score so far, layer by layer', { skip: !process.env.MFX_TEST_STRUDEL && 'set MFX_TEST_STRUDEL to run' }, (t) => {
  const chrome = setup.setupStatusSync(tempStudio(t)).chrome;
  if (!chrome.ok) return t.skip('no Chrome');
  const dir = mkdtempSync(join(tmpdir(), 'mfx-draft-')); t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, 'music'));
  writeFileSync(join(dir, 'music/blueprint.json'), JSON.stringify({ title: 'Glass Harbour', bpm: 96, meter: '4/4', bars: 8, scale: 'A:minor',
    sections: [{ name: 'Fog', start_bar: 1, end_bar: 4, function: 'establish: bass and pad alone', intensity: 5 }, { name: 'Lamp', start_bar: 5, end_bar: 8, function: 'release: drums and the hook arrive', intensity: 5 }],
    chart: [{ bars: [1, 8], chords: ['Am7', 'Fmaj7', 'C', 'G'] }],
    roles: [{ channel: 1, harmony: 'bass', name: 'Sub' }, { channel: 2, harmony: 'chords', name: 'Pad' }, { channel: 10, harmony: 'drums', name: 'Drums' }] }));
  const head = 'setcpm(96 / 4)\nconst cue = (p) => p.filterWhen((t) => t < 8)\n';
  const drums = 'drums: cue(note("[36 ~ 38 ~], [42*4]").velocity(.9)).midichan(10)\n';
  const sub = 'sub: cue(note("<a1 f1 c2 g1>").velocity(.8)).midichan(1)\n';
  const pad = 'pad: cue(note("<[a3,c4,e4,g4] [a3,c4,e4,f4] [g3,c4,e4] [g3,b3,d4]>").velocity(.6)).midichan(2)\n';
  const env = { ...process.env, MFX_STRUDEL: process.env.MFX_TEST_STRUDEL, MFX_CHROME: chrome.path, MFX_SOCKET: '', MFX_TOKEN: '' };
  const run = (...args) => spawnSync('node', [join(REPO, 'pipelines/_shared/skills/music/strudel.mjs'), ...args], { cwd: dir, env, encoding: 'utf8', timeout: 120_000 });
  const draft = () => JSON.parse(readFileSync(join(dir, 'music/draft.json'), 'utf8'));
  const score = (s) => writeFileSync(join(dir, 'music/score.strudel.js'), head + s);

  let r = run('draft'); assert.equal(r.status, 1); assert.match(r.stderr, /name the layer/);
  score(drums);
  r = run('draft', '--layer', 'the beat'); assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /Draft 1 \(the beat\): Drums/);
  assert.match(r.stdout, /Still to come: Sub \(ch 1\), Pad \(ch 2\)/);
  assert.match(r.stdout, /not in a studio session/);
  score(drums + sub);
  run('draft', '--layer', 'the bass');
  score(drums + sub.replace('g1>', 'e1>'));
  r = run('draft', '--layer', 'the bass'); assert.equal(r.status, 0, r.stdout + r.stderr);
  let d = draft();
  assert.deepEqual(d.layers.map((l) => l.name), ['the beat', 'the bass'], 'a layer drafted again is replaced, in place');
  assert.deepEqual(d.layers.map((l) => l.parts), [[10], [1, 10]]);
  assert.equal(d.tracks.find((x) => x.channel === 1).notes.length, 8);
  assert.equal(d.check, null);
  score(drums + sub + pad);
  r = run('draft', '--layer', 'the chords'); assert.equal(r.status, 0);
  r = run('check'); assert.equal(r.status, 0, r.stdout + r.stderr);
  d = draft();
  assert.equal(d.check.ok, true);
  assert.equal(d.layers.length, 3, 'the check keeps the layers');
  r = run('draft', '--layer', 'the beat, again'); assert.equal(r.status, 0);
  assert.deepEqual(draft().layers.map((l) => l.name), ['the beat, again'], 'after a passing check, a new version starts a new list');

  score(drums + 'sub: cue(note("<a1 f1".velocity(.8)).midichan(1)\n');
  r = run('draft', '--layer', 'broken'); assert.equal(r.status, 1);
  assert.match(r.stderr, /didn't evaluate/);
});

// Inside a session, the draft tells the studio itself: the status line names the layer and the page hears of it.
test('live build: in a session, strudel.mjs draft reaches the studio through the bridge', { skip: !process.env.MFX_TEST_STRUDEL && 'set MFX_TEST_STRUDEL to run' }, async (t) => {
  const root = tempStudio(t);
  const chrome = setup.setupStatusSync(root).chrome;
  if (!chrome.ok) return t.skip('no Chrome');
  const { id, work } = await songAtBlueprint(root);
  const { openBridge } = await import('../src/bridge.mjs');
  const { loadProject } = await import('../src/projects.mjs');
  let changes = 0;
  const bridge = await openBridge({ root, projectId: id, sessionId: 'test', workdir: work, renders: null, sessionEnv: {}, onChange: () => { changes++; } });
  t.after(() => bridge.close());
  writeFileSync(join(work, 'music/blueprint.json'), JSON.stringify({ title: 'Night drive', bpm: 90, meter: '4/4', bars: 4, scale: 'A:minor',
    sections: [{ name: 'All', start_bar: 1, end_bar: 4, function: 'the whole test piece, one groove', intensity: 5 }],
    chart: [{ bars: [1, 4], chords: ['Am', 'F', 'C', 'G'] }], roles: [{ channel: 10, harmony: 'drums', name: 'Drums' }] }));
  writeFileSync(join(work, 'music/score.strudel.js'), 'setcpm(90 / 4)\ndrums: note("[36 ~ 38 ~]").filterWhen((t) => t < 4).midichan(10)\n');
  const { spawn } = await import('node:child_process');
  const child = spawn('node', [join(REPO, 'pipelines/_shared/skills/music/strudel.mjs'), 'draft', '--layer', 'the beat'], {
    cwd: work, env: { ...process.env, ...bridge.env, MFX_STRUDEL: process.env.MFX_TEST_STRUDEL, MFX_CHROME: chrome.path } });
  let out = ''; child.stdout.on('data', (d) => { out += d; }); child.stderr.on('data', (d) => { out += d; });
  const code = await new Promise((ok) => child.on('exit', ok));
  assert.equal(code, 0, out);
  assert.match(out, /The owner sees it on the project page now/);
  assert.equal(loadProject(root, id).status.text, 'Writing the score: the beat');
  assert.ok(changes >= 1);
});
