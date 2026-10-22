// The app studio's Idea stage (src/quickidea.mjs) on the demo backend: the four questions in the chat, the brief after
// the last answer, a change from the chat, and nothing after the Idea stage runs.
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tempStudio } from './helpers.mjs';

process.env.MFX_PRODUCT = 'apps';
const { createService } = await import('../src/service.mjs');
const { Runner } = await import('../src/runner.mjs');
const { loadProject, projectPaths } = await import('../src/projects.mjs');
const { stepView } = await import('../src/gates.mjs');
const { projectPipeline } = await import('../src/projects.mjs');
const { applyMove } = await import('../src/svc/chat.mjs');

const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

test('Idea: four questions in the chat, then the brief; later stages never run', async (t) => {
  const root = tempStudio(t);
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
  const tick = async () => { svc.poke(0); await sleep(30); };
  const runner = new Runner(root);
  const state = (id, k) => stepView(loadProject(root, id), projectPipeline(root, id)).find((s) => s.key === k).state;

  const made = await call('project.new', { pipeline: 'app', title: 'Recipe Box', idea: 'Keep my family recipes in one place', backend: 'demo' });
  const id = made.project?.id || made.id;
  await sleep(50);
  await runner.loop();
  await tick();
  // the questions wait in the chat, each with a suggestion
  const asked = out.filter((m) => m.event === 'chat.message').map((m) => m.data).find((r) => r.card?.kind === 'submission');
  assert.ok(asked?.card.waiting);
  assert.deepEqual(asked.card.questions.map((q) => q.id), ['problem', 'users', 'must_do', 'must_never']);
  assert.ok(asked.card.questions.every((q) => q.default));
  assert.equal(state(id, 'idea'), 'blocked');

  for (const q of asked.card.questions) await call('chat.answer', { question: q.id, value: q.id === 'users' ? 'My family' : '' });
  await tick();
  await runner.loop();
  assert.equal(state(id, 'idea'), 'in_review');
  const brief = await call('idea.get');
  assert.match(brief.brief, /^# Demo app · Brief v1/);
  assert.match(brief.brief, /## What it must never do/);

  // a change asked in the chat sends the brief back, and the next run writes v2
  applyMove(root, id, { kind: 'change', text: 'It must also work offline' });
  assert.equal(state(id, 'idea'), 'changes');
  await runner.loop();
  assert.equal((await call('idea.get')).version, 2);
  assert.ok(existsSync(join(projectPaths(root, id).work, 'brief.md')));

  // Next approves the brief; Requirements isn't built yet, so nothing runs for it
  await call('stage.set', { stage: 1, from: 0, done: true });
  assert.equal(state(id, 'idea'), 'approved');
  assert.equal(state(id, 'requirements'), 'ready');
  assert.notEqual(loadProject(root, id).state, 'queued');
  const sessions = loadProject(root, id).usage.sessions;
  await runner.loop();
  assert.equal(loadProject(root, id).usage.sessions, sessions);
  assert.ok(!readFileSync(join(projectPaths(root, id).work, 'JOURNAL.md'), 'utf8').includes('requirements'));
});
