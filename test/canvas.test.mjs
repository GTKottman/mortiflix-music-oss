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

test('canvas: OK, Cut it and Fix; a fix starts a session that remakes only that shot, and every version is kept', async (t) => {
  const { canvasView, nodeAction, finishEdit } = await import('../src/canvas.mjs');
  const { loadProject } = await import('../src/projects.mjs');
  const { submissions } = await import('../src/gates.mjs');
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'explainer', title: 'Fixes', answers: { topic: 'tides' } });
  startProject(root, p.id);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());
  const approveUntil = async (stop) => {
    for (let k = 0; k < 10; k++) {
      await runner.loop();
      const pr = loadProject(root, p.id);
      const waiting = Object.entries(pr.steps).find(([, v]) => v.state === 'in_review');
      if (!waiting || waiting[0] === stop) return;
      const last = submissions(root, p.id).filter((s) => s.step === waiting[0]).at(-1);
      respond(root, p.id, waiting[0], last.version, { verdict: 'approve' });
    }
  };
  await approveUntil('music');
  let g = canvasView(root, p.id);
  assert.deepEqual(g.order.shots, ['s01', 's02', 's03', 's04']);
  assert.equal(g.nodes.s02.state, 'ready');
  const before = g.nodes.s02.versions.length;

  nodeAction(root, p.id, 's01', { action: 'ok' });
  nodeAction(root, p.id, 's04', { action: 'cut' });
  assert.throws(() => nodeAction(root, p.id, 's02', { action: 'fix', text: ' ' }), /say what to change/);
  const { edit } = nodeAction(root, p.id, 's02', { action: 'fix', text: 'slower, and the circle bigger' });
  assert.equal(edit.id, 'E001');
  assert.equal(loadProject(root, p.id).state, 'queued', 'a fix starts a session even while music waits for the owner');
  assert.throws(() => finishEdit(root, p.id, 'E001', { ok: true }), /no new version yet/);
  g = canvasView(root, p.id);
  assert.deepEqual([g.nodes.s01.state, g.nodes.s02.state, g.nodes.s04.state], ['approved', 'generating', 'cut']);

  await runner.loop();
  g = canvasView(root, p.id);
  assert.equal(g.nodes.s02.versions.length, before + 1, 's02 has a new version');
  assert.equal(g.nodes.s02.versions.at(-1).from_sec, 0, 'remade alone: its own clip');
  assert.equal(g.nodes.s03.versions.length, 1, 'the other shots are untouched');
  assert.equal(g.nodes.s02.state, 'ready');
  assert.equal(g.nodes.s02.edits[0].state, 'done');
  assert.equal(loadProject(root, p.id).state, 'waiting', 'back to waiting on the music review');

  nodeAction(root, p.id, 's03', { action: 'fix', text: 'please fail this one' });
  await runner.loop();
  g = canvasView(root, p.id);
  assert.equal(g.nodes.s03.state, 'needs you');
  assert.equal(g.nodes.s03.edits[0].state, 'failed');
  nodeAction(root, p.id, 's04', { action: 'clear' });
  assert.equal(canvasView(root, p.id).nodes.s04.state, 'ready');
});

test('canvas: the AI runs ahead through canvas steps; the build waits until all are approved; OK on every piece approves', async (t) => {
  const { canvasView, nodeAction } = await import('../src/canvas.mjs');
  const { loadProject } = await import('../src/projects.mjs');
  const { stepView } = await import('../src/gates.mjs');
  const { projectPipeline } = await import('../src/projects.mjs');
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'explainer', title: 'Ahead', answers: { topic: 'tides' } });
  startProject(root, p.id);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());
  const states = () => Object.fromEntries(stepView(loadProject(root, p.id), projectPipeline(root, p.id)).map((s) => [s.key, s.state]));
  await runner.loop();
  respond(root, p.id, 'brief', 1, { verdict: 'approve' });
  await runner.loop();
  // One go: script, frames, transitions and the animatic all sent, and the music made ahead of any review.
  assert.deepEqual(states(), { brief: 'approved', script: 'in_review', 'style-frames': 'in_review', transitions: 'in_review', animatic: 'in_review', music: 'in_review', build: 'blocked', final: 'blocked' });
  respond(root, p.id, 'music', 1, { verdict: 'approve' });
  await runner.loop();
  assert.equal(states().build, 'blocked', 'the build waits for every canvas step');
  // OK every frame: the style frames step approves itself.
  for (const f of canvasView(root, p.id).order.frames) nodeAction(root, p.id, f, { action: 'ok' });
  assert.equal(states()['style-frames'], 'approved');
  for (const s of ['script', 'transitions', 'animatic']) respond(root, p.id, s, 1, { verdict: 'approve' });
  await runner.loop();
  assert.equal(states().final, 'in_review', 'all approved: the build ran and the final waits');
});

test('export: "Edit the video" only on the final; it sends the final back with edit: true', async (t) => {
  const { loadProject, projectPipeline } = await import('../src/projects.mjs');
  const { stepView } = await import('../src/gates.mjs');
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'logo-sting', title: 'Edit me' });
  await addIntakeFile(root, p.id, { field: 'logo', name: 'logo.svg', buffer: Buffer.from('<svg/>') });
  startProject(root, p.id);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());
  await runner.loop();
  assert.throws(() => respond(root, p.id, 'directions', 1, { verdict: 'edit' }), /only the final/);
  respond(root, p.id, 'directions', 1, { verdict: 'approve' });
  await runner.loop();
  respond(root, p.id, 'final', 1, { verdict: 'edit' });
  const pr = loadProject(root, p.id);
  assert.equal(pr.steps.final.state, 'changes');
  assert.equal(pr.steps.final.edit, true);
  assert.equal(stepView(pr, projectPipeline(root, p.id)).find((s) => s.key === 'final').last_feedback.verdict, 'edit');
});
