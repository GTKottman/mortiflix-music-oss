// The techniques the engine listens for: each one heard in a small written example, and not heard where it isn't.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { context, listen, checkTechniques, planProblems, canonical, TECHNIQUES, TECHNIQUE_NAMES, heard } from '../pipelines/_shared/skills/music/lib/techniques.mjs';

const n = (pitch, start, dur = 0.5, velocity = 0.8) => ({ pitch, start, dur, velocity });
const seq = (pitches, { start = 0, ioi = 0.5, dur = null, vel = 0.8 } = {}) => pitches.map((p, i) => n(p, start + i * ioi, dur ?? ioi, vel));
const BP = (extra = {}) => ({
  bars: 8, bpm: 100, meter: '4/4', scale: 'C:major',
  chart: [{ bars: [1, 8], chords: ['C', 'F', 'G', 'C'] }],
  sections: [{ name: 'A', start_bar: 1, end_bar: 4, function: 'x', intensity: 3 }, { name: 'B', start_bar: 5, end_bar: 8, function: 'x', intensity: 6 }],
  roles: [{ channel: 1, harmony: 'melody', name: 'Lead' }, { channel: 2, harmony: 'melody', name: 'Second' }, { channel: 3, harmony: 'bass', name: 'Bass' }, { channel: 10, harmony: 'drums', name: 'Drums' }],
  ...extra,
});
const piece = (tracks) => ({ beats_per_bar: 4, bars: 8, tracks: Object.entries(tracks).map(([ch, notes]) => ({ channel: Number(ch), name: `ch${ch}`, kind: Number(ch) === 10 ? 'drums' : 'pitched', notes })) });
const hear = (name, tracks, t = { part: 1 }, bp = BP()) => listen(context(piece(tracks), bp), name, t);
const yes = (name, tracks, t, bp) => { const r = hear(name, tracks, t, bp); assert.ok(r.found, `${name} should be heard: ${r.evidence}`); return r; };
const no = (name, tracks, t, bp) => { const r = hear(name, tracks, t, bp); assert.ok(!r.found, `${name} should not be heard: ${r.evidence}`); return r; };

test('techniques: every name on the owner\'s list, with how it is measured', () => {
  assert.equal(TECHNIQUE_NAMES.length, 144);
  for (const [name, def] of Object.entries(TECHNIQUES)) {
    assert.ok(['notes', 'pair', 'section', 'motif', 'sound', 'lock'].includes(def.kind), name);
    assert.ok(def.how.length >= 8, name);
  }
  assert.equal(canonical('pickup / anacrusis'), 'Pickup');
  assert.equal(canonical('ARPEGGIO'), 'Arpeggio');
  assert.equal(canonical('moonwalk'), null);
});

test('techniques: articulation and dynamics', () => {
  const eighths = (dur, vel = 0.8) => seq([60, 62, 64, 65, 67, 65, 64, 62, 60], { ioi: 0.5, dur, vel });
  yes('Staccato', { 1: eighths(0.2) }); no('Staccato', { 1: eighths(0.5) });
  yes('Legato', { 1: eighths(0.5) }); no('Legato', { 1: eighths(0.2) });
  yes('Tenuto', { 1: eighths(0.45) });
  const acc = eighths(0.5, 0.6); for (const i of [0, 4, 8]) acc[i].velocity = 0.9;
  yes('Accent', { 1: acc }); no('Accent', { 1: eighths(0.5) });
  const mar = eighths(0.5, 0.6); for (const i of [0, 3, 6]) Object.assign(mar[i], { velocity: 0.9, dur: 0.2 });
  yes('Marcato', { 1: mar });
  const sfz = eighths(0.5, 0.6); sfz[2].velocity = 0.95; sfz[3].velocity = 0.4; sfz[4].velocity = 0.4;
  yes('Sforzando', { 1: sfz });
  const bars = (vels) => vels.flatMap((v, b) => seq([60, 64, 67, 64], { start: b * 4, ioi: 1, vel: v }));
  yes('Crescendo', { 1: bars([0.4, 0.5, 0.6, 0.7]) }); no('Crescendo', { 1: bars([0.6, 0.6, 0.6, 0.6]) });
  yes('Diminuendo', { 1: bars([0.8, 0.7, 0.6, 0.5]) });
  yes('Swell', { 1: bars([0.4, 0.55, 0.7, 0.55, 0.4]) });
  yes('Fade-In', { 1: bars([0.3, 0.45, 0.6, 0.7, 0.7, 0.7, 0.7, 0.7]) });
  yes('Fade-Out', { 1: bars([0.7, 0.7, 0.7, 0.7, 0.7, 0.6, 0.45, 0.3]) });
});

test('techniques: ornaments, repeats and fast figures', () => {
  yes('Trill', { 1: seq([60, 62, 60, 62, 60, 62, 60, 62], { ioi: 0.125 }) });
  no('Trill', { 1: seq([60, 62, 60, 62, 60, 62, 60, 62], { ioi: 0.5 }) });
  yes('Neighbor Oscillation', { 1: seq([60, 62, 60, 62, 60, 62], { ioi: 0.5 }) });
  yes('Tremolo', { 1: seq([60, 60, 60, 60, 60, 60, 60, 60], { ioi: 0.125 }) });
  yes('Ratchet', { 2: seq([62, 62, 62], { ioi: 0.0625 }) }, { part: 2 });
  yes('Roll', { 10: seq([38, 38, 38, 38, 38, 38], { ioi: 0.1 }) }, { part: 10 });
  no('Roll', { 10: seq([38, 38, 38, 38], { ioi: 0.1 }) }, { part: 10 });
  const mordent = (s) => [n(60, s, 0.125), n(62, s + 0.125, 0.125), n(60, s + 0.25, 1)];
  yes('Mordent', { 1: [...mordent(0), ...mordent(4)] });
  yes('Turn', { 1: [...seq([62, 60, 59, 60], { ioi: 0.25 }), ...seq([62, 60, 59, 60], { start: 4, ioi: 0.25 })] });
  const grace = (s) => [n(61, s, 0.1), n(60, s + 0.1, 1)];
  yes('Grace Note', { 1: [...grace(0), ...grace(4)] });
  no('Grace Note', { 1: seq([61, 60, 61, 60], { ioi: 1 }) });
});

test('techniques: chords, arpeggios and figures', () => {
  yes('Arpeggio', { 1: seq([60, 64, 67, 72, 76, 79, 72, 67], { ioi: 0.5 }) });
  no('Arpeggio', { 1: seq([60, 62, 64, 65, 67, 69, 71, 72], { ioi: 0.5 }) });
  yes('Arpeggio Sweep', { 1: seq([60, 64, 67, 72, 76], { ioi: 0.25 }) });
  yes('Alberti Bass', { 3: seq([48, 55, 52, 55, 48, 55, 52, 55, 48, 55, 52, 55], { ioi: 0.5 }) }, { part: 3 });
  const triad = (s, dur = 1) => [n(60, s, dur), n(64, s, dur), n(67, s, dur)];
  yes('Block Chord', { 2: [0, 4, 8, 12].flatMap((s) => triad(s, 4)) }, { part: 2 });
  no('Block Chord', { 2: [0, 4].flatMap((s) => triad(s, 4)) }, { part: 2 });
  yes('Power Chord', { 2: [0, 2, 4, 6].flatMap((s) => [n(48, s, 1), n(55, s, 1), n(60, s, 1)]) }, { part: 2 });
  no('Power Chord', { 2: [0, 2, 4, 6].flatMap((s) => triad(s)) }, { part: 2 });
  yes('Chord Stab', { 2: [0, 1, 2, 3].flatMap((s) => triad(s, 0.25)) }, { part: 2 });
  yes('Chord Pulse', { 2: [0, 0.5, 1, 1.5].flatMap((s) => triad(s, 0.4)) }, { part: 2 });
  yes('Gate Rhythm', { 2: Array.from({ length: 8 }, (_, i) => [n(60, i * 0.25, 0.1), n(64, i * 0.25, 0.1)]).flat() }, { part: 2 });
  const strum = (s, gap) => [n(60, s, 2), n(64, s + gap, 2), n(67, s + 2 * gap, 2)];
  yes('Strumming', { 2: [0, 4, 8, 12].flatMap((s) => strum(s, 0.03)) }, { part: 2 });
  yes('Rolling Chord', { 2: [0, 4].flatMap((s) => strum(s, 0.15)) }, { part: 2 });
  no('Strumming', { 2: [0, 4, 8, 12].flatMap((s) => triad(s, 2)) }, { part: 2 });
  yes('Thirds', { 1: [[60, 64], [62, 65], [64, 67], [65, 69]].flatMap(([a, b], i) => [n(a, i), n(b, i)]) });
  yes('Sixths', { 1: [[60, 69], [62, 71], [64, 72], [65, 74]].flatMap(([a, b], i) => [n(a, i), n(b, i)]) });
  yes('Thirds', { 1: seq([60, 62, 64, 65], { ioi: 1 }), 2: seq([64, 65, 67, 69], { ioi: 1 }) }, { part: [1, 2] });
  yes('Ostinato', { 1: [0, 4, 8, 12].flatMap((s) => seq([60, 63, 67, 63], { start: s, ioi: 1 })) });
  no('Ostinato', { 1: [0, 4, 8, 12].flatMap((s, i) => seq([60 + i, 63, 67, 63], { start: s, ioi: 1 })) });
  yes('Riff', { 1: [0, 4].flatMap((s) => seq([60, 63, 65, 63], { start: s, ioi: 1 })) });
  yes('Repeated-Note Motif', { 1: [...seq([60, 60, 60, 60], { ioi: 0.25 }), ...seq([64, 64, 64, 64], { start: 4, ioi: 0.25 })] });
  yes('Pedal Tone', { 3: seq([36, 36, 36, 36], { ioi: 4, dur: 4 }) }, { part: 3 });
  no('Pedal Tone', { 3: seq([36, 41, 43, 36], { ioi: 4, dur: 4 }) }, { part: 3 });
  yes('Drone', { 3: [n(36, 0, 8)] }, { part: 3 });
  yes('Walking Bass', { 3: seq([48, 50, 52, 53, 55, 57, 59, 60, 59], { ioi: 1 }) }, { part: 3 });
  no('Walking Bass', { 3: seq([48, 48, 48, 48, 48, 48, 48, 48], { ioi: 1 }) }, { part: 3 });
});

test('techniques: runs, sequences and contours', () => {
  yes('Scalar Run', { 1: seq([60, 62, 64, 65, 67], { ioi: 0.25 }) });
  no('Scalar Run', { 1: seq([60, 62, 64, 62, 60], { ioi: 0.25 }) });
  yes('Chromatic Run', { 1: seq([60, 61, 62, 63, 64], { ioi: 0.25 }) });
  no('Chromatic Run', { 1: seq([60, 62, 64, 65, 67], { ioi: 0.25 }) });
  yes('Diatonic Run', { 1: seq([60, 62, 64, 65, 67], { ioi: 0.25 }) });
  yes('Pentatonic Run', { 1: seq([60, 62, 64, 67, 69], { ioi: 0.25 }) });
  yes('Octave Run', { 1: [60, 62, 64, 65].flatMap((p, i) => [n(p, i * 0.5), n(p + 12, i * 0.5)]) });
  yes('Glissando', { 1: seq([60, 62, 64, 65, 67, 69], { ioi: 0.1 }) });
  no('Glissando', { 1: seq([60, 62, 64, 65, 67, 69], { ioi: 0.5 }) });
  yes('Cascade', { 1: seq([79, 76, 74, 72, 69, 67], { ioi: 0.25 }) });
  const fig = (base, shape) => shape.map((d) => base + d);
  yes('Sequence', { 1: seq([...fig(60, [0, 2, 4]), ...fig(62, [0, 2, 4]), ...fig(64, [0, 2, 4])]) });
  yes('Ascending Staircase', { 1: seq([...fig(60, [0, 2, 4]), ...fig(62, [0, 2, 4]), ...fig(64, [0, 2, 4])]) });
  no('Descending Staircase', { 1: seq([...fig(60, [0, 2, 4]), ...fig(62, [0, 2, 4]), ...fig(64, [0, 2, 4])]) });
  yes('Descending Staircase', { 1: seq([...fig(67, [0, -2, -4]), ...fig(65, [0, -2, -4]), ...fig(63, [0, -2, -4])]) });
  yes('Spiral', { 1: seq([...fig(60, [0, 4, 2]), ...fig(62, [0, 4, 2]), ...fig(64, [0, 4, 2])]) });
  no('Spiral', { 1: seq([...fig(60, [0, 2, 4]), ...fig(62, [0, 2, 4]), ...fig(64, [0, 2, 4])]) });
  const phrase = (s, ps) => seq(ps, { start: s, ioi: 0.5, dur: 0.4 });
  yes('Arch', { 1: [...phrase(0, [60, 62, 64, 67, 64, 62, 60]), ...phrase(8, [60, 64, 67, 69, 67, 64, 62])] });
  no('Arch', { 1: [...phrase(0, [60, 62, 64, 65, 67, 69, 71]), ...phrase(8, [60, 62, 64, 65, 67, 69, 71])] });
  yes('Inverted Arch', { 1: [...phrase(0, [67, 65, 64, 60, 64, 65, 67]), ...phrase(8, [72, 69, 67, 64, 67, 69, 72])] });
  yes('Wave', { 1: phrase(0, [60, 64, 67, 64, 60, 64, 67, 64, 60]) });
  yes('Pendulum', { 1: seq([60, 67, 60, 67, 60, 67]) });
  yes('Zigzag', { 1: seq([60, 63, 61, 64, 62, 65]) });
  no('Zigzag', { 1: seq([60, 67, 60, 67, 60, 67]) });
  yes('Leap-and-Step', { 1: [0, 4, 8].flatMap((s) => phrase(s, [60, 67, 65])) });
  yes('Step-and-Leap', { 1: [0, 4, 8].flatMap((s) => phrase(s, [60, 62, 64, 72])) });
});

test('techniques: harmony-aware tones (the chart is C, F, G, C a bar each)', () => {
  yes('Passing Tone', { 1: [...seq([60, 62, 64], { ioi: 0.5 }), ...seq([64, 65, 67], { start: 2, ioi: 0.5 })] });
  no('Passing Tone', { 1: seq([60, 64, 67, 72], { ioi: 0.5 }) });
  yes('Neighbor Tone', { 1: [...seq([64, 65, 64], { ioi: 0.5 }), ...seq([67, 69, 67], { start: 2, ioi: 0.5 })] });
  yes('Approach-Note Targeting', { 1: [n(62, 0.5), n(64, 1), n(65, 1.5), n(64, 2), n(69, 2.5), n(67, 3)] });
  yes('Chromatic Enclosure', { 1: [n(65, 0.5, 0.25), n(63, 0.75, 0.25), n(64, 1), n(68, 2.5, 0.25), n(66, 2.75, 0.25), n(67, 3)] });
  yes('Diatonic Enclosure', { 1: [n(65, 0.5, 0.25), n(62, 0.75, 0.25), n(64, 1), n(69, 2.5, 0.25), n(65, 2.75, 0.25), n(67, 3)] });
  yes('Arpeggio Enclosure', { 1: [n(67, 0.5, 0.25), n(60, 0.75, 0.25), n(64, 1), n(72, 2.5, 0.25), n(64, 2.75, 0.25), n(67, 3)] });
  yes('Anticipation Tone', { 1: [n(60, 0, 3), n(65, 3.5), n(65, 4, 3), n(71, 7.5), n(67, 8)] });
  const sus = BP({ chart: [{ bars: [1, 8], chords: ['Csus4', 'C'] }] });
  yes('Suspension', { 1: [n(65, 0, 4), n(64, 4, 4)] }, { part: 1 }, sus);
  no('Suspension', { 1: [n(67, 0, 4), n(64, 4, 4)] }, { part: 1 }, sus);
  yes('Question-and-Answer Phrase', { 1: [...seq([60, 64, 62], { start: 0 }), ...seq([62, 64, 60], { start: 4 }), ...seq([60, 64, 67], { start: 8 }), ...seq([65, 64, 60], { start: 12 })] });
  assert.ok(hear('Chord-Tone Targeting', { 1: [] }).found, 'the harmony lock guarantees it');
});

test('techniques: rhythm and feel', () => {
  yes('Syncopation', { 1: [0.5, 1.5, 2.5, 4.5, 5.5].map((s) => n(60, s, 1)) });
  no('Syncopation', { 1: seq([60, 62, 64, 65, 67, 69], { ioi: 1 }) });
  yes('Offbeat', { 2: [0.5, 1.5, 2.5, 3.5, 4.5, 5.5, 6.5, 7.5].map((s) => n(64, s, 0.25)) }, { part: 2 });
  const kit = (snareAt) => [0, 4].flatMap((b) => [n(36, b, 0.25), n(36, b + 2, 0.25), ...snareAt.map((x) => n(38, b + x, 0.25))]);
  yes('Backbeat', { 10: kit([1, 3]) }, { part: 10 });
  no('Backbeat', { 10: kit([0, 2]) }, { part: 10 });
  yes('Triplet', { 1: seq([60, 62, 64, 65, 67, 69, 71, 72, 74], { ioi: 1 / 3 }) });
  no('Triplet', { 1: seq([60, 62, 64, 65, 67, 69, 71, 72], { ioi: 0.25 }) });
  yes('Tuplet', { 1: seq([60, 62, 64, 65, 67, 69, 71, 72, 74, 76], { ioi: 0.2 }) });
  const swung = [0, 1, 2, 3, 4, 5, 6, 7].flatMap((b) => [n(60, b, 0.6), n(62, b + 0.66, 0.3)]);
  yes('Swing', { 1: swung }); no('Swing', { 1: seq(Array(16).fill(60), { ioi: 0.5 }) });
  const ioiLine = (iois) => { let t = 0; return iois.map((d, i) => { const x = n(60 + (i % 3), t, d); t += d; return x; }); };
  yes('Dotted Rhythm', { 1: ioiLine([0.75, 0.25, 0.75, 0.25, 0.75, 0.25, 0.75, 0.25, 1]) });
  yes('Gallop', { 1: ioiLine([0.5, 0.25, 0.25, 0.5, 0.25, 0.25, 0.5, 0.25, 0.25, 0.5, 0.25, 0.25, 1]) });
  yes('Tresillo', { 3: [0, 4].flatMap((b) => [n(36, b, 1), n(36, b + 1.5, 1), n(36, b + 3, 1)]) }, { part: 3 });
  no('Tresillo', { 3: [0, 4].flatMap((b) => [n(36, b, 1), n(36, b + 1, 1), n(36, b + 2, 1)]) }, { part: 3 });
  const clave = [0, 8].flatMap((b) => [n(75, b, 0.25), n(75, b + 1.5, 0.25), n(75, b + 3, 0.25), n(75, b + 5, 0.25), n(75, b + 6, 0.25)]);
  yes('Clave', { 10: clave }, { part: 10 });
  const e516 = [0, 4].flatMap((b) => [0, 0.75, 1.5, 2.25, 3].map((x) => n(37, b + x, 0.25)));
  yes('Euclidean Rhythm', { 10: e516 }, { part: 10 });
  no('Euclidean Rhythm', { 10: [0, 4].flatMap((b) => [0, 1, 2, 3].map((x) => n(37, b + x, 0.25))) }, { part: 10 });
  yes('Polymeter', { 1: Array.from({ length: 4 }, (_, k) => seq([60, 64, 67], { start: k * 3, ioi: 1 })).flat() });
  no('Polymeter', { 1: Array.from({ length: 4 }, (_, k) => seq([60, 64, 67, 64], { start: k * 4, ioi: 1 })).flat() });
  yes('Cross-Rhythm', { 1: seq([60, 62, 64, 65, 67, 69], { ioi: 0.75 }) });
  yes('Rubato', { 1: [0.05, 1.07, 2.1, 2.93, 4.06, 5.1, 6.0, 6.92].map((s, i) => n(60 + i, s, 0.8)) });
  no('Rubato', { 1: seq([60, 62, 64, 65, 67, 69, 71, 72], { ioi: 1 }) });
  yes('Push', { 1: [0.95, 1.95, 2.95, 3.95, 4.95, 5.95].map((s) => n(60, s, 0.5)) });
  yes('Drag', { 1: [1.05, 2.05, 3.05, 4.05, 5.05, 6.05].map((s) => n(60, s, 0.5)) });
  no('Drag', { 1: [0.95, 1.95, 2.95, 3.95, 4.95, 5.95].map((s) => n(60, s, 0.5)) });
  yes('Delay', { 1: [0, 4, 8, 12].flatMap((b) => [n(60, b + 0.5, 0.5), n(62, b + 1, 0.5)]) });
  yes('Ritardando', { 1: ioiLine([0.25, 0.3, 0.35, 0.4, 0.5, 0.6]) });
  yes('Accelerando', { 1: ioiLine([0.6, 0.5, 0.4, 0.35, 0.3, 0.25]) });
  yes('Pickup', { 1: [...seq([67, 69], { start: 3.5, ioi: 0.5 }), n(72, 4.5, 1), ...seq([67, 69], { start: 11.5, ioi: 0.5 })] });
  yes('Anticipation', { 3: [0, 4, 8].map((b) => n(48, b + 3.5, 1)) }, { part: 3 });
  no('Anticipation', { 3: [0, 4, 8].map((b) => n(48, b + 4, 1)) }, { part: 3 });
});

test('techniques: drums', () => {
  const ghosts = [...[0, 1, 2, 3, 4, 5].map((b) => n(38, b, 0.25, 0.9)), ...[0.75, 1.75, 2.75, 3.75].map((s) => n(38, s, 0.1, 0.25))];
  yes('Ghost Note', { 10: ghosts }, { part: 10 }); no('Ghost Note', { 10: ghosts.filter((x) => x.velocity > 0.5) }, { part: 10 });
  yes('Dead Note', { 2: [0, 1, 2, 3].map((s) => n(52, s, 0.05, 0.3)) }, { part: 2 });
  yes('Palm Mute', { 2: seq(Array(8).fill(40), { ioi: 0.25, dur: 0.2 }) }, { part: 2 });
  const flam = (s) => [n(38, s, 0.1, 0.4), n(38, s + 0.04, 0.25, 0.9)];
  yes('Flam', { 10: [...flam(0), ...flam(2)] }, { part: 10 });
  const drag = (s) => [n(38, s, 0.05, 0.4), n(38, s + 0.06, 0.05, 0.4), n(38, s + 0.2, 0.25, 0.9)];
  yes('Percussion Drag', { 10: [...drag(0), ...drag(2)] }, { part: 10 });
  yes('Paradiddle', { 10: seq([38, 45, 38, 38, 45, 38, 45, 45], { ioi: 0.25 }) }, { part: 10 });
  no('Paradiddle', { 10: seq([38, 45, 38, 45, 38, 45, 38, 45], { ioi: 0.25 }) }, { part: 10 });
});

test('techniques: between two parts', () => {
  const call = [0, 8, 16].flatMap((b) => seq([60, 62, 64], { start: b, ioi: 0.5 }));
  const answer = [0, 8, 16].flatMap((b) => seq([67, 65, 64], { start: b + 3, ioi: 0.5 }));
  yes('Call and Response', { 1: call, 2: answer }, { part: [1, 2] });
  no('Call and Response', { 1: call, 2: call.map((x) => ({ ...x, pitch: x.pitch + 4 })) }, { part: [1, 2] });
  const a = [0, 4].flatMap((b) => [0, 0.5, 1, 1.5, 2, 2.5].map((x) => n(60, b + x, 0.2)));
  const b = [0, 4].flatMap((bb) => [0.25, 0.75, 1.25, 1.75, 2.25, 2.75].map((x) => n(67, bb + x, 0.2)));
  yes('Hocket', { 1: a, 2: b }, { part: [1, 2] });
  no('Hocket', { 1: a, 2: a.map((x) => ({ ...x, pitch: 67 })) }, { part: [1, 2] });
  yes('Parallel Motion', { 1: seq([60, 62, 64, 65, 67], { ioi: 1 }), 2: seq([64, 65, 67, 69, 71], { ioi: 1 }) }, { part: [1, 2] });
  no('Parallel Motion', { 1: seq([60, 62, 64, 65, 67], { ioi: 1 }), 2: seq([72, 71, 69, 67, 65], { ioi: 1 }) }, { part: [1, 2] });
  yes('Contrary Motion', { 1: seq([60, 62, 64, 65], { ioi: 1 }), 2: seq([72, 71, 69, 67], { ioi: 1 }) }, { part: [1, 2] });
  yes('Oblique Motion', { 3: [n(48, 0, 4), n(48, 4, 4)], 1: [...seq([60, 62, 64], { ioi: 1 }), ...seq([64, 65, 67], { start: 4, ioi: 1 })] }, { part: [3, 1] });
  yes('Polyrhythm', { 1: seq([60, 60, 60, 60, 60, 60, 60], { ioi: 1 }), 2: seq(Array(10).fill(67), { ioi: 2 / 3 }) }, { part: [1, 2] });
  no('Polyrhythm', { 1: seq([60, 60, 60, 60, 60, 60, 60], { ioi: 1 }), 2: seq(Array(12).fill(67), { ioi: 0.5 }) }, { part: [1, 2] });
});

test('techniques: whole sections (A = bars 1-4, B = bars 5-8)', () => {
  const groove = (from, to, extra = []) => { const out = []; for (let b = from; b < to; b++) out.push(n(36, b * 4, 0.25), n(42, b * 4 + 1, 0.25), n(38, b * 4 + 2, 0.25), n(42, b * 4 + 3, 0.25)); return [...out, ...extra]; };
  const fillBar = [0, 0.5, 1, 1.5, 2, 2.5, 3, 3.5].map((x, i) => n(i % 2 ? 45 : 47, 12 + x, 0.25));
  yes('Fill', { 10: groove(0, 3, fillBar) }, { where: 'A' });
  no('Fill', { 10: groove(0, 4) }, { where: 'A' });
  const pad = (from, to) => Array.from({ length: to - from }, (_, k) => n(60, (from + k) * 4, 4));
  const lead = (from, to) => Array.from({ length: (to - from) * 4 }, (_, k) => n(72 + (k % 3), from * 4 + k, 1));
  yes('Break', { 1: [...lead(0, 3), ...lead(4, 8)], 2: [...pad(0, 3), ...pad(4, 8)], 10: groove(0, 8) }, { where: 'A' });
  yes('Drop', { 2: pad(0, 8), 1: lead(4, 8), 10: groove(4, 8) }, { where: 'B' });
  no('Drop', { 2: pad(0, 8), 1: lead(0, 8), 10: groove(0, 8) }, { where: 'B' });
  const build = [1, 2, 4, 8].flatMap((k, b) => Array.from({ length: k }, (_, i) => n(60 + i, b * 4 + i * (4 / k), 4 / k)));
  yes('Buildup', { 1: build }, { where: 'A' });
  yes('Breakdown', { 1: lead(0, 4), 2: pad(0, 8), 10: groove(0, 4) }, { where: 'B' });
  no('Breakdown', { 1: lead(0, 8), 2: pad(0, 8), 10: groove(0, 8) }, { where: 'B' });
  const stops = [0, 4].flatMap((b) => [n(36, b, 0.25), n(48, b, 0.25)]).map((x, i) => ({ ...x, pitch: i % 2 ? 48 : 36 }));
  yes('Stop-Time', { 10: stops.filter((x) => x.pitch === 36), 3: stops.filter((x) => x.pitch === 48) }, { where: 'A' });
  yes('Repetition', { 1: [...lead(0, 4), ...lead(4, 8)] }, { where: 'B' });
  const busy = seq([60, 62, 64, 65, 67, 69, 71, 72], { ioi: 0.25 });
  yes('Fermata', { 1: [...busy, n(72, 2, 3), n(60, 6, 1)] }, {});
});

test('techniques: the motif and what happens to it', () => {
  const at = { channel: 1, bar: 1, beats: 4 };
  const motif = [n(60, 0, 1), n(62, 1, 0.5), n(64, 1.5, 0.5), n(67, 2, 2)];
  const bp = BP({ motif: { at } });
  const later = (pitches, iois, start = 16, ch = 1) => { let t = start; return pitches.map((p, i) => { const x = n(p, t, iois[i] ?? 1); t += iois[i] ?? 1; return x; }); };
  const has = (name, statement) => yes(name, { 1: [...motif, ...statement] }, {}, bp);
  has('Transposition', later([65, 67, 69, 72], [1, 0.5, 0.5, 2]));
  has('Octave Displacement', later([72, 74, 76, 79], [1, 0.5, 0.5, 2]));
  has('Augmentation', later([60, 62, 64, 67], [2, 1, 1, 4]));
  has('Diminution', later([60, 62, 64, 67], [0.5, 0.25, 0.25, 1]));
  has('Inversion', later([60, 58, 56, 53], [1, 0.5, 0.5, 2]));
  has('Retrograde', later([67, 64, 62, 60], [1, 1, 1, 1]));
  has('Retrograde Inversion', later([60, 63, 65, 67], [1, 1, 1, 1]));
  has('Variation', later([60, 62, 64, 68], [1, 0.5, 0.5, 2]));
  has('Fragmentation', [...later([60, 62, 64], [1, 1, 2], 16), ...later([65, 67, 69], [1, 1, 2], 24)]);
  has('Motif', [...later([65, 67, 69, 72], [1, 0.5, 0.5, 2], 8), ...later([72, 74, 76, 79], [1, 0.5, 0.5, 2], 20)]);
  has('Development', [...later([65, 67, 69, 72], [1, 0.5, 0.5, 2], 8), ...later([60, 62, 64, 67], [2, 1, 1, 4], 14), ...later([60, 58, 56, 53], [1, 0.5, 0.5, 2], 24)]);
  no('Inversion', { 1: [...motif, ...later([65, 67, 69, 72], [1, 0.5, 0.5, 2])] }, {}, bp);
  no('Transposition', { 1: motif }, {}, bp);
  // Without "motif.at", a motif technique can't be heard (Variation falls back to comparing sections).
  assert.match(hear('Inversion', { 1: motif }, {}, BP()).evidence, /motif\.at/);
});

test('techniques: a blueprint names each one on a part, in sections, with how; the check holds it to the notes', () => {
  const bp = BP({ techniques: [
    { name: 'staccato', part: 1, where: ['A'], how: 'the lead skips in the opening' },
    { name: 'Call and Response', part: [1], how: 'lead and second trade lines' },
    { name: 'Inversion', part: 1, how: 'the motif upside down at the end' },
    { name: 'Drop', how: 'everything lands at B' },
    { name: 'Moonwalk', part: 1, how: 'not a technique' },
    { name: 'Vibrato', part: 9, where: ['C'], how: 'wobbly' },
  ] });
  const p = planProblems(bp);
  assert.ok(p.some((x) => /Call and Response" happens between two parts/.test(x)), p.join('\n'));
  assert.ok(p.some((x) => /Inversion" works on the motif/.test(x)));
  assert.ok(p.some((x) => /Drop" needs "where"/.test(x)));
  assert.ok(p.some((x) => /Moonwalk/.test(x)));
  assert.ok(p.some((x) => /channel 9, which has no role/.test(x)));
  assert.ok(p.some((x) => /"C", which isn't a section/.test(x)));
  assert.deepEqual(planProblems(BP()), []);

  const good = BP({ techniques: [
    { name: 'Staccato', part: 1, where: ['A'], how: 'the lead skips in the opening' },
    { name: 'Legato', part: 1, where: ['B'], how: 'and sings in the second half' },
    { name: 'Vibrato', part: 1, how: 'a slow wobble on the long notes' },
  ] });
  const lead = [...seq([60, 62, 64, 65, 67, 65, 64, 62, 60], { ioi: 0.5, dur: 0.2 }), ...seq([60, 62, 64, 65, 67, 65, 64, 62, 60], { start: 16, ioi: 0.5, dur: 0.5 })];
  const ok = checkTechniques(piece({ 1: lead }), good);
  assert.deepEqual(ok.errors, []);
  assert.ok(ok.notes.some((x) => /Vibrato: not in the notes; the instruments stage/.test(x)));
  const flat = checkTechniques(piece({ 1: [0, 16].flatMap((start) => seq([60, 62, 64, 65, 67, 65, 64, 62, 60], { start, ioi: 0.5, dur: 0.5 })) }), good);
  assert.equal(flat.errors.length, 1);
  assert.match(flat.errors[0], /Staccato is planned on channel 1 in "A" but isn't in the notes: notes sound 100% of the time to the next .*What counts:/);

  const h = heard(piece({ 1: lead }), good).map((x) => x.name);
  assert.ok(h.includes('Staccato') && h.includes('Legato') && h.includes('Scalar Run'), h.join(', '));
});

test('techniques: a sparse line triggers nothing that needs a pattern', () => {
  const sparse = { 1: [n(60, 0, 1), n(64, 4, 1), n(67, 8, 1), n(72, 12, 1)] };
  const quiet = TECHNIQUE_NAMES.filter((name) => TECHNIQUES[name].kind === 'notes' && hear(name, sparse).found);
  assert.deepEqual(quiet, []);
});
