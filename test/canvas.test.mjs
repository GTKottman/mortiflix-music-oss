// The canvas's project graph: built from what a session writes, with stable ids, beat colours and every version kept.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, writeFileSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { tempStudio } from './helpers.mjs';
import { createProject, startProject, projectPaths, addIntakeFile } from '../src/projects.mjs';
import { respond } from '../src/gates.mjs';
import { Runner } from '../src/runner.mjs';
import { syncCanvas, BEAT_COLOURS } from '../src/canvas.mjs';

const write = (dir, rel, data) => { mkdirSync(join(dir, rel, '..'), { recursive: true }); writeFileSync(join(dir, rel), typeof data === 'string' ? data : JSON.stringify(data)); };

test('canvas: beats, frames and planned shots from the files, one colour per beat, versions on change', (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'explainer', title: 'Bikes', answers: { topic: 'bike lanes' } });
  const work = projectPaths(root, p.id).work;
  write(work, 'script/beats.json', { beats: [{ id: 'b01', words: 'One.', visual: 'A dot.' }, { id: 'b02', words: 'Two.' }] });
  write(work, 'out/f/sf01.svg', '<svg/>');
  write(work, 'design/frames.json', { frames: [{ id: 'sf01', file: 'out/f/sf01.svg', label: 'SF01 · Open', beats: ['b01'] }] });
  let g = syncCanvas(root, p.id);
  assert.deepEqual(g.order.beats, ['b01', 'b02']);
  assert.deepEqual(g.order.shots, ['s01', 's02']);
  assert.equal(g.nodes.b01.colour.label, BEAT_COLOURS[0].label);
  assert.equal(g.nodes.sf01.colour.label, BEAT_COLOURS[0].label);
  assert.equal(g.nodes.b02.colour.label, BEAT_COLOURS[1].label);
  assert.equal(g.nodes.s01.planned, true);
  assert.equal(g.nodes.s01.look, 'sf01');
  assert.ok(existsSync(join(projectPaths(root, p.id).state, g.nodes.sf01.versions[0].file)));
  const rev = g.rev;
  assert.equal(syncCanvas(root, p.id).rev, rev, 'nothing changed: same graph, no new rev');
  write(work, 'script/beats.json', { beats: [{ id: 'b01', words: 'One, rewritten.' }, { id: 'b02', words: 'Two.' }] });
  g = syncCanvas(root, p.id);
  assert.equal(g.nodes.b01.versions.length, 2);
  assert.equal(g.nodes.b01.current, 2);
  assert.equal(g.nodes.b02.versions.length, 1);
  // Ids are forever: a beat that disappears stays, marked missing.
  write(work, 'script/beats.json', { beats: [{ id: 'b02', words: 'Two.' }] });
  g = syncCanvas(root, p.id);
  assert.equal(g.nodes.b01.missing, true);
  assert.deepEqual(g.order.beats, ['b02']);
  assert.equal(g.nodes.b02.colour.label, BEAT_COLOURS[1].label, 'a beat keeps its colour for life');
});

test('canvas: a file outside the working folder is never copied in', (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'explainer', title: 'Escape', answers: { topic: 'x' } });
  const work = projectPaths(root, p.id).work;
  mkdirSync(join(work, 'out'), { recursive: true });
  symlinkSync('/etc/hostname', join(work, 'out', 'secret.svg'));
  write(work, 'design/frames.json', { frames: [{ id: 'sf01', file: 'out/secret.svg', beats: [] }] });
  const g = syncCanvas(root, p.id);
  assert.equal(g.nodes.sf01, undefined);
  assert.match(g.problems.map((x) => x.text).join(' '), /doesn't exist/);
});

test('canvas: the demo lines up beats, frames and shots cut from the animatic', async (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'logo-sting', title: 'Acme' });
  await addIntakeFile(root, p.id, { field: 'logo', name: 'logo.svg', buffer: Buffer.from('<svg/>') });
  startProject(root, p.id);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());
  await runner.loop();
  let g = syncCanvas(root, p.id);
  assert.deepEqual(g.order.frames, ['sf01', 'sf02', 'sf03', 'sf04']);
  assert.equal(g.nodes.sf03.beat, 'b03');
  respond(root, p.id, 'directions', 1, { verdict: 'approve' });
  await runner.loop();
  g = syncCanvas(root, p.id);
  assert.deepEqual(g.order.shots, ['s01', 's02', 's03', 's04']);
  assert.deepEqual([g.nodes.s02.versions.at(-1).from_sec, g.nodes.s02.versions.at(-1).to_sec], [2, 4]);
  assert.equal(g.nodes.s02.beat, 'b02');
});
