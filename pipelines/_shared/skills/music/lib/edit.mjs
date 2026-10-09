// The owner's edits to a score, made in the studio: "bar 9: a different drum beat". An edit replaces one channel in
// some bars and leaves every other note exactly where it was.
//
// How it's forced: every block on that channel gets a per-bar mask that silences it in those bars, and one block of
// the owner's own plays the new bars. A mask is the only splice that works for any score (rewriting a token in place
// would freeze a nested <…> alternation), and it's plain Strudel, so the score still plays in the REPL. The edits live
// in music/edits.json; renderEdits() rebuilds the score from them, so they survive a session rewriting the score.
//
//   blocksOf(code)                         the labelled blocks (label, channel, where the expression is)
//   locate(code, haps, { from, to, ... })  what plays in bars from..to: per channel, its notes and the code behind them
//   renderEdits(code, edits, totalBars)    the score with the edits applied (idempotent; undone edits drop out)
//   handEdit(notes, bars, beatsPerBar)     the bars of a hand edit (notes drawn in the studio's editor) as an edit
//   verifyEdit(before, after, edit)        nothing outside the edit moved, and something inside it did
//   verifyNotes(before, after, cells, …)   a hand edit came out exactly as drawn, and nothing else moved
//
// Two kinds of edit: Claude's ({ bars: { 9: { notes, velocity } } }, mini-notation a bar) and the owner's hand edits,
// which also carry the exact notes ({ notes: [{ pitch, start, dur, velocity }], beats_per_bar }, times in beats from
// the top of the piece). Exact notes play from a block of their own on a fine grid (GRID steps a beat), so a chord
// held over the bar line stays held. Their `bars` say the same per bar, cut at the bar line, for an older engine.
//   describeDifference(a, b, ...)          what changed, in measured words ("closed hat 8 → 16 hits a bar")

export const REGION_START = '// ---- your edits (made in the studio from music/edits.json; they stay unless the owner changes them) ----';
export const REGION_END = '// ---- end of your edits ----';
const KEEP = '/*mfx-keep*/';
const WRAPPED = /^\(([\s\S]*)\n\)\.mask\(\/\*mfx-keep\*\/"[^"]*"\)$/;

// ---------- reading the score ----------

// Top-level labelled blocks (`drums: cue(…).midichan(10)`), each running to the next top-level statement. Strings,
// comments and brackets are skipped over, so a colon inside a pattern or an object never counts.
export function blocksOf(code) {
  const blocks = [];
  const starts = statementStarts(code);
  starts.forEach((at, i) => {
    const end = i + 1 < starts.length ? starts[i + 1] : code.length;
    const m = /^([A-Za-z_$][\w$]*)[ \t]*:(?!:)[ \t]*/.exec(code.slice(at, end));
    if (!m || /^(https?|case|default)$/.test(m[1])) return;
    const exprStart = at + m[0].length;
    let exprEnd = end;
    while (exprEnd > exprStart && /\s/.test(code[exprEnd - 1])) exprEnd -= 1;
    const expr = code.slice(exprStart, exprEnd);
    const ch = /\.midichan\(\s*(\d+)\s*\)/.exec(expr);
    blocks.push({ label: m[1], start: at, end, exprStart, exprEnd, expr, channel: ch ? Number(ch[1]) : null });
  });
  return blocks;
}

// Where each top-level statement begins: a line at bracket depth 0 that doesn't continue the one before (continuations
// start with whitespace, `.`, a closing bracket, an operator or a comment).
function statementStarts(code) {
  const out = [];
  let depth = 0; let i = 0; let lineStart = true;
  while (i < code.length) {
    const c = code[i];
    if (lineStart && depth === 0 && !/[\s.)\]}/,?:+*|&-]/.test(c)) out.push(i);
    lineStart = false;
    if (c === '\n') { lineStart = true; i += 1; continue; }
    if (c === '/' && code[i + 1] === '/') { while (i < code.length && code[i] !== '\n') i += 1; continue; }
    if (c === '/' && code[i + 1] === '*') { const e = code.indexOf('*/', i + 2); i = e < 0 ? code.length : e + 2; continue; }
    if (c === '"' || c === "'" || c === '`') {
      i += 1;
      while (i < code.length && code[i] !== c) i += code[i] === '\\' ? 2 : 1;
      i += 1; continue;
    }
    if ('([{'.includes(c)) depth += 1;
    else if (')]}'.includes(c)) depth = Math.max(0, depth - 1);
    i += 1;
  }
  return out;
}

const noteKey = (h) => `${h.value?.note ?? h.value?.drum ?? h.value?.n ?? h.value?.s}`;
const chanOf = (h) => h.value?.midichan ?? null;

// What plays in bars from..to (1-based, inclusive): per channel, its haps there and the code behind them (the block,
// and each source token with the bars around it). `haps` come from HeadlessStrudel.haps (begin/end in cycles = bars).
export function locate(code, haps, { from, to, channel = null }) {
  const blocks = blocksOf(code);
  const inBars = haps.filter((h) => h.begin >= from - 1 - 1e-9 && h.begin < to - 1e-9 && (channel === null || chanOf(h) === channel));
  const byChannel = new Map();
  for (const h of inBars) {
    const ch = chanOf(h);
    if (!byChannel.has(ch)) byChannel.set(ch, { channel: ch, haps: [], tokens: new Map(), blocks: new Set() });
    const e = byChannel.get(ch);
    e.haps.push(h);
    for (const at of h.locations || []) {
      const b = blocks.find((x) => at >= x.exprStart && at < x.end);
      if (b) e.blocks.add(b.label);
      if (!e.tokens.has(at)) e.tokens.set(at, tokenAt(code, at));
    }
  }
  return [...byChannel.values()].sort((a, b) => (a.channel ?? 99) - (b.channel ?? 99)).map((e) => ({
    channel: e.channel, haps: e.haps, blocks: [...e.blocks],
    tokens: [...e.tokens.entries()].sort((a, b) => a[0] - b[0]).map(([at, t]) => ({ at, ...t })),
  }));
}

// The token at a source offset and its line, with the token marked: «36».
function tokenAt(code, at) {
  const m = /^[^\s"'`<>[\]{},!@*/()~|]+/.exec(code.slice(at)) || [''];
  const ls = code.lastIndexOf('\n', at - 1) + 1; const le = code.indexOf('\n', at); const lineEnd = le < 0 ? code.length : le;
  const line = code.slice(ls, lineEnd);
  const col = at - ls;
  return { token: m[0], line: code.slice(0, ls).split('\n').length, marked: `${line.slice(0, col)}«${m[0]}»${line.slice(col + m[0].length)}`.trim().slice(0, 240) };
}

// ---------- applying the edits ----------

// One value per bar as a Strudel alternation, with repeats folded: [1,1,0,1] → "<1!2 0 1>".
export function perBar(values) {
  const runs = [];
  for (const v of values) { const last = runs.at(-1); if (last && last.v === v) last.n += 1; else runs.push({ v, n: 1 }); }
  return `<${runs.map((r) => (r.n > 1 ? `${r.v}!${r.n}` : r.v)).join(' ')}>`;
}

// The score with the active edits applied. Earlier edit output (masks and the owner's block) is removed first, so this
// always starts from the score as the session wrote it. Returns { code, missing: [channels with no block to mask] }.
export function renderEdits(code, edits, totalBars) {
  let base = stripEdits(code);
  const active = (edits || []).filter((e) => e.status !== 'undone');
  if (!active.length) return { code: base, missing: [] };
  const n = Math.max(1, Math.ceil(totalBars));
  // Per channel: the bars it's replaced in, and what plays there (a later edit wins a bar).
  const layers = new Map();
  for (const e of active) {
    if (!layers.has(e.channel)) layers.set(e.channel, new Map());
    const bpb = e.beats_per_bar || 4;
    for (const [bar, v] of Object.entries(e.bars)) {
      const b = Number(bar);
      layers.get(e.channel).set(b, Array.isArray(e.notes) ? { ...v, bpb, exact: e.notes.filter((x) => barOf(x.start, bpb) === b) } : v);
    }
  }
  const blocks = blocksOf(base);
  const missing = [];
  // Wrap from the end so the offsets stay valid.
  const wraps = [];
  for (const [ch, bars] of layers) {
    const on = blocks.filter((b) => b.channel === ch);
    if (!on.length) missing.push(ch);
    const mask = perBar(Array.from({ length: n }, (_, i) => (bars.has(i + 1) ? 0 : 1)));
    for (const b of on) wraps.push({ b, mask });
  }
  for (const { b, mask } of wraps.sort((x, y) => y.b.exprStart - x.b.exprStart)) {
    base = `${base.slice(0, b.exprStart)}(${b.expr}\n).mask(${KEEP}"${mask}")${base.slice(b.exprEnd)}`;
  }
  const lines = [...layers.entries()].sort((a, b) => a[0] - b[0]).flatMap(([ch, bars]) => {
    const out = [];
    const said = (bar) => { const v = bars.get(bar); return v && !v.exact ? v : null; };
    if ([...bars.values()].some((v) => !v.exact)) {
      const slot = (bar, key, rest) => { const v = said(bar); return v ? `[${String(v[key] ?? rest).trim() || rest}]` : rest; };
      const notes = perBar(Array.from({ length: n }, (_, i) => slot(i + 1, 'notes', '~')));
      const vel = perBar(Array.from({ length: n }, (_, i) => slot(i + 1, 'velocity', said(i + 1) ? '.8' : '0')));
      out.push(`owner_ch${ch}: note("${notes}").velocity("${vel}").filterWhen((t) => t < ${n}).midichan(${ch})`);
    }
    const exact = [...bars.values()].filter((v) => v.exact);
    if (exact.length) out.push(exactLine(ch, exact.flatMap((v) => v.exact), exact[0].bpb, n));
    return out;
  });
  return { code: `${base.replace(/\s*$/, '')}\n\n${REGION_START}\n${lines.join('\n')}\n${REGION_END}\n`, missing };
}

// ---------- hand edits: exact notes ----------

export const GRID = 24; // steps a beat: 16ths are 6, 32nds 3, eighth-note triplets 8
const barOf = (start, bpb) => Math.floor(Math.round(start * GRID) / (bpb * GRID)) + 1;
const r2 = (x) => Math.round(x * 100) / 100;

// Notes on the grid: { p: pitch, s: start step, d: length in steps, v: velocity }.
const onGrid = (notes, offset = 0) => notes.map((x) => ({ p: Math.round(x.pitch), s: Math.round(x.start * GRID) - offset, d: Math.max(1, Math.round(x.dur * GRID)), v: r2(x.velocity ?? 0.8) }));

// Notes packed into voices that never overlap (a chord is a voice a note), each as a weighted sequence over `total`
// steps: "~@12 60@6 ~@78".
function voicesOf(notes, total) {
  const voices = [];
  for (const x of [...notes].sort((a, b) => a.s - b.s || a.p - b.p)) {
    if (x.s < 0 || x.s >= total) continue;
    const y = { ...x, d: Math.min(x.d, total - x.s) };
    const v = voices.find((w) => w.at(-1).s + w.at(-1).d <= y.s);
    if (v) v.push(y); else voices.push([y]);
  }
  return voices;
}
function seq(voice, total, value) {
  const parts = []; let t = 0;
  for (const x of voice) { if (x.s > t) parts.push(`~@${x.s - t}`); parts.push(`${value(x)}@${x.d}`); t = x.s + x.d; }
  if (total > t) parts.push(`~@${total - t}`);
  return parts.join(' ');
}

// A channel's exact notes as one block over the whole piece (n bars), so a note can run over the bar line.
function exactLine(ch, notes, bpb, n) {
  const total = n * bpb * GRID;
  const voices = voicesOf(onGrid(notes), total);
  const body = voices.length ? voices.map((v) => `note("${seq(v, total, (x) => x.p)}").velocity("${seq(v, total, (x) => x.v)}")`).join(',\n  ') : 'note("~")';
  return `owner_notes_ch${ch}: stack(\n  ${body}\n).slow(${n}).filterWhen((t) => t < ${n}).midichan(${ch})`;
}

// A hand edit's bars, for edits.json: the exact notes, and the same per bar as mini-notation (cut at the bar line)
// for an engine from before hand edits. `notes` are { pitch, start, dur, velocity } (beats from the top), each
// starting in one of `bars`.
export function handEdit(notes, bars, bpb = 4) {
  const steps = bpb * GRID;
  const out = {};
  for (const bar of [...bars].sort((a, b) => a - b)) {
    const inBar = notes.filter((x) => barOf(x.start, bpb) === bar);
    const g = onGrid(inBar, (bar - 1) * steps);
    const voices = voicesOf(g, steps);
    if (!voices.length) { out[bar] = { notes: '~', velocity: '0' }; continue; }
    const onsets = [...new Map(g.sort((a, b) => a.s - b.s || b.v - a.v).map((x) => [x.s, x.v])).entries()];
    const vel = onsets.map(([s, v], i) => `${v}@${(onsets[i + 1]?.[0] ?? steps) - s}`);
    if (onsets[0][0] > 0) vel.unshift(`~@${onsets[0][0]}`);
    out[bar] = { notes: voices.map((v) => `[${seq(v, steps, (x) => x.p)}]`).join(', '), velocity: vel.join(' ') };
  }
  return { bars: out, notes: notes.map((x) => ({ pitch: Math.round(x.pitch), start: Math.round(x.start * GRID) / GRID, dur: Math.max(1, Math.round(x.dur * GRID)) / GRID, velocity: r2(x.velocity ?? 0.8) })), beats_per_bar: bpb };
}

// The score without any edit output: the owner's region removed and every mask wrap undone.
export function stripEdits(code) {
  let out = code;
  const a = out.indexOf(REGION_START);
  if (a >= 0) {
    const b = out.indexOf(REGION_END, a);
    out = `${out.slice(0, a).replace(/\s*$/, '\n')}${b < 0 ? '' : out.slice(b + REGION_END.length).replace(/^\s*\n/, '')}`;
  }
  for (const b of blocksOf(out).reverse()) {
    const m = WRAPPED.exec(b.expr);
    if (m) out = `${out.slice(0, b.exprStart)}${m[1]}${out.slice(b.exprEnd)}`;
  }
  return out;
}

// ---------- checking an edit ----------

const r4 = (x) => Math.round(x * 1e4) / 1e4;
const hapId = (h) => `${r4(h.begin)}|${r4(h.end)}|${chanOf(h)}|${noteKey(h)}|${r4(Number(h.value?.velocity ?? 0.8))}`;

// Forced means forced: outside the edited bars of the edited channel every note is exactly where it was, and inside
// them the notes changed. Returns { ok, problems, outside_changed, before: haps there, after: haps there }.
export function verifyEdit(beforeHaps, afterHaps, { channel, bars }) {
  const set = new Set(bars.map(Number));
  const inside = (h) => chanOf(h) === channel && set.has(Math.floor(h.begin + 1e-9) + 1);
  const count = (list) => { const m = new Map(); for (const h of list) m.set(hapId(h), (m.get(hapId(h)) || 0) + 1); return m; };
  const a = count(beforeHaps.filter((h) => !inside(h))); const b = count(afterHaps.filter((h) => !inside(h)));
  let changed = 0;
  for (const k of new Set([...a.keys(), ...b.keys()])) changed += Math.abs((a.get(k) || 0) - (b.get(k) || 0));
  const before = beforeHaps.filter(inside); const after = afterHaps.filter(inside);
  const same = before.length === after.length && [...count(before)].every(([k, v]) => count(after).get(k) === v);
  const problems = [];
  if (changed) problems.push(`${changed} note${changed === 1 ? '' : 's'} outside the edited bars moved`);
  if (same) problems.push('the edited bars came out exactly as they were');
  return { ok: !problems.length, problems, outside_changed: changed, before, after };
}

// A hand edit is exact: in its cells (channel → bars) the notes are the ones drawn (to the grid), and outside them
// every note is where it was. `wanted` are { channel, pitch, start, dur, velocity } in beats.
export function verifyNotes(beforeHaps, afterHaps, cells, wanted, bpb = 4) {
  const inside = (h) => cells.get(chanOf(h))?.has(Math.floor(h.begin + 1e-9) + 1);
  const count = (ids) => { const m = new Map(); for (const k of ids) m.set(k, (m.get(k) || 0) + 1); return m; };
  const diff = (a, b) => { let n = 0; for (const k of new Set([...a.keys(), ...b.keys()])) n += Math.abs((a.get(k) || 0) - (b.get(k) || 0)); return n; };
  const moved = diff(count(beforeHaps.filter((h) => !inside(h)).map(hapId)), count(afterHaps.filter((h) => !inside(h)).map(hapId)));
  const id = (begin, end, ch, pitch, vel) => `${r4(begin)}|${r4(end)}|${ch}|${pitch}|${r2(vel)}`;
  const got = count(afterHaps.filter(inside).map((h) => id(h.begin, h.end, chanOf(h), Number(h.value?.note), Number(h.value?.velocity ?? 0.8))));
  const want = count(wanted.map((x) => { const s = Math.round(x.start * GRID); const d = Math.max(1, Math.round(x.dur * GRID)); return id(s / GRID / bpb, (s + d) / GRID / bpb, x.channel, Math.round(x.pitch), r2(x.velocity ?? 0.8)); }));
  const off = diff(got, want);
  const problems = [];
  if (moved) problems.push(`${moved} note${moved === 1 ? '' : 's'} outside the edited bars moved`);
  if (off) problems.push(`${off} note${off === 1 ? '' : 's'} in the edited bars didn't come out as drawn`);
  return { ok: !problems.length, problems };
}

// ---------- describing what changed ----------

const GM = { 35: 'kick', 36: 'kick', 37: 'rim', 38: 'snare', 39: 'clap', 40: 'snare', 41: 'low tom', 42: 'closed hat', 43: 'low tom', 44: 'pedal hat',
  45: 'mid tom', 46: 'open hat', 47: 'mid tom', 48: 'high tom', 49: 'crash', 50: 'high tom', 51: 'ride', 53: 'ride bell', 54: 'tambourine', 56: 'cowbell', 57: 'crash', 59: 'ride', 69: 'cabasa', 70: 'maracas', 82: 'shaker' };
export const drumName = (p) => GM[p] || `GM ${p}`;
const NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
const pitchName = (p) => `${NAMES[p % 12]}${Math.floor(p / 12) - 1}`;

// Notes are { pitch, start (beats), dur, velocity }, from one channel in the edited bars. Returns short measured facts.
export function describeDifference(before, after, { drums = false, bars = 1, beatsPerBar = 4 } = {}) {
  const per = (n) => +(n / bars).toFixed(1);
  const facts = [];
  if (drums) {
    const count = (list) => { const m = new Map(); for (const n of list) m.set(drumName(n.pitch), (m.get(drumName(n.pitch)) || 0) + 1); return m; };
    const a = count(before); const b = count(after);
    for (const name of [...new Set([...a.keys(), ...b.keys()])]) {
      const x = per(a.get(name) || 0); const y = per(b.get(name) || 0);
      if (x !== y) facts.push(`${name} ${x} → ${y} hits a bar`);
    }
    const offbeat = (list) => list.filter((n) => Math.abs((n.start % 1)) > 0.01).length / Math.max(1, list.length);
    const kicks = (list) => list.filter((n) => drumName(n.pitch) === 'kick');
    const ka = kicks(before); const kb = kicks(after);
    if (kb.length && Math.abs(offbeat(kb) - offbeat(ka)) >= 0.2) facts.push(`kick ${offbeat(kb) > offbeat(ka) ? 'more' : 'less'} syncopated (${Math.round(offbeat(ka) * 100)}% → ${Math.round(offbeat(kb) * 100)}% off the beat)`);
  } else {
    if (before.length !== after.length) facts.push(`${per(before.length)} → ${per(after.length)} notes a bar`);
    const range = (list) => (list.length ? [Math.min(...list.map((n) => n.pitch)), Math.max(...list.map((n) => n.pitch))] : null);
    const ra = range(before); const rb = range(after);
    if (rb && (!ra || ra[0] !== rb[0] || ra[1] !== rb[1])) facts.push(`range ${ra ? `${pitchName(ra[0])}-${pitchName(ra[1])}` : 'silent'} → ${pitchName(rb[0])}-${pitchName(rb[1])}`);
    const mean = (list) => list.reduce((s, n) => s + n.pitch, 0) / Math.max(1, list.length);
    if (ra && rb && Math.abs(mean(after) - mean(before)) >= 3) facts.push(`${mean(after) > mean(before) ? 'higher' : 'lower'} by about ${Math.round(Math.abs(mean(after) - mean(before)))} semitones`);
  }
  const dens = (list) => list.length / Math.max(1, bars);
  if (before.length && after.length && Math.abs(dens(after) / dens(before) - 1) >= 0.15) facts.push(`${dens(after) > dens(before) ? '+' : '−'}${Math.round(Math.abs(dens(after) / dens(before) - 1) * 100)}% density`);
  const vel = (list) => list.reduce((s, n) => s + n.velocity, 0) / Math.max(1, list.length);
  if (before.length && after.length && Math.abs(vel(after) - vel(before)) >= 0.08) facts.push(`${vel(after) > vel(before) ? 'harder' : 'softer'} (velocity ${vel(before).toFixed(2)} → ${vel(after).toFixed(2)})`);
  if (!before.length && after.length) facts.unshift('fills bars that were empty');
  if (before.length && !after.length) facts.unshift('silences those bars');
  return facts.length ? facts : ['the same density and range, a different pattern'];
}

// A channel's notes in some bars, as text the model can read: a 16-step grid per drum sound, or onsets for a pitched part.
export function notesText(notes, { drums = false, from, to, beatsPerBar = 4 }) {
  const lines = [];
  for (let bar = from; bar <= to; bar += 1) {
    const inBar = notes.filter((n) => n.start >= (bar - 1) * beatsPerBar - 1e-6 && n.start < bar * beatsPerBar - 1e-6);
    if (!inBar.length) { lines.push(`bar ${bar}: (silent)`); continue; }
    if (drums) {
      const steps = beatsPerBar * 4;
      const sounds = [...new Set(inBar.map((n) => n.pitch))].sort((a, b) => a - b);
      lines.push(`bar ${bar}:`);
      for (const p of sounds) {
        const row = Array(steps).fill('.');
        for (const n of inBar.filter((x) => x.pitch === p)) {
          const s = Math.round(((n.start - (bar - 1) * beatsPerBar) / beatsPerBar) * steps);
          if (s >= 0 && s < steps) row[s] = n.velocity >= 0.85 ? 'X' : n.velocity < 0.5 ? 'g' : 'x';
        }
        lines.push(`  ${String(p).padStart(2)} ${drumName(p).padEnd(10)} ${row.join('').replace(/(.{4})(?=.)/g, '$1 ')}`);
      }
    } else {
      lines.push(`bar ${bar}: ${inBar.map((n) => `${+(n.start - (bar - 1) * beatsPerBar + 1).toFixed(2)}:${pitchName(n.pitch)}(${+n.dur.toFixed(2)}b, v${n.velocity})`).join(' ')}`);
    }
  }
  return lines.join('\n');
}
