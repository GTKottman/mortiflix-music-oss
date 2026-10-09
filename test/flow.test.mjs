import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tempStudio } from './helpers.mjs';
import { createProject, startProject, addIntakeFile, loadProject, projectPaths, readEvents } from '../src/projects.mjs';
import { respond, submissions, stepView } from '../src/gates.mjs';
import { Runner } from '../src/runner.mjs';
import { projectPipeline } from '../src/projects.mjs';
import { musicStatus, musicNotes } from '../src/music.mjs';

const latest = (root, id, step) => submissions(root, id).filter((s) => s.step === step).at(-1);

test('demo backend: brief to delivered, through the real bridge, with a round of changes', async (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'midi-pack', title: 'Night drive', answers: { genre: 'trip hop', topic: 'a night drive' } });
  await addIntakeFile(root, p.id, { field: 'material', name: 'riff.mid', buffer: Buffer.from('MThd') });
  startProject(root, p.id);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());

  await runner.loop();
  let project = loadProject(root, p.id);
  assert.equal(project.state, 'waiting');
  const v1 = latest(root, p.id, 'blueprint');
  assert.equal(v1.version, 1);
  assert.equal(v1.items[0].kind, 'text');

  // The session brief was written, with the pinned pipeline and its skills beside it.
  const work = projectPaths(root, p.id).work;
  const brief = readFileSync(join(work, 'CLAUDE.md'), 'utf8');
  assert.match(brief, /# Mortiflix Music project: Night drive/);
  assert.match(brief, /The owner's DAW:\*\* not named/);
  assert.ok(existsSync(join(work, '.claude', 'skills', 'music', 'strudel.mjs')));
  assert.ok(existsSync(join(work, 'input', 'material', 'riff.mid')));
  assert.ok(existsSync(join(work, '.mortiflix', 'GATES.md')));

  respond(root, p.id, 'blueprint', 1, { verdict: 'changes', notes: [{ item: 0, paragraph: 1, text: 'slower, more space' }] });
  await runner.loop();
  const v2 = latest(root, p.id, 'blueprint');
  assert.equal(v2.version, 2);
  assert.equal(v2.pin_changes.length, 1);
  assert.match(readFileSync(join(work, 'CLAUDE.md'), 'utf8'), /slower, more space/);

  respond(root, p.id, 'blueprint', 2, { verdict: 'approve' });
  await runner.loop();
  const final = latest(root, p.id, 'final');
  assert.equal(final.items[0].kind, 'midi');
  // The demo leaves the music where the engine would, so the project page's MIDI panel works in the demo too.
  const ms = musicStatus(root, p.id);
  assert.ok(ms.midi.includes('demo-all.mid') && ms.midi.includes('CUE-SHEET.md'));
  assert.equal(ms.masters, false, 'a MIDI pack ends at the MIDI: no master to import');
  assert.equal(musicNotes(root, p.id).tracks.length, 3);

  respond(root, p.id, 'final', 1, { verdict: 'approve' });
  project = loadProject(root, p.id);
  assert.equal(project.state, 'delivered');
  assert.equal(project.deliverables.length, 1);
  assert.ok(existsSync(join(projectPaths(root, p.id).state, project.deliverables[0].file)));
  assert.ok(stepView(project, projectPipeline(root, p.id)).every((s) => ['approved', 'done'].includes(s.state)));

  const names = readEvents(root, p.id).map((e) => e.event);
  for (const e of ['CREATED', 'STARTED', 'SESSION_STARTED', 'SUBMITTED', 'CHANGES_REQUESTED', 'HANDOFF', 'APPROVED', 'SESSION_ENDED']) assert.ok(names.includes(e), e);
  assert.ok(existsSync(join(work, 'JOURNAL.md')));
});

test('a backend that never moves the project forward gets it paused, not looped', async (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'song', title: 'Stuck', answers: { genre: 'house', topic: 'hi' } });
  startProject(root, p.id);
  const runner = new Runner(root);
  const { BACKENDS } = await import('../src/runner.mjs');
  BACKENDS.stuck = { run: async () => ({ ok: false, error: 'boom' }) };
  t.after(() => { delete BACKENDS.stuck; });
  const { saveConfig } = await import('../src/studio.mjs');
  saveConfig(root, { backend: 'stuck' });
  await runner.loop();
  const project = loadProject(root, p.id);
  assert.equal(project.state, 'paused');
  assert.match(project.needs_you.text, /boom/);
  assert.equal(project.usage.sessions, 2);
});

test('renderPrefix wraps every mfx render (a machine-wide queue)', async (t) => {
  const root = tempStudio(t);
  const { saveConfig } = await import('../src/studio.mjs');
  saveConfig(root, { renderPrefix: ['sh', '-c', 'echo "wrapped:{label}" && exec "$@"', 'wrap'] });
  const p = createProject(root, { pipeline: 'song', title: 'Wrap', answers: { genre: 'house', topic: 'a render' } });
  startProject(root, p.id);
  const { openBridge } = await import('../src/bridge.mjs');
  const { RenderQueue } = await import('../src/renderq.mjs');
  const { callStudio } = await import('../src/mfx.mjs');
  const renders = new RenderQueue({ logDir: join(root, 'run') });
  const bridge = await openBridge({ root, projectId: p.id, sessionId: 's1', workdir: projectPaths(root, p.id).work, renders, sessionEnv: {} });
  t.after(() => bridge.close());
  const job = await callStudio('render', { label: 'still', argv: ['echo', 'inner'], cwd: projectPaths(root, p.id).work }, bridge.env);
  let r = job;
  while (!['done', 'failed'].includes(r.state)) r = await callStudio('render-wait', { id: job.id, seconds: 5 }, bridge.env);
  assert.equal(r.state, 'done');
  assert.match(r.output_tail, new RegExp(`wrapped:${p.id}: still`));
  assert.match(r.output_tail, /inner/);
});
