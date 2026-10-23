// The MIDI studio v1 (MFX_PRODUCT=midi): its nine stages, only its own pipeline, its own studio folder, and nothing
// runs yet (each stage's work comes one version at a time).
import test from 'node:test';
import assert from 'node:assert/strict';
import { tempStudio } from './helpers.mjs';

process.env.MFX_PRODUCT = 'midi';
const { createService } = await import('../src/service.mjs');
const { Runner } = await import('../src/runner.mjs');
const { loadProject, projectPipeline } = await import('../src/projects.mjs');
const { stepView } = await import('../src/gates.mjs');
const { STAGES, STAGE_NAMES, STAGE_EDIT, STAGE_RENDER } = await import('../src/svc/stages.mjs');
const { listPipelines } = await import('../src/pipelines.mjs');

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

test('MIDI studio: nine stages, the song pipeline only, nothing runs', async (t) => {
  assert.deepEqual(STAGE_NAMES, ['Idea', 'Genre', 'Song map', 'Chords', 'Shapes', 'Melody', 'Arrange', 'Feel', 'Export']);
  assert.equal(STAGES.length, 9);
  assert.equal(STAGE_EDIT, -1);
  assert.equal(STAGE_RENDER, -1);

  const root = tempStudio(t);
  assert.deepEqual(listPipelines(root).map((p) => p.slug), ['midi-song']);

  const out = [];
  const svc = createService({ root, send: (m) => out.push(m), runner: false, tickMs: 0 });
  t.after(() => svc.close());
  let n = 1;
  const call = async (type, params = {}) => {
    const id = n++;
    await svc.handle({ id, type, params });
    const r = out.find((m) => m.id === id);
    if (!r.ok) throw new Error(`${type}: ${r.error.message}`);
    return r.result;
  };
  assert.equal((await call('hello')).product, 'midi');

  const made = await call('project.new', { pipeline: 'midi-song', title: 'Night Drive', idea: 'Slow synthwave for driving at night', backend: 'demo' });
  const id = made.project?.id || made.id;
  await sleep(50);
  await new Runner(root).loop();
  svc.poke(0);
  await sleep(30);
  const steps = stepView(loadProject(root, id), projectPipeline(root, id));
  assert.equal(steps.length, 9);
  assert.ok(steps.every((s) => !s.version), 'no stage made anything');
  assert.notEqual(loadProject(root, id).state, 'queued');
});
