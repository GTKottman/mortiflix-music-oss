// The MIDI, made for someone still learning: the instruments across the top (each lights up when it plays, so you know
// what you're hearing), and the notes of the whole piece below, in the instruments' colours, to change by hand.
//
//   instrumentStrip({ tracks, roles, colors, pid, focus, onFocus, onNote })   → element with .setFocus(ch)
//   noteEditor({ piece, colors, st, pid, play, editable, focus, onFocus, onChange, onSave, toast, sections })
//                                                                               → element with .setFocus(ch), .goTo(bar)
//
// The editor works on a copy of the notes (`st`, kept by the page so a redraw doesn't lose it). What changed is known
// bar by bar and part by part (a "cell"); "Put it in the song" sends those cells and their notes to the studio, which
// writes them into the score exactly (src/edits.mjs makeNotesEdit).

import { h, fill } from './dom.js';
import * as player from './player.js';

const PALETTE = ['#4ea8f0', '#f0913a', '#4fbf68', '#ec5f8e', '#9b7bf0', '#d9b52b', '#2fb8a8', '#ef6f5a', '#6f86f2', '#a5b83a', '#d46cc6', '#5aa877'];
export function colorsFor(tracks) {
  const m = new Map();
  tracks.forEach((t, i) => m.set(t.channel, PALETTE[i % PALETTE.length]));
  return m;
}
const rgba = (hex, a) => { const n = parseInt(hex.slice(1), 16); return `rgba(${n >> 16},${(n >> 8) & 255},${n & 255},${a})`; };

const isDrums = (t) => t.drums || t.kind === 'drums' || t.channel === 10;
const NAMES = ['C', 'C#', 'D', 'D#', 'E', 'F', 'F#', 'G', 'G#', 'A', 'A#', 'B'];
export const pitchName = (p) => `${NAMES[((p % 12) + 12) % 12]}${Math.floor(p / 12) - 1}`;
const BLACK = new Set([1, 3, 6, 8, 10]);
const GM = { 35: 'Kick 2', 36: 'Kick', 37: 'Rim', 38: 'Snare', 39: 'Clap', 40: 'Snare 2', 41: 'Low tom', 42: 'Closed hat', 43: 'Low tom 2', 44: 'Pedal hat',
  45: 'Mid tom', 46: 'Open hat', 47: 'Mid tom 2', 48: 'High tom', 49: 'Crash', 50: 'High tom 2', 51: 'Ride', 52: 'China', 53: 'Ride bell', 54: 'Tambourine',
  55: 'Splash', 56: 'Cowbell', 57: 'Crash 2', 58: 'Vibraslap', 59: 'Ride 2', 60: 'High bongo', 61: 'Low bongo', 62: 'Muted conga', 63: 'Open conga', 64: 'Low conga',
  65: 'High timbale', 66: 'Low timbale', 67: 'High agogo', 68: 'Low agogo', 69: 'Cabasa', 70: 'Maracas', 71: 'Whistle', 72: 'Long whistle', 73: 'Short guiro', 74: 'Long guiro',
  75: 'Claves', 76: 'High wood block', 77: 'Low wood block', 78: 'Muted cuica', 79: 'Open cuica', 80: 'Muted triangle', 81: 'Triangle', 82: 'Shaker' };
export const drumName = (p) => GM[p] || `Drum ${p}`;
const KIT = [36, 38, 39, 42, 46, 49];   // always offered in the drum rows, so a clap can be added where there was none

// What a part is, in plain words: from the plan's role when there is one, else from its name.
export function partWords(t, role) {
  const kind = player.instrumentKind(t);
  const harmony = role?.harmony;
  if (kind === 'drums' || harmony === 'drums') return 'The beat: kick, snare, hats';
  if (harmony === 'bass' || kind === 'bass') return 'The low notes: the bass line';
  if (harmony === 'melody') return 'A tune: the melody you could hum';
  if (harmony === 'chords') return kind === 'pad' ? 'Long, soft chords in the background' : 'Plays the chords';
  return { pad: 'Long, soft sounds in the background', keys: 'Chords on keys', lead: 'A tune on top', pluck: 'Short, bouncy notes' }[kind] || 'A part of the arrangement';
}

// Every frame while the player plays: `fn(state)` until the element has left the page for good. A page that redraws
// moves its parts into the new page, so a moment off the page doesn't count: only 5 s gone does.
function follow(el, fn) {
  let gone = 0;
  const off = player.on((s) => {
    if (!el.isConnected) {
      if (el.dataset.mounted) { gone ||= Date.now(); if (Date.now() - gone > 5000) off(); }
      return;
    }
    gone = 0; el.dataset.mounted = '1';
    fn(s);
  });
  return off;
}

// ---------- the instruments ----------

const ABOUT = ['Too loud', 'Too quiet', 'Plays too much', 'Needs more going on', 'Wrong sound for it', 'Should be higher', 'Should be lower', 'Love this part'];

export function instrumentStrip({ tracks, roles = [], colors, pid, bpb = 4, load, focus = null, onFocus, onNote = null, selectionBars = () => null }) {
  const roleOf = (t) => roles.find((r) => r.channel === t.channel) || roles.find((r) => r.name && r.name === t.name) || null;
  const cards = new Map();
  const panel = h('div', { class: 'part-panel' });
  const row = h('div', { class: 'parts', role: 'radiogroup', 'aria-label': 'Instruments' });
  for (const t of tracks) {
    const c = colors.get(t.channel);
    const meter = h('span', { class: 'part-meter', 'aria-hidden': 'true' });
    const card = h('button', { type: 'button', class: 'part', role: 'radio', 'aria-checked': 'false', style: { '--c': c }, onclick: () => pick(focus === t.channel ? null : t.channel) },
      h('span', { class: 'part-top' }, h('span', { class: 'swatch', 'aria-hidden': 'true' }), h('span', { class: 'part-name' }, t.name || `Part ${t.channel}`)),
      h('span', { class: 'part-kind' }, partWords(t, roleOf(t))),
      meter);
    cards.set(t.channel, { card, meter, c });
    row.append(card);
  }

  function pick(ch, notify = true) {
    focus = ch;
    for (const [k, { card }] of cards) { card.classList.toggle('on', k === ch); card.setAttribute('aria-checked', String(k === ch)); }
    drawPanel();
    if (notify) onFocus?.(ch);
  }

  function drawPanel() {
    const t = tracks.find((x) => x.channel === focus);
    if (!t) { fill(panel, h('p', { class: 'meta' }, 'Each instrument lights up when it plays. Click one to see what it does, hear it alone, and see its notes below.')); return; }
    const role = roleOf(t);
    const s = player.state();
    const mine = s.id === pid;
    const solo = mine && s.solo === t.channel;
    const muted = mine && s.muted.includes(t.channel);
    const barsWith = new Set(t.notes.map((n) => Math.floor(n.start / bpb) + 1)).size;
    const range = isDrums(t) ? `${[...new Set(t.notes.map((n) => n.pitch))].map(drumName).slice(0, 6).join(', ')}` : (() => { const ps = t.notes.map((n) => n.pitch); return `from ${pitchName(Math.min(...ps))} to ${pitchName(Math.max(...ps))}`; })();
    fill(panel,
      h('div', { class: 'pp-head' }, h('span', { class: 'swatch', style: { '--c': colors.get(t.channel) }, 'aria-hidden': 'true' }), h('h3', null, t.name || `Part ${t.channel}`), h('span', { class: 'meta' }, partWords(t, role))),
      role?.job ? h('p', null, h('b', null, 'What it does: '), role.job) : null,
      role?.sound ? h('p', { class: 'meta' }, `Its sound, as planned: ${role.sound}. The player here uses a simple stand-in.`) : null,
      h('p', { class: 'meta' }, `${t.notes.length} notes, playing in ${barsWith} bars, ${range}.`),
      h('div', { class: 'pp-actions' },
        h('button', { type: 'button', class: `btn${solo ? ' on' : ''}`, 'aria-pressed': String(solo), onclick: () => { load(); player.solo(t.channel); drawPanel(); } }, solo ? 'Hearing it alone' : 'Hear it alone'),
        h('button', { type: 'button', class: `btn${muted ? ' on' : ''}`, 'aria-pressed': String(muted), onclick: () => { load(); player.mute(t.channel, !muted); drawPanel(); } }, muted ? 'Muted' : 'Mute it')),
      onNote ? h('div', { class: 'pp-say' },
        h('div', { class: 'meta' }, (() => { const b = selectionBars(); return b ? `Tell the studio about ${t.name} in ${b[0] === b[1] ? `bar ${b[0]}` : `bars ${b[0]}-${b[1]}`} (the notes you picked):` : `Tell the studio about ${t.name} (the whole song):`; })()),
        h('div', { class: 'choices' }, ABOUT.map((text) => h('button', { type: 'button', class: 'choice', onclick: () => onNote({ part: t.name, channel: t.channel, text, bars: selectionBars() }) }, text)),
          h('button', { type: 'button', class: 'choice', onclick: () => onNote({ part: t.name, channel: t.channel, text: '', bars: selectionBars() }) }, 'Something else…'))) : null);
  }
  const strip = h('div', { class: 'strip' }, row, panel);
  strip.setFocus = (ch) => pick(ch, false);
  pick(focus, false);
  let last = '';
  follow(strip, (s) => {
    const lv = s.id === pid ? player.levels() : {};
    for (const [ch, { card, meter, c }] of cards) {
      const v = lv[ch] || 0;
      card.style.background = v ? rgba(c, 0.07 + v * 0.3) : '';
      card.style.boxShadow = v ? `0 0 ${Math.round(4 + v * 14)}px ${rgba(c, v * 0.55)}` : '';
      meter.style.transform = `scaleX(${v})`;
    }
    const key = `${s.id}|${s.solo}|${(s.muted || []).join(',')}`;
    if (key !== last) {
      last = key;
      for (const [ch, { card }] of cards) {
        const mine = s.id === pid;
        card.classList.toggle('quiet', mine && ((s.solo !== null && s.solo !== undefined && s.solo !== ch) || (s.muted || []).includes(ch)));
      }
      if (focus !== null) drawPanel();
    }
  });
  return strip;
}

// ---------- the notes ----------

const SNAPS = [[1, '1 beat'], [0.5, '½ beat'], [0.25, '¼ beat'], [1 / 3, '⅓ beat (triplet)'], [1 / 24, 'free']];
const GRID = 24;
const q = (x) => Math.round(x * GRID) / GRID;
let nid = 1;

export function noteEditor({ piece, colors, st, pid, play, editable, focus = null, onFocus, onChange = () => {}, onSave, toast, sections = [] }) {
  const bpb = piece.beatsPerBar || piece.beats_per_bar || 4;
  const bpm = piece.bpm || 120;
  const tracks = piece.tracks || [];
  const byCh = new Map(tracks.map((t) => [t.channel, t]));
  const lastBeat = Math.max(1, ...tracks.flatMap((t) => t.notes.map((n) => n.start + n.dur)), (piece.bars || 0) * bpb);
  const totalBars = Math.max(1, Math.ceil(lastBeat / bpb - 1e-6));
  const barOf = (start) => Math.floor(Math.round(start * GRID) / (bpb * GRID)) + 1;
  const cellKey = (ch, bar) => `${ch}|${bar}`;
  const sig = (n) => `${Math.round(n.start * GRID)}:${n.pitch}:${Math.max(1, Math.round(n.dur * GRID))}:${Math.round(n.vel * 100)}`;
  const cellsOf = (notes) => { const m = new Map(); for (const n of notes) { const k = cellKey(n.ch, barOf(n.start)); if (!m.has(k)) m.set(k, []); m.get(k).push(sig(n)); } for (const v of m.values()) v.sort(); return m; };

  if (!st.notes) {
    st.notes = tracks.flatMap((t) => t.notes.map((n) => ({ id: nid++, ch: t.channel, pitch: n.pitch, start: n.start, dur: n.dur, vel: n.velocity ?? 0.8 })));
    st.origCells = cellsOf(st.notes);
    st.history = []; st.future = []; st.selected = new Set();
  }
  st.vb ??= window.innerWidth < 700 ? 4 : 8;
  st.v0 ??= 0;
  st.snap ??= 0.25;
  st.tool ??= 'draw';
  st.follow ??= true;
  let dirty = new Map(); // cellKey → { ch, bar }

  // ---- layout ----
  const HEAD = 34; const GUT = 82; const DROW = 14; const GAP = 8;
  let lo = 48; let hi = 72; let kit = []; let rowH = 8;
  const hasDrums = tracks.some(isDrums);
  const drumCh = tracks.find(isDrums)?.channel ?? 10;
  // The pitch rows: every part's range, or the picked part's when one is picked (bigger notes, easier to hit).
  function layout() {
    const all = st.notes.filter((n) => !isDrums(byCh.get(n.ch) || { channel: n.ch }));
    const own = focus !== null && !isDrums(byCh.get(focus) || { channel: focus }) ? all.filter((n) => n.ch === focus) : [];
    const pitched = own.length ? own : all;
    if (pitched.length) { lo = Math.min(...pitched.map((n) => n.pitch)) - 2; hi = Math.max(...pitched.map((n) => n.pitch)) + 2; }
    while (hi - lo + 1 < 25) { lo -= 1; if (hi - lo + 1 < 25) hi += 1; }
    lo = Math.max(0, lo); hi = Math.min(127, hi);
    kit = hasDrums ? [...new Set([...KIT, ...st.notes.filter((n) => n.ch === drumCh).map((n) => n.pitch)])].sort((a, b) => b - a) : [];
    rowH = Math.max(6, Math.min(14, Math.floor(400 / (hi - lo + 1))));
  }
  const pitchedH = () => (hi - lo + 1) * rowH;
  const laneTop = () => HEAD + pitchedH() + GAP;
  const height = () => laneTop() + kit.length * DROW + 2;

  const canvas = h('canvas', { class: 'seq', tabindex: '0', 'aria-label': `The notes: ${tracks.length} instruments over ${totalBars} bars. Arrow keys move picked notes, Delete removes them.` });
  // On a touch screen a finger on the notes would edit them and never scroll the page: the notes stay to look at until
  // you tap "Edit notes" (the phone's own editor comes later).
  const touch = window.matchMedia?.('(pointer: coarse)').matches;
  let armed = !touch;
  const live = () => editable && armed;
  const arm = (on) => { armed = on; canvas.style.touchAction = live() ? 'none' : 'pan-y'; canvas.classList.toggle('armed', live()); if (armBtn) armBtn.textContent = on ? 'Done editing' : 'Edit notes'; drawTools(); };
  let armBtn = null;
  canvas.style.touchAction = live() ? 'none' : 'pan-y';
  let W = 800;
  const pxBeat = () => (W - GUT) / (st.vb * bpb);
  const x = (beat) => GUT + (beat - st.v0 * bpb) * pxBeat();
  const beatAt = (px) => st.v0 * bpb + (px - GUT) / pxBeat();
  const yOf = (n) => (n.ch === drumCh && hasDrums ? laneTop() + kit.indexOf(n.pitch) * DROW : HEAD + (hi - n.pitch) * rowH);
  const hOf = (n) => (n.ch === drumCh && hasDrums ? DROW - 2 : rowH - 1);
  const area = (py) => (py < HEAD ? 'head' : py < HEAD + pitchedH() ? 'pitched' : py >= laneTop() && py < laneTop() + kit.length * DROW ? 'drums' : null);
  const rowPitch = (py) => (area(py) === 'drums' ? kit[Math.min(kit.length - 1, Math.max(0, Math.floor((py - laneTop()) / DROW)))] : Math.max(lo, Math.min(hi, hi - Math.floor((py - HEAD) / rowH))));
  const snapBeat = (b) => Math.round(b / st.snap) * st.snap;
  const floorSnap = (b) => Math.floor(b / st.snap + 1e-6) * st.snap;

  // ---- drawing ----
  let sounding = null; // the beat the player is at, while it plays this piece
  function draw() {
    const css = getComputedStyle(document.documentElement);
    const col = (v) => css.getPropertyValue(v).trim();
    W = canvas.clientWidth || 800;
    const H = height();
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) { canvas.width = Math.round(W * dpr); canvas.height = Math.round(H * dpr); canvas.style.height = `${H}px`; }
    const g = canvas.getContext('2d');
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, W, H);
    const b0 = st.v0 * bpb; const b1 = (st.v0 + st.vb) * bpb;
    // rows: the piano's black keys a shade darker, every C named
    g.font = `10px ${col('--text') || 'sans-serif'}`;
    g.textBaseline = 'middle';
    for (let p = lo; p <= hi; p += 1) {
      const y = HEAD + (hi - p) * rowH;
      if (BLACK.has(p % 12)) { g.fillStyle = rgba(col('--ink').startsWith('#') ? col('--ink') : '#888888', 0.035); g.fillRect(GUT, y, W - GUT, rowH); }
      g.fillStyle = BLACK.has(p % 12) ? col('--faint') : col('--raise');
      g.fillRect(0, y, GUT - 26, rowH - 1);
      if (p % 12 === 0 || rowH >= 11) { g.fillStyle = col('--muted'); g.fillText(pitchName(p), GUT - 24, y + rowH / 2); }
      if (p % 12 === 0) { g.fillStyle = col('--line'); g.fillRect(GUT, y + rowH - 1, W - GUT, 1); }
    }
    kit.forEach((p, i) => {
      const y = laneTop() + i * DROW;
      if (i % 2) { g.fillStyle = col('--line'); g.globalAlpha = 0.35; g.fillRect(GUT, y, W - GUT, DROW); g.globalAlpha = 1; }
      g.fillStyle = col('--muted'); g.fillText(drumName(p), 2, y + DROW / 2);
    });
    if (kit.length) { g.fillStyle = col('--line'); g.fillRect(0, laneTop() - GAP / 2, W, 1); }
    // the grid: snap lines faint, beats, bars with their number
    const gridTop = HEAD; const gridBottom = H;
    if (st.snap < 1 && pxBeat() * st.snap >= 7) { g.fillStyle = col('--line'); g.globalAlpha = 0.45; for (let b = Math.ceil(b0 / st.snap) * st.snap; b < b1; b += st.snap) g.fillRect(Math.round(x(b)), gridTop, 1, gridBottom - gridTop); g.globalAlpha = 1; }
    for (let b = Math.ceil(b0); b <= b1; b += 1) { g.fillStyle = b % bpb === 0 ? col('--faint') : col('--line'); g.fillRect(Math.round(x(b)), b % bpb === 0 ? 0 : gridTop, 1, gridBottom); }
    g.textBaseline = 'alphabetic';
    for (let bar = st.v0; bar < st.v0 + st.vb && bar < totalBars; bar += 1) {
      const changed = [...dirty.values()].some((d) => d.bar === bar + 1);
      g.fillStyle = changed ? col('--pin') : col('--muted');
      g.fillText(`${bar + 1}${changed ? ' •' : ''}`, x(bar * bpb) + 4, 13);
    }
    // The section you're in, and any that start here, on the second line.
    g.fillStyle = col('--body');
    const current = [...sections].reverse().find((s) => (s.start_bar - 1) * bpb <= b0);
    if (current) g.fillText(current.name, GUT + 4, 28);
    for (const s of sections) {
      const sb = (s.start_bar - 1) * bpb;
      if (sb > b0 && sb < b1) { g.fillStyle = col('--bg'); g.fillRect(x(sb) + 1, 18, W, 14); g.fillStyle = col('--body'); g.fillText(s.name, x(sb) + 4, 28); }
    }
    g.fillStyle = col('--bg'); g.fillRect(0, 0, GUT - 1, HEAD);
    // the notes: other parts faint when one is picked, the picked one on top
    const visible = st.notes.filter((n) => n.start < b1 && n.start + n.dur > b0 && (n.ch !== drumCh || !hasDrums || kit.includes(n.pitch)));
    visible.sort((a, b) => (a.ch === focus) - (b.ch === focus));
    g.save(); g.beginPath(); g.rect(GUT, 0, W - GUT, H); g.clip();
    for (const n of visible) {
      const c = colors.get(n.ch) || '#888888';
      const on = sounding !== null && sounding >= n.start && sounding < n.start + n.dur;
      const dim = focus !== null && n.ch !== focus;
      const nx = x(n.start); const nw = Math.max(3, n.dur * pxBeat() - 1); const ny = yOf(n); const nh = hOf(n);
      g.globalAlpha = dim ? 0.2 : on ? 1 : 0.55 + 0.4 * n.vel;
      g.fillStyle = c;
      g.fillRect(nx, ny, nw, nh);
      if (on && !dim) { g.globalAlpha = 0.9; g.strokeStyle = '#ffffff'; g.lineWidth = 1.5; g.strokeRect(nx + 0.75, ny + 0.75, nw - 1.5, nh - 1.5); }
      if (st.selected.has(n.id)) { g.globalAlpha = 1; g.strokeStyle = col('--ink'); g.lineWidth = 2; g.strokeRect(nx - 1, ny - 1, nw + 2, nh + 2); }
    }
    g.globalAlpha = 1;
    if (band) { g.fillStyle = rgba('#8fb5ff', 0.12); g.strokeStyle = col('--link'); g.lineWidth = 1; const r = norm(band); g.fillRect(r.x, r.y, r.w, r.h); g.strokeRect(r.x + 0.5, r.y + 0.5, r.w, r.h); }
    if (sounding !== null && sounding >= b0 && sounding <= b1) { g.fillStyle = col('--ink'); g.fillRect(Math.round(x(sounding)), 0, 2, H); }
    g.restore();
  }
  const norm = (b) => ({ x: Math.min(b.x0, b.x1), y: Math.min(b.y0, b.y1), w: Math.abs(b.x1 - b.x0), h: Math.abs(b.y1 - b.y0) });

  // ---- changes ----
  function recount() {
    const now = cellsOf(st.notes);
    dirty = new Map();
    for (const k of new Set([...now.keys(), ...st.origCells.keys()])) {
      const a = now.get(k) || []; const b = st.origCells.get(k) || [];
      if (a.length !== b.length || a.some((v, i) => v !== b[i])) { const [ch, bar] = k.split('|').map(Number); dirty.set(k, { ch, bar }); }
    }
  }
  function commit(before) {
    st.history.push(before); if (st.history.length > 200) st.history.shift();
    st.future = [];
    changed();
  }
  function changed() {
    // The rows stay put while you work; they only grow when a note goes past them.
    const past = st.notes.some((n) => !isDrumNote(n) && (focus === null || n.ch === focus) && (n.pitch < lo || n.pitch > hi));
    if (past) layout(); else kit = hasDrums ? [...new Set([...kit, ...st.notes.filter((n) => n.ch === drumCh).map((n) => n.pitch)])].sort((a, b) => b - a) : [];
    recount(); draw(); drawTools(); onChange(dirty.size);
    // The player plays what you see: your changes, before they're in the song.
    const s = player.state();
    if (s.id === pid) { player.load({ ...play, piece: workingPiece() }, { force: true }); player.seek(s.time); if (s.playing) player.play(); }
  }
  const snapshot = () => st.notes.slice();
  // Notes are replaced, never changed in place, so a snapshot (a list of the same notes) stays as it was.
  const replace = (ids, fn) => { st.notes = st.notes.map((n) => (ids.has(n.id) ? { ...n, ...fn(n) } : n)); };
  function undo() { if (!st.history.length) return; st.future.push(snapshot()); st.notes = st.history.pop(); prune(); changed(); }
  function redo() { if (!st.future.length) return; st.history.push(snapshot()); st.notes = st.future.pop(); prune(); changed(); }
  const prune = () => { const ids = new Set(st.notes.map((n) => n.id)); for (const k of st.selected) if (!ids.has(k)) st.selected.delete(k); };
  const picked = () => st.notes.filter((n) => st.selected.has(n.id));
  const isDrumNote = (n) => hasDrums && n.ch === drumCh;

  function workingPiece() {
    return { ...piece, tracks: tracks.map((t) => ({ ...t, notes: st.notes.filter((n) => n.ch === t.channel).map((n) => ({ pitch: n.pitch, start: n.start, dur: n.dur, velocity: n.vel })).sort((a, b) => a.start - b.start) })) };
  }
  const hear = (n) => { const t = byCh.get(n.ch); if (t) player.preview(t, n.pitch, n.vel, n.dur); };

  // ---- the mouse (and touch) ----
  let drag = null; let band = null;
  const point = (e) => { const r = canvas.getBoundingClientRect(); return { px: e.clientX - r.left, py: e.clientY - r.top }; };
  const hit = (px, py) => {
    const b = beatAt(px);
    const under = st.notes.filter((n) => b >= n.start && b < n.start + Math.max(n.dur, 3 / pxBeat()) && py >= yOf(n) && py < yOf(n) + hOf(n) + 1 && (!isDrumNote(n) || kit.includes(n.pitch)));
    return under.find((n) => n.ch === focus) || under.at(-1) || null;
  };
  canvas.addEventListener('pointerdown', (e) => {
    if (e.button !== 0) return;
    if (e.pointerType === 'touch' && !live()) return; // a finger here scrolls the page; nothing else
    const { px, py } = point(e);
    if (px < GUT) { // the keys: hear that note on the picked instrument
      const t = focus !== null ? byCh.get(focus) : null;
      const a = area(py);
      if (a === 'drums' && hasDrums) player.preview(byCh.get(drumCh), rowPitch(py), 0.9, 0.25);
      else if (a === 'pitched' && t && !isDrums(t)) player.preview(t, rowPitch(py), 0.8, 1);
      return;
    }
    if (area(py) === 'head') { // the bar numbers: play from there
      const bar = Math.floor(beatAt(px) / bpb);
      if (player.state().id !== pid) player.load({ ...play, piece: workingPiece() });
      player.seek((bar * bpb * 60) / bpm); player.play();
      st.follow = true; followBox.checked = true;
      return;
    }
    const n = hit(px, py);
    if (n) {
      if (e.shiftKey) { if (st.selected.has(n.id)) st.selected.delete(n.id); else st.selected.add(n.id); }
      else if (!st.selected.has(n.id)) { st.selected = new Set([n.id]); }
      if (focus !== n.ch) { focus = n.ch; onFocus?.(n.ch); }
      hear(n);
      if (live() && st.selected.has(n.id)) {
        const edge = x(n.start + n.dur) - px <= Math.min(8, n.dur * pxBeat() / 3);
        drag = { kind: edge ? 'resize' : 'move', before: snapshot(), anchor: n, b0: beatAt(px), p0: rowPitch(py), a0: area(py), from: new Map(picked().map((m) => [m.id, { ...m }])), moved: false, lastPitch: n.pitch };
        canvas.setPointerCapture(e.pointerId);
      }
      draw(); drawTools();
      return;
    }
    if (!e.shiftKey) st.selected = new Set();
    if (live() && st.tool === 'draw') {
      const a = area(py);
      if (!a) { draw(); drawTools(); return; }
      if (focus === null) { toast('Pick an instrument at the top first: new notes go to it'); draw(); drawTools(); return; }
      const t = byCh.get(focus);
      if ((a === 'drums') !== isDrums(t)) { toast(a === 'drums' ? `${t.name} plays notes, not drums: put its notes in the rows above` : 'The drums go in the drum rows at the bottom'); draw(); drawTools(); return; }
      const start = Math.max(0, floorSnap(beatAt(px)));
      if (start >= totalBars * bpb) return;
      const before = snapshot();
      const note = { id: nid++, ch: focus, pitch: rowPitch(py), start: q(start), dur: q(isDrums(t) ? Math.min(st.snap, 0.25) : Math.max(st.snap, 0.25)), vel: 0.8 };
      st.notes = [...st.notes, note];
      st.selected = new Set([note.id]);
      hear(note);
      drag = { kind: 'resize', before, anchor: note, b0: beatAt(px), from: new Map([[note.id, { ...note }]]), moved: true, created: true };
      canvas.setPointerCapture(e.pointerId);
      changedSoft();
      return;
    }
    if (!live()) { draw(); drawTools(); return; }
    // Select: drag a box around notes.
    band = { x0: px, y0: py, x1: px, y1: py, add: e.shiftKey };
    canvas.setPointerCapture(e.pointerId);
    draw(); drawTools();
  });
  canvas.addEventListener('pointermove', (e) => {
    const { px, py } = point(e);
    if (band) { band.x1 = px; band.y1 = py; draw(); return; }
    if (!drag) {
      const n = hit(px, py);
      canvas.style.cursor = px < GUT ? 'pointer' : area(py) === 'head' ? 'pointer' : n ? (live() && x(n.start + n.dur) - px <= Math.min(8, n.dur * pxBeat() / 3) ? 'ew-resize' : live() ? 'grab' : 'pointer') : live() && st.tool === 'draw' && focus !== null ? 'crosshair' : 'default';
      return;
    }
    const db = beatAt(px) - drag.b0;
    if (drag.kind === 'move') {
      const sel = [...drag.from.values()];
      const pitchedOnly = sel.every((m) => !isDrumNote(m)); const drumsOnly = sel.every(isDrumNote);
      let dp = 0;
      if (pitchedOnly && area(py) === 'pitched') dp = rowPitch(py) - drag.p0;
      if (drumsOnly && area(py) === 'drums' && drag.a0 === 'drums') dp = kit.indexOf(rowPitch(py)) - kit.indexOf(drag.p0);
      const shift = snapBeat(db);
      const minStart = Math.min(...sel.map((m) => m.start));
      const s = Math.max(-minStart, shift);
      replace(new Set(drag.from.keys()), (n) => {
        const o = drag.from.get(n.id);
        const pitch = isDrumNote(o) ? kit[Math.max(0, Math.min(kit.length - 1, kit.indexOf(o.pitch) + dp))] : Math.max(0, Math.min(127, o.pitch + dp));
        return { start: q(Math.min(totalBars * bpb - 1 / GRID, o.start + s)), pitch };
      });
      const a = st.notes.find((n) => n.id === drag.anchor.id);
      if (a && a.pitch !== drag.lastPitch) { drag.lastPitch = a.pitch; hear(a); }
      drag.moved = drag.moved || s !== 0 || dp !== 0;
    } else {
      const o = drag.from.get(drag.anchor.id);
      const end = Math.max(o.start + 1 / GRID, snapBeat(o.start + o.dur + db) || o.start + st.snap);
      const grow = end - (o.start + o.dur);
      replace(new Set(drag.from.keys()), (n) => { const f = drag.from.get(n.id); return { dur: q(Math.max(1 / GRID, f.dur + grow)) }; });
      drag.moved = drag.moved || Math.abs(grow) > 1e-6;
    }
    changedSoft();
  });
  const endDrag = () => {
    if (band) {
      const r = norm(band); band = null;
      if (r.w > 3 || r.h > 3) {
        const inBox = st.notes.filter((n) => x(n.start + n.dur) >= r.x && x(n.start) <= r.x + r.w && yOf(n) + hOf(n) >= r.y && yOf(n) <= r.y + r.h && (focus === null || n.ch === focus));
        for (const n of inBox) st.selected.add(n.id);
      }
      draw(); drawTools();
      return;
    }
    if (!drag) return;
    const d = drag; drag = null;
    if (d.moved) commit(d.before); else { draw(); drawTools(); }
  };
  canvas.addEventListener('pointerup', endDrag);
  canvas.addEventListener('pointercancel', endDrag);
  canvas.addEventListener('dblclick', (e) => {
    if (!live()) return;
    const { px, py } = point(e);
    const n = hit(px, py);
    if (n) { const before = snapshot(); st.notes = st.notes.filter((m) => m.id !== n.id); st.selected.delete(n.id); commit(before); }
  });
  canvas.addEventListener('wheel', (e) => {
    const dx = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.shiftKey ? e.deltaY : 0;
    if (!dx) return;
    e.preventDefault();
    st.acc = (st.acc || 0) + dx;
    const step = Math.trunc(st.acc / 60);
    if (step) { st.acc -= step * 60; go(st.v0 + step, true); }
  }, { passive: false });
  // While dragging: redraw without counting changes on every move.
  const changedSoft = () => { draw(); };

  // ---- keys ----
  canvas.addEventListener('keydown', (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); if (e.shiftKey) redo(); else undo(); return; }
    if (mod && e.key.toLowerCase() === 'y') { e.preventDefault(); redo(); return; }
    if (mod && e.key.toLowerCase() === 'a') { e.preventDefault(); st.selected = new Set(st.notes.filter((n) => (focus === null || n.ch === focus) && n.start >= st.v0 * bpb && n.start < (st.v0 + st.vb) * bpb).map((n) => n.id)); draw(); drawTools(); return; }
    if (e.key === 'Escape') { st.selected = new Set(); draw(); drawTools(); return; }
    if (!live() || !st.selected.size) return;
    if (mod && e.key.toLowerCase() === 'd') { e.preventDefault(); act.repeat(); return; }
    if (e.key === 'Delete' || e.key === 'Backspace') { e.preventDefault(); act.del(); return; }
    const arrows = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, 1], ArrowDown: [0, -1] };
    if (arrows[e.key]) { e.preventDefault(); const [t, p] = arrows[e.key]; act.nudge(t * st.snap, p * (e.shiftKey ? 12 : 1)); }
  });

  // ---- what you can do with the notes you picked ----
  const act = {
    del() { const before = snapshot(); st.notes = st.notes.filter((n) => !st.selected.has(n.id)); st.selected = new Set(); commit(before); },
    nudge(beats, semis) {
      const sel = picked();
      if (Math.min(...sel.map((n) => n.start)) + beats < 0) return;
      const before = snapshot();
      replace(st.selected, (n) => (isDrumNote(n) ? { start: q(n.start + beats), pitch: semis ? kit[Math.max(0, Math.min(kit.length - 1, kit.indexOf(n.pitch) - Math.sign(semis)))] : n.pitch } : { start: q(n.start + beats), pitch: Math.max(0, Math.min(127, n.pitch + semis)) }));
      commit(before);
      const a = picked()[0]; if (a && semis) hear(a);
    },
    louder(f) { const before = snapshot(); replace(st.selected, (n) => ({ vel: Math.round(Math.max(0.05, Math.min(1, n.vel * f)) * 100) / 100 })); commit(before); hear(picked()[0]); },
    octave(d) { const before = snapshot(); replace(st.selected, (n) => (isDrumNote(n) ? {} : { pitch: Math.max(0, Math.min(127, n.pitch + 12 * d)) })); commit(before); },
    longer(f) { const before = snapshot(); replace(st.selected, (n) => ({ dur: q(Math.max(1 / GRID, n.dur * f)) })); commit(before); },
    repeat() {
      const sel = picked(); if (!sel.length) return;
      const from = Math.min(...sel.map((n) => n.start)); const to = Math.max(...sel.map((n) => n.start + n.dur));
      const span = Math.max(st.snap, Math.ceil((to - from) / bpb - 1e-6) * bpb > to - from + 1e-6 && to - from >= bpb / 2 ? Math.ceil((to - from) / bpb - 1e-6) * bpb : Math.ceil((to - from) / st.snap - 1e-6) * st.snap);
      if (to + span > totalBars * bpb) { toast('There\'s no room after the end of the piece'); return; }
      const before = snapshot();
      const copies = sel.map((n) => ({ ...n, id: nid++, start: q(n.start + span) }));
      st.notes = [...st.notes, ...copies];
      st.selected = new Set(copies.map((n) => n.id));
      commit(before);
      if (copies[0].start >= (st.v0 + st.vb) * bpb) go(Math.floor(copies[0].start / bpb));
    },
    moveTo(ch) {
      const t = byCh.get(ch); if (!t) return;
      const sel = picked().filter((n) => !isDrumNote(n));
      if (isDrums(t) || !sel.length) { toast('Notes move between instruments that play notes; the drums keep their own rows'); return; }
      const before = snapshot();
      replace(new Set(sel.map((n) => n.id)), () => ({ ch }));
      focus = ch; onFocus?.(ch);
      commit(before);
      toast(`${sel.length} note${sel.length === 1 ? '' : 's'} moved to ${t.name}`);
    },
  };

  // ---- the controls ----
  const where = h('span', { class: 'seq-where' });
  const go = (bar, byHand = false) => {
    st.v0 = Math.max(0, Math.min(Math.max(0, totalBars - st.vb), Math.round(bar)));
    if (byHand && player.state().playing && player.state().id === pid && st.follow) { st.follow = false; followBox.checked = false; }
    where.textContent = `Bars ${st.v0 + 1}-${Math.min(totalBars, st.v0 + st.vb)} of ${totalBars}`;
    draw();
    box.dispatchEvent(new CustomEvent('view', { detail: { from: st.v0 + 1, to: Math.min(totalBars, st.v0 + st.vb) } }));
  };
  const followBox = h('input', { type: 'checkbox', checked: st.follow, onchange: (e) => { st.follow = e.target.checked; } });
  const select = (label, options, value, onchange) => h('label', { class: 'seq-pick' }, h('span', { class: 'meta' }, label),
    h('select', { onchange: (e) => onchange(e.target.value) }, options.map(([v, t]) => { const o = h('option', { value: String(v) }, t); if (String(v) === String(value)) o.selected = true; return o; })));
  const toolBtn = (tool, label, help) => h('button', { type: 'button', class: `choice${st.tool === tool ? ' on' : ''}`, role: 'radio', 'aria-checked': String(st.tool === tool), title: help, onclick: () => { st.tool = tool; fill(tools, toolBtn('draw', 'Draw', 'Click empty space to add a note'), toolBtn('select', 'Select', 'Drag a box to pick several notes')); } }, label);
  const tools = h('span', { class: 'choices', role: 'radiogroup', 'aria-label': 'Tool' }, toolBtn('draw', 'Draw', 'Click empty space to add a note'), toolBtn('select', 'Select', 'Drag a box to pick several notes'));
  const nav = h('div', { class: 'seq-nav' },
    h('button', { type: 'button', class: 'btn', 'aria-label': 'Earlier bars', onclick: () => go(st.v0 - Math.max(1, st.vb / 2), true) }, '◀'),
    where,
    h('button', { type: 'button', class: 'btn', 'aria-label': 'Later bars', onclick: () => go(st.v0 + Math.max(1, st.vb / 2), true) }, '▶'),
    select('Show', [[2, '2 bars'], [4, '4 bars'], [8, '8 bars'], [16, '16 bars'], [32, '32 bars']], st.vb, (v) => { st.vb = Number(v); go(st.v0); }),
    editable ? select('Snap to', SNAPS.map(([v, t]) => [v, t]), st.snap, (v) => { st.snap = Number(v); draw(); }) : null,
    editable ? tools : null,
    h('label', { class: 'seq-pick' }, followBox, h('span', { class: 'meta' }, 'Follow the music')),
    touch && editable ? (armBtn = h('button', { type: 'button', class: 'btn primary', onclick: () => arm(!armed) }, 'Edit notes')) : null);
  const selBar = h('div', { class: 'seq-sel' });
  const pending = h('div', { class: 'seq-pending' });
  const help = h('details', { class: 'seq-help' }, h('summary', null, 'How to read and change this'),
    h('ul', null,
      h('li', null, 'Left to right is time: each numbered column is a bar (4 beats). Up and down is pitch: higher notes sit higher, like a piano on its side (the keys on the left).'),
      h('li', null, 'Each colour is one instrument from the row at the top. The drums have their own rows at the bottom: one row per drum.'),
      h('li', null, 'While it plays, the notes you\'re hearing light up, and so does their instrument at the top.'),
      editable ? [
        h('li', null, 'Pick an instrument, then click empty space to add a note for it (drag right to make it longer). Drag a note to move it, drag its right end to change its length, double-click it to remove it.'),
        h('li', null, 'Click a note to pick it (Shift-click for more, or the Select tool to drag a box). Arrow keys move picked notes; Shift with ↑↓ moves an octave; Delete removes them; Ctrl+Z undoes.'),
        h('li', null, '"Snap to" keeps notes on the beat: ¼ beat is a 16th note. Your changes play straight away here; "Put it in the song" writes them into the score.'),
      ] : h('li', null, 'Notes can be changed here while this MIDI waits for your review.'),
      h('li', null, 'Click the bar numbers to play from that bar. Click the keys on the left to hear a pitch on the picked instrument.')));

  function drawTools() {
    const sel = picked();
    if (!sel.length) fill(selBar, h('span', { class: 'meta' }, editable && !live() ? 'Tap "Edit notes" to change them; until then the page scrolls freely.' : editable ? (focus === null ? 'Pick an instrument above, then click in the grid to add its notes.' : `Adding notes to ${byCh.get(focus)?.name || 'this part'}. Click a note to pick it.`) : 'Click a note to see what it is.'));
    else {
      const chs = [...new Set(sel.map((n) => n.ch))];
      const one = sel.length === 1 ? sel[0] : null;
      const what = one ? `${byCh.get(one.ch)?.name}: ${isDrumNote(one) ? drumName(one.pitch) : pitchName(one.pitch)}, bar ${barOf(one.start)} beat ${+((one.start % bpb) + 1).toFixed(2)}, ${+one.dur.toFixed(2)} beat${one.dur === 1 ? '' : 's'} long, loudness ${Math.round(one.vel * 100)}%`
        : `${sel.length} notes picked (${chs.map((c) => byCh.get(c)?.name).join(', ')})`;
      const others = tracks.filter((t) => !isDrums(t) && !(chs.length === 1 && chs[0] === t.channel));
      fill(selBar, h('span', { class: 'seq-what' }, what), editable ? h('span', { class: 'seq-acts' },
        h('button', { type: 'button', class: 'btn', onclick: () => act.louder(0.8) }, 'Softer'),
        h('button', { type: 'button', class: 'btn', onclick: () => act.louder(1.25) }, 'Louder'),
        sel.some((n) => !isDrumNote(n)) ? [h('button', { type: 'button', class: 'btn', onclick: () => act.octave(1) }, 'Octave up'), h('button', { type: 'button', class: 'btn', onclick: () => act.octave(-1) }, 'Octave down')] : null,
        h('button', { type: 'button', class: 'btn', onclick: () => act.longer(0.5) }, 'Shorter'),
        h('button', { type: 'button', class: 'btn', onclick: () => act.longer(2) }, 'Longer'),
        h('button', { type: 'button', class: 'btn', title: 'Ctrl+D', onclick: () => act.repeat() }, 'Repeat after'),
        sel.some((n) => !isDrumNote(n)) && others.length ? select('Give to', [['', 'another instrument…'], ...others.map((t) => [t.channel, t.name])], '', (v) => { if (v) act.moveTo(Number(v)); }) : null,
        h('button', { type: 'button', class: 'btn', onclick: () => act.del() }, 'Delete')) : null);
    }
    if (!editable) { fill(pending); return; }
    if (!dirty.size) { fill(pending, st.history.length ? h('span', { class: 'meta' }, 'No changes waiting.') : null); return; }
    const bars = [...new Set([...dirty.values()].map((d) => d.bar))].sort((a, b) => a - b);
    const parts = [...new Set([...dirty.values()].map((d) => byCh.get(d.ch)?.name || `channel ${d.ch}`))];
    fill(pending,
      h('div', null, h('b', null, `You changed ${bars.length} bar${bars.length === 1 ? '' : 's'}`), ` (${runs(bars)}) in ${parts.join(', ')}. `, h('span', { class: 'meta' }, 'You hear them here already; they\'re not in the song yet.')),
      h('div', { class: 'seq-acts' },
        h('button', { type: 'button', class: 'btn', onclick: () => { player.load({ ...play, piece: workingPiece() }, { force: true }); player.seek(Math.max(0, ((bars[0] - 2) * bpb * 60) / bpm)); player.play(); } }, 'Hear it'),
        h('button', { type: 'button', class: 'btn primary', onclick: save }, 'Put it in the song'),
        h('button', { type: 'button', class: 'btn link', onclick: () => { const before = snapshot(); st.notes = st.notes.filter((n) => !dirty.has(cellKey(n.ch, barOf(n.start)))).concat(origNotes().filter((n) => dirty.has(cellKey(n.ch, barOf(n.start))))); st.selected = new Set(); commit(before); } }, 'Throw away'),
        h('button', { type: 'button', class: 'btn link', disabled: !st.history.length, onclick: undo }, 'Undo')));
  }
  // The notes as they came in (for "Throw away"): from the piece itself.
  const origNotes = () => tracks.flatMap((t) => t.notes.map((n) => ({ id: nid++, ch: t.channel, pitch: n.pitch, start: n.start, dur: n.dur, vel: n.velocity ?? 0.8 })));
  let saving = false;
  async function save() {
    if (saving || !dirty.size) return;
    saving = true;
    fill(pending, h('div', { class: 'edit-line' }, h('span', { class: 'pulse', 'aria-hidden': 'true' }), ' Writing your notes into the score…'));
    const cells = [...dirty.values()].map((d) => ({ channel: d.ch, bar: d.bar }));
    const notes = st.notes.filter((n) => dirty.has(cellKey(n.ch, barOf(n.start)))).map((n) => ({ channel: n.ch, pitch: n.pitch, start: n.start, dur: n.dur, velocity: n.vel }));
    try { await onSave({ cells, notes, firstBar: Math.min(...cells.map((c) => c.bar)) }); } catch (e) { toast(e.message, true); drawTools(); } finally { saving = false; }
  }

  const box = h('div', { class: 'seq-box' }, nav, h('div', { class: 'seq-stage' }, canvas), selBar, pending, help);
  box.setFocus = (ch) => { focus = ch; layout(); draw(); drawTools(); };
  box.goTo = (bar) => { if (bar - 1 < st.v0 || bar - 1 >= st.v0 + st.vb) go(bar - 1, true); };
  box.piece = workingPiece;
  box.dirty = () => dirty.size;
  layout(); recount();
  requestAnimationFrame(() => { go(st.v0); drawTools(); });
  new ResizeObserver(() => draw()).observe(canvas);
  follow(box, (s) => {
    const mine = s.id === pid && (s.playing || s.time > 0);
    const beat = mine ? (s.time * bpm) / 60 : null;
    if (beat === sounding) return;
    sounding = s.playing && mine ? beat : mine ? beat : null;
    if (s.playing && mine && st.follow && !drag && (beat < st.v0 * bpb || beat >= (st.v0 + st.vb) * bpb)) { go(Math.floor(beat / bpb)); return; }
    draw();
  });
  return box;
}

const runs = (bars) => {
  const out = []; let a = bars[0]; let b = bars[0];
  for (const x of bars.slice(1)) { if (x === b + 1) b = x; else { out.push(a === b ? `${a}` : `${a}-${b}`); a = x; b = x; } }
  out.push(a === b ? `${a}` : `${a}-${b}`);
  return `bar${bars.length === 1 ? '' : 's'} ${out.join(', ')}`;
};
