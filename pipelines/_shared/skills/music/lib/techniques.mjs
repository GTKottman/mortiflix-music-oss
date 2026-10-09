// The techniques: the owner's descriptor list (music-prompt-1, 1.b), each one either heard in the MIDI or handed to the
// sound stage. A blueprint names the techniques it uses, on which part and in which sections; the check then listens
// for each one in the captured notes and refuses a score that only names it.
//
//   kind   notes    measured on each named part's notes (every named part must show it)
//          pair     measured between two named parts (call and response, hocket, parallel motion…)
//          section  measured on the whole piece inside the named sections (a drop, a breakdown, a fill…)
//          motif    measured against the blueprint's motif ("motif": { "at": { "channel", "bar", "beats" } })
//          sound    not in the notes: an instruction for the instruments stage (vibrato, sidechain pumping…)
//          lock     already guaranteed by the harmony lock (chord-tone targeting)
//
// Times are in beats (quarter notes). Every detector returns { found, count, evidence } in plain words.
import { chartTimeline, parseScale } from './harmony.mjs';

const E = 1e-6;
const near = (a, b, tol = 0.02) => Math.abs(a - b) <= tol;
const median = (xs) => { if (!xs.length) return 0; const s = [...xs].sort((a, b) => a - b); const m = s.length >> 1; return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2; };
const posIn = (beat, unit) => ((beat % unit) + unit) % unit;

// ---- shared views of the notes ----

// Notes that start together (within tol) as one group, in time order.
function groups(notes, tol = 0.02) {
  const out = [];
  for (const n of [...notes].sort((a, b) => a.start - b.start || a.pitch - b.pitch)) {
    const g = out.at(-1);
    if (g && n.start - g.start <= tol) g.notes.push(n); else out.push({ start: n.start, notes: [n] });
  }
  return out;
}
// One note per onset: the highest (a melody's line) or the lowest (a bass line).
const line = (notes, which = 'top') => groups(notes).map((g) => g.notes.reduce((a, b) => (which === 'top' ? (b.pitch > a.pitch ? b : a) : (b.pitch < a.pitch ? b : a))));
// A line split where it rests for at least `rest` beats.
function phrases(l, rest = 0.75) {
  const out = []; let cur = [];
  for (const n of l) {
    const prev = cur.at(-1);
    if (prev && n.start - (prev.start + prev.dur) >= rest - E) { out.push(cur); cur = []; }
    cur.push(n);
  }
  if (cur.length) out.push(cur);
  return out;
}
// Runs of consecutive items for which ok(prev, cur, i) holds; returns the runs (arrays) of at least `min` items.
function runs(items, ok, min) {
  const out = []; let cur = items.length ? [items[0]] : [];
  for (let i = 1; i < items.length; i++) {
    if (ok(items[i - 1], items[i], i)) cur.push(items[i]);
    else { if (cur.length >= min) out.push(cur); cur = [items[i]]; }
  }
  if (cur.length >= min) out.push(cur);
  return out;
}
const sign = (x) => (x > 0 ? 1 : x < 0 ? -1 : 0);

// ---- the context a detector reads ----

// manifest: the captured MIDI (music/notes.json); bp: the blueprint. Returns what every detector needs.
export function context(manifest, bp) {
  const bpb = manifest.beats_per_bar || 4;
  const bars = bp?.bars || manifest.bars || Math.ceil(Math.max(0, ...manifest.tracks.flatMap((t) => t.notes.map((n) => n.start + n.dur))) / bpb);
  let timeline = [];
  try { timeline = bp?.chart ? chartTimeline(bp.chart, { bars, beatsPerBar: bpb }).timeline : []; } catch { timeline = []; }
  let key = null; let tonic = null;
  try { if (bp?.scale) { key = parseScale(bp.scale); tonic = scaleTonic(bp.scale); } } catch { key = null; }
  const ranges = (bp?.sections || []).map((s) => ({ name: s.name, start: (s.start_bar - 1) * bpb, end: s.end_bar * bpb, bars: s.end_bar - s.start_bar + 1 }));
  const ctx = { tracks: manifest.tracks, bpb, bars, timeline, key, tonic, sections: ranges, bp };
  ctx.motif = bp?.motif?.at ? extractMotif(ctx, bp.motif.at) : null;
  return ctx;
}
const PCS = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
function scaleTonic(scale) {
  const m = String(scale).match(/^\s*([A-G])([#b♯♭]?)/);
  return m ? (PCS[m[1]] + (m[2] === '#' || m[2] === '♯' ? 1 : m[2] === 'b' || m[2] === '♭' ? -1 : 0) + 12) % 12 : null;
}
const chordAt = (ctx, beat) => { let c = null; for (const x of ctx.timeline) { if (x.begin <= beat + E) c = x; else break; } return c && beat < c.end + E ? c : null; };
const isChordTone = (ctx, n) => { const c = chordAt(ctx, n.start); return c ? c.chord.tones.has(((n.pitch % 12) + 12) % 12) : true; };
const inKey = (ctx, pitch) => (ctx.key ? ctx.key.has(((pitch % 12) + 12) % 12) : true);

const at = (ctx, beat) => `bar ${Math.floor(beat / ctx.bpb + E) + 1} beat ${+(posIn(beat, ctx.bpb) + 1).toFixed(2)}`;
const result = (ctx, count, first, what, min = 1) => ({ found: count >= min, count, evidence: count ? `${count} ${what}${first !== undefined && first !== null ? `, first at ${at(ctx, first)}` : ''}` : `no ${what}` });

// ---- detectors on one part's notes ----

function articulation(notes) {
  const l = line(notes);
  const ratios = [];
  for (let i = 0; i + 1 < l.length; i++) { const ioi = l[i + 1].start - l[i].start; if (ioi > E) ratios.push(l[i].dur / ioi); }
  return { ratios, n: ratios.length, med: median(ratios), durs: l.map((x) => x.dur) };
}
const D = {
  staccato: (ctx, notes) => { const a = articulation(notes); const ok = a.n >= 6 && a.med <= 0.55; return { found: ok, count: a.n, evidence: a.n < 6 ? `only ${a.n} notes to judge` : `notes sound ${Math.round(a.med * 100)}% of the time to the next (staccato is at most 55%)` }; },
  legato: (ctx, notes) => { const a = articulation(notes); const ok = a.n >= 6 && a.med >= 0.95; return { found: ok, count: a.n, evidence: a.n < 6 ? `only ${a.n} notes to judge` : `notes sound ${Math.round(a.med * 100)}% of the time to the next (legato is 95% or more: each note runs into the next)` }; },
  held: (ctx, notes) => { const a = articulation(notes); const ok = a.n >= 6 && a.med >= 0.85 && a.med <= 1.02; return { found: ok, count: a.n, evidence: a.n < 6 ? `only ${a.n} notes to judge` : `notes sound ${Math.round(a.med * 100)}% of their value (tenuto is 85-100%: full value, not overlapping)` }; },
  accent: (ctx, notes) => {
    const v = notes.map((n) => n.velocity); const m = median(v);
    const acc = notes.filter((n) => n.velocity >= m + 0.15);
    const ok = notes.length >= 8 && acc.length >= 3 && acc.length <= notes.length * 0.5;
    return { found: ok, count: acc.length, evidence: `${acc.length} of ${notes.length} notes stand out by 0.15+ velocity over the part's ${m.toFixed(2)}${acc.length ? `, first at ${at(ctx, acc[0].start)}` : ''}` };
  },
  marcato: (ctx, notes) => {
    const l = line(notes); const m = median(l.map((n) => n.velocity));
    let c = 0; let first = null;
    for (let i = 0; i + 1 < l.length; i++) { const ioi = l[i + 1].start - l[i].start; if (l[i].velocity >= m + 0.15 && ioi > E && l[i].dur / ioi <= 0.7) { c++; first ??= l[i].start; } }
    return result(ctx, c, first, 'accented short notes', 3);
  },
  sforzando: (ctx, notes) => {
    const l = line(notes); const m = median(l.map((n) => n.velocity)); let c = 0; let first = null;
    for (let i = 0; i + 2 < l.length; i++) if (l[i].velocity >= m + 0.25 && (l[i + 1].velocity + l[i + 2].velocity) / 2 <= m - 0.1) { c++; first ??= l[i].start; }
    return result(ctx, c, first, 'sudden loud attacks dropping straight back to soft');
  },
  crescendo: (ctx, notes) => velocityShape(ctx, notes, 'up'),
  diminuendo: (ctx, notes) => velocityShape(ctx, notes, 'down'),
  swell: (ctx, notes) => velocityShape(ctx, notes, 'swell'),
  fadein: (ctx, notes) => velocityShape(ctx, notes, 'fadein'),
  fadeout: (ctx, notes) => velocityShape(ctx, notes, 'fadeout'),
  trill: (ctx, notes) => alternation(ctx, notes, { steps: [1, 2], maxIoi: 0.25, min: 6, what: 'trills (6+ fast alternations a step apart)' }),
  neighborosc: (ctx, notes) => alternation(ctx, notes, { steps: [1, 2], minIoi: 0.26, min: 5, what: 'neighbour oscillations (5+ slower alternations a step apart)' }),
  tremolo: (ctx, notes) => {
    const l = line(notes); const fast = (a, b) => b.start - a.start <= 0.25 + E;
    const same = runs(l, (a, b) => fast(a, b) && a.pitch === b.pitch, 6);
    const wide = alternation(ctx, notes, { steps: [3, 4, 5, 6, 7, 8, 9, 10, 11, 12], maxIoi: 0.25, min: 6, what: '' });
    const c = same.length + wide.count;
    return result(ctx, c, same[0]?.[0].start ?? (wide.count ? wide.first : null), 'tremolos (6+ fast repeats of a note, or of two notes a 3rd or more apart)');
  },
  ratchet: (ctx, notes) => repeats(ctx, notes, { maxIoi: 0.125, min: 3, what: 'ratchets (3+ repeats at a 32nd or faster)' }),
  roll: (ctx, notes) => repeats(ctx, notes, { maxIoi: 0.125, min: 6, what: 'rolls (6+ hits at a 32nd or faster on one sound)' }),
  mordent: (ctx, notes) => {
    const l = line(notes); let c = 0; let first = null;
    for (let i = 0; i + 2 < l.length; i++) {
      const [a, b, x] = [l[i], l[i + 1], l[i + 2]];
      if (a.pitch === x.pitch && [1, 2].includes(Math.abs(b.pitch - a.pitch)) && b.start - a.start <= 0.2 + E && x.start - b.start <= 0.2 + E && a.dur <= 0.2 + E && b.dur <= 0.2 + E && x.dur >= Math.max(a.dur, b.dur) * 1.5) { c++; first ??= a.start; i += 2; }
    }
    return result(ctx, c, first, 'mordents (main, quick neighbour, main held)', 2);
  },
  turn: (ctx, notes) => {
    const l = line(notes); let c = 0; let first = null;
    for (let i = 0; i + 3 < l.length; i++) {
      const w = l.slice(i, i + 4);
      if (w.slice(1).some((n, k) => n.start - w[k].start > 0.25 + E)) continue;
      const [u, m, lo, m2] = w.map((n) => n.pitch);
      if (m === m2 && [1, 2].includes(u - m) && [1, 2].includes(m - lo)) { c++; first ??= w[0].start; i += 3; }
      else if (m === m2 && [1, 2].includes(m - u) && [1, 2].includes(lo - m)) { c++; first ??= w[0].start; i += 3; }
    }
    return result(ctx, c, first, 'turns (above, main, below, main: quick)', 2);
  },
  grace: (ctx, notes) => {
    const l = line(notes); let c = 0; let first = null;
    for (let i = 0; i + 1 < l.length; i++) {
      const [g, n] = [l[i], l[i + 1]];
      if (g.dur <= 0.15 + E && n.start - g.start <= 0.17 + E && Math.abs(n.pitch - g.pitch) >= 1 && Math.abs(n.pitch - g.pitch) <= 4 && n.dur >= g.dur * 2) { c++; first ??= g.start; i++; }
    }
    return result(ctx, c, first, 'grace notes (a very short note leaning into the next, a step or two away)', 2);
  },
  arpeggio: (ctx, notes) => {
    const singles = groups(notes).filter((g) => g.notes.length === 1).map((g) => g.notes[0]);
    let c = 0; let first = null;
    for (let i = 0; i + 3 < singles.length;) {
      const w = singles.slice(i, i + 4);
      const ok = w.slice(1).every((n, k) => { const d = Math.abs(n.pitch - w[k].pitch); return d >= 3 && d <= 12 && n.start - w[k].start <= 1 + E; })
        && new Set(w.map((n) => n.pitch % 12)).size >= 3 && w.every((n) => isChordTone(ctx, n));
      if (ok) { c++; first ??= w[0].start; i += 4; } else i++;
    }
    return result(ctx, c, first, 'arpeggiated figures (4 chord tones one at a time, a 3rd or more apart)', 2);
  },
  sweep: (ctx, notes) => {
    const singles = groups(notes).filter((g) => g.notes.length === 1).map((g) => g.notes[0]);
    const r = runs(singles, (a, b, i) => { const d = b.pitch - a.pitch; return Math.abs(d) >= 3 && Math.abs(d) <= 9 && b.start - a.start <= 0.25 + E && (i < 2 || sign(d) === sign(a.pitch - singles[i - 2].pitch)); }, 5);
    return result(ctx, r.length, r[0]?.[0].start, 'arpeggio sweeps (5+ chord leaps one way, a 16th apart or faster)');
  },
  alberti: (ctx, notes) => {
    const l = groups(notes).filter((g) => g.notes.length === 1).map((g) => g.notes[0]);
    let c = 0; let first = null;
    for (let i = 0; i + 3 < l.length;) {
      const [a, b, x, b2] = l.slice(i, i + 4).map((n) => n.pitch);
      if (b === b2 && a < x && x < b) { c++; first ??= l[i].start; i += 4; } else i++;
    }
    return result(ctx, c, first, 'Alberti figures (low, high, middle, high)', 3);
  },
  ostinato: (ctx, notes) => {
    const r = barRepeats(ctx, notes, { minNotes: 2 });
    const ok = r.best1 >= 4 || r.best2 >= 3;
    return { found: ok, count: Math.max(r.best1, r.best2), evidence: ok ? `the same ${r.best1 >= 4 ? '1-bar' : '2-bar'} pattern ${Math.max(r.best1, r.best2)} times in a row from ${at(ctx, r.at)}` : `the longest exact repeat is ${r.best1} bar${r.best1 === 1 ? '' : 's'} (an ostinato repeats one bar 4+ times, or two bars 3+ times)` };
  },
  riff: (ctx, notes) => {
    const r = barRepeats(ctx, notes, { minNotes: 4, minPitches: 3 });
    const ok = r.best1 >= 2 || r.best2 >= 2;
    return { found: ok, count: Math.max(r.best1, r.best2), evidence: ok ? `a melodic ${r.best1 >= 2 ? '1-bar' : '2-bar'} figure repeated ${Math.max(r.best1, r.best2)} times from ${at(ctx, r.at)}` : 'no melodic 1- or 2-bar figure (4+ notes, 3+ pitches) played twice in a row' };
  },
  repeatednote: (ctx, notes) => {
    const r = runs(line(notes), (a, b) => a.pitch === b.pitch && b.start - a.start <= 1 + E, 4);
    return result(ctx, r.length, r[0]?.[0].start, 'repeated-note figures (one pitch 4+ times)', 2);
  },
  pedal: (ctx, notes) => {
    const l = line(notes, 'bottom'); if (!l.length) return result(ctx, 0, null, 'pedal');
    const span = l.at(-1).start + l.at(-1).dur - l[0].start;
    const byPitch = new Map(); for (const n of l) byPitch.set(n.pitch, (byPitch.get(n.pitch) || 0) + n.dur);
    const [p, dur] = [...byPitch.entries()].sort((a, b) => b[1] - a[1])[0];
    const under = new Set(ctx.timeline.filter((c) => l.some((n) => n.pitch === p && n.start < c.end && n.start + n.dur > c.begin)).map((c) => c.chord.sym));
    const ok = dur >= span * 0.6 && span >= ctx.bpb * 2 && (ctx.timeline.length ? under.size >= 2 : span >= ctx.bpb * 4);
    return { found: ok, count: under.size, evidence: `${Math.round((dur / Math.max(E, span)) * 100)}% of the part is one pitch${ctx.timeline.length ? `, under ${under.size} different chord${under.size === 1 ? '' : 's'}` : ''} (a pedal holds or repeats one note under 2+ chords for 60%+ of the time)` };
  },
  drone: (ctx, notes) => {
    const long = notes.filter((n) => n.dur >= ctx.bpb * 2 - E);
    return result(ctx, long.length, long[0]?.start, 'notes held 2 bars or longer');
  },
  walking: (ctx, notes) => {
    const l = line(notes, 'bottom');
    const r = runs(l, (a, b) => near(b.start - a.start, 1, 0.1) && b.pitch !== a.pitch && Math.abs(b.pitch - a.pitch) <= 7, 8)
      .filter((run) => { const steps = run.slice(1).filter((n, k) => Math.abs(n.pitch - run[k].pitch) <= 2).length; return steps >= (run.length - 1) * 0.5; });
    return result(ctx, r.length, r[0]?.[0].start, 'walking lines (8+ quarter notes, a new pitch each beat, mostly by step)');
  },
  scalarrun: (ctx, notes) => run(ctx, notes, { steps: [1, 2], min: 5, maxIoi: 0.5, what: 'scalar runs (5+ notes by step one way)' }),
  diatonicrun: (ctx, notes) => run(ctx, notes, { steps: [1, 2], min: 5, maxIoi: 0.5, keep: (r) => r.every((n) => inKey(ctx, n.pitch)) && r.some((n, k) => k && Math.abs(n.pitch - r[k - 1].pitch) === 2), what: 'diatonic runs (5+ scale notes by step one way)' }),
  chromaticrun: (ctx, notes) => run(ctx, notes, { steps: [1], min: 5, maxIoi: 0.5, what: 'chromatic runs (5+ notes by semitone one way)' }),
  pentatonicrun: (ctx, notes) => run(ctx, notes, { steps: [2, 3], min: 5, maxIoi: 0.5, keep: (r) => r.slice(1).some((n, k) => Math.abs(n.pitch - r[k].pitch) === 3), what: 'pentatonic runs (5+ notes one way in whole steps and minor 3rds)' }),
  octaverun: (ctx, notes) => {
    const g = groups(notes).filter((x) => x.notes.length >= 2 && x.notes.some((a) => x.notes.some((b) => b.pitch - a.pitch === 12)));
    const r = runs(g, (a, b) => { const d = Math.max(...b.notes.map((n) => n.pitch)) - Math.max(...a.notes.map((n) => n.pitch)); return d !== 0 && Math.abs(d) <= 2 && b.start - a.start <= 0.5 + E; }, 4);
    return result(ctx, r.length, r[0]?.[0].start, 'runs in octaves (4+ steps, doubled an octave apart)');
  },
  glissando: (ctx, notes) => run(ctx, notes, { steps: [1, 2], min: 6, maxIoi: 0.125, what: 'glissandi (6+ notes by step at a 32nd or faster)' }),
  cascade: (ctx, notes) => run(ctx, notes, { steps: [1, 2, 3, 4], min: 6, maxIoi: 0.25, dir: -1, keep: (r) => r[0].pitch - r.at(-1).pitch >= 7, what: 'cascades (6+ fast notes tumbling down a 5th or more)' }),
  sequence: (ctx, notes) => sequences(ctx, notes, {}),
  staircaseup: (ctx, notes) => sequences(ctx, notes, { dir: 1 }),
  staircasedown: (ctx, notes) => sequences(ctx, notes, { dir: -1 }),
  spiral: (ctx, notes) => sequences(ctx, notes, { turning: true }),
  arch: (ctx, notes) => contour(ctx, notes, 'arch'),
  invarch: (ctx, notes) => contour(ctx, notes, 'invarch'),
  wave: (ctx, notes) => contour(ctx, notes, 'wave'),
  pendulum: (ctx, notes) => zig(ctx, notes, { minLeap: 3, net: false, what: 'pendulum swings (6+ notes turning every note, a 3rd or more each way)' }),
  zigzag: (ctx, notes) => zig(ctx, notes, { minLeap: 1, net: true, what: 'zigzags (6+ notes turning every note while climbing or falling 4+ semitones)' }),
  leapstep: (ctx, notes) => {
    const l = line(notes); let c = 0; let first = null;
    for (let i = 0; i + 2 < l.length; i++) { const a = l[i + 1].pitch - l[i].pitch; const b = l[i + 2].pitch - l[i + 1].pitch; if (Math.abs(a) >= 5 && Math.abs(b) >= 1 && Math.abs(b) <= 2 && sign(a) !== sign(b)) { c++; first ??= l[i].start; } }
    return result(ctx, c, first, 'leaps answered by a step back', 3);
  },
  stepleap: (ctx, notes) => {
    const l = line(notes); let c = 0; let first = null;
    for (let i = 0; i + 3 < l.length; i++) { const a = l[i + 1].pitch - l[i].pitch; const b = l[i + 2].pitch - l[i + 1].pitch; const x = l[i + 3].pitch - l[i + 2].pitch; if ([1, 2].includes(Math.abs(a)) && [1, 2].includes(Math.abs(b)) && sign(a) === sign(b) && Math.abs(x) >= 5) { c++; first ??= l[i].start; } }
    return result(ctx, c, first, 'steps that spring into a leap', 3);
  },
  passing: (ctx, notes) => tones(ctx, notes, 'passing'),
  neighbor: (ctx, notes) => tones(ctx, notes, 'neighbor'),
  approach: (ctx, notes) => tones(ctx, notes, 'approach'),
  enclosurechrom: (ctx, notes) => enclosures(ctx, notes, 'chromatic'),
  enclosurediat: (ctx, notes) => enclosures(ctx, notes, 'diatonic'),
  enclosurearp: (ctx, notes) => enclosures(ctx, notes, 'arpeggio'),
  enclosure: (ctx, notes) => { const a = enclosures(ctx, notes, 'chromatic'); const b = enclosures(ctx, notes, 'diatonic'); const c = a.count + b.count; return { found: c >= 2, count: c, evidence: `${c} enclosures (a target approached from above and below)` }; },
  anticipationtone: (ctx, notes) => {
    let c = 0; let first = null;
    for (const n of line(notes)) {
      const cur = chordAt(ctx, n.start); if (!cur) continue;
      const nextC = ctx.timeline.find((x) => x.begin > cur.begin + E); if (!nextC || nextC.begin - n.start > 0.5 + E || nextC.chord.sym === cur.chord.sym) continue;
      const pc = ((n.pitch % 12) + 12) % 12;
      if (nextC.chord.tones.has(pc) && !cur.chord.tones.has(pc)) { c++; first ??= n.start; }
    }
    return result(ctx, c, first, 'anticipation tones (the next chord\'s note, sounded just before it arrives)', 2);
  },
  suspension: (ctx, notes) => {
    let c = 0; let first = null;
    for (let i = 0; i + 1 < ctx.timeline.length; i++) {
      const [a, b] = [ctx.timeline[i], ctx.timeline[i + 1]];
      if (!/sus/.test(a.chord.sym) || /sus/.test(b.chord.sym) || a.chord.root !== b.chord.root) continue;
      const fourth = (a.chord.root + 5) % 12;
      const hasFourth = notes.some((n) => n.start < a.end && n.start + n.dur > a.begin && ((n.pitch % 12) + 12) % 12 === fourth);
      const resolves = notes.some((n) => n.start >= b.begin - E && n.start < b.end && b.chord.tones.has(((n.pitch % 12) + 12) % 12) && [3, 4].includes(((n.pitch - a.chord.root) % 12 + 12) % 12));
      if (hasFourth && resolves) { c++; first ??= a.begin; }
    }
    return result(ctx, c, first, 'suspensions (a sus chord\'s 4th, resolved to the 3rd on the same root)');
  },
  block: (ctx, notes) => { const g = groups(notes).filter((x) => x.notes.length >= 3); return result(ctx, g.length, g[0]?.start, 'block chords (3+ notes struck together)', 4); },
  power: (ctx, notes) => {
    const g = groups(notes).filter((x) => { const ps = x.notes.map((n) => n.pitch); const lo = Math.min(...ps); const pcs = new Set(ps.map((p) => ((p - lo) % 12 + 12) % 12)); return ps.length >= 2 && pcs.has(7) && [...pcs].every((p) => p === 0 || p === 7); });
    return result(ctx, g.length, g[0]?.start, 'power chords (root and 5th, no 3rd)', 4);
  },
  stab: (ctx, notes) => {
    const g = groups(notes).filter((x) => x.notes.length >= 3); let c = 0; let first = null;
    for (let i = 0; i < g.length; i++) { const next = g[i + 1]?.start ?? Infinity; const d = Math.max(...g[i].notes.map((n) => n.dur)); if (d <= 0.5 + E && next - g[i].start >= d * 2) { c++; first ??= g[i].start; } }
    return result(ctx, c, first, 'chord stabs (short struck chords with space after)', 4);
  },
  pulse: (ctx, notes) => {
    const g = groups(notes).filter((x) => x.notes.length >= 3);
    const r = runs(g, (a, b, i) => b.start - a.start <= 1 + E && (i < 2 || near(b.start - a.start, a.start - g[i - 2].start, 0.03)), 4);
    return result(ctx, r.length, r[0]?.[0].start, 'chord pulses (4+ chords struck on an even beat or faster)');
  },
  gate: (ctx, notes) => {
    const g = groups(notes).filter((x) => x.notes.length >= 2 && Math.max(...x.notes.map((n) => n.dur)) <= 0.2 + E);
    const r = runs(g, (a, b) => b.start - a.start <= 0.25 + E, 8);
    return result(ctx, r.length, r[0]?.[0].start, 'gated chord rhythms (8+ chopped chords at a 16th or faster)');
  },
  strum: (ctx, notes) => spreadChords(ctx, notes, { lo: 0.005, hi: 0.15, what: 'strums (3+ chord notes a hair apart, in pitch order)', min: 4 }),
  rolledchord: (ctx, notes) => spreadChords(ctx, notes, { lo: 0.1, hi: 0.75, what: 'rolled chords (3+ notes spread upward, each held)', min: 2 }),
  thirds: (ctx, notes) => dyads(ctx, notes, [3, 4], 'thirds'),
  sixths: (ctx, notes) => dyads(ctx, notes, [8, 9], 'sixths'),
  questionanswer: (ctx, notes) => {
    if (ctx.tonic === null) return { found: false, count: 0, evidence: 'the blueprint has no scale to judge the tonic by' };
    const ph = phrases(line(notes)).filter((p) => p.length >= 3); let c = 0; let first = null;
    for (let i = 0; i + 1 < ph.length; i++) { const end = (p) => ((p.at(-1).pitch % 12) + 12) % 12; if (end(ph[i]) !== ctx.tonic && end(ph[i + 1]) === ctx.tonic) { c++; first ??= ph[i][0].start; i++; } }
    return result(ctx, c, first, 'question-and-answer pairs (a phrase left open, answered by one that lands home)', 2);
  },
  syncopation: (ctx, notes) => {
    const g = groups(notes); let c = 0; let first = null;
    for (let i = 0; i < g.length; i++) {
      const p = posIn(g[i].start, 1); if (p < 0.05 || p > 0.95) continue;
      const nextBeat = Math.ceil(g[i].start - E);
      const held = g[i].notes.some((n) => n.start + n.dur > nextBeat + 0.05);
      const nothingOnBeat = !g.some((x) => near(x.start, nextBeat, 0.02));
      if (held || nothingOnBeat) { c++; first ??= g[i].start; }
    }
    const ok = c >= 4 && c >= g.length * 0.2;
    return { found: ok, count: c, evidence: `${c} of ${g.length} attacks are off the beat and push across it${first !== null ? `, first at ${at(ctx, first)}` : ''} (syncopation needs 4+ and 20%+)` };
  },
  offbeat: (ctx, notes) => {
    const g = groups(notes); const off = g.filter((x) => near(posIn(x.start, 1), 0.5, 0.03)).length; const on = g.filter((x) => near(posIn(x.start, 1), 0, 0.03) || near(posIn(x.start, 1), 1, 0.03)).length;
    const ok = g.length >= 6 && off >= g.length * 0.4 && on <= g.length * 0.3;
    return { found: ok, count: off, evidence: `${off} of ${g.length} attacks on the "and", ${on} on the beat (offbeat: 40%+ on the "and", 30% or less on the beat)` };
  },
  backbeat: (ctx, notes) => {
    const sn = notes.filter((n) => [37, 38, 39, 40].includes(n.pitch));
    const beatsBack = ctx.bpb >= 4 ? [1, 3] : [1];
    const on = sn.filter((n) => beatsBack.some((b) => near(posIn(n.start, ctx.bpb), b, 0.05)));
    const barsWith = new Set(on.map((n) => Math.floor(n.start / ctx.bpb + E))).size;
    const spanBars = notes.length ? new Set(notes.map((n) => Math.floor(n.start / ctx.bpb + E))).size : 0;
    const ok = sn.length >= 4 && on.length >= sn.length * 0.7 && barsWith >= spanBars * 0.6;
    return { found: ok, count: on.length, evidence: `${on.length} of ${sn.length} snare/clap hits on beats ${beatsBack.map((b) => b + 1).join(' and ')}, in ${barsWith} of ${spanBars} bars (a backbeat: 70%+ of them, in 60%+ of the bars)` };
  },
  triplet: (ctx, notes) => grid(ctx, notes, [1 / 3, 2 / 3], 'attacks on triplet positions'),
  tuplet: (ctx, notes) => grid(ctx, notes, [1 / 3, 2 / 3, 0.2, 0.4, 0.6, 0.8, 1 / 6, 5 / 6], 'attacks on tuplet positions (thirds, fifths or sixths of a beat)'),
  swing: (ctx, notes) => {
    const g = groups(notes); const offs = g.map((x) => posIn(x.start, 1)).filter((p) => p > 0.05 && p < 0.95);
    const swung = offs.filter((p) => p >= 0.58 && p <= 0.72).length; const straight = offs.filter((p) => near(p, 0.5, 0.03)).length;
    const ok = swung >= 6 && swung >= offs.length * 0.6 && straight <= offs.length * 0.25;
    return { found: ok, count: swung, evidence: `${swung} of ${offs.length} off-beat attacks land late (58-72% through the beat), ${straight} dead straight (swing: 6+ late, 60%+ of them, 25% or fewer straight)` };
  },
  dotted: (ctx, notes) => ioiPattern(ctx, notes, [[0.75, 0.25], [1.5, 0.5], [0.375, 0.125]], 'dotted rhythms (long-short in 3:1)', 4),
  gallop: (ctx, notes) => ioiPattern(ctx, notes, [[0.5, 0.25, 0.25], [0.25, 0.25, 0.5]], 'gallops (an 8th and two 16ths)', 4),
  tresillo: (ctx, notes) => barPattern(ctx, notes, [[0, 1.5, 3], [0, 0.75, 1.5, 2, 2.75, 3.5]], 'tresillo bars (3+3+2)', 2),
  clave: (ctx, notes) => {
    const per = barOnsets(ctx, notes);
    const pats = [[[0, 1.5, 3], [1, 2]], [[1, 2], [0, 1.5, 3]], [[0, 1.5, 3.5], [1, 2]], [[1, 2], [0, 1.5, 3.5]]];
    let c = 0; let first = null;
    for (let b = 0; b + 1 < per.length; b++) if (pats.some(([x, y]) => same(per[b].on, x) && same(per[b + 1].on, y))) { c++; first ??= per[b].bar * ctx.bpb; b++; }
    return result(ctx, c, first, 'clave cycles (3-2 or 2-3, son or rumba)', 2);
  },
  euclid: (ctx, notes) => {
    const slots = Math.round(ctx.bpb * 4); if (!Number.isInteger(ctx.bpb * 4)) return result(ctx, 0, null, 'euclidean bars');
    let c = 0; let first = null;
    for (const { bar: b, on } of barOnsets(ctx, notes)) {
      const hits = new Array(slots).fill(0); let off = false;
      for (const p of on) { const s = Math.round(p * 4); if (!near(p * 4, s, 0.05)) off = true; else hits[s % slots] = 1; }
      const k = hits.reduce((a, x) => a + x, 0);
      if (off || k < 3 || slots % k === 0 || k >= slots) continue;
      const e = Array.from({ length: slots }, (_, i) => (Math.floor(((i + 1) * k) / slots) - Math.floor((i * k) / slots)));
      const pat = hits.join(''); const ep = e.join('');
      if ([...Array(slots).keys()].some((r) => ep.slice(r) + ep.slice(0, r) === pat)) { c++; first ??= b * ctx.bpb; }
    }
    return result(ctx, c, first, 'euclidean bars (k hits spread as evenly as the 16th grid allows)', 2);
  },
  polymeter: (ctx, notes) => {
    const step = 0.25; const slots = new Map();
    for (const n of notes) { const s = Math.round(n.start / step); if (near(n.start / step, s, 0.05)) slots.set(s, `${slots.get(s) || ''}${n.pitch},`); }
    if (slots.size < 6) return result(ctx, 0, null, 'polymetric cycles');
    const lo = Math.min(...slots.keys()); const hi = Math.max(...slots.keys()); const bar = Math.round(ctx.bpb / step);
    const val = (i) => slots.get(i) || '';
    for (let P = 2; P <= bar * 2; P++) {
      if (P % bar === 0 || bar % P === 0) continue;
      for (let s0 = lo; s0 + 3 * P <= hi + 1; s0 += 1) {
        let ok = true; let filled = 0;
        for (let i = s0; i < s0 + 3 * P - P; i++) { if (val(i) !== val(i + P)) { ok = false; break; } if (val(i)) filled++; }
        if (ok && filled >= 4) return { found: true, count: 1, evidence: `a ${P * step}-beat pattern repeats 3+ times against the ${ctx.bpb}-beat bar from ${at(ctx, s0 * step)}` };
      }
    }
    return { found: false, count: 0, evidence: 'no pattern that repeats 3+ times on a length the bar doesn\'t divide' };
  },
  crossrhythm: (ctx, notes) => {
    const l = line(notes);
    const r = runs(l, (a, b, i) => { const d = b.start - a.start; return [0.75, 1.5, 2 / 3, 4 / 3].some((x) => near(d, x, 0.02)) && (i < 2 || near(d, a.start - l[i - 2].start, 0.02)); }, 5);
    return result(ctx, r.length, r[0]?.[0].start, 'cross-rhythms against the beat (5+ even attacks every ¾, 1½, ⅔ or 1⅓ beats)');
  },
  rubato: (ctx, notes) => {
    const g = groups(notes); const off = g.filter((x) => { const s = offGrid(x.start); return s >= 0.02 && s <= 0.2; }).length;
    const ok = g.length >= 6 && off >= g.length * 0.3;
    return { found: ok, count: off, evidence: `${off} of ${g.length} attacks pulled off the grid by 0.02-0.2 beat (rubato: 30%+)` };
  },
  push: (ctx, notes) => feel(ctx, notes, -1),
  drag: (ctx, notes) => feel(ctx, notes, 1),
  delay: (ctx, notes) => {
    const ph = phrases(line(notes), 1); let c = 0; let first = null;
    for (const p of ph.slice(1)) { const pos = posIn(p[0].start, ctx.bpb); if (pos >= 0.25 - E && pos <= 0.75 + E) { c++; first ??= p[0].start; } }
    return result(ctx, c, first, 'delayed entries (a phrase coming in a 16th to an 8th after the bar line)', 3);
  },
  ritardando: (ctx, notes) => tempoTrend(ctx, notes, 1),
  accelerando: (ctx, notes) => tempoTrend(ctx, notes, -1),
  pickup: (ctx, notes) => {
    const ph = phrases(line(notes), 1.5); let c = 0; let first = null;
    for (const p of ph) {
      const pos = posIn(p[0].start, ctx.bpb);
      const down = Math.ceil(p[0].start / ctx.bpb - E) * ctx.bpb;
      if (pos >= ctx.bpb - 1 - E && pos > E && p.some((n) => near(n.start, down, 0.02))) { c++; first ??= p[0].start; }
    }
    return result(ctx, c, first, 'pickups (a phrase starting in the last beat, leading into the downbeat)', 2);
  },
  anticipation: (ctx, notes) => {
    let c = 0; let first = null;
    for (const n of notes) {
      const pos = posIn(n.start, ctx.bpb); const down = Math.ceil(n.start / ctx.bpb - E) * ctx.bpb;
      if (pos >= ctx.bpb - 0.5 - E && pos <= ctx.bpb - 0.25 + E && n.start + n.dur > down + 0.1 && !notes.some((x) => near(x.start, down, 0.02))) { c++; first ??= n.start; }
    }
    return result(ctx, c, first, 'anticipations (attacks just before the bar line, held across it, nothing on the downbeat)', 3);
  },
  ghost: (ctx, notes) => {
    const m = median(notes.map((n) => n.velocity)); const g = notes.filter((n) => n.velocity <= 0.35);
    const ok = m >= 0.55 && g.length >= 4;
    return { found: ok, count: g.length, evidence: `${g.length} notes at velocity 0.35 or under, among notes around ${m.toFixed(2)} (ghost notes: 4+ under a louder part)` };
  },
  dead: (ctx, notes) => { const d = notes.filter((n) => n.dur <= 0.1 + E && n.velocity <= 0.5); return result(ctx, d.length, d[0]?.start, 'dead notes (very short and soft)', 4); },
  palmmute: (ctx, notes) => {
    const r = runs(line(notes, 'bottom'), (a, b) => b.dur <= 0.3 + E && a.dur <= 0.3 + E && b.pitch <= 60 && b.start - a.start <= 0.5 + E, 8);
    return result(ctx, r.length, r[0]?.[0].start, 'palm-muted chugs (8+ short low notes in a row)');
  },
  flam: (ctx, notes) => {
    const s = [...notes].sort((a, b) => a.start - b.start); let c = 0; let first = null;
    for (let i = 0; i + 1 < s.length; i++) { const d = s[i + 1].start - s[i].start; if (s[i].pitch === s[i + 1].pitch && d >= 0.02 - E && d <= 0.08 + E && s[i].velocity < s[i + 1].velocity) { c++; first ??= s[i].start; i++; } }
    return result(ctx, c, first, 'flams (a softer grace hit just before the main one)', 2);
  },
  percdrag: (ctx, notes) => {
    const s = [...notes].sort((a, b) => a.start - b.start); let c = 0; let first = null;
    for (let i = 0; i + 2 < s.length; i++) { const [a, b, x] = s.slice(i, i + 3); if (a.pitch === b.pitch && b.pitch === x.pitch && b.start - a.start <= 0.1 + E && x.start - a.start <= 0.25 + E && a.velocity < x.velocity && b.velocity < x.velocity) { c++; first ??= a.start; i += 2; } }
    return result(ctx, c, first, 'drags (two quick grace hits into a main one)', 2);
  },
  paradiddle: (ctx, notes) => {
    const l = line(notes); const pitches = l.map((n) => n.pitch); let c = 0; let first = null;
    for (let i = 0; i + 7 < l.length; i++) {
      const w = pitches.slice(i, i + 8); const [A, B] = [w[0], w[1]];
      if (A === B) continue;
      if (w.join(',') === [A, B, A, A, B, A, B, B].join(',') && l.slice(i, i + 8).every((n, k) => !k || n.start - l[i + k - 1].start <= 0.5 + E)) { c++; first ??= l[i].start; i += 7; }
    }
    return result(ctx, c, first, 'paradiddles (two sounds in the pattern A B A A, B A B B)');
  },
  // Section-level and pair detectors read ctx and scope directly (below).
};

function velocityShape(ctx, notes, kind) {
  const bars = new Map();
  for (const n of notes) { const b = Math.floor(n.start / ctx.bpb + E); if (!bars.has(b)) bars.set(b, []); bars.get(b).push(n.velocity); }
  const series = [...bars.entries()].sort((a, b) => a[0] - b[0]).map(([b, v]) => ({ b, v: v.reduce((x, y) => x + y, 0) / v.length }));
  const ups = runs(series, (a, b) => b.b === a.b + 1 && b.v > a.v + 0.01, 3).filter((r) => r.at(-1).v - r[0].v >= 0.15);
  const downs = runs(series, (a, b) => b.b === a.b + 1 && b.v < a.v - 0.01, 3).filter((r) => r[0].v - r.at(-1).v >= 0.15);
  const bar = (r) => r[0].b * ctx.bpb;
  if (kind === 'up') return result(ctx, ups.length, ups[0] ? bar(ups[0]) : null, 'crescendos (velocity rising 0.15+ over 3+ bars in a row)');
  if (kind === 'down') return result(ctx, downs.length, downs[0] ? bar(downs[0]) : null, 'diminuendos (velocity falling 0.15+ over 3+ bars in a row)');
  if (kind === 'swell') { const s = ups.filter((u) => downs.some((d) => d[0].b === u.at(-1).b)); return result(ctx, s.length, s[0] ? bar(s[0]) : null, 'swells (a crescendo turning straight into a diminuendo)'); }
  if (!series.length) return result(ctx, 0, null, kind === 'fadein' ? 'fade-ins' : 'fade-outs');
  const first = series[0].b; const last = series.at(-1).b; const quarter = Math.max(2, (last - first + 1) / 4);
  if (kind === 'fadein') { const f = ups.filter((r) => r[0].b - first <= quarter && r[0].v <= 0.45); return result(ctx, f.length, f[0] ? bar(f[0]) : null, 'fade-ins (velocity rising from 0.45 or less, at the start)'); }
  const f = downs.filter((r) => last - r.at(-1).b <= quarter && r.at(-1).v <= 0.45);
  return result(ctx, f.length, f[0] ? bar(f[0]) : null, 'fade-outs (velocity falling to 0.45 or less, at the end)');
}
function alternation(ctx, notes, { steps, maxIoi = Infinity, minIoi = 0, min, what }) {
  const l = line(notes);
  const r = runs(l, (a, b, i) => { const d = b.start - a.start; return d <= maxIoi + E && d >= minIoi - E && steps.includes(Math.abs(b.pitch - a.pitch)) && (i < 2 || b.pitch === l[i - 2].pitch); }, min);
  return { ...result(ctx, r.length, r[0]?.[0].start, what), first: r[0]?.[0].start ?? null };
}
function repeats(ctx, notes, { maxIoi, min, what }) {
  const byPitch = new Map(); for (const n of notes) { if (!byPitch.has(n.pitch)) byPitch.set(n.pitch, []); byPitch.get(n.pitch).push(n); }
  const all = [...byPitch.values()].flatMap((ns) => runs(ns.sort((a, b) => a.start - b.start), (a, b) => b.start - a.start <= maxIoi + E, min));
  all.sort((a, b) => a[0].start - b[0].start);
  return result(ctx, all.length, all[0]?.[0].start, what);
}
function run(ctx, notes, { steps, min, maxIoi, dir = 0, keep = () => true, what }) {
  const l = line(notes);
  const r = runs(l, (a, b, i) => { const d = b.pitch - a.pitch; return steps.includes(Math.abs(d)) && b.start - a.start <= maxIoi + E && (dir ? sign(d) === dir : i < 2 || sign(d) === sign(a.pitch - l[i - 2].pitch)); }, min).filter(keep);
  return result(ctx, r.length, r[0]?.[0].start, what);
}
// The same 3-5 note figure played at least three times in a row, each time moved by the same step.
function sequences(ctx, notes, { dir = 0, turning = false }) {
  const l = line(notes); let c = 0; let first = null;
  for (let k = 3; k <= 5; k++) {
    for (let i = 0; i + 3 * k <= l.length; i++) {
      const fig = (j) => l.slice(j, j + k);
      const shape = (f) => f.slice(1).map((n, m) => `${n.pitch - f[m].pitch}@${(n.start - f[m].start).toFixed(3)}`).join(' ');
      const [a, b, x] = [fig(i), fig(i + k), fig(i + 2 * k)];
      if (shape(a) !== shape(b) || shape(b) !== shape(x)) continue;
      const t1 = b[0].pitch - a[0].pitch; const t2 = x[0].pitch - b[0].pitch;
      if (t1 === 0 || t1 !== t2 || Math.abs(t1) > 7) continue;
      if (dir && (sign(t1) !== dir || Math.abs(t1) > 4)) continue;
      const iv = a.slice(1).map((n, m) => n.pitch - a[m].pitch);
      if (turning && !iv.some((d, m) => m && sign(d) && sign(iv[m - 1]) && sign(d) !== sign(iv[m - 1]))) continue;
      c++; first ??= a[0].start; i += 3 * k - 1;
    }
    if (c) break;
  }
  const what = dir === 1 ? 'ascending staircases (a figure repeated 3+ times, a step higher each time)' : dir === -1 ? 'descending staircases (a figure repeated 3+ times, a step lower each time)' : turning ? 'spirals (a turning figure repeated 3+ times at new heights)' : 'sequences (a figure repeated 3+ times, moved by the same interval each time)';
  return result(ctx, c, first, what);
}
function contour(ctx, notes, kind) {
  const ph = phrases(line(notes)).filter((p) => p.length >= 5); let c = 0; let first = null;
  for (const p of ph) {
    const ps = p.map((n) => n.pitch); const hi = Math.max(...ps); const lo = Math.min(...ps);
    const iHi = ps.indexOf(hi) / (ps.length - 1); const iLo = ps.indexOf(lo) / (ps.length - 1);
    let ok = false;
    if (kind === 'arch') ok = iHi >= 0.2 && iHi <= 0.8 && hi - ps[0] >= 3 && hi - ps.at(-1) >= 3;
    if (kind === 'invarch') ok = iLo >= 0.2 && iLo <= 0.8 && ps[0] - lo >= 3 && ps.at(-1) - lo >= 3;
    if (kind === 'wave') {
      // Swings: the contour cut where it changes direction; a wave rises and falls at least twice each by a 3rd or more.
      const swings = []; let from = ps[0]; let dir = 0;
      for (let i = 1; i < ps.length; i++) {
        const d = sign(ps[i] - ps[i - 1]); if (!d) continue;
        if (dir && d !== dir) { swings.push(ps[i - 1] - from); from = ps[i - 1]; }
        dir = d;
      }
      swings.push(ps.at(-1) - from);
      const big = swings.filter((x) => Math.abs(x) >= 3);
      ok = big.filter((x) => x > 0).length >= 2 && big.filter((x) => x < 0).length >= 2;
    }
    if (ok) { c++; first ??= p[0].start; }
  }
  const what = { arch: 'arched phrases (rising to a peak in the middle, falling back)', invarch: 'inverted arches (falling to a low point in the middle, rising back)', wave: 'wave phrases (rising and falling twice or more, by a 3rd or more each time)' }[kind];
  return result(ctx, c, first, what, kind === 'wave' ? 1 : 2);
}
// Turning every note. A pendulum swings between the same heights; a zigzag's turning points keep climbing (or falling).
function zig(ctx, notes, { minLeap, net, what }) {
  const l = line(notes);
  const r = runs(l, (a, b, i) => { const d = b.pitch - a.pitch; return Math.abs(d) >= minLeap && (i < 2 || sign(d) === -sign(a.pitch - l[i - 2].pitch)); }, 6)
    .filter((x) => {
      const drift = x.slice(2).map((m, k) => sign(m.pitch - x[k].pitch));
      return net ? drift.every((d) => d && d === drift[0]) && Math.abs(x.at(-1).pitch - x[0].pitch) >= 4 : drift.every((d) => !d);
    });
  return result(ctx, r.length, r[0]?.[0].start, what);
}
function tones(ctx, notes, kind) {
  if (!ctx.timeline.length) return { found: false, count: 0, evidence: 'the blueprint has no chart to judge chord tones by' };
  const l = line(notes); let c = 0; let first = null;
  // An approach note needs no note before it: look at every note as the middle one, with a stand-in before the first.
  for (let i = kind === 'approach' ? -1 : 0; i + 2 < l.length; i++) {
    const [a, b, x] = [l[i] ?? null, l[i + 1], l[i + 2]];
    const step = (p, q) => [1, 2].includes(Math.abs(q - p));
    if (isChordTone(ctx, b) || (!a && kind !== 'approach')) continue;
    let ok = false;
    if (kind === 'passing') ok = isChordTone(ctx, a) && isChordTone(ctx, x) && step(a.pitch, b.pitch) && step(b.pitch, x.pitch) && sign(b.pitch - a.pitch) === sign(x.pitch - b.pitch);
    if (kind === 'neighbor') ok = isChordTone(ctx, a) && a.pitch === x.pitch && step(a.pitch, b.pitch);
    if (kind === 'approach') ok = isChordTone(ctx, x) && step(b.pitch, x.pitch) && near(posIn(x.start, 1), 0, 0.03);
    if (ok) { c++; first ??= b.start; }
  }
  const what = { passing: 'passing tones (a non-chord note stepping between two chord tones)', neighbor: 'neighbour tones (a step away from a chord tone and back)', approach: 'approach notes (a non-chord note stepping into a chord tone on the beat)' }[kind];
  return result(ctx, c, first, what, kind === 'approach' ? 3 : 2);
}
function enclosures(ctx, notes, kind) {
  const l = line(notes); let c = 0; let first = null;
  for (let i = 0; i + 2 < l.length; i++) {
    const [a, b, t] = [l[i], l[i + 1], l[i + 2]];
    if (!near(posIn(t.start, 1), 0, 0.03) || !isChordTone(ctx, t)) continue;
    const da = a.pitch - t.pitch; const db = b.pitch - t.pitch;
    if (sign(da) === sign(db) || !da || !db) continue;
    let ok = false;
    if (kind === 'chromatic') ok = Math.abs(da) === 1 && Math.abs(db) === 1;
    if (kind === 'diatonic') ok = [1, 2].includes(Math.abs(da)) && [1, 2].includes(Math.abs(db)) && (Math.abs(da) === 2 || Math.abs(db) === 2) && inKey(ctx, a.pitch) && inKey(ctx, b.pitch);
    if (kind === 'arpeggio') ok = Math.abs(da) >= 3 && Math.abs(db) >= 3 && isChordTone(ctx, a) && isChordTone(ctx, b);
    if (ok) { c++; first ??= a.start; }
  }
  return result(ctx, c, first, `${kind} enclosures (a target on the beat, approached from above and below)`, 2);
}
function spreadChords(ctx, notes, { lo, hi, what, min }) {
  const s = [...notes].sort((a, b) => a.start - b.start); let c = 0; let first = null;
  for (let i = 0; i < s.length;) {
    let j = i + 1;
    while (j < s.length && s[j].start - s[j - 1].start <= Math.min(0.25, hi) + E && s[j].start - s[i].start <= hi + E && s.slice(i, j).every((n) => n.start + n.dur > s[j].start)) j++;
    const g = s.slice(i, j); const spread = g.length ? g.at(-1).start - g[0].start : 0;
    const ordered = g.length >= 3 && (g.every((n, k) => !k || n.pitch > g[k - 1].pitch) || g.every((n, k) => !k || n.pitch < g[k - 1].pitch));
    if (ordered && spread >= lo - E && spread <= hi + E) { c++; first ??= g[0].start; }
    i = Math.max(j, i + 1);
  }
  return result(ctx, c, first, what, min);
}
function dyads(ctx, notes, ivs, name) {
  const g = groups(notes).filter((x) => x.notes.length === 2 && ivs.includes(Math.abs(x.notes[1].pitch - x.notes[0].pitch)));
  const r = runs(g, (a, b) => b.start - a.start <= 2 + E, 4);
  return result(ctx, r.length, r[0]?.[0].start, `passages in ${name} (4+ two-note chords a ${name === 'thirds' ? '3rd' : '6th'} apart in a row)`);
}
// Every bar from the part's first to its last: { bar (0-based), on: the attack positions inside it, in beats }.
function barOnsets(ctx, notes) {
  const bars = []; for (const n of notes) { const b = Math.floor(n.start / ctx.bpb + E); (bars[b] ||= new Set()).add(+posIn(n.start, ctx.bpb).toFixed(3)); }
  const first = bars.findIndex(Boolean); if (first < 0) return [];
  return Array.from({ length: bars.length - first }, (_, i) => ({ bar: first + i, on: [...(bars[first + i] || [])].sort((x, y) => x - y) }));
}
const same = (on, pat) => on.length === pat.length && on.every((p, i) => near(p, pat[i], 0.05));
function barPattern(ctx, notes, pats, what, min) {
  let c = 0; let first = null;
  for (const { bar, on } of barOnsets(ctx, notes)) if (pats.some((p) => same(on, p))) { c++; first ??= bar * ctx.bpb; }
  return result(ctx, c, first, what, min);
}
function ioiPattern(ctx, notes, pats, what, min) {
  const l = line(notes); const ioi = l.slice(1).map((n, i) => n.start - l[i].start); let c = 0; let first = null;
  for (let i = 0; i < ioi.length; i++) for (const p of pats) if (p.every((x, k) => i + k < ioi.length && near(ioi[i + k], x, 0.02))) { c++; first ??= l[i].start; i += p.length - 1; break; }
  return result(ctx, c, first, what, min);
}
function barRepeats(ctx, notes, { minNotes = 1, minPitches = 1 }) {
  const sig = new Map();
  for (const n of notes) { const b = Math.floor(n.start / ctx.bpb + E); if (!sig.has(b)) sig.set(b, []); sig.get(b).push(`${posIn(n.start, ctx.bpb).toFixed(2)}:${n.pitch}`); }
  const bars = [...sig.keys()].sort((a, b) => a - b); const sigOf = (b) => (sig.get(b) || []).sort().join(' ');
  const rich = (b, len) => { const ns = []; for (let k = 0; k < len; k++) ns.push(...(sig.get(b + k) || [])); return ns.length >= minNotes && new Set(ns.map((x) => x.split(':')[1])).size >= minPitches; };
  let best1 = 0; let best2 = 0; let atBar = bars[0] ?? 0;
  for (const b of bars) {
    if (rich(b, 1)) { let k = 1; while (sigOf(b + k) === sigOf(b)) k++; if (k > best1) { best1 = k; if (k >= 2) atBar = b; } }
    if (rich(b, 2) && sigOf(b) !== sigOf(b + 1)) { let k = 1; while (sigOf(b + 2 * k) === sigOf(b) && sigOf(b + 2 * k + 1) === sigOf(b + 1)) k++; if (k > best2) { best2 = k; if (best1 < 2) atBar = b; } }
  }
  return { best1, best2, at: atBar * ctx.bpb };
}
function grid(ctx, notes, positions, what) {
  const g = groups(notes); const hits = g.filter((x) => positions.some((p) => near(posIn(x.start, 1), p, 0.02)));
  const ok = hits.length >= 6 && hits.length >= g.length * 0.25;
  return { found: ok, count: hits.length, evidence: `${hits.length} of ${g.length} ${what}${hits.length ? `, first at ${at(ctx, hits[0].start)}` : ''} (needs 6+ and 25%+)` };
}
const offGrid = (beat) => { const q = Math.min(...[0.25, 1 / 3].map((u) => { const p = posIn(beat, u); return Math.min(p, u - p); })); return q; };
function feel(ctx, notes, dir) {
  const g = groups(notes); const leaning = g.filter((x) => { const p = posIn(x.start, 0.25); const d = dir < 0 ? 0.25 - p : p; return d >= 0.02 - E && d <= 0.12 + E; });
  const ok = g.length >= 6 && leaning.length >= g.length * 0.4;
  return { found: ok, count: leaning.length, evidence: `${leaning.length} of ${g.length} attacks ${dir < 0 ? 'early' : 'late'} by 0.02-0.12 beat (${dir < 0 ? 'push' : 'drag'}: 40%+)` };
}
// Inside a phrase (a rest isn't slowing down): 6+ notes whose gaps keep growing (or shrinking), 1.5× by the end.
function tempoTrend(ctx, notes, dir) {
  const r = phrases(line(notes)).flatMap((p) => {
    const ioi = p.slice(1).map((n, i) => ({ d: n.start - p[i].start, at: p[i].start }));
    return runs(ioi, (a, b) => (dir > 0 ? b.d > a.d * 1.02 : b.d < a.d / 1.02), 5).filter((x) => (dir > 0 ? x.at(-1).d / x[0].d >= 1.5 : x[0].d / x.at(-1).d >= 1.5));
  });
  return result(ctx, r.length, r[0]?.[0].at, dir > 0 ? 'ritardandos (6+ notes in a phrase, each gap longer, 1.5× by the end)' : 'accelerandos (6+ notes in a phrase, each gap shorter, 1.5× faster by the end)');
}

// ---- detectors between two parts ----

const P = {
  thirds: (ctx, a, b) => between(ctx, a, b, [3, 4, 15, 16], 'thirds'),
  sixths: (ctx, a, b) => between(ctx, a, b, [8, 9, 20, 21], 'sixths'),
  parallel: (ctx, a, b) => motion(ctx, a, b, 'parallel'),
  contrary: (ctx, a, b) => motion(ctx, a, b, 'contrary'),
  oblique: (ctx, a, b) => {
    let c = 0; let first = null;
    for (const [holder, mover] of [[a, b], [b, a]]) {
      for (const h of holder) {
        if (h.dur < 1 - E) continue;
        const moving = new Set(mover.filter((m) => m.start >= h.start - E && m.start < h.start + h.dur - E).map((m) => m.pitch));
        if (moving.size >= 3) { c++; first ??= h.start; }
      }
    }
    return result(ctx, c, first, 'oblique motion (one part holds a note while the other moves through 3+ pitches)', 2);
  },
  callresponse: (ctx, a, b) => exchanges(ctx, a, b, 'call and response (one part plays while the other rests, then they swap)'),
  hocket: (ctx, a, b) => {
    const slots = (ns) => new Set(ns.map((n) => Math.round(n.start * 4)));
    const barsA = new Map(); for (const n of a) { const k = Math.floor(n.start / ctx.bpb + E); (barsA.get(k) || barsA.set(k, []).get(k)).push(n); }
    let c = 0; let first = null;
    for (const [k, na] of barsA) {
      const nb = b.filter((n) => Math.floor(n.start / ctx.bpb + E) === k);
      const sa = slots(na); const sb = slots(nb);
      if (sa.size < 3 || sb.size < 3) continue;
      const shared = [...sa].filter((s) => sb.has(s)).length;
      const order = [...sa].map((s) => [s, 'a']).concat([...sb].map((s) => [s, 'b'])).sort((x, y) => x[0] - y[0]).map((x) => x[1]);
      const swaps = order.slice(1).filter((x, i) => x !== order[i]).length;
      if (shared <= 1 && swaps >= 4) { c++; first ??= k * ctx.bpb; }
    }
    return result(ctx, c, first, 'hocket bars (two parts interlocking, never on the same 16th)', 2);
  },
  polyrhythm: (ctx, a, b) => {
    const dom = (ns) => { const l = line(ns); const counts = new Map(); for (let i = 1; i < l.length; i++) { const d = +(l[i].start - l[i - 1].start).toFixed(3); counts.set(d, (counts.get(d) || 0) + 1); } const best = [...counts.entries()].sort((x, y) => y[1] - x[1])[0]; return best && best[1] >= 5 ? best[0] : null; };
    const [x, y] = [dom(a), dom(b)];
    if (!x || !y) return { found: false, count: 0, evidence: 'one of the parts has no steady pulse (5+ equal gaps)' };
    const r = Math.max(x, y) / Math.min(x, y);
    const ok = [1.5, 4 / 3, 5 / 4, 5 / 3, 2.5].some((v) => near(r, v, 0.03));
    return { found: ok, count: ok ? 1 : 0, evidence: `the parts pulse every ${x} and ${y} beats (${ok ? 'a' : 'not a'} polyrhythm: 3:2, 4:3, 5:4, 5:3 or 5:2)` };
  },
};
function between(ctx, a, b, ivs, name) {
  const pairs = [];
  for (const n of line(a)) { const m = line(b).find((x) => near(x.start, n.start, 0.03)); if (m) pairs.push({ start: n.start, iv: Math.abs(n.pitch - m.pitch) }); }
  const r = runs(pairs, (x, y) => ivs.includes(y.iv) && ivs.includes(x.iv), 4);
  return result(ctx, r.length, r[0]?.[0].start, `passages in ${name} between the two parts (4+ shared attacks a ${name === 'thirds' ? '3rd' : '6th'} apart)`);
}
function motion(ctx, a, b, kind) {
  const la = line(a); const lb = line(b); const shared = [];
  for (const n of la) { const m = lb.find((x) => near(x.start, n.start, 0.05)); if (m) shared.push([n.pitch, m.pitch, n.start]); }
  const ok = (p, q) => { const da = q[0] - p[0]; const db = q[1] - p[1]; if (!da || !db) return false; return kind === 'parallel' ? sign(da) === sign(db) && Math.abs(Math.abs(da) - Math.abs(db)) <= 1 : sign(da) === -sign(db); };
  const r = runs(shared, ok, kind === 'parallel' ? 5 : 4);
  return result(ctx, r.length, r[0]?.[0][2], kind === 'parallel' ? 'parallel passages (4+ moves together the same way, the same distance)' : 'contrary passages (3+ moves where the parts go opposite ways)');
}
function exchanges(ctx, a, b, what) {
  const pa = phrases(line(a)).filter((p) => p.length >= 2).map((p) => ({ who: 'a', s: p[0].start, e: p.at(-1).start + p.at(-1).dur }));
  const pb = phrases(line(b)).filter((p) => p.length >= 2).map((p) => ({ who: 'b', s: p[0].start, e: p.at(-1).start + p.at(-1).dur }));
  const all = [...pa, ...pb].sort((x, y) => x.s - y.s); let c = 0; let first = null;
  for (let i = 0; i + 1 < all.length; i++) {
    const [x, y] = [all[i], all[i + 1]];
    if (x.who !== y.who && y.s >= x.e - 0.25 - E && y.s - x.e <= 2 + E) { c++; first ??= x.s; }
  }
  return result(ctx, c, first, `exchanges: ${what}`, 3);
}

// ---- detectors on whole sections ----

function levels(ctx, tracks) {
  const out = [];
  for (let b = 0; b < ctx.bars; b++) out.push({ notes: 0, parts: new Set(), vel: 0, lo: 127, hi: 0, drums: 0, drumPitches: new Set(), toms: 0 });
  for (const t of tracks) for (const n of t.notes) {
    const b = Math.floor(n.start / ctx.bpb + E); if (b < 0 || b >= ctx.bars) continue;
    const x = out[b]; x.notes++; x.parts.add(t.channel); x.vel += n.velocity;
    if (t.kind === 'drums' || t.channel === 10) { x.drums++; x.drumPitches.add(n.pitch); if (n.pitch >= 41 && n.pitch <= 50 && n.pitch !== 42 && n.pitch !== 44 && n.pitch !== 46 && n.pitch !== 49) x.toms++; }
    else { x.lo = Math.min(x.lo, n.pitch); x.hi = Math.max(x.hi, n.pitch); }
  }
  return out.map((x) => ({ ...x, level: (x.notes / ctx.bpb) * 0.5 + x.parts.size * 1.2 + (x.notes ? x.vel / x.notes : 0) * 3 + (x.hi > x.lo ? (x.hi - x.lo) / 12 : 0) * 0.6 }));
}
const S = {
  break: (ctx, tracks, ranges) => {
    const L = levels(ctx, tracks); let c = 0; let first = null;
    for (const r of ranges) for (let b = r.start / ctx.bpb; b < r.end / ctx.bpb; b++) {
      const around = [L[b - 1], L[b + 1]].filter(Boolean);
      if (L[b] && L[b].notes > 0 && L[b].parts.size <= 1 && around.some((x) => x.parts.size >= 3)) { c++; first ??= b * ctx.bpb; }
    }
    return result(ctx, c, first, 'break bars (one part left alone, next to bars with 3+ parts)');
  },
  stoptime: (ctx, tracks, ranges) => {
    let c = 0; let first = null;
    for (const r of ranges) for (let b = r.start / ctx.bpb; b < r.end / ctx.bpb; b++) {
      const lo = b * ctx.bpb; const hi = lo + ctx.bpb;
      const ns = tracks.flatMap((t) => t.notes.filter((n) => n.start >= lo - E && n.start < hi - E).map((n) => ({ ...n, ch: t.channel })));
      const playing = new Set(ns.map((n) => n.ch));
      const times = [...new Set(ns.map((n) => +n.start.toFixed(2)))];
      const together = times.every((tm) => new Set(ns.filter((n) => near(n.start, tm, 0.02)).map((n) => n.ch)).size === playing.size);
      let silent = 0; for (let x = lo; x < hi; x += 0.25) if (!tracks.some((t) => t.notes.some((n) => n.start < x + 0.25 - E && n.start + n.dur > x + E))) silent += 0.25;
      if (playing.size >= 2 && times.length >= 1 && times.length <= 3 && together && silent >= 1 - E) { c++; first ??= lo; }
    }
    return result(ctx, c, first, 'stop-time bars (every part hits together, then a beat or more of silence)', 2);
  },
  breakdown: (ctx, tracks, ranges) => sectionsWhere(ctx, tracks, ranges, (cur, prev) => prev && cur.level <= prev.level * 0.65 && cur.parts < prev.parts, 'breakdowns (a section 35%+ quieter than the one before, with fewer parts)'),
  buildup: (ctx, tracks, ranges) => {
    const L = levels(ctx, tracks); let c = 0; let first = null;
    for (const r of ranges) {
      const lv = L.slice(r.start / ctx.bpb, r.end / ctx.bpb).map((x) => x.level);
      if (lv.length < 3) continue;
      const n = lv.length; const mx = (n - 1) / 2; const my = lv.reduce((a, b) => a + b, 0) / n;
      const slope = lv.reduce((a, y, i) => a + (i - mx) * (y - my), 0) / lv.reduce((a, _, i) => a + (i - mx) ** 2, 0);
      if (slope > 0 && lv.at(-1) >= lv[0] * 1.3) { c++; first ??= r.start; }
    }
    return result(ctx, c, first, 'buildups (a section rising bar by bar, ending 30%+ busier than it began)');
  },
  drop: (ctx, tracks, ranges) => {
    const L = levels(ctx, tracks); let c = 0; let first = null;
    for (const r of ranges) {
      const b = r.start / ctx.bpb; if (b < 1) continue;
      const [prev, cur] = [L[b - 1], L[b]];
      if (cur && prev && cur.level >= Math.max(prev.level * 1.5, prev.level + 2) && cur.parts.size >= prev.parts.size + 2) { c++; first ??= r.start; }
    }
    return result(ctx, c, first, 'drops (a section landing 1.5× busier than the bar before, with 2+ more parts)');
  },
  fill: (ctx, tracks, ranges) => {
    const L = levels(ctx, tracks); let c = 0; let first = null;
    for (const r of ranges) {
      // A one-bar section (a fill loop in a kit) is measured against the section before it.
      const bars = L.slice(r.start / ctx.bpb, r.end / ctx.bpb);
      const i = ctx.sections.findIndex((s) => s.start === r.start);
      const rest = bars.length >= 2 ? bars.slice(0, -1) : i > 0 ? L.slice(ctx.sections[i - 1].start / ctx.bpb, ctx.sections[i - 1].end / ctx.bpb) : [];
      if (!bars.length || !rest.length) continue;
      const last = bars.at(-1);
      const avg = rest.reduce((a, x) => a + x.drums, 0) / rest.length; const usual = Math.max(...rest.map((x) => x.drumPitches.size));
      if (last.drums >= Math.max(4, avg * 1.5) && (last.toms > 0 || last.drumPitches.size > usual)) { c++; first ??= r.end - ctx.bpb; }
    }
    return result(ctx, c, first, 'fills (a section\'s last bar with 1.5× the drum hits, bringing in toms or new sounds)');
  },
  repetition: (ctx, tracks, ranges) => similarSections(ctx, tracks, ranges, 0.8, 1.01, 'repetitions (a section that replays 80%+ of an earlier one)'),
  sectionvariation: (ctx, tracks, ranges) => similarSections(ctx, tracks, ranges, 0.4, 0.8, 'variations (a section keeping 40-80% of an earlier one and changing the rest)'),
};
function sectionsWhere(ctx, tracks, ranges, test, what) {
  const L = levels(ctx, tracks); let c = 0; let first = null;
  const sum = (r) => { const bs = L.slice(r.start / ctx.bpb, r.end / ctx.bpb); return { level: bs.reduce((a, x) => a + x.level, 0) / Math.max(1, bs.length), parts: new Set(bs.flatMap((x) => [...x.parts])).size }; };
  for (const r of ranges) {
    const i = ctx.sections.findIndex((s) => s.start === r.start);
    const prev = i > 0 ? sum(ctx.sections[i - 1]) : null;
    if (test(sum(r), prev)) { c++; first ??= r.start; }
  }
  return result(ctx, c, first, what);
}
function similarSections(ctx, tracks, ranges, lo, hi, what) {
  const sig = (r) => new Set(tracks.flatMap((t) => t.notes.filter((n) => n.start >= r.start - E && n.start < r.end - E).map((n) => `${t.channel}:${(n.start - r.start).toFixed(2)}:${n.pitch}`)));
  let c = 0; let first = null;
  for (const r of ranges) {
    const i = ctx.sections.findIndex((s) => s.start === r.start); const mine = sig(r); if (!mine.size) continue;
    for (const e of ctx.sections.slice(0, Math.max(0, i))) {
      if (e.bars !== r.bars) continue;
      const theirs = sig(e); const overlap = [...mine].filter((x) => theirs.has(x)).length / Math.max(mine.size, theirs.size);
      if (overlap >= lo && overlap < hi) { c++; first ??= r.start; break; }
    }
  }
  return result(ctx, c, first, what);
}

// ---- the motif ----

// The motif as written at its first statement: the top line of one channel from a bar, for `beats` beats.
export function extractMotif(ctx, where) {
  const t = ctx.tracks.find((x) => x.channel === where.channel); if (!t) return { error: `the motif's channel ${where.channel} doesn't play` };
  const start = (where.bar - 1) * ctx.bpb + ((where.beat ?? 1) - 1); const len = where.beats ?? (ctx.bp?.motif?.length_bars ?? 2) * ctx.bpb;
  const notes = line(t.notes.filter((n) => n.start >= start - E && n.start < start + len - E));
  if (notes.length < 3) return { error: `the motif at bar ${where.bar} on channel ${where.channel} has ${notes.length} note${notes.length === 1 ? '' : 's'}: point "motif.at" at its first full statement (3+ notes)` };
  return { channel: where.channel, start, notes, iv: notes.slice(1).map((n, i) => n.pitch - notes[i].pitch), ioi: notes.slice(1).map((n, i) => n.start - notes[i].start) };
}
// Every place the motif comes back, and how it was changed. Returns [{ kind, channel, start }].
export function motifStatements(ctx, scopeRanges = null) {
  const m = ctx.motif; if (!m || m.error) return [];
  const k = m.notes.length; const out = [];
  const eqIv = (x, y) => x.length === y.length && x.every((v, i) => v === y[i]);
  for (const t of ctx.tracks) {
    if (t.kind === 'drums' || t.channel === 10) continue;
    const l = line(t.notes);
    const take = (start) => !scopeRanges || scopeRanges.some((r) => start >= r.start - E && start < r.end - E);
    const claimed = new Set();
    for (let i = 0; i + k <= l.length; i++) {
      const w = l.slice(i, i + k); if (t.channel === m.channel && near(w[0].start, m.start, 0.01)) { for (let j = i; j < i + k; j++) claimed.add(j); continue; }
      if (!take(w[0].start)) continue;
      const iv = w.slice(1).map((n, j) => n.pitch - w[j].pitch); const ioi = w.slice(1).map((n, j) => n.start - w[j].start);
      const ratios = ioi.map((d, j) => d / m.ioi[j]); const rho = median(ratios); const even = ratios.every((r) => Math.abs(r - rho) <= rho * 0.12);
      const shift = w[0].pitch - m.notes[0].pitch;
      let kind = null;
      if (eqIv(iv, m.iv) && even) kind = near(rho, 1, 0.12) ? (shift === 0 ? 'restatement' : shift % 12 === 0 ? 'octave' : 'transposition') : rho >= 1.5 ? 'augmentation' : rho <= 0.67 ? 'diminution' : 'variation';
      else if (even && near(rho, 1, 0.12) && iv.every((d, j) => ((d - m.iv[j]) % 12 + 12) % 12 === 0) && iv.some((d, j) => d !== m.iv[j])) kind = 'octave';
      else if (eqIv(iv, m.iv.map((d) => -d))) kind = 'inversion';
      else if (eqIv(iv, [...m.iv].reverse().map((d) => -d))) kind = 'retrograde';
      else if (eqIv(iv, [...m.iv].reverse())) kind = 'retroinversion';
      // A diatonic inversion: every interval mirrored, each within a semitone of the original's size (the scale bends it).
      else if (iv.every((d, j) => sign(d) === -sign(m.iv[j]) && Math.abs(Math.abs(d) - Math.abs(m.iv[j])) <= 1) && m.iv.some((d) => d)) kind = 'inversion';
      else if (eqIv(iv, m.iv)) kind = 'variation';
      else if (even && near(rho, 1, 0.12) && iv.filter((d, j) => d === m.iv[j]).length >= Math.ceil(m.iv.length / 2)) kind = 'variation';
      if (kind) { out.push({ kind, channel: t.channel, start: w[0].start }); for (let j = i; j < i + k; j++) claimed.add(j); i += k - 1; }
    }
    // Fragments: 3+ notes of the motif's own intervals, played on their own (not inside a full statement).
    const fk = Math.max(3, Math.ceil(k / 2));
    if (fk < k) for (let i = 0; i + fk <= l.length; i++) {
      if ([...Array(fk).keys()].some((j) => claimed.has(i + j))) continue;
      const w = l.slice(i, i + fk); if (!take(w[0].start)) continue;
      const iv = w.slice(1).map((n, j) => n.pitch - w[j].pitch);
      for (let s = 0; s + iv.length <= m.iv.length; s++) if (eqIv(iv, m.iv.slice(s, s + iv.length))) { out.push({ kind: 'fragment', channel: t.channel, start: w[0].start }); i += fk - 1; break; }
    }
  }
  return out.sort((a, b) => a.start - b.start);
}
const M = {
  motif: (ctx, st) => { const r = st.filter((s) => s.kind !== 'fragment'); return result(ctx, r.length, r[0]?.start, 'returns of the motif (in any recognisable form)', 2); },
  repetition: (ctx, st) => { const r = st.filter((s) => s.kind === 'restatement'); return result(ctx, r.length, r[0]?.start, 'exact restatements of the motif', 2); },
  transposition: (ctx, st) => { const r = st.filter((s) => s.kind === 'transposition'); return result(ctx, r.length, r[0]?.start, 'transposed statements of the motif'); },
  octave: (ctx, st) => { const r = st.filter((s) => s.kind === 'octave'); return result(ctx, r.length, r[0]?.start, 'statements of the motif moved by an octave (whole or in part)'); },
  augmentation: (ctx, st) => { const r = st.filter((s) => s.kind === 'augmentation'); return result(ctx, r.length, r[0]?.start, 'augmented statements (the motif slowed 1.5× or more)'); },
  diminution: (ctx, st) => { const r = st.filter((s) => s.kind === 'diminution'); return result(ctx, r.length, r[0]?.start, 'diminished statements (the motif sped up 1.5× or more)'); },
  inversion: (ctx, st) => { const r = st.filter((s) => s.kind === 'inversion'); return result(ctx, r.length, r[0]?.start, 'inversions (the motif upside down)'); },
  retrograde: (ctx, st) => { const r = st.filter((s) => s.kind === 'retrograde'); return result(ctx, r.length, r[0]?.start, 'retrogrades (the motif backwards)'); },
  retroinversion: (ctx, st) => { const r = st.filter((s) => s.kind === 'retroinversion'); return result(ctx, r.length, r[0]?.start, 'retrograde inversions (backwards and upside down)'); },
  fragmentation: (ctx, st) => { const r = st.filter((s) => s.kind === 'fragment'); return result(ctx, r.length, r[0]?.start, 'fragments of the motif played on their own', 2); },
  variation: (ctx, st) => { const r = st.filter((s) => s.kind === 'variation'); return result(ctx, r.length, r[0]?.start, 'varied statements (the motif\'s rhythm or most of its intervals kept, the rest changed)'); },
  development: (ctx, st) => { const kinds = new Set(st.map((s) => s.kind).filter((k) => !['restatement'].includes(k))); return { found: kinds.size >= 3, count: kinds.size, evidence: `the motif comes back ${kinds.size} different ways${kinds.size ? ` (${[...kinds].join(', ')})` : ''} (development: 3+ ways)` }; },
};

// ---- the catalogue: every name on the owner's list ----

const N = (detector, how) => ({ kind: 'notes', detector, how });
const PR = (detector, how) => ({ kind: 'pair', detector, how });
const SE = (detector, how) => ({ kind: 'section', detector, how });
const MO = (detector, how) => ({ kind: 'motif', detector, how });
const SO = (how) => ({ kind: 'sound', how });
export const TECHNIQUES = {
  'Arpeggio': N('arpeggio', 'chord tones one at a time: 4 in a row, a 3rd or more apart, at least twice'),
  'Arpeggiate': N('arpeggio', 'as Arpeggio'),
  'Staccato': N('staccato', 'notes sound at most 55% of the time to the next (the median)'),
  'Legato': N('legato', 'each note runs into the next: 95%+ of the time to the next (the median)'),
  'Tenuto': N('held', 'notes held their full value: 85-100% of the time to the next'),
  'Marcato': N('marcato', '3+ notes both accented (0.15+ velocity over the part) and short'),
  'Accent': N('accent', '3+ notes 0.15+ velocity over the part\'s median, at most half of them'),
  'Portamento': SO('a glide between notes: the instrument\'s glide or pitch envelope'),
  'Glissando': N('glissando', '6+ notes by step at a 32nd or faster, one way'),
  'Tremolo': N('tremolo', '6+ fast repeats (16th or faster) of one note, or of two notes a 3rd or more apart'),
  'Trill': N('trill', '6+ fast alternations (16th or faster) between two notes a step apart'),
  'Mordent': N('mordent', 'main note, quick neighbour, main note held: twice or more'),
  'Turn': N('turn', 'above, main, below, main (or the reverse), quick: twice or more'),
  'Grace Note': N('grace', 'a very short note (≤ 0.15 beat) leaning into the next, a step or two away: twice or more'),
  'Appoggiatura': N('grace', 'a leaning note resolving by step (the harmony lock keeps it short: as Grace Note)'),
  'Acciaccatura': N('grace', 'as Grace Note: crushed in just before the main note'),
  'Ostinato': N('ostinato', 'one bar repeated exactly 4+ times in a row, or two bars 3+ times'),
  'Riff': N('riff', 'a melodic 1- or 2-bar figure (4+ notes, 3+ pitches) played twice or more in a row'),
  'Motif': MO('motif', 'the motif ("motif.at") comes back twice or more, in any recognisable form'),
  'Sequence': N('sequence', 'a 3-5 note figure played 3+ times in a row, moved by the same interval each time'),
  'Pedal Tone': N('pedal', 'one pitch held or repeated for 60%+ of the part, under 2+ different chords'),
  'Drone': N('drone', 'a note held 2 bars or longer'),
  'Alberti Bass': N('alberti', 'low, high, middle, high: 3+ times'),
  'Walking Bass': N('walking', '8+ quarter notes in a row, a new pitch each beat, mostly by step'),
  'Broken Chord': N('arpeggio', 'as Arpeggio'),
  'Block Chord': N('block', '3+ notes struck together, 4+ times'),
  'Power Chord': N('power', 'root and 5th (and octaves), no 3rd: 4+ times'),
  'Chord Stab': N('stab', 'short struck chords (≤ ½ beat) with space after: 4+'),
  'Chord Pulse': N('pulse', '4+ chords in a row on an even pulse of a beat or faster'),
  'Strumming': N('strum', '3+ chord notes a hair apart (≤ 0.15 beat) in pitch order: 4+ times'),
  'Rolling Chord': N('rolledchord', '3+ notes spread upward over 0.1-0.75 beat, each held: twice or more'),
  'Arpeggio Sweep': N('sweep', '5+ chord leaps one way at a 16th or faster'),
  'Scalar Run': N('scalarrun', '5+ notes by step (1-2 semitones) one way'),
  'Chromatic Run': N('chromaticrun', '5+ notes by semitone one way'),
  'Diatonic Run': N('diatonicrun', '5+ notes of the scale by step one way'),
  'Pentatonic Run': N('pentatonicrun', '5+ notes one way in whole steps and minor 3rds'),
  'Octave Run': N('octaverun', '4+ steps doubled an octave apart'),
  'Thirds': N('thirds', '4+ two-note chords a 3rd apart in a row (or, between two parts, 4+ shared attacks a 3rd apart)'),
  'Sixths': N('sixths', '4+ two-note chords a 6th apart in a row (or, between two parts, 4+ shared attacks a 6th apart)'),
  'Contrary Motion': PR('contrary', 'two parts: 3+ shared moves in a row going opposite ways'),
  'Parallel Motion': PR('parallel', 'two parts: 4+ shared moves in a row the same way, the same distance'),
  'Oblique Motion': PR('oblique', 'two parts: one holds a note while the other moves through 3+ pitches, twice or more'),
  'Call and Response': PR('callresponse', 'two parts: 3+ exchanges where one plays a phrase, then the other answers'),
  'Question-and-Answer Phrase': N('questionanswer', 'a phrase left open (not on the tonic) answered by one landing on the tonic, twice or more (with two parts: as Call and Response)'),
  'Antiphonal Pattern': PR('callresponse', 'two parts: as Call and Response'),
  'Hocket': PR('hocket', 'two parts interlocking, never on the same 16th: 2+ bars'),
  'Syncopation': N('syncopation', '4+ attacks (20%+) off the beat and held across it, or leaving the next beat empty'),
  'Backbeat': N('backbeat', 'snare or clap (GM 37-40): 70%+ of the hits on beats 2 and 4, in 60%+ of the bars'),
  'Offbeat': N('offbeat', '40%+ of the attacks on the "and", 30% or less on the beat'),
  'Polyrhythm': PR('polyrhythm', 'two parts with steady pulses in a 3:2, 4:3, 5:4, 5:3 or 5:2 ratio'),
  'Cross-Rhythm': N('crossrhythm', '5+ even attacks every ¾, 1½, ⅔ or 1⅓ beats, against the beat'),
  'Tuplet': N('tuplet', '6+ attacks (25%+) on thirds, fifths or sixths of a beat'),
  'Triplet': N('triplet', '6+ attacks (25%+) on triplet positions'),
  'Swing': N('swing', '6+ off-beat attacks (60%+) late, 58-72% through the beat'),
  'Shuffle': N('swing', 'as Swing'),
  'Dotted Rhythm': N('dotted', 'long-short in 3:1 (dotted 8th + 16th, or dotted quarter + 8th): 4+ times'),
  'Gallop': N('gallop', 'an 8th and two 16ths (or the reverse): 4+ times'),
  'Tresillo': N('tresillo', 'bars on the 3+3+2 grid: twice or more'),
  'Clave': N('clave', 'two-bar clave cycles (3-2 or 2-3, son or rumba): twice or more'),
  'Euclidean Rhythm': N('euclid', 'bars whose k hits are spread as evenly as the 16th grid allows (k not dividing 16): twice or more'),
  'Polymeter': N('polymeter', 'a pattern repeating 3+ times on a length the bar doesn\'t divide (3 beats in 4/4)'),
  'Rubato': N('rubato', '30%+ of the attacks pulled off the grid by 0.02-0.2 beat'),
  'Ritardando': N('ritardando', '6+ notes in a phrase whose gaps keep growing, 1.5× by the end'),
  'Accelerando': N('accelerando', '6+ notes in a phrase whose gaps keep shrinking, 1.5× faster by the end'),
  'Fermata': SE('fermata', 'every part stops attacking for 2+ beats while a note rings, after a busy passage'),
  'Pickup': N('pickup', 'a phrase starting in the last beat of a bar and leading into the downbeat: twice or more'),
  'Anacrusis': N('pickup', 'as Pickup'),
  'Anticipation': N('anticipation', 'attacks a 16th to an 8th before the bar line, held across it, nothing on the downbeat: 3+'),
  'Delay': N('delay', 'phrases coming in a 16th to an 8th after the bar line: 3+'),
  'Push': N('push', '40%+ of the attacks early by 0.02-0.12 beat'),
  'Drag': N('drag', '40%+ of the attacks late by 0.02-0.12 beat'),
  'Ghost Note': N('ghost', '4+ notes at velocity 0.35 or under, in a part around 0.55 or louder'),
  'Dead Note': N('dead', '4+ very short (≤ 0.1 beat), soft (≤ 0.5) notes'),
  'Muted Note': N('dead', 'as Dead Note'),
  'Palm Mute': N('palmmute', '8+ short (≤ 0.3 beat), low (≤ C4) notes in a row'),
  'Pizzicato': N('staccato', 'plucked: short and detached (as Staccato); the instrument gives the pluck'),
  'Spiccato': N('staccato', 'bounced: short and detached (as Staccato)'),
  'Col Legno': SO('struck with the wood of the bow: the instrument\'s attack'),
  'Harmonic': SO('a glassy overtone: the instrument\'s timbre'),
  'Vibrato': SO('a pitch wobble on held notes: the instrument (.vib)'),
  'Pitch Bend': SO('a bend into or out of a note: the instrument (pitch envelope)'),
  'Scoop': SO('a bend up into the note: the instrument'),
  'Fall': SO('a bend down off the note: the instrument'),
  'Doit': SO('a quick bend up off the note: the instrument'),
  'Rip': SO('a fast smeared run up into a note: the instrument'),
  'Flutter Tongue': SO('a rolled, buzzing tone: the instrument'),
  'Growl': SO('a rough, distorted tone: the instrument (.shape, .distort)'),
  'Swell': N('swell', 'a crescendo turning straight into a diminuendo'),
  'Crescendo': N('crescendo', 'velocity rising 0.15+ over 3+ bars in a row'),
  'Diminuendo': N('diminuendo', 'velocity falling 0.15+ over 3+ bars in a row'),
  'Sforzando': N('sforzando', 'a sudden loud attack (0.25+ over the part) dropping straight back to soft'),
  'Fade-In': N('fadein', 'velocity rising from 0.45 or less at the start of the part'),
  'Fade-Out': N('fadeout', 'velocity falling to 0.45 or less at the end of the part'),
  'Sidechain Pumping': SO('the mix ducking with the kick: the instruments stage (sidechain)'),
  'Gate Rhythm': N('gate', '8+ chopped chords (≤ 0.2 beat) at a 16th or faster'),
  'Chopping': SO('a sample cut into rhythmic slices: the instruments stage'),
  'Stutter': N('ratchet', '3+ repeats at a 32nd or faster'),
  'Retrigger': N('ratchet', 'as Stutter'),
  'Ratchet': N('ratchet', 'as Stutter'),
  'Roll': N('roll', '6+ hits at a 32nd or faster on one sound'),
  'Flam': N('flam', 'a softer grace hit 0.02-0.08 beat before the main one, on the same sound: twice or more'),
  'Percussion Drag': N('percdrag', 'two quick grace hits into a main one: twice or more'),
  'Paradiddle': N('paradiddle', 'two sounds in the pattern A B A A, B A B B'),
  'Fill': SE('fill', 'a section\'s last bar (or a one-bar loop) with 1.5× the drum hits of the bars before, bringing in toms or new sounds'),
  'Break': SE('break', 'a bar with one part left alone, next to bars with 3+ parts'),
  'Drop': SE('drop', 'a section landing 1.5× busier than the bar before, with 2+ more parts'),
  'Buildup': SE('buildup', 'a section rising bar by bar, ending 30%+ busier than it began'),
  'Breakdown': SE('breakdown', 'a section 35%+ quieter than the one before, with fewer parts'),
  'Stop-Time': SE('stoptime', 'bars where every part hits together, then a beat or more of silence: twice or more'),
  'Repetition': SE('repetition', 'a section that replays 80%+ of an earlier one of the same length'),
  'Variation': MO('variation', 'the motif with its rhythm or most of its intervals kept and the rest changed (without a motif: a section keeping 40-80% of an earlier one)'),
  'Inversion': MO('inversion', 'the motif upside down (its intervals mirrored)'),
  'Retrograde': MO('retrograde', 'the motif backwards'),
  'Retrograde Inversion': MO('retroinversion', 'the motif backwards and upside down'),
  'Augmentation': MO('augmentation', 'the motif slowed 1.5× or more'),
  'Diminution': MO('diminution', 'the motif sped up 1.5× or more'),
  'Fragmentation': MO('fragmentation', '3+ notes of the motif played on their own, twice or more'),
  'Development': MO('development', 'the motif comes back 3+ different ways'),
  'Transposition': MO('transposition', 'the motif moved to another pitch, rhythm kept'),
  'Octave Displacement': MO('octave', 'the motif (or some of its notes) moved by an octave'),
  'Suspension': N('suspension', 'a sus chord\'s 4th in the part, resolved to the 3rd on the same root'),
  'Anticipation Tone': N('anticipationtone', 'the next chord\'s note sounded just before it arrives: twice or more'),
  'Passing Tone': N('passing', 'a non-chord note stepping between two chord tones: twice or more'),
  'Neighbor Tone': N('neighbor', 'a step away from a chord tone and back: twice or more'),
  'Chromatic Enclosure': N('enclosurechrom', 'a chord tone on the beat approached from a semitone above and below: twice or more'),
  'Diatonic Enclosure': N('enclosurediat', 'a chord tone on the beat approached by scale steps above and below: twice or more'),
  'Walking Enclosure': N('enclosure', 'enclosures by semitone or scale step, walking into the target: twice or more'),
  'Arpeggio Enclosure': N('enclosurearp', 'a chord tone on the beat approached from chord tones above and below: twice or more'),
  'Contrary Arpeggio': PR('contrary', 'two parts arpeggiating opposite ways: as Contrary Motion'),
  'Cascade': N('cascade', '6+ fast notes (16th or faster) tumbling down a 5th or more'),
  'Ascending Staircase': N('staircaseup', 'a figure repeated 3+ times, 1-4 semitones higher each time'),
  'Descending Staircase': N('staircasedown', 'a figure repeated 3+ times, 1-4 semitones lower each time'),
  'Pendulum': N('pendulum', '6+ notes turning every note, a 3rd or more each way'),
  'Zigzag': N('zigzag', '6+ notes turning every note while climbing or falling 4+ semitones'),
  'Arch': N('arch', 'phrases rising to a peak in the middle and falling back: twice or more'),
  'Inverted Arch': N('invarch', 'phrases falling to a low point in the middle and rising back: twice or more'),
  'Spiral': N('spiral', 'a turning figure repeated 3+ times at new heights'),
  'Wave': N('wave', 'a phrase rising and falling twice or more, by a 3rd or more each time'),
  'Leap-and-Step': N('leapstep', 'a leap (5+ semitones) answered by a step back: 3+ times'),
  'Step-and-Leap': N('stepleap', 'two steps one way springing into a leap: 3+ times'),
  'Repeated-Note Motif': N('repeatednote', 'one pitch 4+ times in a row: twice or more'),
  'Neighbor Oscillation': N('neighborosc', '5+ slower alternations (longer than a 16th) between two notes a step apart'),
  'Chord-Tone Targeting': { kind: 'lock', how: 'strong beats land on chord tones: the harmony lock already guarantees it' },
  'Approach-Note Targeting': N('approach', 'a non-chord note stepping into a chord tone on the beat: 3+ times'),
};
S.fermata = (ctx, tracks, ranges) => {
  let c = 0; let first = null;
  const starts = tracks.flatMap((t) => t.notes.map((n) => n.start)).sort((a, b) => a - b);
  for (const r of ranges) for (let i = 0; i + 1 < starts.length; i++) {
    const [a, b] = [starts[i], starts[i + 1]];
    if (a < r.start - E || a >= r.end - E || b - a < 2 - E) continue;
    const ringing = tracks.some((t) => t.notes.some((n) => n.start <= a + E && n.start + n.dur >= a + 1.5));
    const busy = starts.filter((s) => s >= a - 2 - E && s <= a + E).length >= 4;
    if (ringing && busy) { c++; first ??= a; }
  }
  return result(ctx, c, first, 'fermatas (every part stops attacking for 2+ beats while a note rings, after a busy passage)');
};

export const TECHNIQUE_NAMES = Object.keys(TECHNIQUES);
const LOWER = new Map(TECHNIQUE_NAMES.map((n) => [n.toLowerCase(), n]));
// A name as the owner's list spells it ("arpeggio" → "Arpeggio", "Pickup / Anacrusis" → "Pickup").
export function canonical(name) {
  const s = String(name || '').trim().toLowerCase();
  return LOWER.get(s) || LOWER.get(s.split('/')[0].trim()) || null;
}

// ---- checking a blueprint's planned techniques against the captured MIDI ----

// bp.techniques: [{ name, part: channel | [channels], where: "all" | [section names], how }]
export function planProblems(bp) {
  const out = [];
  const list = bp.techniques;
  if (list === undefined) return out;
  if (!Array.isArray(list)) return ['blueprint.json: "techniques" must be a list of { name, part, where, how }'];
  const channels = new Set((bp.roles || []).map((r) => r.channel));
  const sections = new Set((bp.sections || []).map((s) => s.name));
  const seen = new Set();
  for (const [i, t] of list.entries()) {
    const name = canonical(t?.name);
    const label = `technique ${i + 1} (${t?.name ?? '?'})`;
    if (!name) { out.push(`blueprint.json: ${label} isn't on the technique list (node strudel.mjs techniques lists them)`); continue; }
    if (seen.has(name)) out.push(`blueprint.json: "${name}" is listed twice: one entry per technique (name every part and section in it)`);
    seen.add(name);
    const def = TECHNIQUES[name];
    const parts = t.part === undefined || t.part === null ? [] : [].concat(t.part);
    if (def.kind === 'notes' && !parts.length && !['Backbeat', 'Fill'].includes(name)) out.push(`blueprint.json: "${name}" needs "part": the channel (or channels) that plays it`);
    if (def.kind === 'pair' && parts.length < 2) out.push(`blueprint.json: "${name}" happens between two parts: "part": [channel, channel]`);
    for (const p of parts) if (!channels.has(p)) out.push(`blueprint.json: "${name}" is on channel ${p}, which has no role`);
    const where = t.where === undefined || t.where === 'all' ? [] : [].concat(t.where);
    for (const w of where) if (!sections.has(w)) out.push(`blueprint.json: "${name}" happens in "${w}", which isn't a section`);
    if (def.kind === 'section' && !where.length && !['Fermata', 'Repetition'].includes(name)) out.push(`blueprint.json: "${name}" needs "where": the section(s) it happens in`);
    if (def.kind === 'motif' && !bp.motif?.at) out.push(`blueprint.json: "${name}" works on the motif: give "motif.at": { "channel", "bar", "beats" } (its first full statement)`);
    if (String(t.how || '').trim().length < 8) out.push(`blueprint.json: "${name}" needs "how": what it does in this piece, in words`);
  }
  return out;
}

// Listens for every planned technique. Returns { results: [{ name, kind, found, evidence, parts, where }], errors, notes }.
export function checkTechniques(manifest, bp) {
  const ctx = context(manifest, bp);
  const results = []; const errors = []; const notes = [];
  if (ctx.motif?.error && (bp.techniques || []).some((t) => TECHNIQUES[canonical(t.name)]?.kind === 'motif')) errors.push(`motif: ${ctx.motif.error}`);
  for (const t of bp.techniques || []) {
    const name = canonical(t.name); if (!name) continue;
    const def = TECHNIQUES[name];
    const r = listen(ctx, name, t);
    results.push({ name, kind: def.kind, ...r, parts: [].concat(t.part ?? []), where: t.where ?? 'all' });
    if (def.kind === 'sound') notes.push(`${name}: not in the notes; the instruments stage gives it (${def.how})`);
    else if (def.kind === 'lock') notes.push(`${name}: ${def.how}`);
    else if (!r.found) errors.push(`${name} is planned${r.scopeText} but isn't in the notes: ${r.evidence}. What counts: ${def.how}.`);
  }
  return { results, errors, notes };
}

// One technique, where the blueprint says it happens. Exported for the flow checks and the `techniques` command.
export function listen(ctx, name, t = {}) {
  const def = TECHNIQUES[name];
  const where = t.where === undefined || t.where === 'all' ? [] : [].concat(t.where);
  const ranges = where.length ? ctx.sections.filter((s) => where.includes(s.name)) : [{ name: 'all', start: 0, end: ctx.bars * ctx.bpb }];
  const inRanges = (n) => ranges.some((r) => n.start >= r.start - E && n.start < r.end - E);
  const parts = [].concat(t.part ?? []);
  const scopeText = `${parts.length ? ` on channel${parts.length > 1 ? 's' : ''} ${parts.join(' and ')}` : ''}${where.length ? ` in ${where.map((w) => `"${w}"`).join(', ')}` : ''}`;
  const track = (ch) => ctx.tracks.find((x) => x.channel === ch);
  if (def.kind === 'sound' || def.kind === 'lock') return { found: true, count: 0, evidence: def.how, scopeText };
  if (def.kind === 'motif') {
    if (!ctx.motif || ctx.motif.error) {
      if (name === 'Variation') { const r = S.sectionvariation(ctx, parts.length ? parts.map(track).filter(Boolean) : ctx.tracks, where.length ? ranges : ctx.sections); return { ...r, scopeText }; }
      return { found: false, count: 0, evidence: ctx.motif?.error || 'the blueprint has no "motif.at"', scopeText };
    }
    const st = motifStatements(ctx, where.length ? ranges : null).filter((s) => !parts.length || parts.includes(s.channel));
    return { ...M[def.detector](ctx, st), scopeText };
  }
  if (def.kind === 'section') {
    const tracks = parts.length ? parts.map(track).filter(Boolean) : ctx.tracks;
    return { ...S[def.detector](ctx, tracks, where.length ? ranges : ctx.sections.length ? ctx.sections : ranges), scopeText };
  }
  if (def.kind === 'pair') {
    const [a, b] = parts.map(track);
    if (!a || !b) return { found: false, count: 0, evidence: `channel ${!a ? parts[0] : parts[1]} doesn't play`, scopeText };
    return { ...P[def.detector](ctx, a.notes.filter(inRanges), b.notes.filter(inRanges)), scopeText };
  }
  // notes: two parts named on a technique that also exists between parts (thirds, sixths) → between them.
  if (parts.length >= 2 && P[def.detector]) {
    const [a, b] = parts.map(track);
    if (a && b) return { ...P[def.detector](ctx, a.notes.filter(inRanges), b.notes.filter(inRanges)), scopeText };
  }
  if (parts.length >= 2 && name === 'Question-and-Answer Phrase') {
    const [a, b] = parts.map(track);
    if (a && b) return { ...exchanges(ctx, a.notes.filter(inRanges), b.notes.filter(inRanges), 'question and answer'), scopeText };
  }
  const chans = parts.length ? parts : ctx.tracks.filter((x) => x.kind === 'drums' || x.channel === 10).map((x) => x.channel);
  const each = chans.map((ch) => { const tr = track(ch); return tr ? { ch, ...D[def.detector](ctx, tr.notes.filter(inRanges)) } : { ch, found: false, count: 0, evidence: `channel ${ch} doesn't play` }; });
  if (!each.length) return { found: false, count: 0, evidence: 'no part to listen to', scopeText };
  const missing = each.filter((x) => !x.found);
  return { found: !missing.length, count: each.reduce((a, x) => a + x.count, 0), evidence: missing.length ? missing.map((x) => (each.length > 1 ? `channel ${x.ch}: ${x.evidence}` : x.evidence)).join('; ') : each.map((x) => (each.length > 1 ? `channel ${x.ch}: ${x.evidence}` : x.evidence)).join('; '), scopeText };
}

// Everything the notes show, whether or not it was planned, section by section: for the music sheet, and for saving
// a piece as a flow. Returns [{ name, parts, where: [section names], evidence }].
export function heard(manifest, bp) {
  const ctx = context(manifest, bp);
  const out = [];
  const pitched = ctx.tracks.filter((t) => t.kind !== 'drums' && t.channel !== 10);
  const spans = ctx.sections.length ? ctx.sections.map((x) => [x.name]) : [[]];
  for (const [name, def] of Object.entries(TECHNIQUES)) {
    if (['sound', 'lock'].includes(def.kind)) continue;
    const tries = def.kind === 'pair' ? pitched.flatMap((a, i) => pitched.slice(i + 1).map((b) => [a.channel, b.channel]))
      : def.kind === 'notes' ? ctx.tracks.map((t) => [t.channel]) : [[]];
    for (const parts of tries) {
      const where = []; let evidence = null;
      for (const span of (def.kind === 'notes' || def.kind === 'pair' ? spans : [[]])) {
        const r = listen(ctx, name, { part: parts.length ? (parts.length === 1 ? parts[0] : parts) : undefined, where: span.length ? span : undefined });
        if (r.found) { where.push(...span); evidence ??= r.evidence; }
      }
      if (evidence) { out.push({ name, parts, where, evidence }); break; }
    }
  }
  return out;
}
