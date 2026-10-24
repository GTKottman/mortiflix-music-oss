// The MIDI studio's whole song with no AI and no tokens: a fake Claude Code answers each stage's call, the real studio
// code checks it, writes the notes with real Strudel, arranges, adds the feel and exports the MIDI files.
import test from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { tempStudio } from './helpers.mjs';

process.env.MFX_PRODUCT = 'midi';
process.env.MFX_STRUDEL ||= join(process.env.HOME, 'Mortiflix', 'tools', 'strudel');
const { createProject, loadProject, projectPaths, projectPipeline } = await import('../src/projects.mjs');
const { stepView, respond } = await import('../src/gates.mjs');
const { quickMidi, quickMidiStep, applyFeel, chordProblems, mapProblems, closeMidi } = await import('../src/quickmidi.mjs');
const { reopenStage } = await import('../src/svc/stages.mjs');
test.after(() => closeMidi());

const has = existsSync(join(process.env.MFX_STRUDEL, 'node_modules', '@strudel', 'core'));

const MAP = {
  title: 'Test Song', bpm: 96, tonic: 'A', mode: 'minor', meter: '4/4', note: 'n',
  parts: [
    { id: 'drums', name: 'Drums', role: 'drums', program: 0, low: 35, high: 81 },
    { id: 'bass', name: 'Bass', role: 'bass', program: 33, low: 28, high: 55 },
    { id: 'keys', name: 'Keys', role: 'chords', program: 4, low: 48, high: 84 },
    { id: 'arp', name: 'Arp', role: 'arp', program: 81, low: 55, high: 88 },
    { id: 'pad', name: 'Pad', role: 'pad', program: 89, low: 48, high: 84 },
    { id: 'lead', name: 'Lead', role: 'lead', program: 80, low: 60, high: 88 },
  ],
  sections: [
    { name: 'Intro', kind: 'intro', bars: 4, energy: 2, parts: ['pad', 'keys'] },
    { name: 'Verse', kind: 'verse', bars: 8, energy: 3, parts: ['drums', 'bass', 'keys', 'lead'] },
    { name: 'Chorus', kind: 'chorus', bars: 8, energy: 5, parts: ['drums', 'bass', 'keys', 'arp', 'pad', 'lead'] },
    { name: 'Outro', kind: 'outro', bars: 4, energy: 1, parts: ['pad'] },
  ],
};
const ANSWERS = {
  purpose: { purpose: 'a', feel: 'b', length: 'c', like: 'd' },
  title: { title: 'Test Song', summary: 's', purpose: 'p', genre: 'synth pop', feel: 'f', length_sec: 60, like: 'l', must: ['m'], never: ['n'], note: 'n' },
  name: { name: 'Synth pop', summary: 's', bpm_min: 90, bpm_max: 120, keys: ['A minor'], meter: '4/4', swing: 0.2, drums: 'd', bass: 'b', harmony: 'h', melody: 'm', instruments: [{ role: 'bass', sound: 'synth bass', program: 38 }], form: 'f', moves: ['m'], sources: ['a', 'b', 'c'], note: 'n' },
  tonic: MAP,
  kinds_chords: { note: 'n', kinds: ['intro', 'verse', 'chorus', 'outro'].map((kind) => ({ kind, chords: ['Am7', 'Fmaj7', 'C', 'G'], bars_per_chord: 1, why: 'w' })) },
  parts: { note: 'n', parts: [
    { part: 'drums', suggested: 0, shapes: [{ name: 'four', code: 's("bd ~ sd ~, hh*8")', why: 'w' }, { name: 'busy', code: 's("bd bd sd ~, hh*16")', why: 'w' }] },
    { part: 'bass', suggested: 0, shapes: [{ name: 'roots', code: 'chord("<CHORDS>").rootNotes(2).struct("x ~ x [~ x]")', why: 'w' }, { name: 'eighths', code: 'chord("<CHORDS>").rootNotes(2).struct("x*8")', why: 'w' }] },
    { part: 'keys', suggested: 0, shapes: [{ name: 'stabs', code: 'chord("<CHORDS>").voicing().struct("~ x ~ x")', why: 'w' }, { name: 'held', code: 'chord("<CHORDS>").voicing()', why: 'w' }] },
    { part: 'arp', suggested: 0, shapes: [{ name: 'up', code: 'n("0 1 2 3 2 1 0 1").chord("<CHORDS>").voicing()', why: 'w' }, { name: 'down', code: 'n("3 2 1 0").chord("<CHORDS>").voicing()', why: 'w' }] },
    { part: 'pad', suggested: 1, shapes: [{ name: 'held', code: 'chord("<CHORDS>").voicing()', why: 'w' }, { name: 'swell', code: 'chord("<CHORDS>").voicing().struct("x ~")', why: 'w' }] },
  ] },
  octave: { octave: 5, note: 'n', kinds: ['verse', 'chorus'].map((kind) => ({ kind, code: 'n("<[0 ~ 2 1] [0 ~ ~ ~] [2 ~ 4 2] [1 ~ ~ ~]>").scale("<SCALE>")', why: 'w' })) },
};
// the fake Claude Code: answers by the schema's first required key
const fakeRun = async ({ config }) => {
  const schema = JSON.parse(config.claudeArgs[config.claudeArgs.indexOf('--json-schema') + 1]);
  const req = schema.required;
  const key = req.includes('kinds') && !req.includes('octave') ? 'kinds_chords' : req.includes('tonic') ? 'tonic' : req.includes('octave') ? 'octave' : req[0];
  return { ok: true, structured: structuredClone(ANSWERS[key]), usage: { input_tokens: 1000, output_tokens: 100 } };
};

test('MIDI flow: idea to MIDI files, every stage checked', { skip: !has && 'Strudel not installed' }, async (t) => {
  const root = tempStudio(t, { backend: 'claude-code' });
  const { id } = createProject(root, { pipeline: 'midi-song', title: 'Test Song', answers: { idea: 'a short synth pop song' } });
  const config = { backend: 'claude-code', models: {} };
  const seen = [];
  for (let guard = 0; guard < 12; guard++) {
    const view = quickMidiStep(root, id);
    if (!view) break;
    seen.push(view.key);
    const r = await quickMidi({ root, id, view, run: fakeRun, runArgs: {}, config, onActivity: () => {} });
    assert.ok(r.ok, `${view.key}: ${r.error}`);
    const sv = stepView(loadProject(root, id), projectPipeline(root, id)).find((s) => s.key === view.key);
    assert.equal(sv.state, 'in_review', view.key);
    respond(root, id, view.key, sv.version, { verdict: 'approve', answers: {} });
  }
  assert.deepEqual(seen, ['questions', 'idea', 'genre', 'song-map', 'chords', 'shapes', 'melody', 'arrange', 'feel', 'export']);
  const out = join(projectPaths(root, id).work, 'midi', 'out');
  assert.ok(existsSync(join(out, 'test-song.mid')));
  assert.equal(readdirSync(join(out, 'parts')).length, 6);
  assert.equal(readdirSync(join(out, 'sections')).length, 4);
  const mid = readFileSync(join(out, 'test-song.mid'));
  assert.equal(mid.subarray(0, 4).toString(), 'MThd');
  assert.equal(mid.readUInt16BE(10), 7, 'conductor + 6 parts');
  const fin = JSON.parse(readFileSync(join(projectPaths(root, id).work, 'midi', 'final.json'), 'utf8'));
  assert.ok(fin.parts.lead.length > 10 && fin.parts.drums.length > 50);
  assert.ok(fin.parts.bass.every((n) => n.key >= 28 && n.key <= 55), 'bass inside its range');
  // a confirmed change to the approved song map reopens it and every later stage is made again, in order
  const r = reopenStage(root, id, 'songMap', 'one more chorus');
  assert.deepEqual(r.remade, ['Chords', 'Shapes', 'Melody', 'Arrange', 'Feel', 'Export']);
  assert.equal(quickMidiStep(root, id).key, 'song-map');
  const after = stepView(loadProject(root, id), projectPipeline(root, id));
  assert.ok(after.filter((s) => ['chords', 'export'].includes(s.key)).every((s) => !s.version));
  for (const step of ['shapes', 'melody', 'chords']) assert.doesNotMatch(readFileSync(join(projectPaths(root, id).work, 'midi', `${step === 'chords' ? 'chords' : step}.md`), 'utf8'), /✗/, `${step} passed its checks`);
});

test('song map and chord checks name the problems', () => {
  const bad = { ...MAP, parts: MAP.parts.slice(0, 4), sections: [{ ...MAP.sections[0], bars: 3, parts: ['nope'] }] };
  const p = mapProblems(bad, { genre: { bpm_min: 120, bpm_max: 130 }, brief: { length_sec: 300 } });
  assert.ok(p.some((x) => /only 4 parts/.test(x)) && p.some((x) => /no lead/.test(x)) && p.some((x) => /isn't a part/.test(x)) && p.some((x) => /even number/.test(x)) && p.some((x) => /outside the genre/.test(x)) && p.some((x) => /never plays/.test(x)));
  const c = chordProblems({ kinds: [{ kind: 'verse', chords: ['Am', 'F#', 'Xyz'], bars_per_chord: 1 }] }, { ...MAP, sections: [{ name: 'V', kind: 'verse', bars: 8, parts: [] }] });
  assert.ok(c.some((x) => /F# is outside/.test(x)) && c.some((x) => /Xyz/.test(x)) && c.some((x) => /doesn't fit/.test(x)));
});

test('feel: fills into a lift, swing on the off-beats, no overlaps', () => {
  const drums = [];
  for (let b = 0; b < 8; b++) for (let i = 0; i < 8; i++) drums.push({ t: b * 3840 + i * 480, len: 480, key: i % 4 === 2 ? 38 : 42, vel: 0.8 });
  const map = { ...MAP, sections: [{ name: 'A', kind: 'verse', bars: 4, energy: 2, parts: ['drums'] }, { name: 'B', kind: 'chorus', bars: 4, energy: 5, parts: ['drums'] }], parts: [MAP.parts[0]] };
  const { parts, stats } = applyFeel({ drums }, map, { swing: 0.3, seed: 3 });
  assert.equal(stats.fills, 1);
  assert.ok(parts.drums.some((n) => n.key === 49 && Math.abs(n.t - 4 * 3840) < 10), 'crash on the lift');
  assert.ok(stats.swung > 0);
});

test('a fix round sends back only the refused part and merges it', { skip: !has && 'Strudel not installed' }, async (t) => {
  const root = tempStudio(t, { backend: 'claude-code' });
  const { id } = createProject(root, { pipeline: 'midi-song', title: 'Test Song', answers: { idea: 'x' } });
  const config = { backend: 'claude-code', models: {} };
  const asked = [];
  const run = async (args) => {
    const r = await fakeRun(args);
    const schema = JSON.parse(args.config.claudeArgs[args.config.claudeArgs.indexOf('--json-schema') + 1]);
    if (schema.required.includes('parts') && !schema.required.includes('tonic')) {
      // the keys part names notes instead of using <CHORDS>: refused
      r.structured.parts.find((p) => p.part === 'keys').shapes[0].code = 'note("c4 e4 g4")';
    }
    if (schema.required[0] === 'answer') {
      asked.push(args.prompt);
      return { ok: true, structured: { answer: { parts: [{ part: 'keys', suggested: 0, shapes: [{ name: 'fixed', code: 'chord("<CHORDS>").voicing().struct("x ~")', why: 'w' }, { name: 'held', code: 'chord("<CHORDS>").voicing()', why: 'w' }] }] } }, usage: {} };
    }
    return r;
  };
  for (let guard = 0; guard < 6; guard++) {
    const view = quickMidiStep(root, id);
    if (!view) break;
    const r = await quickMidi({ root, id, view, run, runArgs: {}, config, onActivity: () => {} });
    assert.ok(r.ok, `${view.key}: ${r.error}`);
    const sv = stepView(loadProject(root, id), projectPipeline(root, id)).find((s) => s.key === view.key);
    respond(root, id, view.key, sv.version, { verdict: 'approve', answers: {} });
    if (view.key === 'shapes') break;
  }
  assert.equal(asked.length, 1, 'one fix round');
  assert.match(asked[0], /"part":"keys"/);
  assert.doesNotMatch(asked[0], /"part":"drums"/, 'only the refused part is sent');
  const shapes = JSON.parse(readFileSync(join(projectPaths(root, id).work, 'midi', 'shapes.json'), 'utf8'));
  assert.equal(shapes.parts.length, 5, 'the other parts are kept');
  assert.equal(shapes.parts.find((p) => p.part === 'keys').shapes[0].name, 'fixed');
  assert.doesNotMatch(readFileSync(join(projectPaths(root, id).work, 'midi', 'shapes.md'), 'utf8'), /✗/);
});
