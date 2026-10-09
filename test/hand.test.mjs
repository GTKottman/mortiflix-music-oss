// Hand edits: notes the owner draws in the review room's editor go into the score exactly (lib/edit.mjs handEdit,
// verifyNotes; src/edits.mjs makeNotesEdit), and one real round trip through the web server where Strudel is installed.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdirSync, readFileSync, symlinkSync } from 'node:fs';
import { join } from 'node:path';
import { request } from 'node:http';
import { tmpdir } from 'node:os';
import { randomBytes } from 'node:crypto';
import { tempStudio } from './helpers.mjs';
import { handEdit, renderEdits, stripEdits, verifyNotes, GRID } from '../pipelines/_shared/skills/music/lib/edit.mjs';
import { listEdits } from '../src/edits.mjs';

const SCORE = `setcpm(120 / 4)
pad: note("<[c3,e3,g3] [a2,c3,e3]>").midichan(3)
drums: note("36 42 38 42").velocity(.8).midichan(10)
`;

test('hand edits: exact notes in a block of their own, held over the bar line; bar by bar for an older engine', () => {
  const chord = [{ pitch: 57, start: 4, dur: 8, velocity: 0.7 }, { pitch: 64, start: 4, dur: 8, velocity: 0.7 }, { pitch: 72, start: 5.5, dur: 0.25, velocity: 0.9 }];
  const e = handEdit(chord, [2], 4);
  assert.equal(GRID, 24);
  // Each bar as mini-notation (a voice per held note), cut at the bar line, with the loudness of each onset.
  assert.deepEqual(e.bars, { 2: { notes: '[57@96], [64@96], [~@36 72@6 ~@54]', velocity: '0.7@36 0.9@60' } });
  assert.deepEqual(e.notes[0], { pitch: 57, start: 4, dur: 8, velocity: 0.7 });
  const r = renderEdits(SCORE, [{ channel: 3, ...e, status: 'tried' }], 6);
  assert.match(r.code, /pad: \(note\("<\[c3,e3,g3\] \[a2,c3,e3\]>"\)\.midichan\(3\)\n\)\.mask\(\/\*mfx-keep\*\/"<1 0 1!4>"\)/);
  assert.match(r.code, /owner_notes_ch3: stack\(\n {2}note\("~@96 57@192 ~@288"\)\.velocity\("~@96 0\.7@192 ~@288"\),/);
  assert.match(r.code, /\)\.slow\(6\)\.filterWhen\(\(t\) => t < 6\)\.midichan\(3\)/);
  assert.doesNotMatch(r.code, /owner_ch3:/); // no bar of Claude's on that channel
  assert.equal(stripEdits(r.code), SCORE);
  // An empty bar: silenced.
  assert.deepEqual(handEdit([], [3], 4).bars, { 3: { notes: '~', velocity: '0' } });
});

test('hand edits: a later edit wins its bars, whichever kind', () => {
  const hand = { channel: 10, ...handEdit([{ pitch: 36, start: 8, dur: 0.25, velocity: 1 }], [3], 4), status: 'kept' };
  const claude = { channel: 10, bars: { 3: { notes: '36 38 36 38', velocity: '.9' } }, status: 'tried' };
  const later = renderEdits(SCORE, [hand, claude], 6).code;
  assert.match(later, /owner_ch10: note\("<~!2 \[36 38 36 38\] ~!3>"\)/);
  assert.doesNotMatch(later, /owner_notes_ch10/);
  const other = renderEdits(SCORE, [claude, hand], 6).code;
  assert.doesNotMatch(other, /owner_ch10/);
  assert.match(other, /owner_notes_ch10: stack/);
});

test('hand edits: verified note for note, and nothing else may move', () => {
  const h = (begin, end, ch, note, velocity = 0.8) => ({ begin, end, value: { note, velocity, midichan: ch } });
  const before = [h(0, 0.25, 10, 36), h(2, 2.25, 10, 36), h(2.5, 2.75, 10, 38), h(0, 1, 3, 'c3')];
  const wanted = [{ channel: 10, pitch: 36, start: 8, dur: 0.25, velocity: 1 }, { channel: 10, pitch: 39, start: 9.5, dur: 0.25, velocity: 0.6 }];
  const cells = new Map([[10, new Set([3])]]);
  const good = [h(0, 0.25, 10, 36), h(2, 2.0625, 10, 36, 1), h(2.375, 2.4375, 10, 39, 0.6), h(0, 1, 3, 'c3')];
  assert.deepEqual(verifyNotes(before, good, cells, wanted, 4), { ok: true, problems: [] });
  const off = [h(0, 0.25, 10, 36), h(2, 2.0625, 10, 36, 1), h(2.375, 2.4375, 10, 40, 0.6), h(0, 1, 3, 'c3')];
  assert.match(verifyNotes(before, off, cells, wanted, 4).problems[0], /2 notes in the edited bars didn't come out as drawn/);
  const moved = [...good.slice(0, 3), h(0, 1, 3, 'd3')];
  assert.match(verifyNotes(before, moved, cells, wanted, 4).problems[0], /2 notes outside the edited bars moved/);
});

// The real thing: a demo piece, its final MIDI under review, notes drawn by hand on two instruments at once.
test('hand edits: drawn notes go into the song exactly, the review\'s MIDI follows, undo and keep take the whole change', { skip: !process.env.MFX_TEST_STRUDEL && 'set MFX_TEST_STRUDEL to run' }, async (t) => {
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
  const socketPath = join(tmpdir(), `mfx-hand-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true, runner: false });
  t.after(() => srv.close());
  const call = (method, path, body) => new Promise((ok, fail) => {
    const req = request({ socketPath, method, path, headers: { host: 'localhost', 'content-type': 'application/json', ...(method !== 'GET' ? { 'x-mortiflix': '1' } : {}) } }, (res) => {
      let d = ''; res.on('data', (c) => { d += c; }); res.on('end', () => ok({ status: res.statusCode, json: JSON.parse(d || '{}') }));
    });
    req.on('error', fail); req.end(body ? JSON.stringify(body) : undefined);
  });
  const st = (await call('GET', `/api/projects/${p.id}/music/edits`)).json;
  assert.equal(st.can, true, JSON.stringify(st));
  assert.ok(st.roles.length >= 2 && st.roles.every((r) => 'job' in r && 'channel' in r));

  const pp = projectPaths(root, p.id);
  const notesFile = join(pp.work, 'music', 'notes.json');
  // The demo writes its notes without Strudel: capture them through the engine first, so "before" is what Strudel plays.
  const { spawnSync } = await import('node:child_process');
  const { REPO } = await import('../src/studio.mjs');
  const env = { ...process.env, MFX_STRUDEL: process.env.MFX_TEST_STRUDEL, MFX_CHROME: (await import('../src/setup.mjs')).setupStatusSync(root).chrome.path };
  const c = spawnSync(process.execPath, [join(REPO, 'pipelines/_shared/skills/music/strudel.mjs'), 'capture'], { cwd: pp.work, env, encoding: 'utf8' });
  assert.equal(c.status, 0, c.stdout + c.stderr);
  const before = JSON.parse(readFileSync(notesFile, 'utf8'));
  const pitched = before.tracks.filter((x) => x.channel !== 10);
  assert.ok(pitched.length >= 2, 'two pitched parts to move a note between');
  const [a, b] = pitched;
  const reviewed = join(pp.state, gates.submissions(root, p.id).find((s) => s.step === 'final').items.find((i) => i.kind === 'midi').file);
  const midiBefore = readFileSync(reviewed);

  // Bar 3 of the drums: four kicks. Bar 4 of part a: its notes go to part b, and part a holds one note over bars 4-5.
  const inBar = (tr, bar) => tr.notes.filter((n) => n.start >= (bar - 1) * 4 && n.start < bar * 4);
  const given = inBar(a, 4);
  assert.ok(given.length, 'part a plays in bar 4');
  const notes = [
    ...[0, 1, 2, 3].map((k) => ({ channel: 10, pitch: 36, start: 8 + k, dur: 0.25, velocity: 1 })),
    ...given.map((n) => ({ channel: b.channel, pitch: n.pitch, start: n.start, dur: n.dur, velocity: n.velocity })),
    ...inBar(b, 4).map((n) => ({ channel: b.channel, pitch: n.pitch, start: n.start, dur: n.dur, velocity: n.velocity })),
    { channel: a.channel, pitch: 60, start: 12, dur: 6, velocity: 0.5 },
  ];
  const cells = [{ channel: 10, bar: 3 }, { channel: a.channel, bar: 4 }, { channel: b.channel, bar: 4 }];
  const r = await call('POST', `/api/projects/${p.id}/music/notes-edit`, { cells, notes });
  assert.equal(r.status, 200, JSON.stringify(r.json));
  assert.equal(r.json.edits.length, 3);
  assert.ok(r.json.edits.every((e) => e.batch === r.json.batch && e.kind === 'notes' && e.facts.length));

  // Exactly as drawn (on the grid), and everything else as it was.
  const after = JSON.parse(readFileSync(notesFile, 'utf8'));
  const tr = (m, ch) => m.tracks.find((x) => x.channel === ch)?.notes || [];
  const key = (n) => `${n.pitch}@${Math.round(n.start * 24)}+${Math.round(n.dur * 24)}`;
  assert.deepEqual(inBar({ notes: tr(after, 10) }, 3).map(key), ['36@192+6', '36@216+6', '36@240+6', '36@264+6']);
  assert.deepEqual(inBar({ notes: tr(after, a.channel) }, 4).map(key), ['60@288+144']); // held over the bar line
  assert.equal(inBar({ notes: tr(after, b.channel) }, 4).length, given.length + inBar(b, 4).length);
  const outside = (m) => m.tracks.flatMap((x) => x.notes.filter((n) => !((x.channel === 10 && n.start >= 8 && n.start < 12) || ([a.channel, b.channel].includes(x.channel) && n.start >= 12 && n.start < 16))).map((n) => `${x.channel}:${key(n)}`)).sort();
  assert.deepEqual(outside(after), outside(before));
  assert.notDeepEqual(readFileSync(reviewed), midiBefore);
  assert.match(readFileSync(join(pp.work, 'music', 'score.strudel.js'), 'utf8'), /owner_notes_ch10: stack/);

  // Undo takes the whole change back, on all three parts.
  const u = await call('POST', `/api/projects/${p.id}/music/edits/${r.json.edits[0].id}/undo`);
  assert.equal(u.status, 200, JSON.stringify(u.json));
  assert.equal(u.json.undone.length, 3);
  const every = (m) => m.tracks.flatMap((x) => x.notes.map((n) => `${x.channel}:${key(n)}:${n.velocity}`)).sort();
  assert.deepEqual(every(JSON.parse(readFileSync(notesFile, 'utf8'))), every(before));

  // Again, and keep: all three are kept and remembered.
  const r2 = await call('POST', `/api/projects/${p.id}/music/notes-edit`, { cells, notes });
  assert.equal(r2.status, 200, JSON.stringify(r2.json));
  await call('POST', `/api/projects/${p.id}/music/edits/${r2.json.edits[1].id}/keep`);
  assert.deepEqual(listEdits(root, p.id).filter((e) => e.batch === r2.json.batch).map((e) => e.status), ['kept', 'kept', 'kept']);
  const taste = JSON.parse(readFileSync(join(root, 'taste.json'), 'utf8'));
  assert.equal(taste.differences.filter((x) => x.status === 'kept' && x.by_hand).length, 3);

  // A note outside the bars it says it changed is refused.
  const bad = await call('POST', `/api/projects/${p.id}/music/notes-edit`, { cells: [{ channel: 10, bar: 2 }], notes: [{ channel: 10, pitch: 36, start: 0, dur: 1 }] });
  assert.equal(bad.status, 400); assert.match(bad.json.error, /outside the bars you changed/);
});
