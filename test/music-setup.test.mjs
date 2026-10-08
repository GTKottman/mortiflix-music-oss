import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, mkdirSync, writeFileSync, readFileSync, rmSync, readdirSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { tmpdir } from 'node:os';
import { tempStudio } from './helpers.mjs';
import { hapsToChannels, checkBlueprint, checkScore, intensity, timeOf, sectionRanges, clipOf, checkLoops } from '../pipelines/_shared/skills/music/lib/score.mjs';
import { importSteps, DAW_NAMES } from '../pipelines/_shared/skills/music/lib/daw.mjs';
import { checkHarmony } from '../pipelines/_shared/skills/music/lib/harmony.mjs';
import { writeMidi, readMidi } from '../pipelines/_shared/skills/music/lib/midi.mjs';
import * as setup from '../src/setup.mjs';
import { validatePipeline } from '../src/pipelines.mjs';
import { createProject, startProject, loadProject, projectPipeline } from '../src/projects.mjs';
import { stepView } from '../src/gates.mjs';
import { prepareWorkdir, writeTorch } from '../src/torch.mjs';
import { projectPaths } from '../src/projects.mjs';

const REPO = resolve(import.meta.dirname, '..');

// An 8-bar cue in 4/4 at 120 BPM: 2 s a bar. Bars 1-4 quiet, 5-8 lifted, a hit at 8 s (bar 5).
const BP = {
  title: 'Test', bpm: 120, meter: '4/4', bars: 8, scale: 'A:minor', offset_sec: 0,
  sections: [
    { name: 'Low glow', start_bar: 1, end_bar: 4, function: 'establish: bass and pad alone', intensity: 2 },
    { name: 'The lift', start_bar: 5, end_bar: 8, function: 'release: drums and hook arrive', intensity: 8 },
  ],
  hits: [{ t_sec: 8, bar: 5, beat: 1, kind: 'direct', what: 'logo lands' }],
  chart: [{ bars: [1, 8], chords: ['Am7', 'Fmaj7', 'C', 'G'] }],
  roles: [{ channel: 1, harmony: 'bass', name: 'Bass' }, { channel: 2, harmony: 'melody', name: 'Hook' }, { channel: 10, harmony: 'drums', name: 'Drums' }],
};
const ROOTS = ['a1', 'f1', 'c2', 'g1'];
const hap = (bar, beat, dur, value) => ({ begin: bar - 1 + (beat - 1) / 4, end: bar - 1 + (beat - 1 + dur) / 4, value });
function cueHaps({ hook = 'c5', late = false } = {}) {
  const h = [];
  for (let b = 1; b <= 8 + (late ? 1 : 0); b++) h.push(hap(b, 1, 4, { note: ROOTS[(b - 1) % 4], midichan: 1, velocity: 0.8 }));
  for (let b = 5; b <= 8; b++) {
    for (let k = 1; k <= 4; k++) h.push(hap(b, k, 1, { note: (b - 1) % 4 === 0 ? 'e5' : (b - 1) % 4 === 1 ? 'a4' : (b - 1) % 4 === 2 ? hook : 'd5', midichan: 2, velocity: 0.7 + k * 0.05 }));
    for (let k = 1; k <= 8; k++) h.push(hap(b, 1 + (k - 1) / 2, 0.25, { note: k % 2 ? 36 : 42, midichan: 10, velocity: 0.9 }));
  }
  return h;
}
const manifestOf = (ch) => ({ bpm: ch.bpm, meter: '4/4', beats_per_bar: 4, bars: 8, tracks: ch.tracks.map((t) => ({ ...t, notes: t.notes.filter((n) => n.start < 32) })) });

test('music: a cue that ends, lands its hit and builds where it should passes', () => {
  assert.deepEqual(checkBlueprint(BP), []);
  const ch = hapsToChannels({ haps: cueHaps(), cps: 0.5, beatsPerCycle: 4, names: { 1: 'Bass', 2: 'Hook', 10: 'Drums' } });
  assert.equal(ch.bpm, 120);
  assert.deepEqual(ch.problems, []);
  const score = checkScore(ch, BP, { video_sec: 16 });
  assert.deepEqual(score.errors, []);
  assert.equal(score.hits[0].music_sec, 8);
  assert.equal(timeOf(BP, 5), 8);
  assert.deepEqual(intensity(manifestOf(ch), BP).problems, []);
  assert.equal(checkHarmony(manifestOf(ch), BP).ok, true);
});

test('music: the check catches looping past the end, a missed hit, a flat build and a wrong note', () => {
  const ch = hapsToChannels({ haps: cueHaps({ late: true }), cps: 0.5, beatsPerCycle: 4, names: { 1: 'Bass', 2: 'Hook', 10: 'Drums' } });
  assert.match(checkScore(ch, BP).errors.join('\n'), /Bass keeps playing after the last bar/);
  const offHit = checkScore(ch, { ...BP, hits: [{ t_sec: 8.5, bar: 5, beat: 1, what: 'late logo' }] });
  assert.match(offHit.errors.join('\n'), /late logo.*more than a frame off/);
  const flat = intensity(manifestOf(ch), { ...BP, sections: [{ ...BP.sections[0], intensity: 9 }, { ...BP.sections[1], intensity: 1 }] });
  assert.match(flat.problems[0], /"Low glow" was asked to be more intense/);
  const sour = hapsToChannels({ haps: cueHaps({ hook: 'c#5' }), cps: 0.5, beatsPerCycle: 4 });
  const h = checkHarmony(manifestOf(sour), BP);
  assert.equal(h.ok, false);
  assert.ok(h.counts['out of key and not in the chord'] >= 1);
  assert.match(checkBlueprint({ ...BP, sections: [{ ...BP.sections[0], end_bar: 3 }, BP.sections[1]] }).join('\n'), /starts at bar 5; it should start at 4/);
  const noChan = hapsToChannels({ haps: [hap(1, 1, 1, { note: 'a3' })], cps: 0.5 });
  assert.match(noChan.problems[0], /no \.midichan/);
});

test('music: the MIDI pack keeps each part on its role channel, with tempo, meter and markers', () => {
  const ch = hapsToChannels({ haps: cueHaps(), cps: 0.5, beatsPerCycle: 4, names: { 1: 'Bass', 2: 'Hook', 10: 'Drums' } });
  const buf = writeMidi({ bpm: 120, meter: '3/4', tracks: ch.tracks, markers: [{ beat: 16, text: 'HIT logo lands' }] });
  const r = readMidi(buf);
  assert.equal(r.tempo, 500000);
  assert.deepEqual(r.tracks.slice(1).map((t) => t.name), ['Bass', 'Hook', 'Drums']);
  assert.equal(r.tracks[1].notes[0].channel, 0);
  assert.equal(r.tracks[2].notes[0].channel, 1);
  assert.equal(r.tracks[3].notes[0].channel, 9);
  assert.ok(buf.includes(Buffer.from('HIT logo lands')));
});

test('music: a clip per section starts at its own bar 1 and stops at its end', () => {
  const ch = hapsToChannels({ haps: cueHaps(), cps: 0.5, beatsPerCycle: 4, names: { 1: 'Bass', 2: 'Hook', 10: 'Drums' } });
  const [low, lift] = sectionRanges(BP);
  assert.deepEqual([low.start, low.end, low.bars, lift.start, lift.end], [0, 16, 4, 16, 32]);
  const hook = ch.tracks.find((t) => t.channel === 2);
  assert.equal(clipOf(hook, low).notes.length, 0);
  const clip = clipOf(hook, lift);
  assert.equal(clip.notes.length, 16);
  assert.equal(clip.notes[0].start, 0);
  assert.ok(clip.notes.every((n) => n.start + n.dur <= 16 + 1e-9));
  // A held note is cut where its section ends.
  const held = clipOf({ channel: 1, name: 'Pad', notes: [{ pitch: 57, start: 12, dur: 8, velocity: 0.5 }] }, low);
  assert.deepEqual(held.notes.map((n) => [n.start, n.dur]), [[12, 4]]);
});

test('music: a loop kit refuses notes over the loop point and empty loops, and warns off the grid', () => {
  const kit = { ...BP, loops: true, sections: [{ ...BP.sections[0], end_bar: 3 }, { ...BP.sections[1], start_bar: 4 }] };
  const manifest = { tracks: [
    { channel: 1, name: 'Bass', notes: [{ pitch: 33, start: 0, dur: 4, velocity: 0.8 }, { pitch: 33, start: 10, dur: 4, velocity: 0.8 }] },
    { channel: 2, name: 'Hook', notes: [{ pitch: 76, start: 20, dur: 1, velocity: 0.8 }] },
  ] };
  const r = checkLoops(manifest, kit);
  assert.match(r.errors.join('\n'), /Bass \(ch 1\): 1 note rings past the end of loop "Low glow" \(first at bar 3 beat 3\)/);
  assert.match(r.warnings.join('\n'), /loop "Low glow" is 3 bars/);
  assert.match(r.warnings.join('\n'), /loop "The lift" is 5 bars/);
  const empty = checkLoops({ tracks: [{ channel: 1, name: 'Bass', notes: [{ pitch: 33, start: 0, dur: 1, velocity: 0.8 }] }] }, { ...BP, loops: true });
  assert.match(empty.errors.join('\n'), /loop "The lift" \(bars 5-8\) has no notes at all/);
  assert.deepEqual(checkLoops({ tracks: [{ channel: 1, name: 'Bass', notes: [{ pitch: 33, start: 0, dur: 2, velocity: 0.8 }, { pitch: 33, start: 16, dur: 15.9, velocity: 0.8 }] }] }, { ...BP, loops: true }), { errors: [], warnings: [] });
});

test('music: the cue sheet says how to bring the MIDI into the owner\'s DAW', () => {
  for (const id of Object.keys(DAW_NAMES)) assert.ok(importSteps(id).length >= 3, id);
  assert.match(importSteps('fl').join(' '), /File › Import › MIDI file/);
  assert.match(importSteps(null).join(' '), /Set your project's tempo/);
  assert.match(importSteps('nonsense').join(' '), /Set your project's tempo/);
  assert.match(importSteps('ableton').join(' '), /Channel 10 is drums/);
  // The studio's DAW list and the engine's agree.
  assert.deepEqual(Object.keys(setup.DAWS).filter((d) => d !== 'other').sort(), Object.keys(DAW_NAMES).sort());
});

test('setup: the DAW is a named choice; the engine is always on and sessions get both', (t) => {
  const root = tempStudio(t);
  assert.deepEqual(setup.dawConfig({}), { id: null, name: null });
  assert.deepEqual(setup.saveDaw(root, 'FL'), { id: 'fl', name: 'FL Studio' });
  assert.throws(() => setup.saveDaw(root, 'kazoo'), /which DAW\?/);
  assert.equal(setup.sessionToolsEnv(root).MFX_DAW, 'fl');
  assert.deepEqual(JSON.parse(setup.sessionToolsEnv(root).MFX_MUSIC), { engine: 'strudel', midi: true });
  assert.deepEqual(setup.saveDaw(root, 'none'), { id: null, name: null });
  assert.equal(setup.sessionToolsEnv(root).MFX_DAW, undefined);
});

test('pipelines: music review modes and work kinds, and the session brief says what the studio has', (t) => {
  assert.match(validatePipeline({ slug: 'x', name: 'X', steps: [{ key: 'a', review: 'video' }] }).join('\n'), /review must be one of questions, document, audio, midi, internal/);
  assert.match(validatePipeline({ slug: 'x', name: 'X', steps: [{ key: 'a', review: 'midi', work: ['motion'] }] }).join('\n'), /work "motion" must be one of/);
  const root = tempStudio(t);
  setup.saveDaw(root, 'ableton');
  const p = createProject(root, { pipeline: 'loop-kit', title: 'Kit', answers: { genre: 'UK garage', feel: 'late night' } });
  startProject(root, p.id);
  assert.equal(stepView(loadProject(root, p.id), projectPipeline(root, p.id))[0].state, 'ready');
  prepareWorkdir(root, p.id);
  writeTorch(root, p.id, { backend: 'demo', reason: 'test' });
  const md = readFileSync(join(projectPaths(root, p.id).work, 'CLAUDE.md'), 'utf8');
  assert.match(md, /\*\*The music engine:\*\* Strudel isn't set up yet/);
  assert.match(md, /\*\*The owner's DAW:\*\* Ableton Live/);
  assert.match(md, /making this midi in the Mortiflix Music studio/);
  const skills = readdirSync(join(projectPaths(root, p.id).work, '.claude', 'skills'));
  assert.deepEqual(skills, ['music']);
});

// A real render, only where Strudel and Chrome are installed (MFX_TEST_STRUDEL=<a studio's tools/strudel>).
test('music: strudel.mjs checks, renders and exports a real piece, with its clips and a loop kit', { skip: !process.env.MFX_TEST_STRUDEL && 'set MFX_TEST_STRUDEL to run' }, (t) => {
  const chrome = setup.setupStatusSync(tempStudio(t)).chrome;
  if (!chrome.ok) return t.skip('no Chrome');
  const dir = mkdtempSync(join(tmpdir(), 'mfx-cue-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, 'music'));
  writeFileSync(join(dir, 'music', 'blueprint.json'), JSON.stringify(BP));
  writeFileSync(join(dir, 'music', 'score.strudel.js'), `setcpm(120 / 4)
const cue = (p) => p.filterWhen((t) => t < 8)
const lift = "<0!4 1!4>"
bass: cue(note("<a1 f1 c2 g1>").s("sawtooth").lpf(500).velocity(.8)).midichan(1)
hook: cue(note("<[e5 e5 e5 e5] [a4 a4 a4 a4] [c5 c5 c5 c5] [d5 d5 d5 d5]>").s("square").velocity(".75 .8 .85 .9").gain(.2).mask(lift)).midichan(2)
drums: cue(note("36 42 36 42 36 42 36 42").s("sine").decay(.1).sustain(0).velocity(.9).mask(lift)).midichan(10)
`);
  const env = { ...process.env, MFX_STRUDEL: process.env.MFX_TEST_STRUDEL, MFX_CHROME: chrome.path, MFX_DAW: 'reaper' };
  const tool = join(REPO, 'pipelines/_shared/skills/music/strudel.mjs');
  const run = (...args) => spawnSync('node', [tool, ...args], { cwd: dir, env, encoding: 'utf8', timeout: 120_000 });
  const check = run('check', '--video-sec', '16');
  assert.equal(check.status, 0, check.stdout + check.stderr);
  assert.match(check.stdout, /Check passed/);
  assert.equal(run('render').status, 0);
  assert.ok(existsSync(join(dir, 'out/music/score.wav')));
  assert.equal(run('midi').status, 0);
  const sheet = readFileSync(join(dir, 'out/music/midi/CUE-SHEET.md'), 'utf8');
  assert.match(sheet, /## Bringing it into REAPER/);
  assert.match(sheet, /sections\/`: a clip per section/);
  const lift = join(dir, 'out/music/midi/sections/02-the-lift');
  assert.deepEqual(readdirSync(lift).sort(), ['01-bass.mid', '02-hook.mid', '10-drums.mid', 'the-lift-all.mid']);
  assert.equal(readMidi(readFileSync(join(lift, '02-hook.mid'))).tracks[1].notes[0].start, 0);
  // The same piece as a loop kit: every note ends inside its loop, so it passes; then a bass half a bar late fails.
  writeFileSync(join(dir, 'music', 'blueprint.json'), JSON.stringify({ ...BP, loops: true, length_fixed: true }));
  assert.equal(run('check').status, 0);
  const score = readFileSync(join(dir, 'music', 'score.strudel.js'), 'utf8');
  writeFileSync(join(dir, 'music', 'score.strudel.js'), score.replace('bass: cue(note("<a1 f1 c2 g1>")', 'bass: cue(note("<a1 f1 c2 g1>").late(.5)'));
  const over = run('check');
  assert.equal(over.status, 1);
  assert.match(over.stdout, /Bass \(ch 1\): 1 note rings past the end of loop "Low glow" \(first at bar 4 beat 3\)/);
});
