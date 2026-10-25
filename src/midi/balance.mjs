// The balance pass: each part is rendered alone, measured section by section (K-weighted, like LUFS), and turned
// down only where it rises over its role's ceiling against the anchor (drums + bass, whose level never goes down).
// Nothing is ever turned up. Ceilings loosen in high-energy sections so a part meant to hit still hits.
import { execFile } from 'node:child_process';

const RATE = 48000;

// dB under the anchor each role may reach (at energy 3 or less)
export const CEILINGS = { lead: -2, counter: -8, keys: -5, chords: -5, arp: -5, pad: -6 };
// extra room per energy step over 3
export const LOOSEN = 1.5;
// the drum channel (drums, perc) and the bass are the anchor: never balanced
export const ANCHOR_ROLES = ['drums', 'perc', 'bass'];

// BS.1770 K-weighting at 48 kHz: a high shelf, then a high-pass
const K = [
  { b: [1.53512485958697, -2.69169618940638, 1.19839281085285], a: [-1.69065929318241, 0.73248077421585] },
  { b: [1.0, -2.0, 1.0], a: [-1.99004745483398, 0.99007225036621] },
];

function kWeight(x) {
  const y = Float64Array.from(x);
  for (const { b, a } of K) {
    let x1 = 0, x2 = 0, y1 = 0, y2 = 0;
    for (let i = 0; i < y.length; i++) {
      const x0 = y[i];
      const o = b[0] * x0 + b[1] * x1 + b[2] * x2 - a[0] * y1 - a[1] * y2;
      x2 = x1; x1 = x0; y2 = y1; y1 = o;
      y[i] = o;
    }
  }
  return y;
}

// A WAV (or any audio ffmpeg reads) as mono float samples at 48 kHz.
export function decode(file) {
  return new Promise((ok, fail) => execFile('ffmpeg', ['-loglevel', 'error', '-i', file, '-ac', '1', '-ar', String(RATE), '-f', 'f32le', '-'],
    { encoding: 'buffer', maxBuffer: 1 << 30 }, (e, out) => (e ? fail(e) : ok(new Float32Array(out.buffer, out.byteOffset, out.length >> 2)))));
}

const lufs = (ms) => (ms > 1e-12 ? -0.691 + 10 * Math.log10(ms) : -Infinity);

// K-weighted power per section: the loudest 2-bar stretch inside it (mean square), so a burst counts, not just the
// average. bounds: [[startSec, endSec]]; win: the stretch in seconds.
export function sectionPower(samples, bounds, win = Infinity) {
  const k = kWeight(samples);
  const ms = (a, b) => {
    const i0 = Math.min(k.length, Math.round(a * RATE)), i1 = Math.min(k.length, Math.round(b * RATE));
    let s = 0;
    for (let i = i0; i < i1; i++) s += k[i] * k[i];
    return i1 > i0 ? s / (i1 - i0) : 0;
  };
  return bounds.map(([a, b]) => {
    let best = 0;
    for (let t = a; t < b - 1e-6; t += Math.min(win, b - a)) best = Math.max(best, ms(t, Math.min(b, t + win)));
    return best;
  });
}

export const barSeconds = (map) => (map.meter === '3/4' ? 3 : 4) * 60 / map.bpm;

export function sectionBounds(map) {
  const barSec = (map.meter === '3/4' ? 3 : 4) * 60 / map.bpm;
  let at = 0;
  return map.sections.map((s) => { const r = [at * barSec, (at + s.bars) * barSec]; at += s.bars; return r; });
}

// power: { partId: [mean square per section] }. Returns { gains: { partId: [dB per section, ≤ 0] }, rows } where a
// part only gets a cut in sections it plays in and only by how far it's over its ceiling.
export function balance(map, power, { ceilings = CEILINGS, loosen = LOOSEN } = {}) {
  const role = Object.fromEntries(map.parts.map((p) => [p.id, p.role]));
  const anchorIds = map.parts.filter((p) => ANCHOR_ROLES.includes(p.role)).map((p) => p.id);
  const gains = {};
  const rows = [];
  map.sections.forEach((sec, i) => {
    const anchor = lufs(anchorIds.reduce((n, id) => n + (power[id]?.[i] || 0), 0));
    for (const p of map.parts) {
      const ceil = ceilings[role[p.id]];
      gains[p.id] ??= map.sections.map(() => 0);
      const level = lufs(power[p.id]?.[i] || 0);
      if (ceil === undefined || !Number.isFinite(anchor) || !Number.isFinite(level) || !sec.parts.includes(p.id)) continue;
      const limit = anchor + ceil + loosen * Math.max(0, sec.energy - 3);
      if (level > limit) {
        gains[p.id][i] = Math.round((limit - level) * 10) / 10;
        rows.push({ section: sec.name, part: p.id, level: Math.round(level * 10) / 10, anchor: Math.round(anchor * 10) / 10, limit: Math.round(limit * 10) / 10, cut: gains[p.id][i] });
      }
    }
  });
  return { gains, rows };
}

// Parts whose dense sound gets its lows cut (General MIDI guitars, 0-based 24-31, and counter lines).
export const isDense = (p) => p.role === 'counter' || (p.program >= 24 && p.program <= 31);
export const DENSE_HIGHPASS = 150;

// A gain in dB as a MIDI expression (CC11) value, by the General MIDI curve (dB = 40·log10(v/127)).
export const ccFor = (db) => Math.max(1, Math.min(127, Math.round(127 * 10 ** (db / 40))));
