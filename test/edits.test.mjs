// Quick edits: the splice (lib/edit.mjs), the context engine and saved differences (src/context.mjs), the studio side
// (src/edits.mjs), and one real edit through the web server where Strudel is installed (MFX_TEST_STRUDEL).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { request } from 'node:http';
import { tmpdir } from 'node:os';
import { randomBytes } from 'node:crypto';
import { tempStudio } from './helpers.mjs';
import { REPO } from '../src/studio.mjs';
import { blocksOf, perBar, renderEdits, stripEdits, verifyEdit, describeDifference, notesText, REGION_START } from '../pipelines/_shared/skills/music/lib/edit.mjs';
import { cleanAnswer, packFileFor, listEdits } from '../src/edits.mjs';
import { saveDifference, relevantDifferences, differencesBrief, editContext } from '../src/context.mjs';

const SCORE = `setcpm(90 / 4)
const BARS = 8
const cue = (p) => p.filterWhen((t) => t < BARS)
const lift = "<0!4 1!4>"   // a mask: the hook from bar 5

bass:  cue(note("<a1 f1 c2 g1>").velocity(.8)).midichan(1)   // the root
drums: cue(note("<[36 ~ 38 ~]!4 [36 36 38 ~]!4>, [42*8]")
  .velocity(.8))
  .midichan(10)
fill: cue(note("<~!7 [45 45 43 43]>")).midichan(10)
$: cue(note("<a4 c5>").mask(lift)).midichan(3)
`;

test('edit splice: blocks, per-bar masks, the owner\'s block, and back again', () => {
  const blocks = blocksOf(SCORE);
  assert.deepEqual(blocks.map((b) => [b.label, b.channel]), [['bass', 1], ['drums', 10], ['fill', 10], ['$', 3]]);
  assert.match(blocks[1].expr, /^cue\(note[\s\S]*\.midichan\(10\)$/); // a block runs over its continuation lines
  assert.equal(perBar([1, 1, 0, 1, 1, 1]), '<1!2 0 1!3>');

  const edits = [{ id: 'a', channel: 10, bars: { 3: { notes: '[36 ~ ~ 36], [42*16]', velocity: '.9' } }, status: 'tried' }];
  const { code, missing } = renderEdits(SCORE, edits, 10);
  assert.deepEqual(missing, []);
  // Both drum blocks are silenced in bar 3 only; the bass and the hook are untouched; a trailing comment survives.
  assert.equal((code.match(/\/\*mfx-keep\*\/"<1!2 0 1!7>"/g) || []).length, 2);
  assert.match(code, /bass: {2}cue\(note\("<a1 f1 c2 g1>"\)\.velocity\(\.8\)\)\.midichan\(1\) {3}\/\/ the root/);
  assert.match(code, /owner_ch10: note\("<~!2 \[\[36 ~ ~ 36\], \[42\*16\]\] ~!7>"\)\.velocity\("<0!2 \[\.9\] 0!7>"\)\.filterWhen\(\(t\) => t < 10\)\.midichan\(10\)/);
  assert.ok(code.includes(REGION_START));
  // Idempotent, and undoing every edit gives the session's score back, byte for byte.
  assert.equal(renderEdits(code, edits, 10).code, code);
  assert.equal(stripEdits(code), SCORE);
  assert.equal(renderEdits(code, [{ ...edits[0], status: 'undone' }], 10).code, SCORE);
  // A later edit on the same bar wins; another channel gets its own block.
  const two = renderEdits(code, [...edits, { id: 'b', channel: 10, bars: { 3: { notes: '36*4' } } }, { id: 'c', channel: 1, bars: { 5: { notes: 'a1 e2' } } }], 10).code;
  assert.match(two, /owner_ch10: note\("<~!2 \[36\*4\] ~!7>"\)/);
  assert.match(two, /owner_ch1: note\("<~!4 \[a1 e2\] ~!5>"\)/);
  // A channel no block plays on is reported.
  assert.deepEqual(renderEdits(SCORE, [{ id: 'd', channel: 5, bars: { 1: { notes: 'c4' } } }], 10).missing, [5]);
});

test('edit splice: verifying an edit, and the difference in measured words', () => {
  const hap = (begin, note, chan = 10, velocity = 0.8) => ({ begin, end: begin + 0.25, value: { note, midichan: chan, velocity } });
  const base = [hap(0, 36), hap(0.5, 38), hap(2, 36), hap(2.5, 38), hap(2, 45, 1)];
  const ok = verifyEdit(base, [hap(0, 36), hap(0.5, 38), hap(2, 36), hap(2.25, 36), hap(2.5, 38), hap(2, 45, 1)], { channel: 10, bars: [3] });
  assert.equal(ok.ok, true);
  assert.equal(ok.before.length, 2); assert.equal(ok.after.length, 3);
  const leaked = verifyEdit(base, [hap(0, 36), hap(2, 36), hap(2.25, 36), hap(2.5, 38), hap(2, 45, 1)], { channel: 10, bars: [3] });
  assert.deepEqual(leaked.problems, ['1 note outside the edited bars moved']);
  assert.deepEqual(verifyEdit(base, base, { channel: 10, bars: [3] }).problems, ['the edited bars came out exactly as they were']);

  const n = (pitch, start, velocity = 0.8) => ({ pitch, start, dur: 0.25, velocity });
  const before = [n(36, 0), n(38, 1), n(36, 2), n(38, 3), ...[0, 1, 2, 3, 4, 5, 6, 7].map((i) => n(42, i / 2, 0.5))];
  const after = [n(36, 0), n(38, 1), n(36, 1.5), n(36, 2.75), n(38, 3), ...Array.from({ length: 16 }, (_, i) => n(42, i / 4, 0.5))];
  const facts = describeDifference(before, after, { drums: true });
  assert.ok(facts.includes('kick 2 → 3 hits a bar'), facts.join('; '));
  assert.ok(facts.includes('closed hat 8 → 16 hits a bar'));
  assert.ok(facts.some((f) => /kick more syncopated/.test(f)));
  assert.ok(facts.some((f) => /^\+\d+% density$/.test(f)));
  assert.match(notesText(after, { drums: true, from: 1, to: 1 }), /36 kick {7}x\.\.\. \.\.x\. \.\.\.x \.\.\.\./);
  assert.match(describeDifference([n(57, 0), n(60, 1)], [n(69, 0), n(72, 1), n(76, 2)], {}).join('; '), /range A3-C4 → A4-E5/);
});

test('quick edits: an answer is held to the marked bars and to safe mini-notation', () => {
  const a = cleanAnswer({ channel: 10, bars: { 9: { notes: '[36 ~ 38 ~], [42*8]', velocity: 0.85 } }, said: 'busier' }, { channel: null, bars: [9] });
  assert.deepEqual(a, { channel: 10, bars: { 9: { notes: '[36 ~ 38 ~], [42*8]', velocity: '0.85' } }, said: 'busier' });
  assert.equal(cleanAnswer({ channel: 3, bars: { 9: { notes: '36' } } }, { channel: 10, bars: [9] }).channel, 10); // the owner's pick wins
  assert.throws(() => cleanAnswer({ channel: 10, bars: {} }, { channel: null, bars: [9] }), /entry for bar 9/);
  // Nothing that could close the string it's spliced into.
  assert.throws(() => cleanAnswer({ channel: 10, bars: { 9: { notes: '36"); fetch("x' } } }, { channel: null, bars: [9] }), /characters/);
  assert.throws(() => cleanAnswer({ channel: 10, bars: { 9: { notes: '36\n38' } } }, { channel: null, bars: [9] }), /characters/);
  assert.throws(() => cleanAnswer({ channel: 10, bars: { 9: { notes: '36', velocity: '`x`' } } }, { channel: null, bars: [9] }), /velocity/);
  assert.throws(() => cleanAnswer({ channel: 99, bars: { 9: { notes: '36' } } }, { channel: null, bars: [9] }), /channel/);
});

test('quick edits: the reviewed MIDI is matched to the new pack by what it is', () => {
  const pack = ['harbour-lights-all.mid', '01-bass.mid', '10-drums.mid', 'sections/01-intro/intro-all.mid'];
  assert.equal(packFileFor('out/final-v1/demo-all.mid', pack), 'harbour-lights-all.mid');
  assert.equal(packFileFor('out/music/midi/01-bass.mid', pack), '01-bass.mid');
  assert.equal(packFileFor('deliver/10-drums.mid', pack), '10-drums.mid');
  assert.equal(packFileFor('x/sections/01-intro/intro-all.mid', pack), 'sections/01-intro/intro-all.mid');
  assert.equal(packFileFor('notes.mid', pack), null);
});

test('context engine: saved differences are ranked for the part, and reach a session\'s brief', (t) => {
  const root = tempStudio(t);
  const d = (id, status, genre, kind, fn, facts, at) => ({ id, status, genre, kind, part: kind === 'drums' ? 'Drums' : 'Keys', section: { name: fn, function: fn, intensity: 7 }, ask: 'busier', facts, at });
  saveDifference(root, d('1', 'kept', 'trip hop', 'drums', 'chorus', ['closed hat 8 → 16 hits a bar'], '2026-10-01'));
  saveDifference(root, d('2', 'kept', 'house', 'drums', 'verse', ['kick 4 → 4 hits a bar'], '2026-10-05'));
  saveDifference(root, d('3', 'undone', 'trip hop', 'drums', 'chorus', ['kick 2 → 5 hits a bar'], '2026-10-06'));
  saveDifference(root, d('4', 'kept', 'trip hop', 'pitched', 'chorus', ['range A3-C4 → A4-E5'], '2026-10-07'));
  const r = relevantDifferences(root, { genre: 'Bristol trip hop', kind: 'drums', section: { name: 'chorus', function: 'chorus', intensity: 8 } });
  assert.deepEqual(r.kept.map((x) => x.id), ['1', '2']); // the same genre and section first; only drums
  assert.deepEqual(r.undone.map((x) => x.id), ['3']);
  const brief = differencesBrief(root, { intake: { answers: { genre: 'trip hop' } } });
  assert.match(brief, /closed hat 8 → 16 hits a bar/);
  assert.match(brief, /tried and undid[\s\S]*kick 2 → 5/);
  // Saving the same edit again replaces it (kept after a try).
  saveDifference(root, { ...d('3', 'kept', 'trip hop', 'drums', 'chorus', ['kick 2 → 5 hits a bar'], '2026-10-08') });
  assert.equal(relevantDifferences(root, { genre: 'trip hop', kind: 'drums' }).undone.length, 0);
});

test('context engine: the brief for an edit carries the piece, the section, the code behind the bars and the owner', async (t) => {
  const root = tempStudio(t);
  const { createProject, projectPaths } = await import('../src/projects.mjs');
  const p = createProject(root, { pipeline: 'midi-pack', title: 'Harbour', answers: { genre: 'trip hop', topic: 'fog' } });
  const w = projectPaths(root, p.id).work;
  mkdirSync(join(w, 'music'), { recursive: true });
  writeFileSync(join(w, 'music', 'blueprint.json'), JSON.stringify({ title: 'Harbour', bpm: 90, meter: '4/4', bars: 8, scale: 'A minor',
    sections: [{ name: 'Fog', start_bar: 1, end_bar: 4, function: 'the bass alone, setting the scene', intensity: 2 }, { name: 'Lights', start_bar: 5, end_bar: 8, function: 'the beat and the hook arrive', intensity: 7 }],
    roles: [{ name: 'Bass', channel: 1, harmony: 'bass', job: 'roots' }, { name: 'Drums', channel: 10, harmony: 'drums', job: 'the beat' }],
    chart: [{ bars: [1, 8], chords: ['Am', 'F', 'C', 'G'] }], techniques: [{ name: 'Ghost notes', part: 10, where: ['Lights'], how: 'quiet snares between the backbeats' }] }));
  writeFileSync(join(w, 'music', 'genre.json'), JSON.stringify({ name: 'Trip hop', beat: { description: 'a slow, heavy breakbeat behind the beat', grids: [{ name: 'Main', steps: 16, parts: { kick: 'x.....x...x.....', snare: '....X.......X..g' } }] } }));
  saveDifference(root, { id: 'k', status: 'kept', genre: 'trip hop', kind: 'drums', part: 'Drums', section: { name: 'Lights', function: 'the beat and the hook arrive', intensity: 7 }, ask: 'dustier', facts: ['closed hat 8 → 4 hits a bar'], at: '2026-10-08' });
  const manifest = { bpm: 90, beats_per_bar: 4, tracks: [{ channel: 1, name: 'Bass', notes: [{ pitch: 33, start: 24, dur: 1, velocity: 0.8 }] },
    { channel: 10, name: 'Drums', kind: 'drums', notes: [{ pitch: 36, start: 24, dur: 0.25, velocity: 0.9 }, { pitch: 38, start: 25, dur: 0.25, velocity: 0.8 }] }] };
  const located = [{ channel: 10, blocks: ['drums'], tokens: [{ at: 140, line: 6, marked: 'drums: cue(note("<[«36» ~ 38 ~]!4 …' }] }, { channel: 1, blocks: ['bass'], tokens: [] }];
  const ctx = editContext(root, p.id, { from: 7, to: 7, channel: null, ask: 'a different drum beat', manifest, code: SCORE, located });
  for (const want of [/Bars 7: "a different drum beat"/, /Lights \(bars 5-8\): the beat and the hook arrive, intensity 7/, /bars 1-8: Am F C G/, /Ghost notes on Drums \(channel 10\)/,
    /drums: cue\(note/, /«36»/, /bar 7:\n {2}36 kick/, /a slow, heavy breakbeat/, /kick {3}x\.\.\.\.\.x\.\.\.x\.\.\.\.\./, /What the owner kept before[\s\S]*closed hat 8 → 4/]) assert.match(ctx.prompt, want);
  // The drums got the detail; the bass is only listed among the other parts.
  assert.doesNotMatch(ctx.prompt, /## Bass \(channel 1\)/);
  assert.match(ctx.prompt, /## The other parts in those bars\nBass \(channel 1\)/);
  assert.equal(ctx.kind, 'drums');
});

// One real edit, end to end through the web server: the demo backend writes the bars (no Claude), Strudel evaluates.
test('quick edits: mark bars, change them, the review\'s MIDI follows, undo puts it back, sending the review keeps edits', { skip: !process.env.MFX_TEST_STRUDEL && 'set MFX_TEST_STRUDEL to run' }, async (t) => {
  const root = tempStudio(t);
  mkdirSync(join(root, 'tools'), { recursive: true });
  symlinkSync(process.env.MFX_TEST_STRUDEL, join(root, 'tools', 'strudel'));
  const { createProject, startProject, projectPaths } = await import('../src/projects.mjs');
  const { Runner } = await import('../src/runner.mjs');
  const { startServer } = await import('../src/web/server.mjs');
  const gates = await import('../src/gates.mjs');
  const p = createProject(root, { pipeline: 'midi-pack', title: 'Harbour lights', answers: { genre: 'trip hop', topic: 'fog' } });
  startProject(root, p.id);
  const runner = new Runner(root); runner.acquire();
  await runner.loop();
  gates.respond(root, p.id, 'blueprint', 1, { verdict: 'approve' });
  await runner.loop();
  runner.release();
  const socketPath = join(tmpdir(), `mfx-edit-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true, runner: false });
  t.after(() => srv.close());
  const call = (method, path, body) => new Promise((ok, fail) => {
    const req = request({ socketPath, method, path, headers: { host: 'localhost', 'content-type': 'application/json', ...(method !== 'GET' ? { 'x-mortiflix': '1' } : {}) } }, (res) => {
      let d = ''; res.on('data', (c) => { d += c; }); res.on('end', () => ok({ status: res.statusCode, json: JSON.parse(d || '{}') }));
    });
    req.on('error', fail); req.end(body ? JSON.stringify(body) : undefined);
  });
  const st = (await call('GET', `/api/projects/${p.id}/music/edits`)).json;
  assert.equal(st.can, true, JSON.stringify(st)); assert.equal(st.step, 'final');

  const pp = projectPaths(root, p.id);
  const sub = gates.submissions(root, p.id).find((s) => s.step === 'final');
  const reviewed = join(pp.state, sub.items.find((i) => i.kind === 'midi').file);
  const midiBefore = readFileSync(reviewed);
  const notesBefore = JSON.parse(readFileSync(join(pp.work, 'music', 'notes.json'), 'utf8'));
  const scoreBefore = readFileSync(join(pp.work, 'music', 'score.strudel.js'), 'utf8');

  const r = await call('POST', `/api/projects/${p.id}/music/edits`, { from: 3, to: 3, ask: 'a different drum beat' });
  assert.equal(r.status, 200, JSON.stringify(r.json));
  assert.equal(r.json.edit.channel, 10);
  assert.ok(r.json.edit.facts.length);
  // The notes changed in bar 3 of the drums, and nowhere else.
  const notesAfter = JSON.parse(readFileSync(join(pp.work, 'music', 'notes.json'), 'utf8'));
  const outside = (m) => m.tracks.flatMap((tr) => tr.notes.filter((n) => tr.channel !== 10 || n.start < 8 || n.start >= 12).map((n) => `${tr.channel}:${n.pitch}:${n.start}`)).sort();
  assert.deepEqual(outside(notesAfter), outside(notesBefore));
  assert.notDeepEqual(notesAfter.tracks.find((x) => x.channel === 10).notes.filter((n) => n.start >= 8 && n.start < 12), notesBefore.tracks.find((x) => x.channel === 10).notes.filter((n) => n.start >= 8 && n.start < 12));
  // The version under review shows it.
  assert.notDeepEqual(readFileSync(reviewed), midiBefore);
  assert.deepEqual(gates.submissions(root, p.id).find((s) => s.step === 'final').owner_edits, [r.json.edit.id]);

  // Undo: the score and the reviewed MIDI are as they were.
  const midiEdited = readFileSync(reviewed);
  const u = await call('POST', `/api/projects/${p.id}/music/edits/${r.json.edit.id}/undo`);
  assert.equal(u.json.edit.status, 'undone');
  assert.equal(readFileSync(join(pp.work, 'music', 'score.strudel.js'), 'utf8').trim(), scoreBefore.trim());
  const every = (m) => m.tracks.flatMap((tr) => tr.notes.map((n) => `${tr.channel}:${n.pitch}:${n.start}:${n.velocity}`)).sort();
  assert.deepEqual(every(JSON.parse(readFileSync(join(pp.work, 'music', 'notes.json'), 'utf8'))), every(notesBefore));
  assert.notDeepEqual(readFileSync(reviewed), midiEdited);

  // Another edit, then the review goes back: it's kept, and remembered as a difference.
  const r2 = await call('POST', `/api/projects/${p.id}/music/edits`, { from: 2, to: 3, channel: 10, ask: 'busier hats' });
  assert.equal(r2.status, 200, JSON.stringify(r2.json));
  // Try another: the first try is undone, and the new one is still measured from the bars as they were.
  const r3 = await call('POST', `/api/projects/${p.id}/music/edits`, { from: 2, to: 3, channel: 10, ask: 'busier hats', retry_of: r2.json.edit.id });
  assert.equal(r3.status, 200, JSON.stringify(r3.json));
  assert.equal(r3.json.edit.before, r2.json.edit.before);
  assert.equal(listEdits(root, p.id).find((e) => e.id === r2.json.edit.id).status, 'undone');
  assert.equal((await call('POST', `/api/projects/${p.id}/reviews/final/1`, { verdict: 'approve' })).status, 200);
  assert.equal(listEdits(root, p.id).find((e) => e.id === r3.json.edit.id).status, 'kept');
  const taste = JSON.parse(readFileSync(join(root, 'taste.json'), 'utf8'));
  assert.deepEqual(taste.differences.map((x) => x.status).sort(), ['kept', 'undone', 'undone']);
  assert.ok(existsSync(join(pp.work, 'music', 'edits.json')));
  // A session that rewrites the score can't lose a kept edit: the engine puts it back on its next check.
  writeFileSync(join(pp.work, 'music', 'score.strudel.js'), readFileSync(join(pp.work, 'music', 'score.strudel.js'), 'utf8').split(REGION_START)[0]);
  const { spawnSync } = await import('node:child_process');
  const env = { ...process.env, MFX_STRUDEL: process.env.MFX_TEST_STRUDEL, MFX_CHROME: (await import('../src/setup.mjs')).setupStatusSync(root).chrome.path };
  const c = spawnSync(process.execPath, [join(REPO, 'pipelines/_shared/skills/music/strudel.mjs'), 'capture'], { cwd: pp.work, env, encoding: 'utf8' });
  assert.equal(c.status, 0, c.stdout + c.stderr);
  assert.match(c.stdout, /put the owner's 1 edit back/);
  assert.ok(readFileSync(join(pp.work, 'music', 'score.strudel.js'), 'utf8').includes('owner_ch10'));
});
