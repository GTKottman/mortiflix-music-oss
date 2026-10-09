// The flow: the shape a piece takes, chosen with the brief (flow/flow.json in the working folder). The plan has to
// follow it (its energy curve and rules, its required techniques, nothing it avoids), and the score has to sound like
// it (the same rules measured on the notes).
//
// flow.json (see docs/FLOWS.md):
//   { "slug", "name", "for": "song" | "kit", "bucket", "feels": [...], "summary",
//     "shape": { "curve": [[0, 2], [0.6, 7], [1, 9]], "rules": ["rising", …] },
//     "sections": { "min": 4, "max": 7 },                       songs only: how many sections
//     "techniques": { "required": [...], "avoid": [...], "free": 3 },
//     "motif": "what the motif does", "surprise": "…", "ending": "…" }
import { canonical, context, listen, TECHNIQUES } from './techniques.mjs';

const E = 1e-6;

// Every rule: what it asks in words, and how the plan (asked intensities) and the score (measured levels) are held to it.
export const RULES = {
  rising: { kit: true, says: 'every section at least as intense as the one before (no dips)' },
  no_breakdown: { says: 'no section drops below the one before it, until the last' },
  peak_first: { says: 'the most intense section starts in the first 30% of the piece' },
  peak_late: { says: 'the most intense section starts in the last 40% of the piece' },
  alternating: { kit: true, says: 'intensity swings up and down from section to section (4+ sections)' },
  widening: { says: 'each swing is at least as wide as the one before' },
  quiet_before_peak: { says: 'the section right before the peak is quiet (intensity 3 or less)' },
  ends_low: { says: 'the last section is quiet (3 or less) and well below the peak' },
  rondo: { says: 'sections marked "material": "A" return 3+ times, never back to back, between 2+ other materials' },
  merge: { says: 'an "A" world and a "B" world apart first, then a section marked "A+B" that plays both' },
  false_stop: { says: 'a near-silent section of 1-2 bars (intensity 1 or less) after the music has built (5+), with more after it' },
  through: { says: 'no section repeats an earlier one: every section is new material' },
  pairs: { kit: true, says: 'loops in pairs: each odd loop (1st, 3rd…) a calm twin of the busier loop after it' },
  shared_drums: { kit: true, says: 'every loop keeps the first loop\'s drum pattern (75%+ of its hits)' },
  layers: { kit: true, says: 'every loop is one or two parts: layers that stack on each other' },
};

// The curve's value at x (0..1), straight lines between its points.
export function curveAt(curve, x) {
  const pts = [...curve].sort((a, b) => a[0] - b[0]);
  if (x <= pts[0][0]) return pts[0][1];
  for (let i = 1; i < pts.length; i++) if (x <= pts[i][0] + E) { const [x0, y0] = pts[i - 1]; const [x1, y1] = pts[i]; return x1 === x0 ? y1 : y0 + ((y1 - y0) * (x - x0)) / (x1 - x0); }
  return pts.at(-1)[1];
}
const pearson = (a, b) => {
  const n = a.length; const ma = a.reduce((s, x) => s + x, 0) / n; const mb = b.reduce((s, x) => s + x, 0) / n;
  const cov = a.reduce((s, x, i) => s + (x - ma) * (b[i] - mb), 0); const va = a.reduce((s, x) => s + (x - ma) ** 2, 0); const vb = b.reduce((s, x) => s + (x - mb) ** 2, 0);
  return va < E || vb < E ? null : cov / Math.sqrt(va * vb);
};
const mids = (bp) => (bp.sections || []).map((s, i, all) => (bp.loops ? (all.length === 1 ? 0.5 : i / (all.length - 1)) : (s.start_bar - 1 + (s.end_bar - s.start_bar + 1) / 2) / bp.bars));
const peakIndex = (vals) => vals.indexOf(Math.max(...vals));

// The plan against the flow. Returns problems in plain words (blocking).
export function flowPlanProblems(bp, flow) {
  const out = [];
  const add = (m) => out.push(`blueprint.json (flow "${flow.name}"): ${m}`);
  if (bp.flow !== flow.slug) add(`set "flow": "${flow.slug}" (the flow this piece was given; flow/FLOW.md says how it goes)`);
  const secs = bp.sections || [];
  if (!secs.length) return out;
  const asked = secs.map((s) => Number(s.intensity));
  if (flow.for === 'song' && flow.sections) {
    const { min = 1, max = 99 } = flow.sections;
    if (secs.length < min || secs.length > max) add(`${secs.length} sections; this flow takes ${min === max ? min : `${min}-${max}`}`);
  }
  // The curve: the asked intensities follow the flow's shape (correlation 0.6 or more).
  const curve = flow.shape?.curve;
  if (curve?.length >= 2 && secs.length >= 3) {
    const want = mids(bp).map((x) => curveAt(curve, x));
    const r = pearson(asked, want);
    if (r === null ? Math.max(...want) - Math.min(...want) > 1 : r < 0.6) {
      add(`the intensities don't follow the flow's curve${r === null ? '' : ` (they match it ${Math.round(r * 100)}%, 60% needed)`}. The curve asks for about: ${secs.map((s, i) => `"${s.name}" ${Math.round(want[i])}`).join(', ')}`);
    }
  }
  for (const rule of flow.shape?.rules || []) {
    const p = planRule(rule, bp, asked);
    if (p) add(`${RULES[rule]?.says || rule}: ${p}`);
  }
  // Techniques: the flow's own, nothing it avoids, and the free picks.
  const planned = new Set((bp.techniques || []).map((t) => canonical(t?.name)).filter(Boolean));
  const req = (flow.techniques?.required || []).map(canonical).filter(Boolean);
  const avoid = (flow.techniques?.avoid || []).map(canonical).filter(Boolean);
  for (const t of req) if (!planned.has(t)) add(`it needs ${t}: add it to "techniques" with its part, where and how (${TECHNIQUES[t].how})`);
  for (const t of avoid) if (planned.has(t)) add(`it avoids ${t}: take it out (this flow gets its effect another way: flow/FLOW.md)`);
  const free = flow.techniques?.free ?? 0;
  const own = [...planned].filter((t) => !req.includes(t)).length;
  if (own < free) add(`${own} technique${own === 1 ? '' : 's'} of your own choosing; this flow asks for ${free} beyond its ${req.length} (pick from the list: node strudel.mjs techniques)`);
  return out;
}

function planRule(rule, bp, asked) {
  const secs = bp.sections; const n = secs.length;
  const name = (i) => `"${secs[i].name}"`;
  const pk = peakIndex(asked);
  switch (rule) {
    case 'rising': { const i = asked.findIndex((v, k) => k && v < asked[k - 1]); return i < 0 ? null : `${name(i)} (${asked[i]}) is below ${name(i - 1)} (${asked[i - 1]})`; }
    case 'no_breakdown': { const i = asked.findIndex((v, k) => k && k < n - 1 && v < asked[k - 1] - 1); return i < 0 ? null : `${name(i)} (${asked[i]}) falls below ${name(i - 1)} (${asked[i - 1]})`; }
    case 'peak_first': return (secs[pk].start_bar - 1) / bp.bars <= 0.3 + E ? null : `the peak, ${name(pk)}, starts at bar ${secs[pk].start_bar} of ${bp.bars}`;
    case 'peak_late': return (secs[pk].start_bar - 1) / bp.bars >= 0.6 - E ? null : `the peak, ${name(pk)}, starts at bar ${secs[pk].start_bar} of ${bp.bars}`;
    case 'alternating': {
      if (n < 4) return `${n} sections; it takes 4 or more`;
      const d = asked.slice(1).map((v, i) => v - asked[i]);
      const i = d.findIndex((x, k) => !x || (k && Math.sign(x) === Math.sign(d[k - 1])));
      return i < 0 ? null : `${name(i)} to ${name(i + 1)} ${d[i] ? 'moves the same way as the swing before it' : 'doesn\'t move'}`;
    }
    case 'widening': { const d = asked.slice(1).map((v, i) => Math.abs(v - asked[i])); const i = d.findIndex((x, k) => k && x < d[k - 1]); return i < 0 ? null : `the swing into ${name(i + 1)} (${d[i]}) is narrower than the one before (${d[i - 1]})`; }
    case 'quiet_before_peak': return pk === 0 ? 'the peak is the first section: nothing comes before it' : asked[pk - 1] <= 3 ? null : `${name(pk - 1)}, right before the peak, is ${asked[pk - 1]}`;
    case 'ends_low': return asked[n - 1] <= 3 && asked[n - 1] <= asked[pk] - 4 ? null : `the last section, ${name(n - 1)}, is ${asked[n - 1]} (the peak is ${asked[pk]})`;
    case 'rondo': {
      const m = secs.map((s) => String(s.material || '').trim());
      if (m.some((x) => !x)) return 'give every section a "material" letter (A for the theme, B, C… for the detours)';
      const as = m.filter((x) => x === 'A').length;
      if (m[0] !== 'A') return 'the first section is the theme: "material": "A"';
      if (as < 3) return `"A" comes ${as} time${as === 1 ? '' : 's'}; a rondo returns to it 3+ times`;
      if (m.some((x, i) => i && x === 'A' && m[i - 1] === 'A')) return 'two "A" sections back to back: put a detour between them';
      if (new Set(m.filter((x) => x !== 'A')).size < 2) return 'the detours need 2+ different materials (B, C…)';
      return null;
    }
    case 'merge': {
      const m = secs.map((s) => String(s.material || '').replace(/\s/g, ''));
      const both = m.findIndex((x) => x === 'A+B' || x === 'B+A');
      if (both < 0) return 'mark the section where the worlds meet "material": "A+B"';
      if (!m.slice(0, both).includes('A') || !m.slice(0, both).includes('B')) return 'before they meet, the "A" and "B" worlds each need a section of their own';
      return null;
    }
    case 'false_stop': {
      const i = secs.findIndex((s, k) => k > 0 && k < n - 1 && s.intensity <= 1 && s.end_bar - s.start_bar + 1 <= 2 && Math.max(...asked.slice(0, k)) >= 5);
      return i >= 0 ? null : 'add a section of 1-2 bars at intensity 0-1 once the music has built to 5 or more, with a section after it';
    }
    case 'through': { const m = secs.map((s) => s.material).filter(Boolean); return new Set(m).size === m.length ? null : 'two sections share a "material": every section is new'; }
    case 'pairs': {
      if (n % 2) return `${n} loops; pairs need an even number`;
      const i = [...Array(n / 2).keys()].find((k) => asked[2 * k] > asked[2 * k + 1] - 2);
      return i === undefined ? null : `${name(2 * i)} (${asked[2 * i]}) should be at least 2 calmer than its twin ${name(2 * i + 1)} (${asked[2 * i + 1]})`;
    }
    case 'shared_drums': return (bp.roles || []).some((r) => r.channel === 10 || r.harmony === 'drums') ? null : 'the kit needs a drum part (channel 10) to share';
    default: return null;
  }
}

// The score against the flow, from the captured MIDI and the intensity report. Returns { errors, warnings }.
export function flowScoreProblems(manifest, bp, flow, curve) {
  const errors = []; const warnings = [];
  const add = (m) => errors.push(`flow "${flow.name}": ${m}`);
  const meas = curve.sections.map((s) => s.measured);
  const secs = bp.sections || []; const n = secs.length;
  const name = (i) => `"${secs[i].name}"`;
  const pk = peakIndex(meas);
  const ctx = context(manifest, bp);
  const sig = (sec, filter = () => true) => new Set(manifest.tracks.filter(filter).flatMap((t) => t.notes.filter((x) => x.start >= sec.start - E && x.start < sec.end - E).map((x) => `${t.channel}:${(x.start - sec.start).toFixed(2)}:${x.pitch}`)));
  const overlap = (a, b) => (a.size && b.size ? [...a].filter((x) => b.has(x)).length / Math.max(a.size, b.size) : 0);
  for (const rule of flow.shape?.rules || []) {
    switch (rule) {
      case 'rising': { const i = meas.findIndex((v, k) => k && v < meas[k - 1] * 0.97); if (i > 0) add(`${name(i)} measures ${meas[i]}, below ${name(i - 1)} (${meas[i - 1]}): every section at least as busy as the last`); break; }
      case 'no_breakdown': { const i = meas.findIndex((v, k) => k && k < n - 1 && v < meas[k - 1] * 0.85); if (i > 0) add(`${name(i)} measures ${meas[i]}, a drop from ${name(i - 1)} (${meas[i - 1]}): no breakdown before the end`); break; }
      case 'peak_first': if ((secs[pk].start_bar - 1) / bp.bars > 0.3 + E) add(`the busiest section is ${name(pk)} (bar ${secs[pk].start_bar}): the peak belongs in the first 30%`); break;
      case 'peak_late': if ((secs[pk].start_bar - 1) / bp.bars < 0.6 - E) add(`the busiest section is ${name(pk)} (bar ${secs[pk].start_bar}): the peak belongs in the last 40%`); break;
      case 'alternating': {
        const asked = curve.sections.map((s) => s.asked);
        const i = meas.slice(1).findIndex((v, k) => Math.sign(v - meas[k]) !== Math.sign(asked[k + 1] - asked[k]));
        if (i >= 0) add(`${name(i)} to ${name(i + 1)} was asked to go ${asked[i + 1] > asked[i] ? 'up' : 'down'} but measures ${meas[i]} → ${meas[i + 1]}`);
        break;
      }
      case 'quiet_before_peak': if (pk > 0 && !(meas[pk - 1] <= meas[pk] * 0.6 && (pk < 2 || meas[pk - 1] < meas[pk - 2]))) add(`${name(pk - 1)} measures ${meas[pk - 1]} before the peak (${meas[pk]}): thin it to 60% of the peak or less, below the section before it`); break;
      case 'ends_low': if (meas[n - 1] > meas[pk] * 0.6) add(`the last section measures ${meas[n - 1]}, more than 60% of the peak (${meas[pk]})`); break;
      case 'false_stop': {
        const med = [...meas].sort((a, b) => a - b)[Math.floor(n / 2)];
        const i = secs.findIndex((s, k) => k > 0 && k < n - 1 && s.intensity <= 1);
        if (i >= 0 && meas[i] > med * 0.25) add(`${name(i)} should be near-silent but measures ${meas[i]} (the middle section measures ${med})`);
        break;
      }
      case 'rondo': {
        const as = ctx.sections.filter((s, i) => String(secs[i].material).trim() === 'A');
        for (const s of as.slice(1)) { const o = overlap(sig(as[0]), sig(s)); if (o < 0.5) add(`"${s.name}" is marked "A" but keeps ${Math.round(o * 100)}% of the first "A" (a return keeps 50%+ of its notes)`); }
        break;
      }
      case 'merge': {
        const mat = secs.map((s) => String(s.material || '').replace(/\s/g, ''));
        const i = mat.findIndex((x) => x === 'A+B' || x === 'B+A'); const a = mat.indexOf('A'); const b = mat.indexOf('B');
        if (i >= 0 && a >= 0 && b >= 0) {
          const pcs = (sec) => new Set(manifest.tracks.flatMap((t) => t.notes.filter((x) => x.start >= sec.start - E && x.start < sec.end - E).map((x) => `${t.channel}:${(x.start - sec.start) % ctx.bpb}:${x.pitch}`)));
          const both = pcs(ctx.sections[i]); const oa = overlap(pcs(ctx.sections[a]), both); const ob = overlap(pcs(ctx.sections[b]), both);
          if (oa < 0.2 || ob < 0.2) add(`${name(i)} should play both worlds but keeps ${Math.round(oa * 100)}% of ${name(a)} and ${Math.round(ob * 100)}% of ${name(b)} (20%+ of each)`);
        }
        break;
      }
      case 'through': {
        for (let i = 1; i < ctx.sections.length; i++) for (let j = 0; j < i; j++) {
          if (ctx.sections[i].bars !== ctx.sections[j].bars) continue;
          const o = overlap(sig(ctx.sections[j]), sig(ctx.sections[i]));
          if (o >= 0.6) { add(`${name(i)} replays ${Math.round(o * 100)}% of ${name(j)}: every section is new`); break; }
        }
        break;
      }
      case 'pairs': for (let k = 0; k + 1 < n; k += 2) if (meas[k] >= meas[k + 1]) add(`${name(k)} measures ${meas[k]}, not calmer than its twin ${name(k + 1)} (${meas[k + 1]})`); break;
      case 'shared_drums': {
        const isDrum = (t) => t.channel === 10 || t.kind === 'drums';
        const first = sig(ctx.sections[0], isDrum);
        for (const s of ctx.sections.slice(1)) { const o = overlap(first, sig(s, isDrum)); if (o < 0.75) add(`"${s.name}" keeps ${Math.round(o * 100)}% of the first loop's drums (75%+)`); }
        break;
      }
      case 'layers': for (const s of ctx.sections) { const parts = new Set(manifest.tracks.filter((t) => t.notes.some((x) => x.start >= s.start - E && x.start < s.end - E)).map((t) => t.channel)); if (parts.size > 2) add(`"${s.name}" has ${parts.size} parts: a layer is one or two`); }
        break;
      default: break;
    }
  }
  // What the flow avoids shouldn't be heard either (a warning: a detector can hear what nobody wrote on purpose).
  for (const t of (flow.techniques?.avoid || []).map(canonical).filter(Boolean)) {
    const def = TECHNIQUES[t];
    if (['sound', 'lock'].includes(def.kind)) continue;
    const tries = def.kind === 'notes' ? manifest.tracks.map((x) => ({ part: x.channel })) : def.kind === 'pair' ? [] : [{}];
    for (const tr of tries) { const r = listen(ctx, t, tr); if (r.found) { warnings.push(`flow "${flow.name}" avoids ${t}, and it's heard${tr.part ? ` on channel ${tr.part}` : ''}: ${r.evidence}`); break; } }
  }
  return { errors, warnings };
}

// The flow in a few lines, for the blueprint's readable version and the music sheet.
export function flowLines(flow) {
  return [
    `**Flow: ${flow.name}** (${flow.bucket}${flow.feels?.length ? `; ${flow.feels.join(', ')}` : ''}). ${flow.summary}`,
    ...(flow.shape?.rules || []).map((r) => `- ${RULES[r]?.says || r}`),
    flow.techniques?.required?.length ? `- Its techniques: ${flow.techniques.required.join(', ')}${flow.techniques.free ? `, plus ${flow.techniques.free} of the piece's own` : ''}` : null,
    flow.techniques?.avoid?.length ? `- It avoids: ${flow.techniques.avoid.join(', ')}` : null,
  ].filter(Boolean);
}
