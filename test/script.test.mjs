// The Script panel's messages (script.get / script.edit / script.changed) on the demo backend: an edited line is saved
// into the script and its voice line, and only the shot and voice line that used those words are remade (v2).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { tempStudio } from './helpers.mjs';
import { createService } from '../src/service.mjs';
import { Runner } from '../src/runner.mjs';
import { loadProject } from '../src/projects.mjs';
import { autoName } from '../src/svc/script.mjs';

const hasFfmpeg = spawnSync('ffmpeg', ['-version']).status === 0;
const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

function harness(t, root) {
  const out = [];
  const svc = createService({ root, send: (m) => out.push(m), runner: false, tickMs: 0 });
  t.after(() => svc.close());
  let n = 1;
  const call = async (type, params = {}) => {
    const id = n++;
    await svc.handle({ id, type, params });
    const r = out.find((m) => m.id === id);
    if (!r.ok) { const e = new Error(`${type}: ${r.error.code}: ${r.error.message}`); e.code = r.error.code; throw e; }
    return r.result;
  };
  const events = (name) => out.filter((m) => m.event === name).map((m) => m.data);
  const tick = async () => { svc.poke(0); await sleep(30); };
  return { call, events, tick };
}

async function until(fn, { ms = 15_000, step = 25 } = {}) {
  const t0 = Date.now();
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - t0 > ms) throw new Error('timed out');
    await sleep(step);
  }
}

test('script: a beat with no name is named by its first words', () => {
  assert.equal(autoName('Open on a single shape that becomes the logo.'), 'Open on a single');
  assert.equal(autoName('  Close.  '), 'Close');
  assert.equal(autoName(''), '');
});

test('script: get, edit a line (its shot and voice line remade, v2), add, delete and restore, rename', { skip: !hasFfmpeg && 'needs ffmpeg' }, async (t) => {
  const root = tempStudio(t);
  const { call, events, tick } = harness(t, root);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());
  const run = async () => { await runner.loop(); await tick(); };

  await assert.rejects(call('script.get'), /no_project/);
  const made = await call('project.new', { preset: 'explainer', title: 'Bikes', idea: 'Why bike lanes pay off', backend: 'demo' });
  const id = made.project.id;
  await until(() => loadProject(root, id).state === 'queued');
  await run();
  await run();
  await run();

  const s = await call('script.get');
  assert.equal(s.beats.length, 4);
  assert.equal(s.writing, false);
  const b1 = s.beats[0];
  assert.deepEqual([b1.id, b1.no, b1.name, b1.name_auto], ['b01', 1, 'Open on a single', true]);
  assert.ok(b1.colour?.hex);
  assert.equal(b1.lines.length, 1);
  const l1 = b1.lines[0];
  assert.equal(l1.id, 'b01-1');
  assert.equal(l1.words, 'Open on a single shape that becomes the logo.');
  assert.deepEqual(l1.feeds.map((f) => [f.node, f.kind, f.made]), [['b01-1', 'line', true], ['s01', 'shot', true]]);
  assert.equal(l1.read_only, false);
  const g0 = await call('graph.get');
  const v0 = (n) => g0.nodes.find((x) => x.id === n).current;
  assert.equal(v0('s01'), 1);
  assert.equal(v0('b01-1'), 1);

  // Edit one line: saved into beats.json + voice/lines.json, only s01 and b01-1 are remade.
  const r = await call('script.edit', { beat: 'b01', line: 'b01-1', words: 'A dot opens its eyes and becomes the logo.' });
  assert.deepEqual(r.changed, ['b01-1']);
  assert.deepEqual(r.remaking.map((x) => x.node).sort(), ['b01-1', 's01']);
  assert.equal(events('script.changed').length, 1);
  const work = join(root, 'projects', id);
  const beats = JSON.parse(readFileSync(join(work, 'script', 'beats.json'), 'utf8'));
  assert.equal(beats.beats[0].words, 'A dot opens its eyes and becomes the logo.');
  assert.equal(beats.beats[1].words, 'Three ideas, one per beat, each on its own colour.', 'other beats untouched');
  const voice = JSON.parse(readFileSync(join(work, 'voice', 'lines.json'), 'utf8'));
  assert.equal(voice[0].script, 'A dot opens its eyes and becomes the logo.');
  const busy = await call('script.get');
  assert.deepEqual(busy.beats[0].lines[0].remaking, { shots: 1, lines: 1 });
  assert.equal(busy.beats[0].lines[0].changed, true);
  assert.deepEqual(busy.beats[1].lines[0].remaking, { shots: 0, lines: 0 });
  const chat = await call('chat.history');
  assert.match(chat.at(-1).text, /Script: 1 line changed · remaking 1 shot and 1 voice line/);

  // The AI remakes them (demo): v2 of the shot and the voice line, the others stay v1; versions kept.
  await run();
  const g1 = await call('graph.get');
  const node = (n) => g1.nodes.find((x) => x.id === n);
  assert.equal(node('s01').current, 2);
  assert.equal(node('s01').versions.length, 2);
  assert.equal(node('b01-1').current, 2);
  assert.equal(node('s02').current, 1);
  assert.equal(node('b02-1').current, 1);
  assert.equal(node('b01').title, 'A dot opens its eyes and becomes the logo.');
  const after = await call('script.get');
  assert.deepEqual(after.beats[0].lines[0].remaking, { shots: 0, lines: 0 });

  // A batch (the app's Save): rename a beat, add two lines (the second after the first, by key), delete one.
  const batch = await call('script.edit', { edits: [
    { beat: 'b02', beat_name: 'Three ideas' },
    { beat: 'b02', add_after: 'b02-1', words: 'Each one gets its own colour.', key: 'n1' },
    { beat: 'b02', add_after: 'n1', words: 'Then they line up.', key: 'n2' },
    { beat: 'b03', line: 'b03-1', delete: true },
  ] });
  assert.deepEqual(batch.added, ['b02-2', 'b02-3']);
  assert.deepEqual(batch.keys, { n1: 'b02-2', n2: 'b02-3' });
  assert.deepEqual(batch.deleted, ['b03-1']);
  const s2 = await call('script.get');
  assert.equal(s2.beats[1].name, 'Three ideas');
  assert.equal(s2.beats[1].name_auto, false);
  assert.deepEqual(s2.beats[1].lines.map((l) => l.id), ['b02-1', 'b02-2', 'b02-3']);
  assert.equal(s2.beats[2].lines.length, 0);
  const g2 = await call('graph.get');
  assert.equal(g2.nodes.find((x) => x.id === 'b03-1').owner, 'cut', 'a deleted line is not spoken');
  assert.ok(g2.nodes.find((x) => x.id === 'b02-2'), 'a new line is a voice line');
  // Undo the delete: the line and its voice line are back.
  await call('script.edit', { beat: 'b03', line: 'b03-1', delete: false });
  assert.equal((await call('script.get')).beats[2].lines[0].id, 'b03-1');
  assert.equal((await call('graph.get')).nodes.find((x) => x.id === 'b03-1').owner, null);

  // Refusals: unknown beat or line, empty words.
  await assert.rejects(call('script.edit', { beat: 'b09', line: 'b09-1', words: 'x' }), /no beat b09/);
  await assert.rejects(call('script.edit', { beat: 'b01', line: 'b01-7', words: 'x' }), /no line b01-7/);
  await assert.rejects(call('script.edit', { beat: 'b01', line: 'b01-1', words: '  ' }), /empty/);
});
