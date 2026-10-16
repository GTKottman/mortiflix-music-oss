// The song lane's timing rules (doc 19 §1–2): bars on the video's time axis, shots back to back, cut marks by the
// one-frame rule; and the API that feeds it (the song, or a draft from the score before the import).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { cpSync } from 'node:fs';
import { join } from 'node:path';
import { timeAxis, shotTimes, cutMarks } from '../web/songlane.js';
import { emptySong } from '../src/song.mjs';

test('bars and ticks land on the video\'s seconds from offsetSec', () => {
  const s = emptySong({ bpm: 120, offsetSec: 0.6 });
  const ax = timeAxis(s);
  assert.equal(ax.tpb, 3840);
  assert.equal(ax.barSec(1), 0.6);
  assert.equal(ax.barSec(3), 0.6 + 4);              // 2 bars of 4/4 at 120 = 4 s
  assert.equal(ax.tickSec(960), 0.6 + 0.5);
  const six = timeAxis(emptySong({ bpm: 90, num: 6, den: 8 }));
  assert.equal(six.tpb, 2880);
  assert.ok(Math.abs(six.barSec(2) - 2) < 1e-9);      // 3 quarter notes at 90
});

test('shots play back to back; cut ones are left out', () => {
  const graph = { order: { shots: ['s01', 's02', 's03'] }, nodes: {
    s01: { label: 'S01', current: 1, versions: [{ v: 1, from_sec: 0, to_sec: 2.5 }] },
    s02: { label: 'S02', current: 1, state: 'cut', versions: [{ v: 1, from_sec: 2.5, to_sec: 4 }] },
    s03: { label: 'S03', current: 2, versions: [{ v: 1, from_sec: 4, to_sec: 6 }, { v: 2, from_sec: 0, to_sec: 3 }] } } };
  assert.deepEqual(shotTimes(graph).map((s) => [s.id, s.from, s.to]), [['s01', 0, 2.5], ['s03', 2.5, 5.5]]);
});

test('cut marks: a hit within one frame lands, one further off is "missed by", none is a plain cut', () => {
  const s = emptySong({ bpm: 120 });
  // hit planned at bar 2 (2.0 s in the music); cuts at 2.0 s (lands), 4.0 s (hit at 4.25 s: missed), 6.0 s (none)
  s.markers.push({ id: 'h01', tick: 3840, kind: 'hit', name: 'logo', sec: 2 }, { id: 'h02', tick: 3840 * 2 + 480, kind: 'hit', name: 'title', sec: 4 });
  const shots = [0, 2, 4, 6, 8].slice(0, -1).map((t, i) => ({ id: `s0${i + 1}`, from: t, to: t + 2 }));
  assert.deepEqual(cutMarks(s, shots, 30).map((c) => [c.at, c.kind, c.text]), [[2, 'hit', 'cut · hit'], [4, 'miss', 'missed by 0.25 s'], [6, 'cut', 'cut']]);
});

test('GET /music/song: a draft from the score before the import, the song after', async (t) => {
  const { tempStudio } = await import('./helpers.mjs');
  const { createProject } = await import('../src/projects.mjs');
  const { songPaths, songView } = await import('../src/song-store.mjs');
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'explainer', title: 'Lane', answers: { topic: 'tides' } });
  assert.equal(songView(root, p.id), null);
  const sp = songPaths(root, p.id);
  for (const f of ['blueprint.json', 'notes.json', 'instruments.json']) cpSync(join(import.meta.dirname, 'fixtures', 'songs', 'four', f), join(sp.music, f));
  const draft = songView(root, p.id);
  assert.equal(draft.draft, true);
  assert.deepEqual(draft.song.patterns.map((x) => x.id), ['m01', 'm02']);
  assert.equal(draft.song.patterns[0].notes.ch02.length, 13);
});
