// MIDI ⇄ Strudel, both ways (doc 28). `notes` runs a pattern's text and returns its notes; `edit` takes the text and
// the notes the owner wants and returns the smallest text change that runs to exactly those notes (the ladder, §3.4):
//   1. bar lines    the text is line-per-bar: write it again; only the edited bars' lines differ
//   2. in place     only pitch or velocity changed, each from one word used by no other note: change that word
//   3. replace bars anything else: mfx.bars(<music>, from, to, <those bars, line per bar>)
//   4. fresh        the whole pattern in the short form (only if 3 can't be made exact)
// Every result is run and its notes must equal the wanted notes, or the next step is tried.
import { createEvaluator, PPQ } from './evaluator.mjs';
import { notesToStrudel, notesToIdiomatic, keyText, sortNotes, strip, sameNotes } from './notes.mjs';

const NOTE_RE = /^([a-gA-G])([#bsf]*)(-?\d+)?$/;
const CH = { c: 0, d: 2, e: 4, f: 5, g: 7, a: 9, b: 11 };
const BARS_RE = /^mfx\.bars\(([\s\S]*), (\d+), (\d+), (`<[\s\S]*>`\.as\("[a-z:]+"\))\)$/;

function tokenKey(tok) {
  if (/^-?\d+(\.\d+)?$/.test(tok)) return { kind: 'num', key: Number(tok) };
  const m = tok.match(NOTE_RE);
  if (!m) return null;
  const acc = [...m[2]].reduce((s, c) => s + (c === '#' || c === 's' ? 1 : -1), 0);
  return { kind: 'name', key: (Number(m[3] ?? 3) + 1) * 12 + CH[m[1].toLowerCase()] + acc, flats: m[2].includes('b') };
}

export function createStrudel({ strudelDir, timeoutMs } = {}) {
  const ev = createEvaluator({ strudelDir, timeoutMs });

  async function notes(code, { bars, tpb = 4 * PPQ }) {
    const r = await ev.run(code, { bars, tpb });
    if (!r.ok) { const e = new Error(r.error); e.line = r.line; throw e; }
    return { notes: r.notes, problems: r.problems };
  }

  async function edit(code, wanted, { bars, tpb = 4 * PPQ }) {
    const t0 = performance.now();
    const text = code.trim();
    const target = sortNotes(strip(wanted));
    for (const n of target) if (n.t < 0 || n.t >= bars * tpb) throw new Error(`a note at tick ${n.t} is outside the ${bars} bars`);
    const old = (await notes(text, { bars, tpb })).notes;
    const opts = { bars, ticksPerBar: tpb };
    const check = async (c) => { try { const r = await notes(c, { bars, tpb }); return sameNotes(r.notes, target) ? r : null; } catch { return null; } };
    const done = (level, c, r, extra = {}) => ({ level, code: c, notes: r.notes, problems: r.problems, ms: Math.round((performance.now() - t0) * 10) / 10, ...extra });

    // 1. bar lines
    let canonical = null;
    try { canonical = notesToStrudel(old, opts); } catch { canonical = null; }
    if (canonical === text) {
      const c = notesToStrudel(target, opts);
      const r = await check(c);
      if (r) return done('bar lines', c, r);
    }
    // 2. in place
    const inPlace = wordEdit(text, old, target);
    if (inPlace) { const r = await check(inPlace); if (r) return done('in place', inPlace, r); }
    // 3. replace bars. An edit inside bars already replaced rewrites only those inner bar lines; a new spot gets its own
    // mfx.bars around the rest (at most 3 deep, then the spots are joined).
    const changed = changedBars(old, target, tpb, bars);
    if (changed.length) {
      const c = replaceBars(text, target, changed, { ...opts, tpb });
      const r = await check(c.code);
      if (r) return done('replace bars', c.code, r, { from: c.from, to: c.to });
    }
    // 4. fresh
    for (const c of [notesToIdiomatic(target, opts), notesToStrudel(target, opts)]) {
      const r = await check(c);
      if (r) return done('fresh', c, r);
    }
    throw new Error('no text runs to exactly these notes');
  }

  return { notes, edit, close: ev.close };
}

// Peels mfx.bars(<music>, from, to, <bars>) layers off the text, outermost first.
function layersOf(text) {
  const layers = [];
  let inner = text;
  for (let m; (m = inner.match(BARS_RE));) { layers.push({ from: Number(m[2]), to: Number(m[3]), repl: m[4] }); inner = m[1]; }
  return { inner, layers: layers.reverse() };
}

function replaceBars(text, target, changed, { bars, ticksPerBar: tpb }) {
  const { inner, layers } = layersOf(text);
  // the changed bars (1-based) in runs of neighbours
  const runs = [];
  for (const b of changed.map((x) => x + 1)) { const r = runs.at(-1); if (r && b === r.to + 1) r.to = b; else runs.push({ from: b, to: b }); }
  const all = layers.map((l) => ({ ...l, dirty: false }));
  for (const run of runs) {
    const hit = all.filter((l) => l.from <= run.to + 1 && run.from <= l.to + 1);
    if (!hit.length) { all.push({ ...run, dirty: true }); continue; }
    const keep = hit[0];
    keep.from = Math.min(run.from, ...hit.map((l) => l.from));
    keep.to = Math.max(run.to, ...hit.map((l) => l.to));
    keep.dirty = true;
    for (const l of hit.slice(1)) all.splice(all.indexOf(l), 1);
  }
  while (all.length > 3) {   // join the two closest spots
    all.sort((a, b) => a.from - b.from);
    let best = 0;
    for (let i = 1; i < all.length - 1; i++) if (all[i + 1].from - all[i].to < all[best + 1].from - all[best].to) best = i;
    const [a, b] = [all[best], all[best + 1]];
    all.splice(best, 2, { from: a.from, to: b.to, dirty: true });
  }
  let code = inner;
  for (const l of all) {
    const repl = l.dirty ? notesToStrudel(target.filter((n) => n.t >= (l.from - 1) * tpb && n.t < l.to * tpb).map((n) => ({ ...n, t: n.t - (l.from - 1) * tpb })), { bars: l.to - l.from + 1, ticksPerBar: tpb }) : l.repl;
    code = `mfx.bars(${code}, ${l.from}, ${l.to}, ${repl})`;
  }
  const dirty = all.filter((l) => l.dirty);
  return { code, from: Math.min(...dirty.map((l) => l.from)), to: Math.max(...dirty.map((l) => l.to)) };
}

// Bars (0-based) where the notes differ.
function changedBars(a, b, tpb, bars) {
  const key = (n) => `${n.t}|${n.len}|${n.key}|${n.vel}`;
  const A = new Set(a.map(key));
  const B = new Set(b.map(key));
  const out = new Set();
  for (const n of a) if (!B.has(key(n))) out.add(Math.floor(n.t / tpb));
  for (const n of b) if (!A.has(key(n))) out.add(Math.floor(n.t / tpb));
  return [...out].filter((x) => x >= 0 && x < bars).sort((x, y) => x - y);
}

// Step 2: same notes at the same times, only pitch or velocity changed, each through a word only that note uses.
function wordEdit(code, oldNotes, target) {
  if (oldNotes.length !== target.length) return null;
  const group = (ns) => { const m = new Map(); for (const n of ns) { const k = `${n.t}|${n.len}`; if (!m.has(k)) m.set(k, []); m.get(k).push(n); } return m; };
  const olds = group(sortNotes(oldNotes));
  const news = group(target);
  const uses = new Map();
  for (const n of oldNotes) for (const l of n.loc || []) { const k = l.join(':'); uses.set(k, (uses.get(k) || 0) + 1); }
  const edits = [];
  for (const [k, os] of olds) {
    const ns = news.get(k);
    if (!ns || ns.length !== os.length) return null;
    const removed = os.filter((x) => !ns.some((y) => y.key === x.key && y.vel === x.vel));
    const added = ns.filter((y) => !os.some((x) => x.key === y.key && x.vel === y.vel));
    if (removed.length !== added.length) return null;
    for (let i = 0; i < removed.length; i++) {
      const was = removed[i];
      const now = added[i];
      let keyDone = now.key === was.key;
      let velDone = now.vel === was.vel;
      for (const l of was.loc || []) {
        if (uses.get(l.join(':')) !== 1) continue;
        const tok = code.slice(l[0], l[1]);
        const tk = tokenKey(tok);
        if (!keyDone && tk && tk.key === was.key) { edits.push([l[0], l[1], tk.kind === 'num' ? String(now.key) : keyText(now.key, { flats: tk.flats })]); keyDone = true; continue; }
        if (!velDone && /^\.?\d*\.?\d+$/.test(tok) && Number(tok) === was.vel) { edits.push([l[0], l[1], String(now.vel).replace(/^0\./, '.')]); velDone = true; }
      }
      if (!keyDone || !velDone) return null;
    }
  }
  if (!edits.length) return null;
  edits.sort((a, b) => b[0] - a[0]);
  let c = code;
  for (const [s, e, t] of edits) c = c.slice(0, s) + t + c.slice(e);
  return c;
}
