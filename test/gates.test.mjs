import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, mkdirSync, symlinkSync, existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { tempStudio, songAtBlueprint } from './helpers.mjs';
import { createProject, startProject, projectPaths, loadProject } from '../src/projects.mjs';
import { writeMidi } from '../pipelines/_shared/skills/music/lib/midi.mjs';
import * as gates from '../src/gates.mjs';

const checksFor = (root, id, step) => gates.stepChecks(root, id, step).map((c) => ({ id: c.id, result: 'pass' }));

test('a required intake answer blocks the start', (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'song', title: 'x', answers: { genre: 'house' } });
  assert.throws(() => startProject(root, p.id), /required/);
});

test('submit enforces items, checks, folder and review mode', async (t) => {
  const root = tempStudio(t);
  const { id, work } = await songAtBlueprint(root);
  const ok = { note: 'The plan', items: [{ path: 'music/BLUEPRINT.md', label: 'Blueprint' }], error_checks: checksFor(root, id, 'blueprint') };

  assert.throws(() => gates.submit(root, id, 'blueprint', { ...ok, note: '' }), /note/);
  mkdirSync(join(work, 'out'), { recursive: true });
  writeFileSync(join(work, 'out', 'sketch.wav'), 'RIFF');
  assert.throws(() => gates.submit(root, id, 'blueprint', { ...ok, items: [{ path: 'out/sketch.wav' }] }), /document step needs/);
  assert.throws(() => gates.submit(root, id, 'blueprint', { ...ok, error_checks: [] }), /missing/);
  assert.throws(() => gates.submit(root, id, 'blueprint', { ...ok, error_checks: [{ id: ok.error_checks[0].id, result: 'n/a' }] }), /needs a note/);
  assert.throws(() => gates.submit(root, id, 'blueprint', { ...ok, items: [{ path: '../../../etc/passwd' }] }), /outside|doesn't exist/);
  symlinkSync('/etc/hostname', join(work, 'out', 'sneaky.md'));
  assert.throws(() => gates.submit(root, id, 'blueprint', { ...ok, items: [{ path: 'out/sneaky.md' }] }), /outside/);
  assert.throws(() => gates.submit(root, id, 'final', ok), /can't start yet/);

  const r = gates.submit(root, id, 'blueprint', ok);
  assert.equal(r.version, 1);
  assert.equal(loadProject(root, id).state, 'waiting');
  // What was submitted is a copy: changing the working file doesn't change the review.
  writeFileSync(join(work, 'music', 'BLUEPRINT.md'), 'changed');
  const sub = gates.submissions(root, id)[0];
  assert.match(sub.items[0].text, /Hazy, 90 BPM/);
  assert.throws(() => gates.submit(root, id, 'blueprint', ok), /already waiting/);
});

test('a midi step needs the MIDI, and a .mid item is a midi item', (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'midi-pack', title: 'Clips', answers: { genre: 'house', topic: 'sunrise' } });
  startProject(root, p.id);
  for (const step of ['genre']) gates.stepDone(root, p.id, step, checksFor(root, p.id, step));
  const work = projectPaths(root, p.id).work;
  mkdirSync(join(work, 'music'), { recursive: true });
  writeFileSync(join(work, 'music', 'BLUEPRINT.md'), '# Clips\n\nPlan.\n');
  gates.submit(root, p.id, 'blueprint', { note: 'Plan', items: [{ path: 'music/BLUEPRINT.md' }], error_checks: checksFor(root, p.id, 'blueprint') });
  gates.respond(root, p.id, 'blueprint', 1, { verdict: 'approve' });
  for (const step of ['score', 'sketch']) gates.stepDone(root, p.id, step, checksFor(root, p.id, step));
  mkdirSync(join(work, 'out', 'music', 'midi'), { recursive: true });
  writeFileSync(join(work, 'out', 'music', 'score.wav'), 'RIFF');
  writeFileSync(join(work, 'out', 'music', 'midi', 'clips-all.mid'), writeMidi({ bpm: 120, tracks: [{ channel: 1, name: 'Bass', notes: [{ pitch: 40, start: 0, dur: 1, velocity: 0.8 }] }] }));
  const sub = { note: 'Your MIDI', error_checks: checksFor(root, p.id, 'final') };
  assert.throws(() => gates.submit(root, p.id, 'final', { ...sub, items: [{ path: 'out/music/score.wav' }] }), /midi step needs the MIDI/);
  const r = gates.submit(root, p.id, 'final', { ...sub, items: [{ path: 'out/music/midi/clips-all.mid', label: 'Arrangement' }, { path: 'out/music/score.wav', label: 'Sketch' }] });
  assert.equal(r.version, 1);
  assert.deepEqual(gates.submissions(root, p.id).at(-1).items.map((x) => x.kind), ['midi', 'audio']);
});

test('only the reviewer approves; internal steps need their checks', async (t) => {
  const root = tempStudio(t);
  const { id } = await songAtBlueprint(root);
  assert.throws(() => gates.stepDone(root, id, 'blueprint', []), /only your approval/);
  assert.throws(() => gates.stepDone(root, id, 'score', checksFor(root, id, 'score')), /can't finish yet/);
  assert.throws(() => gates.status(root, id, 'made-up'), /no status line/);
  assert.equal(gates.status(root, id, 'score').text, 'Writing the score');
});

test('every note is answered in the next version', async (t) => {
  const root = tempStudio(t);
  const { id } = await songAtBlueprint(root);
  const sub = { note: 'v', items: [{ path: 'music/BLUEPRINT.md', label: 'Blueprint' }], error_checks: checksFor(root, id, 'blueprint'), questions: [{ id: 'pick', text: 'Which?', choices: ['A', 'B'], default: 'A' }, { id: 'name', text: 'Title?' }] };
  gates.submit(root, id, 'blueprint', sub);
  assert.throws(() => gates.respond(root, id, 'blueprint', 1, { verdict: 'changes', notes: [{ text: 'darker', item: 0, paragraph: 1 }] }), /needs an answer/);
  gates.respond(root, id, 'blueprint', 1, { verdict: 'changes', notes: [{ text: 'darker', item: 0, paragraph: 1 }, { text: 'slower' }], answers: { name: 'Night drive' } });
  const p = loadProject(root, id);
  assert.equal(p.state, 'queued');
  assert.ok(existsSync(join(projectPaths(root, id).work, 'feedback', 'blueprint-v1.json')));

  assert.throws(() => gates.submit(root, id, 'blueprint', sub), /note 1 .* has no answer/);
  assert.throws(() => gates.submit(root, id, 'blueprint', { ...sub, pin_changes: [{ note: 1, change: 'darker', status: 'done' }] }), /note 2/);
  const r = gates.submit(root, id, 'blueprint', { ...sub, pin_changes: [{ note: 1, change: 'Moved the chart to A minor', status: 'done' }, { note: 2, change: 'Down to 84 BPM', status: 'partly' }] });
  assert.equal(r.version, 2);
  gates.respond(root, id, 'blueprint', 2, { verdict: 'approve', answers: { name: 'Night drive' } });
  assert.equal(loadProject(root, id).steps.blueprint.state, 'approved');
  assert.throws(() => gates.submit(root, id, 'blueprint', sub), /already approved/);
});

test('questions pause the project until answered', async (t) => {
  const root = tempStudio(t);
  const { id } = await songAtBlueprint(root);
  gates.ask(root, id, 'blueprint', 'Should the beat come in early?', { default: 'No' });
  assert.equal(loadProject(root, id).state, 'waiting');
  gates.answerQuestion(root, id, 'q1', null);
  const p = loadProject(root, id);
  assert.equal(p.state, 'queued');
  assert.equal(p.questions[0].answer, 'No');
  assert.equal(p.questions[0].used_default, true);
});

test('proposed checks join the checklist once approved', async (t) => {
  const root = tempStudio(t);
  const { id } = await songAtBlueprint(root);
  const before = gates.stepChecks(root, id, 'score').length;
  const { id: cid } = gates.proposeCheck(root, id, { title: 'No parallel fifths in the pad', how: 'The pad voicing never moves in parallel fifths', work: 'music' });
  assert.equal(gates.stepChecks(root, id, 'score').length, before);
  gates.decideCheck(root, cid, true);
  assert.equal(gates.stepChecks(root, id, 'score').length, before + 1);
  assert.throws(() => gates.proposeCheck(root, id, { title: 'x', how: 'y', work: 'colour' }), /isn't one of/);
});
