// A video's score written by code from a cue plan (owner, 2026-10-19: "the AI makes the musical decisions, code writes
// the notes"). The AI can't hear, so it never writes a note: the studio maps the tempo onto the picture, one AI call
// plans the music (key, chords, a motif, each section's intensity, function and parts), and this module turns that plan
// into the files the music engine already checks: music/blueprint.json and music/score.strudel.js (notes only, every
// part on its own channel), then music/instruments.json (a stand-in palette, chosen by code).
//
// Pitch is right by construction: every pitched note is a tone of the chord of the moment, the bass plays the root on
// every change, nothing is held into a chord it doesn't fit, and no two parts sound a semitone apart. The engine's
// harmony lock and intensity curve (strudel.mjs check) are run here too, so the score only leaves this module passing.
import { parseChord, parseScale, chartTimeline, MODES, checkHarmony } from '../../pipelines/_shared/skills/music/lib/harmony.mjs';
import { intensity } from '../../pipelines/_shared/skills/music/lib/score.mjs';

export const ROLES = {
  bass: { channel: 1, harmony: 'bass', name: 'Floor', job: 'the chord roots, the floor under the voice' },
  pad: { channel: 2, harmony: 'chords', name: 'Bed', job: 'the chords, sustained and simple under the words' },
  melody: { channel: 3, harmony: 'melody', name: 'Theme', job: 'the motif and its variations' },
  arp: { channel: 4, harmony: 'chords', name: 'Pulse', job: 'broken chords that carry the motion' },
  bells: { channel: 5, harmony: 'chords', name: 'Bells', job: 'the hits on the picture and the last chord' },
  drums: { channel: 10, harmony: 'drums', name: 'Drums', job: 'a soft pulse where the picture builds' },
};
export const PARTS = Object.keys(ROLES);
export const VARIATIONS = ['statement', 'fragment', 'inversion', 'augmentation', 'octave-up', 'sequence', 'rest'];
export const PALETTES = ['warm', 'glass', 'dark'];
export const HIT_KINDS = ['direct', 'anticipation', 'aftermath', 'none'];

const r2 = (x) => Math.round(x * 100) / 100;
const r3 = (x) => Math.round(x * 1000) / 1000;
const bpbOf = (meter) => Number(String(meter).split('/')[0]);

// ---- the tempo map (no AI) ----

// The tempo, meter and bar 1 (offset_sec) that land the most structural hits within a frame of an eighth note, then
// the smallest largest error, then the tempo nearest the feel asked for. Tempos are searched in 0.01 BPM steps.
export function tempoMap({ hits, videoSec, fps = 30, bpmMin = 64, bpmMax = 140, prefer = 90, meters = ['4/4', '3/4'] }) {
  const frame = 1 / fps;
  const ts = hits.map((h) => h.t_sec).filter((t) => t > 0 && t < videoSec);
  let best = null;
  for (const meter of meters) {
    for (let c = Math.round(bpmMin * 100); c <= Math.round(bpmMax * 100); c++) {
      const bpm = c / 100;
      const beat = 60 / bpm;
      const grid = beat / 2;
      const anchors = ts.length ? ts : [0];
      for (const a of anchors) {
        const offset = ((a % beat) + beat) % beat;
        let fit = 0; let worst = 0;
        for (const t of ts) {
          const p = (t - offset) / grid;
          const e = Math.abs(p - Math.round(p)) * grid;
          if (e <= frame * 0.95) fit++;
          worst = Math.max(worst, e);
        }
        // 4/4 first when the two fit as well: it's the plainer choice under speech
        const score = fit * 1000 - worst * 100 - Math.abs(bpm - prefer) * 0.5 - (meter === '4/4' ? 0 : 5);
        if (!best || score > best.score) best = { score, bpm, meter, offset: r3(offset), fit, worst: r3(worst) };
      }
    }
  }
  const bpb = bpbOf(best.meter);
  const spb = bpb * 60 / best.bpm;
  const bars = Math.max(2, Math.floor((videoSec - best.offset) / spb + 1e-9));
  return { bpm: best.bpm, meter: best.meter, beats_per_bar: bpb, offset_sec: best.offset, bars, sec_per_bar: r3(spb), hits_fitted: best.fit, hits_total: ts.length };
}

// Where a moment in the video falls in the music: { bar, beat } on the eighth-note grid (bars and beats from 1).
export function placeOf(map, t) {
  const beat = 60 / map.bpm;
  const q = Math.round(((t - map.offset_sec) / beat) * 2) / 2;
  const bar = Math.floor(q / map.beats_per_bar + 1e-9) + 1;
  return { bar, beat: r2(q - (bar - 1) * map.beats_per_bar + 1), at: r3(map.offset_sec + q * beat) };
}

// One section per scene, starting at the bar nearest the scene's start; scenes that land in the same bar share it.
export function sectionsFromScenes(map, scenes) {
  const out = [];
  for (const s of scenes) {
    const start = out.length ? Math.min(map.bars, Math.max(1, Math.round((s.start_sec - map.offset_sec) / map.sec_per_bar) + 1)) : 1;
    const last = out.at(-1);
    if (last && start <= last.start_bar) { last.scenes.push(s.key); last.end_sec = s.end_sec; continue; }
    out.push({ scenes: [s.key], start_bar: start, start_sec: s.start_sec, end_sec: s.end_sec });
  }
  out.forEach((s, i) => { s.end_bar = i + 1 < out.length ? out[i + 1].start_bar - 1 : map.bars; });
  return out.filter((s) => s.end_bar >= s.start_bar);
}

// ---- the plan ----

// Problems with the AI's cue plan the code can't fix itself (it answers them with a fresh fix call).
export function planProblems(plan, sections) {
  const p = [];
  try { parseScale(plan.scale); } catch (e) { p.push(`scale: ${e.message}`); }
  if (!Array.isArray(plan.sections) || plan.sections.length !== sections.length) p.push(`sections: give exactly ${sections.length}, one per section listed, in order`);
  for (const [i, s] of (plan.sections || []).entries()) {
    if (!Array.isArray(s.chords) || !s.chords.length) { p.push(`sections[${i}] "${s.name}": no chords`); continue; }
    for (const c of s.chords.flat()) { try { parseChord(c); } catch (e) { p.push(`sections[${i}] "${s.name}": ${e.message}`); } }
    if (s.chords.some((c) => Array.isArray(c) && c.length !== 2)) p.push(`sections[${i}] "${s.name}": a split bar is exactly two chords, ["Am7", "G"]`);
  }
  const m = plan.motif || {};
  if (!Array.isArray(m.degrees) || m.degrees.length < 3 || m.degrees.length > 10) p.push('motif.degrees: 3-10 scale degrees (0 = the tonic)');
  if (!Array.isArray(m.beats) || m.beats.length !== (m.degrees || []).length) p.push('motif.beats: one length in beats per degree');
  else if (m.beats.some((b) => !(b >= 0.25))) p.push('motif.beats: every length at least 0.25 beats');
  return p;
}

// The blueprint the engine checks (music/blueprint.json, music SKILL.md §4).
export function blueprintFrom({ plan, map, sections, hits, used }) {
  const chart = sections.map((s, i) => ({ bars: [s.start_bar, s.end_bar], chords: plan.sections[i].chords }));
  const kinds = new Map((plan.hits || []).map((h) => [h.cut, h]));
  return {
    title: plan.title,
    bpm: map.bpm, meter: map.meter, bars: map.bars, scale: plan.scale, offset_sec: map.offset_sec,
    dramatic_reading: plan.dramatic_reading,
    motif: { degrees: plan.motif.degrees, beats: plan.motif.beats, role: ROLES.melody.name, contour: plan.motif.contour || null },
    sections: sections.map((s, i) => ({
      name: plan.sections[i].name, start_bar: s.start_bar, end_bar: s.end_bar, function: plan.sections[i].function,
      intensity: plan.sections[i].intensity, picture: plan.sections[i].picture || s.scenes.join(', '),
      variation: plan.sections[i].melody, parts: plan.sections[i].parts,
    })),
    hits: hits.map((h, i) => {
      const k = kinds.get(i + 1);
      const kind = h.fits ? (HIT_KINDS.includes(k?.kind) ? k.kind : 'direct') : 'none';
      return { t_sec: h.t_sec, bar: h.bar, beat: h.beat, kind, what: `${h.what}${k?.response ? `: ${k.response}` : ''}${h.fits ? '' : ' (between beats at this tempo: no hit)'}` };
    }),
    chart,
    roles: PARTS.filter((k) => used.has(k)).map((k) => ({ channel: ROLES[k].channel, harmony: ROLES[k].harmony, name: ROLES[k].name, job: ROLES[k].job })),
    ending: plan.ending,
    made_by: 'the studio: the cue plan by one AI call, the notes by src/music/compose.mjs',
  };
}

// ---- the notes ----

// notes: [{ part, pitch, start, dur, vel }] in beats from bar 1. Every pitched note is a chord tone of its moment and
// never crosses a chord change; a note a semitone from another part's sounding note moves to the nearest other chord
// tone, or is left out.
export function compose({ plan, bp, voiceBusy, boost = {} }) {
  const bpb = bpbOf(bp.meter);
  const { timeline } = chartTimeline(bp.chart, { bars: bp.bars, beatsPerBar: bpb });
  const key = parseScale(bp.scale);
  const mode = MODES[String(bp.scale).split(':')[1].trim().toLowerCase()];
  const tonic = parseChord(String(bp.scale).split(':')[0].trim()).root;
  const notes = [];
  const sounding = (a, b) => notes.filter((n) => n.start < b - 1e-9 && n.start + n.dur > a + 1e-9);
  const clashes = (part, pitch, a, b) => sounding(a, b).some((n) => n.part !== part && Math.abs(n.pitch - pitch) === 1);
  const tonesNear = (chord, target, lo, hi) => {
    const out = [];
    for (let p = lo; p <= hi; p++) if (chord.tones.has(((p % 12) + 12) % 12)) out.push(p);
    return out.sort((x, y) => Math.abs(x - target) - Math.abs(y - target) || x - y);
  };
  const add = (part, pitch, start, dur, vel, chord, range) => {
    const end = start + dur;
    let p = pitch;
    if (part !== 'drums') {
      const choices = tonesNear(chord, pitch, range[0], range[1]);
      p = choices.find((x) => !clashes(part, x, start, end));
      if (p === undefined) return false;
    }
    notes.push({ part, pitch: p, start: r3(start), dur: r3(dur), vel: r2(Math.min(1, Math.max(0.1, vel))) });
    return true;
  };
  const chordAt = (t) => timeline.find((c) => t >= c.begin - 1e-9 && t < c.end - 1e-9);
  const secOf = (beat) => bp.offset_sec + beat * 60 / bp.bpm;
  const under = (beat) => voiceBusy(secOf(beat));
  const level = (i) => Math.max(0, Math.min(10, bp.sections[i].intensity + (boost[i] || 0)));
  const sectionOf = (bar) => bp.sections.findIndex((s) => bar >= s.start_bar && bar <= s.end_bar);
  const plays = (i, part) => (plan.sections[i].parts || []).includes(part);
  const degreePitch = (d, octave) => {
    const n = mode.length; const k = ((d % n) + n) % n;
    return 12 * (octave + 1) + tonic + mode[k] + 12 * Math.floor(d / n);
  };
  const lastBar = bp.bars;

  // 1. the theme first: it has the right of way
  const m = plan.motif;
  const beats = m.beats.map((b) => Math.max(0.25, Math.round(b * 4) / 4));
  bp.sections.forEach((s, i) => {
    const v = plan.sections[i].melody;
    if (v === 'rest' || !VARIATIONS.includes(v)) return;
    let degs = [...m.degrees]; let lens = [...beats];
    if (v === 'fragment') { const k = Math.ceil(degs.length / 2); degs = degs.slice(0, k); lens = lens.slice(0, k); }
    if (v === 'inversion') degs = degs.map((d) => degs[0] - (d - degs[0]));
    if (v === 'augmentation') lens = lens.map((x) => x * 2);
    if (v === 'octave-up') degs = degs.map((d) => d + mode.length);
    const phrase = lens.reduce((a, b) => a + b, 0);
    const slotBars = Math.max(1, Math.ceil(phrase / bpb));
    // a phrase, then the same time to breathe (call and answer); under the words, every other phrase only
    let rep = 0;
    for (let bar = s.start_bar; bar + slotBars - 1 <= s.end_bar; bar += slotBars * 2, rep++) {
      const t0 = (bar - 1) * bpb;
      if (under(t0) && rep % 2 === 1 && level(i) < 7) continue;
      const shift = v === 'sequence' ? rep % 3 : 0;
      let t = t0;
      degs.forEach((d, k) => {
        const c = chordAt(t);
        const dur = Math.min(lens[k], c ? c.end - t : 0);
        if (c && dur >= 0.25) add('melody', degreePitch(d + shift, 4), t, dur, (under(t) ? 0.55 : 0.7) + level(i) * 0.02, c.chord, [60, 84]);
        t += lens[k];
      });
    }
  });

  // 2. the floor: the root on every chord change, more motion as the section rises
  for (const c of timeline) {
    const i = sectionOf(c.bar);
    if (i < 0 || !plays(i, 'bass')) continue;
    const L = level(i); const len = c.end - c.begin;
    const root = 36 + ((c.chord.bass - 0 + 12) % 12); // C2..B2
    const r = root > 45 ? root - 12 : root;           // keep it at or under A2
    const vel = 0.55 + L * 0.03;
    if (L < 4 || len < 2) { add('bass', r, c.begin, len, vel, c.chord, [r, r]); continue; }
    if (L < 7) {
      add('bass', r, c.begin, len / 2, vel, c.chord, [r, r]);
      add('bass', r + 7, c.begin + len / 2, len / 2, vel - 0.05, c.chord, [r + 7, r + 7]) || add('bass', r, c.begin + len / 2, len / 2, vel - 0.05, c.chord, [r, r]);
      continue;
    }
    for (let t = c.begin, k = 0; t < c.end - 1e-9; t += 0.5, k++) add('bass', k % 2 ? r + 12 : r, t, 0.5, k % 2 ? vel - 0.1 : vel, c.chord, k % 2 ? [r + 12, r + 12] : [r, r]);
  }

  // 3. the bed: the chord held for its whole length, voice-led (each voice moves to the nearest tone of the next chord)
  let voicing = null;
  for (const c of timeline) {
    const i = sectionOf(c.bar);
    if (i < 0 || !plays(i, 'pad')) { voicing = null; continue; }
    const target = voicing || [52, 57, 62, 67].slice(0, c.chord.tones.size >= 4 ? 4 : 3);
    const next = [];
    for (const t of target) {
      const p = tonesNear(c.chord, t, 48, 72).find((x) => !next.includes(x) && !next.some((y) => Math.abs(y - x) === 1));
      if (p !== undefined) next.push(p);
    }
    const vel = 0.4 + level(i) * 0.03;
    voicing = [];
    for (const p of next) if (add('pad', p, c.begin, c.end - c.begin, vel, c.chord, [p, p])) voicing.push(p);
    if (!voicing.length) voicing = null;
  }

  // 4. the pulse: broken chords, eighths, sixteenths where the section is at its busiest
  for (const c of timeline) {
    const i = sectionOf(c.bar);
    if (i < 0 || !plays(i, 'arp')) continue;
    const L = level(i);
    const step = L >= 8 ? 0.25 : 0.5;
    const tones = tonesNear(c.chord, 66, 62, 79).slice(0, 4).sort((a, b) => a - b);
    if (!tones.length) continue;
    const shape = [...tones, ...tones.slice(1, -1).reverse()];
    for (let t = c.begin, k = 0; t < c.end - 1e-9; t += step, k++) {
      const vel = (Math.abs(t - Math.round(t)) < 1e-9 ? 0.5 : 0.38) + L * 0.025;
      add('arp', shape[k % shape.length], t, step, under(t) ? vel - 0.08 : vel, c.chord, [62, 79]);
    }
  }

  // 5. the bells: on the hits the plan plays, at the start of a section that rises, and on the last chord
  const bell = (t, vel) => { const c = chordAt(t); if (c) add('bells', 79, t, Math.min(2, c.end - t), vel, c.chord, [72, 88]); };
  for (const h of bp.hits) {
    if (h.kind === 'none') continue;
    const i = sectionOf(h.bar); if (i < 0 || !plays(i, 'bells')) continue;
    const t = (h.bar - 1) * bpb + (h.beat - 1) + (h.kind === 'aftermath' ? 1 : 0);
    if (t < lastBar * bpb) bell(t, 0.7);
  }
  bp.sections.forEach((s, i) => { if (i && plays(i, 'bells') && s.intensity > bp.sections[i - 1].intensity) bell((s.start_bar - 1) * bpb, 0.6); });
  const li = bp.sections.length - 1;
  if (plays(li, 'bells')) bell((lastBar - 1) * bpb, 0.55);

  // 6. the drums (General MIDI: 36 kick, 39 clap, 42 closed hat, 45 tom, 49 crash)
  for (let bar = 1; bar <= lastBar; bar++) {
    const i = sectionOf(bar); if (i < 0 || !plays(i, 'drums')) continue;
    const L = level(i); const t0 = (bar - 1) * bpb;
    add('drums', 36, t0, 0.25, 0.6 + L * 0.03);
    if (L >= 6 && bpb >= 4) add('drums', 36, t0 + 2, 0.25, 0.5 + L * 0.03);
    if (L >= 7) for (let b = 1; b < bpb; b += 2) add('drums', 39, t0 + b, 0.25, 0.45 + L * 0.03);
    if (L >= 4) for (let t = 0; t < bpb - 1e-9; t += L >= 8 ? 0.25 : 0.5) add('drums', 42, t0 + t, 0.25, (Math.abs(t - Math.round(t)) < 1e-9 ? 0.4 : 0.28) + L * 0.02);
    const s = bp.sections[i];
    const nextUp = bp.sections[i + 1] && bp.sections[i + 1].intensity > s.intensity;
    if (bar === s.end_bar && nextUp && L >= 4) for (const t of [bpb - 1, bpb - 0.5]) add('drums', 45, t0 + t, 0.25, 0.55 + L * 0.02);
  }
  for (const h of bp.hits) {
    if (h.kind !== 'direct') continue;
    const i = sectionOf(h.bar); if (i < 0 || !plays(i, 'drums')) continue;
    add('drums', 49, (h.bar - 1) * bpb + (h.beat - 1), 0.25, 0.6);
  }
  return notes.sort((a, b) => a.start - b.start || a.part.localeCompare(b.part) || a.pitch - b.pitch);
}

// The notes as the manifest the engine's checks read (the same shape strudel.mjs captures from the score).
export function manifestOf(notes, bp) {
  const tracks = PARTS.filter((k) => notes.some((n) => n.part === k)).map((k) => ({
    channel: ROLES[k].channel, name: ROLES[k].name, kind: k === 'drums' ? 'drums' : 'pitched',
    notes: notes.filter((n) => n.part === k).map((n) => ({ pitch: n.pitch, start: n.start, dur: n.dur, velocity: n.vel })),
  }));
  return { bpm: bp.bpm, meter: bp.meter, beats_per_bar: bpbOf(bp.meter), bars: bp.bars, tracks };
}

// Compose, then run the engine's own harmony lock and intensity curve on the result. A section measured out of the
// order its intensity asks for gets more (or less) in it, and the notes are written again (up to 8 times).
export function composeChecked({ plan, bp, voiceBusy }) {
  const boost = {};
  for (let round = 0; ; round++) {
    const notes = compose({ plan, bp, voiceBusy, boost });
    const man = manifestOf(notes, bp);
    const harmony = checkHarmony(man, bp);
    const curve = intensity(man, bp);
    if (!curve.problems.length || round >= 8) return { notes, harmony, curve, rounds: round + 1 };
    const names = bp.sections.map((s) => s.name);
    for (const p of curve.problems) {
      const m = p.match(/^"(.+?)" was asked .* than "(.+?)"/);
      if (!m) continue;
      const hi = names.indexOf(m[1]); const lo = names.indexOf(m[2]);
      if (hi >= 0) boost[hi] = (boost[hi] || 0) + 1;
      if (lo >= 0) boost[lo] = (boost[lo] || 0) - 1;
    }
  }
}

// ---- the score file ----

// music/score.strudel.js: one labelled part per channel, every bar written out on a grid of sixteenths (one `<…>` entry
// per bar), velocities in a second pattern with the same steps, every part cut at the last bar with cue().
export function scoreText({ notes, bp, plan }) {
  const bpb = bpbOf(bp.meter);
  const steps = bpb * 4;
  const lines = [
    `// ${bp.title}: written by the studio from the cue plan (src/music/compose.mjs). Notes only: the instruments come from music/instruments.json.`,
    `// ${bp.dramatic_reading}`,
    `setcpm(${bp.bpm} / ${bpb})`,
    `const BARS = ${bp.bars}`,
    'const cue = (p) => p.filterWhen((t) => t < BARS)   // every part ends with the piece',
    `// sections: ${bp.sections.map((s) => `bars ${s.start_bar}-${s.end_bar} "${s.name}" (${s.intensity})`).join(' · ')}`,
    '',
  ];
  const fmtPitch = (ps) => (ps.length > 1 ? `[${ps.join(',')}]` : String(ps[0]));
  for (const k of PARTS) {
    const mine = notes.filter((n) => n.part === k);
    if (!mine.length) continue;
    const barsN = []; const barsV = [];
    for (let bar = 1; bar <= bp.bars; bar++) {
      const t0 = (bar - 1) * bpb;
      // group the bar's notes by start step: one chunk per start (a chord = same start and length)
      const chunks = new Map();
      for (const n of mine) {
        if (n.start < t0 - 1e-9 || n.start >= t0 + bpb - 1e-9) continue;
        const s = Math.round((n.start - t0) * 4);
        const len = Math.max(1, Math.min(steps - s, Math.round(n.dur * 4)));
        const c = chunks.get(s) || { s, len, pitches: [], vel: 0 };
        c.len = Math.min(c.len, len); c.pitches.push(n.pitch); c.vel = Math.max(c.vel, n.vel);
        chunks.set(s, c);
      }
      const order = [...chunks.values()].sort((a, b) => a.s - b.s);
      const segN = []; const segV = [];
      let at = 0;
      order.forEach((c, j) => {
        if (c.s > at) { segN.push(`~@${c.s - at}`); segV.push(`~@${c.s - at}`); }
        const nextS = order[j + 1]?.s ?? steps;
        const len = Math.min(c.len, nextS - c.s);
        segN.push(`${fmtPitch([...new Set(c.pitches)].sort((a, b) => a - b))}@${len}`);
        segV.push(`${c.vel}@${len}`);
        at = c.s + len;
      });
      if (at < steps) { segN.push(`~@${steps - at}`); segV.push(`~@${steps - at}`); }
      barsN.push(`[${segN.join(' ')}]`); barsV.push(`[${segV.join(' ')}]`);
    }
    const r = ROLES[k];
    lines.push(`// ${r.name} (channel ${r.channel}, ${r.harmony}): ${r.job}`);
    lines.push(`${k}: cue(note(\`<\n  ${barsN.join('\n  ')}\n>\`).velocity(\`<\n  ${barsV.join('\n  ')}\n>\`)).midichan(${r.channel})`, '');
  }
  return lines.join('\n');
}

// ---- the stand-in instruments (music/instruments.json) ----

// Three palettes; inside each, every pitched part is a different family of sound, so parts sharing a register never
// share a timbre (the engine's instruments check). Clean, nothing driven (the owner's rule: never accidental grit).
const KIT = {
  36: { sound: 'sine', set: '.penv(18).pdecay(.08).decay(.25).sustain(0)', gain: 1 },
  39: { sound: 'pink', set: '.hpf(900).decay(.12).sustain(0).room(.2)', gain: 0.5 },
  42: { sound: 'white', set: '.hpf(7000).decay(.035).sustain(0)', gain: 0.3 },
  45: { sound: 'sine', set: '.penv(7).pdecay(.15).decay(.3).sustain(0)', gain: 0.7 },
  49: { sound: 'white', set: '.hpf(5000).decay(1.2).sustain(0).room(.3)', gain: 0.25 },
};
const PALETTE = {
  warm: {
    concept: 'Warm and soft: a round sine floor, a filtered saw bed with a slow attack, a hollow triangle theme, a muted square pulse and soft FM bells. Clean, nothing driven.',
    bass: ['sine', '.lpf(400).attack(.01).release(.25)', 0.75, 'dark', 'a round, pure low end that leaves the voice its register'],
    pad: ['sawtooth', '.lpf(1100).hpf(140).attack(.4).release(.6).room(.25).size(.5)', 0.22, 'warm', 'a soft, filtered bed; the slow attack keeps chord changes from poking through the narration'],
    melody: ['triangle', '.lpf(2400).attack(.02).release(.4).vib(4).room(.2)', 0.4, 'warm', 'a hollow, flute-like voice for the theme'],
    arp: ['square', '.lpf(1600).hpf(250).attack(.003).decay(.12).sustain(0).release(.05)', 0.15, 'neutral', 'dry and woody, very short: motion without weight'],
    bells: ['sine', '.fm(2).fmh(3).attack(.002).decay(.6).sustain(0).release(.8).hpf(400).room(.35)', 0.3, 'bright', 'a bell for the moments the picture turns'],
  },
  glass: {
    concept: 'Glassy and light: a triangle floor, a wide supersaw bed far under the voice, an FM glass theme, a reedy pulse and pure sine bells.',
    bass: ['triangle', '.lpf(500).attack(.01).release(.3)', 0.75, 'dark', 'a soft, rounded floor'],
    pad: ['supersaw', '.lpf(1300).hpf(160).attack(.5).release(.8).room(.3).size(.6)', 0.16, 'warm', 'a wide shimmer that sits back under the words'],
    melody: ['sine', '.fm(3).fmh(3.5).attack(.005).decay(.5).sustain(.2).release(.5).delay(.2).delaytime(.33).delayfeedback(.2)', 0.32, 'bright', 'glass, light and clear'],
    arp: ['pulse', '.lpf(1800).hpf(300).attack(.003).decay(.1).sustain(0)', 0.14, 'neutral', 'a small reedy tick that keeps time'],
    bells: ['sine', '.attack(.002).decay(.9).sustain(0).release(1).hpf(500).room(.4)', 0.28, 'neutral', 'pure bells for the hits'],
  },
  dark: {
    concept: 'Dark and close: a sine sub, a low-passed saw bed, a muted square theme, a soft triangle pulse and low FM bells.',
    bass: ['sine', '.lpf(300).attack(.01).release(.3)', 0.8, 'dark', 'a deep sub floor'],
    pad: ['sawtooth', '.lpf(750).hpf(120).attack(.6).release(.8).room(.3).size(.6)', 0.24, 'dark', 'a low, closed bed'],
    melody: ['square', '.lpf(1400).attack(.02).release(.4).room(.25)', 0.28, 'warm', 'a muted, woody theme'],
    arp: ['triangle', '.lpf(2000).attack(.003).decay(.15).sustain(0)', 0.2, 'neutral', 'soft motion'],
    bells: ['sine', '.fm(1.5).fmh(2).attack(.002).decay(.8).sustain(0).release(.8).hpf(300).room(.4)', 0.28, 'warm', 'a low bell for the turns'],
  },
};
export function instrumentsFor(palette, used) {
  const pal = PALETTE[PALETTES.includes(palette) ? palette : 'warm'];
  const pans = { bass: 0, pad: 0, melody: 0.2, arp: -0.25, bells: 0.3 };
  const parts = PARTS.filter((k) => used.has(k)).map((k) => {
    if (k === 'drums') {
      const kit = Object.fromEntries([...used.get('drums')].map((gm) => [String(gm), KIT[gm]]));
      return { channel: 10, role: ROLES.drums.name, gain: 0.7, pan: 0, why: 'soft, tight and dry, so the facts never feel like a trailer', kit };
    }
    const [sound, set, gain, character, why] = pal[k];
    return { channel: ROLES[k].channel, role: ROLES[k].name, sound, set, gain, pan: pans[k], character, why };
  });
  return { concept: `${pal.concept} A stand-in chosen by the studio, for judging the notes against the picture.`, parts };
}

// Which parts play, and for the drums which General MIDI numbers.
export function usedParts(notes) {
  const used = new Map();
  for (const n of notes) {
    if (!used.has(n.part)) used.set(n.part, new Set());
    if (n.part === 'drums') used.get('drums').add(n.pitch);
  }
  return used;
}
