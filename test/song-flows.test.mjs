// Flows: the built-in set (each one possible to follow), picking one with taste, pinning it to a project, rating,
// saving a piece as a new flow, sharing; and the engine holding a plan and a score to a flow.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { tempStudio } from './helpers.mjs';
import { zip } from '../src/zip.mjs';
import * as flows from '../src/flows.mjs';
import { createProject, startProject, loadProject, projectPaths, readEvents } from '../src/projects.mjs';
import { prepareWorkdir, writeTorch } from '../src/torch.mjs';
import { flowPlanProblems, flowScoreProblems, curveAt, RULES } from '../pipelines/_shared/skills/music/lib/flow.mjs';
import { intensity } from '../pipelines/_shared/skills/music/lib/score.mjs';
import { TECHNIQUES } from '../pipelines/_shared/skills/music/lib/techniques.mjs';

const seeded = (seed = 1) => () => { seed = (seed * 16807) % 2147483647; return (seed - 1) / 2147483646; };

// A blueprint that follows a flow: sections spread over 48 bars with intensities off its curve, the materials its
// rules ask for, its techniques plus free picks.
function followingPlan(f) {
  const n = f.for === 'kit' ? 6 : Math.min(Math.max(f.sections?.min ?? 5, 6), f.sections?.max ?? 6);
  const rules = f.shape.rules;
  let lens = Array(n).fill(48 / n);
  if (rules.includes('false_stop')) lens = [10, 10, 10, 2, 10, 6].slice(0, n);
  if (rules.includes('quiet_before_peak')) lens = [8, 8, 6, 2, 14, 10].slice(0, n);
  let bar = 1;
  const sections = lens.map((len, i) => { const s = { name: `S${i + 1}`, start_bar: bar, end_bar: bar + len - 1, function: 'what happens here', intensity: 5 }; bar += len; return s; });
  const bars = bar - 1;
  const mid = (s, i) => (f.for === 'kit' ? i / (n - 1) : (s.start_bar - 1 + (s.end_bar - s.start_bar + 1) / 2) / bars);
  sections.forEach((s, i) => { s.intensity = f.shape.curve ? Math.round(curveAt(f.shape.curve, mid(s, i))) : 5; });
  if (rules.includes('pairs')) sections.forEach((s, i) => { s.intensity = i % 2 ? 7 : 3; });
  if (rules.includes('alternating')) sections.forEach((s, i) => { s.intensity = [4, 6, 3, 8, 2, 9, 1, 10][i]; });
  if (rules.includes('false_stop')) { sections[3].intensity = 0; sections[2].intensity = 8; sections[4].intensity = 9; }
  if (rules.includes('rondo')) ['A', 'B', 'A', 'C', 'A', 'B'].forEach((m, i) => { if (sections[i]) sections[i].material = m; });
  if (rules.includes('merge')) ['A', 'B', 'A', 'B', 'A+B', 'A'].forEach((m, i) => { if (sections[i]) sections[i].material = m; });
  if (rules.includes('through')) sections.forEach((s, i) => { s.material = String.fromCharCode(65 + i); });
  const free = ['Syncopation', 'Legato', 'Staccato', 'Arpeggio', 'Ghost Note', 'Accent', 'Swell', 'Thirds'].filter((t) => !f.techniques.required.includes(t) && !f.techniques.avoid.includes(t)).slice(0, f.techniques.free);
  const techniques = [...f.techniques.required, ...free].map((name) => {
    const k = TECHNIQUES[name].kind;
    return { name, part: k === 'pair' ? [1, 2] : 1, ...(k === 'section' ? { where: [sections.at(-2).name] } : {}), how: 'how it happens here' };
  });
  return { flow: f.slug, bars, bpm: 100, meter: '4/4', loops: f.for === 'kit', sections, techniques,
    motif: { at: { channel: 1, bar: 1, beats: 4 } }, roles: [{ channel: 1, harmony: 'melody', name: 'Lead' }, { channel: 2, harmony: 'melody', name: 'Answer' }, { channel: 10, harmony: 'drums', name: 'Drums' }] };
}

test('flows: the built-in set validates, each one can be followed, and a plan that ignores it can\'t', () => {
  const all = flows.listFlows(null);
  assert.equal(all.filter((f) => f.error).length, 0, all.filter((f) => f.error).map((f) => `${f.slug}: ${f.error}`).join('\n'));
  assert.equal(all.filter((f) => f.for === 'song').length, 12);
  assert.equal(all.filter((f) => f.for === 'kit').length, 6);
  for (const f of all) {
    assert.ok(Object.keys(flows.BUCKETS).includes(f.bucket), `${f.slug}: bucket ${f.bucket}`);
    assert.ok(f.feels.every((x) => flows.FEELS.includes(x)), `${f.slug}: feels ${f.feels}`);
    assert.ok(readFileSync(join(f.dir, 'FLOW.md'), 'utf8').length > 300, `${f.slug}: FLOW.md`);
    assert.deepEqual(flowPlanProblems(followingPlan(f), f), [], `${f.slug} can't be followed`);
  }
  // Every bucket has a song flow, and the kit flows cover at least three.
  for (const b of Object.keys(flows.BUCKETS)) assert.ok(all.some((f) => f.for === 'song' && f.bucket === b), b);
  assert.ok(new Set(all.filter((f) => f.for === 'kit').map((f) => f.bucket)).size >= 3);

  const burn = all.find((f) => f.slug === 'slow-burn');
  const flat = followingPlan(burn);
  flat.flow = 'drop'; flat.sections.forEach((s) => { s.intensity = 5; });
  flat.sections[2].intensity = 2;
  flat.techniques = flat.techniques.filter((t) => t.name !== 'Pedal Tone').concat([{ name: 'Drop', where: ['S5'], how: 'a big drop' }]);
  const p = flowPlanProblems(flat, burn);
  assert.ok(p.some((x) => /set "flow": "slow-burn"/.test(x)), p.join('\n'));
  assert.ok(p.some((x) => /don't follow the flow's curve/.test(x)));
  assert.ok(p.some((x) => /no section drops below/.test(x)));
  assert.ok(p.some((x) => /it needs Pedal Tone/.test(x)));
  assert.ok(p.some((x) => /it avoids Drop/.test(x)));
});

test('flows: the studio\'s pick leans toward what you loved, away from what you just had, and says why', (t) => {
  const root = tempStudio(t);
  const counts = (n, opts) => { const c = {}; const rng = seeded(7); for (let i = 0; i < n; i++) { const { flow } = flows.pickFlow(root, 'song', { ...opts, rng }); c[flow.slug] = (c[flow.slug] || 0) + 1; } return c; };
  const fresh = counts(600);
  assert.equal(Object.keys(fresh).length, 12, 'with no history every flow comes up');
  assert.ok(Object.values(fresh).every((v) => v > 20));

  // Love "rondo" three times (its pieces delivered and rated): it comes up far more often.
  for (let i = 0; i < 3; i++) {
    const p = createProject(root, { pipeline: 'song', title: `R${i}`, answers: { genre: 'g', topic: 't' }, flow: 'rondo' });
    startProject(root, p.id);
    flows.rateProject(root, p.id, 'love');
  }
  // …then a piece in another flow, so rondo isn't the very latest.
  for (const slug of ['staircase', 'drop']) startProject(root, createProject(root, { pipeline: 'song', title: slug, answers: { genre: 'g', topic: 't' }, flow: slug }).id);
  const after = counts(600);
  assert.ok(after.rondo > fresh.rondo * 2, `rondo ${fresh.rondo} → ${after.rondo}`);
  assert.ok((after.drop || 0) < fresh.drop / 2, `the last flow used is held back: ${fresh.drop} → ${after.drop}`);
  const { flow, why } = flows.pickFlow(root, 'song', { rng: () => 0 });
  assert.ok(flow && why.length > 5);
  const loved = flows.pickFlow(root, 'song', { among: { bucket: 'cycles' }, rng: seeded(3) });
  assert.equal(loved.flow.bucket, 'cycles');
  assert.equal(flows.pickFlow(root, 'kit', { among: { feel: 'hypnotic' }, rng: seeded(2) }).flow.feels.includes('hypnotic'), true);
  assert.throws(() => flows.pickFlow(root, 'song', { among: { feel: 'nonexistent' } }), /no flow for songs/);

  const stats = flows.flowStats(root);
  assert.equal(stats.rondo.love, 3);
  assert.equal(stats.rondo.used, 3);
});

test('flows: a project pins its flow; the session gets it in flow/ and in CLAUDE.md, with your taste', (t) => {
  const root = tempStudio(t);
  assert.throws(() => createProject(root, { pipeline: 'song', title: 'x', flow: 'energy-ladder' }), /is a flow for loop kits/);
  assert.throws(() => createProject(root, { pipeline: 'song', title: 'x', flow: 'nope' }), /no flow "nope"/);
  const none = createProject(root, { pipeline: 'song', title: 'No flow', flow: 'none' });
  assert.equal(none.flow, null);

  const p = createProject(root, { pipeline: 'midi-pack', title: 'Pinned', answers: { genre: 'g', topic: 't' }, flow: 'pendulum' });
  assert.deepEqual({ slug: p.flow.slug, picked: p.flow.picked }, { slug: 'pendulum', picked: 'you' });
  assert.ok(existsSync(join(projectPaths(root, p.id).flow, 'flow.json')));
  assert.match(readEvents(root, p.id)[0].details, /flow: Pendulum \(your choice\)/);

  const kit = createProject(root, { pipeline: 'loop-kit', title: 'Kit', answers: { genre: 'g', feel: 'f' }, flow: 'shape:builds', rng: seeded(4) });
  assert.equal(kit.flow.bucket, 'builds');
  assert.equal(kit.flow.picked, 'studio');
  assert.ok(['energy-ladder', 'layer-stack'].includes(kit.flow.slug));

  startProject(root, p.id);
  // Something the owner loved before, so the brief carries it.
  const old = createProject(root, { pipeline: 'song', title: 'Old', answers: { genre: 'g', topic: 't' }, flow: 'rondo' });
  startProject(root, old.id);
  mkdirSync(join(projectPaths(root, old.id).work, 'music'), { recursive: true });
  writeFileSync(join(projectPaths(root, old.id).work, 'music', 'blueprint.json'), JSON.stringify({ techniques: [{ name: 'hocket' }, { name: 'Walking Bass' }] }));
  flows.rateProject(root, old.id, 'love');
  prepareWorkdir(root, p.id);
  assert.ok(existsSync(join(projectPaths(root, p.id).work, 'flow', 'FLOW.md')));
  const md = writeTorch(root, p.id, { backend: 'demo', reason: 'test' });
  assert.match(md, /## The flow: how this piece moves/);
  assert.match(md, /\*\*Pendulum\*\* \(the owner chose it\)/);
  assert.match(md, /Set `"flow": "pendulum"`/);
  assert.match(md, /intensity swings up and down/);
  assert.match(md, /The owner loved pieces with: Hocket, Walking Bass/);
  assert.match(md, /Used in their last pieces \(reach past these\): Hocket, Walking Bass/);
  assert.equal(loadProject(root, old.id).rating, 'love');
  assert.throws(() => flows.rateProject(root, old.id, 'meh'), /rate it love, fine, no/);
});

test('flows: a piece you made becomes a flow of your own, and flows travel as zips', (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'song', title: 'Glass harbour', answers: { genre: 'g', topic: 't' }, flow: 'slow-burn' });
  assert.throws(() => flows.saveFlowFrom(root, p.id, { name: 'Harbour' }), /no blueprint yet/);
  const w = projectPaths(root, p.id).work;
  mkdirSync(join(w, 'music'), { recursive: true });
  const bp = { bars: 32, bpm: 90, meter: '4/4', dramatic_reading: 'fog -> light', ending: 'a held chord', motif: { role: 'Bell', rhythm: 'long-short-short' },
    sections: [{ name: 'Fog', start_bar: 1, end_bar: 8, function: 'pad and pedal only', intensity: 2 }, { name: 'Lamps', start_bar: 9, end_bar: 20, function: 'the bell enters', intensity: 5 }, { name: 'Light', start_bar: 21, end_bar: 32, function: 'everything', intensity: 9 }],
    techniques: [{ name: 'Ostinato', part: 2, how: 'the bell loops' }, { name: 'Pedal Tone', part: 1, how: 'the sub holds D' }] };
  writeFileSync(join(w, 'music', 'blueprint.json'), JSON.stringify(bp));
  writeFileSync(join(w, 'music', 'check.json'), JSON.stringify({ techniques: [{ name: 'Ostinato', kind: 'notes', found: true }, { name: 'Pedal Tone', kind: 'notes', found: true }, { name: 'Vibrato', kind: 'sound', found: true }] }));
  const mine = flows.saveFlowFrom(root, p.id, { name: 'Harbour light' });
  assert.equal(mine.slug, 'harbour-light');
  assert.equal(mine.source, 'yours');
  assert.equal(mine.bucket, 'builds', 'keeps the bucket of the flow it came from');
  assert.deepEqual(mine.techniques.required, ['Ostinato', 'Pedal Tone']);
  assert.ok(mine.shape.rules.includes('rising') && mine.shape.rules.includes('peak_late'));
  assert.deepEqual(mine.sections, { min: 2, max: 4 });
  assert.match(readFileSync(join(mine.dir, 'FLOW.md'), 'utf8'), /\*\*Lamps\*\* \(12 bars, intensity 5\): the bell enters/);
  assert.ok(flows.listFlows(root, 'song').some((f) => f.slug === 'harbour-light'));
  assert.throws(() => flows.saveFlowFrom(root, p.id, { name: 'Harbour light' }), /already have a flow/);
  assert.throws(() => flows.saveFlowFrom(root, p.id, { name: 'Rondo' }), /built-in flow's name/);
  // Usable right away, and it can be followed.
  assert.equal(createProject(root, { pipeline: 'song', title: 'Again', answers: { genre: 'g', topic: 't' }, flow: 'harbour-light' }).flow.slug, 'harbour-light');
  assert.deepEqual(flowPlanProblems({ ...bp, flow: 'harbour-light', techniques: [...bp.techniques, { name: 'Legato', part: 2, how: 'the bell rings on' }, { name: 'Accent', part: 2, how: 'the first of each bar' }] }, mine), []);

  const x = flows.exportFlow(root, 'harbour-light');
  const other = tempStudio(t);
  const added = flows.addFlow(other, x.zip);
  assert.deepEqual({ slug: added.slug, for: added.for }, { slug: 'harbour-light', for: 'song' });
  assert.throws(() => flows.addFlow(other, x.zip), /already has a flow/);
  assert.equal(flows.addFlow(other, x.zip, { replace: true }).slug, 'harbour-light');
  assert.throws(() => flows.addFlow(other, zip([{ name: 'evil/flow.json', data: Buffer.from(readFileSync(join(mine.dir, 'flow.json'))) }, { name: 'evil/../../x.sh', data: Buffer.from('rm') }])), /two files|unsafe/);
});

test('flows: the score is held to the flow\'s shape, measured', () => {
  const n = (pitch, start, dur = 1, velocity = 0.8) => ({ pitch, start, dur, velocity });
  const bars = (from, to, per, pitch = 60) => { const out = []; for (let b = from; b < to; b++) for (let k = 0; k < per; k++) out.push(n(pitch + (k % 3), b * 4 + (k * 4) / per, 4 / per)); return out; };
  const sec = (name, a, b, intensity, extra = {}) => ({ name, start_bar: a, end_bar: b, function: 'x', intensity, ...extra });
  const piece = (tracks, barsN) => ({ beats_per_bar: 4, bars: barsN, tracks: Object.entries(tracks).map(([ch, notes]) => ({ channel: Number(ch), name: `ch${ch}`, kind: Number(ch) === 10 ? 'drums' : 'pitched', notes })) });
  const run = (bp, tracks, flow) => { const m = piece(tracks, bp.bars); return flowScoreProblems(m, bp, flow, intensity(m, bp)); };

  const rising = { slug: 'r', name: 'Rising', for: 'song', shape: { rules: ['rising'] }, techniques: { avoid: ['Pedal Tone'] } };
  const bp = { bars: 12, meter: '4/4', sections: [sec('One', 1, 4, 2), sec('Two', 5, 8, 5), sec('Three', 9, 12, 8)] };
  assert.deepEqual(run(bp, { 1: [...bars(0, 4, 1), ...bars(4, 8, 2), ...bars(8, 12, 4)], 2: bars(4, 12, 2, 72) }, rising).errors, []);
  const dip = run(bp, { 1: [...bars(0, 4, 4), ...bars(4, 8, 1), ...bars(8, 12, 4)] }, rising);
  assert.match(dip.errors[0], /"Two" measures .* below "One"/);
  const pedal = run(bp, { 1: [n(36, 0, 48)], 2: [...bars(0, 4, 1, 72), ...bars(4, 8, 2, 72), ...bars(8, 12, 4, 72)] }, rising);
  assert.ok(pedal.warnings.some((w) => /avoids Pedal Tone, and it's heard on channel 1/.test(w)), pedal.warnings.join('\n'));

  const kit = (rule) => ({ slug: 'k', name: 'Kit', for: 'kit', shape: { rules: [rule] }, techniques: {} });
  const loops = { bars: 8, meter: '4/4', loops: true, sections: [sec('L1', 1, 2, 3), sec('L2', 3, 4, 6), sec('L3', 5, 6, 3), sec('L4', 7, 8, 6)] };
  const beat = (from, to, alt = false) => { const out = []; for (let b = from; b < to; b++) out.push(n(36, b * 4, 0.25), n(alt ? 39 : 38, b * 4 + 2, 0.25), n(42, b * 4 + 1, 0.25)); return out; };
  assert.deepEqual(run(loops, { 10: beat(0, 8), 1: bars(2, 4, 4) }, kit('shared_drums')).errors, []);
  assert.match(run(loops, { 10: [...beat(0, 6), ...beat(6, 8, true).map((x) => ({ ...x, pitch: x.pitch + 1 }))] }, kit('shared_drums')).errors[0], /"L4" keeps 0% of the first loop's drums/);
  assert.match(run(loops, { 10: beat(0, 8), 1: bars(0, 8, 2), 2: bars(0, 8, 1, 72) }, kit('layers')).errors[0], /has 3 parts/);
  assert.deepEqual(run(loops, { 1: [...bars(0, 2, 1), ...bars(2, 4, 4), ...bars(4, 6, 1), ...bars(6, 8, 4)], 2: [...bars(2, 4, 2, 72), ...bars(6, 8, 2, 72)] }, kit('pairs')).errors, []);
  assert.match(run(loops, { 1: bars(0, 8, 2) }, kit('pairs')).errors[0], /"L1" measures .* not calmer than its twin "L2"/);

  const through = { slug: 't', name: 'Through', for: 'song', shape: { rules: ['through'] }, techniques: {} };
  const tb = { bars: 8, meter: '4/4', sections: [sec('A', 1, 4, 4), sec('B', 5, 8, 6)] };
  assert.match(run(tb, { 1: [...bars(0, 4, 2), ...bars(4, 8, 2)] }, through).errors[0], /"B" replays 100% of "A"/);
  const rondo = { slug: 'o', name: 'Rondo', for: 'song', shape: { rules: ['rondo'] }, techniques: {} };
  const rb = { bars: 12, meter: '4/4', sections: [sec('A1', 1, 4, 4, { material: 'A' }), sec('B', 5, 8, 6, { material: 'B' }), sec('A2', 9, 12, 4, { material: 'A' })] };
  assert.deepEqual(run(rb, { 1: [...bars(0, 4, 2), ...bars(4, 8, 3, 70), ...bars(8, 12, 2)] }, rondo).errors, []);
  assert.match(run(rb, { 1: [...bars(0, 4, 2), ...bars(4, 8, 3, 70), ...bars(8, 12, 2, 50)] }, rondo).errors[0], /"A2" is marked "A" but keeps 0%/);
  assert.equal(Object.keys(RULES).length, 15);
});

test('flows: docs/FLOWS.md lists every technique, every rule and every built-in flow', () => {
  const doc = readFileSync(new URL('../docs/FLOWS.md', import.meta.url), 'utf8');
  for (const name of Object.keys(TECHNIQUES)) assert.ok(doc.includes(`| ${name} |`), name);
  for (const r of Object.keys(RULES)) assert.ok(doc.includes(`| \`${r}\` |`), r);
  for (const f of flows.listFlows(null)) assert.ok(doc.includes(`(\`${f.slug}\`)`), f.slug);
});

test('flows: the command line lists them, puts one on a brief, and rates the piece', async (t) => {
  const { spawnSync } = await import('node:child_process');
  const root = tempStudio(t);
  const cli = (...args) => spawnSync(process.execPath, [new URL('../bin/mortiflix-music', import.meta.url).pathname, '--studio', root, ...args], { encoding: 'utf8', env: { ...process.env, NO_COLOR: '1' } });
  const listed = JSON.parse(cli('flows', '--json').stdout);
  assert.equal(listed.flows.length, 18);
  assert.match(cli('flows').stdout, /Cycles: something keeps coming back[\s\S]*Rondo/);
  assert.match(cli('flows', '--by', 'feel').stdout, /hypnotic[\s\S]*Ostinato trance/);
  assert.match(cli('flows', 'show', 'pendulum').stdout, /# Pendulum/);
  const made = cli('new', 'midi-pack', '--title', 'Tide', '--set', 'genre=dub', '--set', 'topic=the tide', '--flow', 'pendulum', '--backend', 'demo', '--yes');
  assert.equal(made.status, 0, made.stderr);
  assert.match(made.stdout, /Flow: Pendulum \(your choice\)/);
  const id = made.stdout.match(/queued as (\S+)/)[1];
  assert.match(cli('rate', id, 'love').stdout, /Love it/);
  assert.match(cli('status', id).stdout, /Flow: Pendulum \(your choice\) · you rated it: Love it/);
  assert.equal(JSON.parse(cli('status', id, '--json').stdout).rating, 'love');
  assert.notEqual(cli('rate', id, 'meh').status, 0);
  assert.notEqual(cli('new', 'midi-pack', '--title', 'x', '--set', 'genre=g', '--set', 'topic=t', '--flow', 'layer-stack', '--yes').status, 0);
});

// The engine for real (MFX_TEST_STRUDEL=<a studio's tools/strudel>): a 16-bar piece in "Slow burn", planned and checked
// with flow/flow.json beside it; then the same piece without its crescendo, which the check refuses.
test('flows: strudel.mjs holds a real score to its flow and its techniques', { skip: !process.env.MFX_TEST_STRUDEL && 'set MFX_TEST_STRUDEL to run' }, async (t) => {
  const { spawnSync } = await import('node:child_process');
  const { mkdtempSync, rmSync, cpSync } = await import('node:fs');
  const { tmpdir } = await import('node:os');
  const setup = await import('../src/setup.mjs');
  const chrome = setup.setupStatusSync(tempStudio(t)).chrome;
  if (!chrome.ok) return t.skip('no Chrome');
  const dir = mkdtempSync(join(tmpdir(), 'mfx-flow-')); t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, 'music'));
  cpSync(flows.findFlow(null, 'slow-burn').dir, join(dir, 'flow'), { recursive: true });
  const bp = { title: 'Lantern', flow: 'slow-burn', bpm: 90, meter: '4/4', bars: 16, scale: 'A:minor',
    sections: [{ name: 'Wick', start_bar: 1, end_bar: 4, function: 'the pedal and the bell alone', intensity: 2 }, { name: 'Glow', start_bar: 5, end_bar: 8, function: 'the pad joins under the bell', intensity: 4 },
      { name: 'Flame', start_bar: 9, end_bar: 12, function: 'hats arrive, the bell grows', intensity: 6 }, { name: 'Light', start_bar: 13, end_bar: 16, function: 'the beat lands under everything', intensity: 9 }],
    chart: [{ bars: [1, 16], chords: ['Am7', 'Fmaj7'] }],
    roles: [{ channel: 1, harmony: 'pedal', name: 'Sub' }, { channel: 2, harmony: 'chords', name: 'Pad' }, { channel: 3, harmony: 'melody', name: 'Bell' }, { channel: 10, harmony: 'drums', name: 'Drums' }],
    techniques: [
      { name: 'Ostinato', part: 3, how: 'the bell plays one bar, over and over' }, { name: 'Crescendo', part: 3, how: 'the bell grows from a whisper to full' },
      { name: 'Pedal Tone', part: 1, how: 'the sub holds A under both chords' }, { name: 'Legato', part: 2, how: 'the pad chords run into each other' },
      { name: 'Block Chord', part: 2, how: 'the pad strikes whole chords' }, { name: 'Backbeat', part: 10, where: ['Light'], how: 'the snare on 2 and 4 when the beat lands' }] };
  writeFileSync(join(dir, 'music/blueprint.json'), JSON.stringify(bp));
  const score = (bell) => `setcpm(90 / 4)
const cue = (p) => p.filterWhen((t) => t < 16)
sub: cue(note("a1").velocity(.7)).midichan(1)
bell: cue(note("[e5 c5 a4 c5]").velocity("${bell}")).midichan(3)
pad: cue(note("<[a3,c4,e4,g4] [a3,c4,e4,f4]>").velocity(.55).mask("<0!4 1!12>")).midichan(2)
hats: cue(note("[~ 42]*4").velocity(.5).mask("<0!8 1!8>")).midichan(10)
beat: cue(note("[36 38 36 38]").velocity(.9).mask("<0!12 1!4>")).midichan(10)
`;
  writeFileSync(join(dir, 'music/score.strudel.js'), score('<.4 .45 .5 .55 .6 .65 .7 .75 .8 .82 .84 .86 .88 .9 .92 .95>'));
  const env = { ...process.env, MFX_STRUDEL: process.env.MFX_TEST_STRUDEL, MFX_CHROME: chrome.path };
  const run = (...args) => spawnSync('node', [new URL('../pipelines/_shared/skills/music/strudel.mjs', import.meta.url).pathname, ...args], { cwd: dir, env, encoding: 'utf8', timeout: 180_000 });
  let r = run('plan'); assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(readFileSync(join(dir, 'music/BLUEPRINT.md'), 'utf8'), /## The flow[\s\S]*Slow burn[\s\S]*## Techniques/);
  r = run('check'); assert.equal(r.status, 0, r.stdout + r.stderr);
  assert.match(r.stdout, /✔ Crescendo/);
  assert.match(r.stdout, /it moves like "Slow burn"/);
  const check = JSON.parse(readFileSync(join(dir, 'music/check.json'), 'utf8'));
  assert.equal(check.flow, 'slow-burn');
  assert.ok(check.techniques.every((x) => x.found), JSON.stringify(check.techniques));
  assert.match(readFileSync(join(dir, 'music/MUSIC-SHEET.md'), 'utf8'), /## Techniques, heard[\s\S]*✔ Backbeat/);
  r = run('techniques'); assert.match(r.stdout, /What music\/notes\.json plays now/);

  writeFileSync(join(dir, 'music/score.strudel.js'), score('.7'));
  r = run('check'); assert.equal(r.status, 1);
  assert.match(r.stdout, /Crescendo is planned on channel 3 but isn't in the notes/);
});
