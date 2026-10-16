// Operations: the only way the song changes (doc 18 §5–6, doc 28 §3.4). The UI, `mfx`, imports and undo all send the
// same ops: { op, ...args, by: 'hand' | 'ai' | 'import', edit? }. Note ops become text edits of the section's row in
// song.strudel.js through the edit ladder (the original kept word for word, the changed bars hard-coded in plain
// Strudel), so the notes always come from the text. Locks keep the AI off what the owner touched or approved; every
// change records its inverse for undo (music/daw/undo.jsonl).
import { existsSync, readFileSync, appendFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { loadSong, saveSong, songPaths, strudelDirOf } from './song-store.mjs';
import { ticksPerBar, sectionLayout } from './song.mjs';
import { parseSongFile, setRow } from './strudel/songfile.mjs';
import { createStrudel } from './strudel/edit.mjs';
import { UserError } from './studio.mjs';

const sha16 = (s) => createHash('sha256').update(s).digest('hex').slice(0, 16);
export const noteName = (n) => `${n.t}:${n.key}:${n.len}`;
const clone = (x) => JSON.parse(JSON.stringify(x));

// One shared ladder per process (its evaluator is a locked-down child process).
let shared = null;
export function ladderFor(root) {
  if (!shared) shared = createStrudel({ strudelDir: strudelDirOf(root), timeoutMs: 4000 });
  return shared;
}
export function closeLadder() { shared?.close(); shared = null; }

// The locks an op would touch (for 'ai' and 'import' ops). Bars are 1-based in messages, ticks inside.
function blockingLocks(song, op, scope) {
  return (song.locks || []).filter((l) => {
    const s = l.scope || {};
    if (scope.pattern && s.pattern && s.pattern !== scope.pattern) return false;
    if (scope.channel && s.channel && s.channel !== scope.channel) return false;
    if (s.fromTick !== undefined && scope.fromTick !== undefined && (scope.toTick <= s.fromTick || scope.fromTick >= s.toTick)) return false;
    if (s.plugin || s.insert || s.automation || s.item || s.controller) return Object.entries(s).every(([k, v]) => scope[k] === v);
    return Boolean(s.pattern || s.channel);
  }).filter((l) => !(op.unlock || []).includes(l.id));
}
function lockWords(song, l) {
  const s = l.scope || {};
  const tpb = ticksPerBar(song.meter[0]);
  const ch = song.channels.find((c) => c.id === s.channel);
  const bars = s.fromTick !== undefined ? ` bars ${s.fromTick / tpb + 1}–${s.toTick / tpb}` : '';
  return `${s.pattern || ''} ${ch?.name || s.channel || ''}${bars} ${l.by === 'ok' ? 'were OK\'d' : 'are hand-edited'} (${l.id})`.replace(/\s+/g, ' ').trim();
}

// Adds a hand lock over whole bars, merged with the pattern × channel's touching locks.
function addHandLock(song, pattern, channel, fromTick, toTick, rev) {
  const near = song.locks.filter((l) => l.by === 'hand' && l.scope.pattern === pattern && l.scope.channel === channel && l.scope.fromTick !== undefined && l.scope.fromTick <= toTick && fromTick <= l.scope.toTick);
  for (const l of near) { fromTick = Math.min(fromTick, l.scope.fromTick); toTick = Math.max(toTick, l.scope.toTick); }
  song.locks = song.locks.filter((l) => !near.includes(l));
  const n = Math.max(0, ...song.locks.map((l) => Number(String(l.id).slice(1)) || 0)) + 1;
  const lock = { id: `L${String(n).padStart(3, '0')}`, scope: { pattern, channel, fromTick, toTick }, by: 'hand', at: new Date().toISOString(), rev };
  song.locks.push(lock);
  return { added: lock, removed: near };
}

// Bars (0-based, in the section) where two note lists differ.
function changedBars(a, b, tpb) {
  const k = (n) => `${n.t}|${n.len}|${n.key}|${n.vel ?? 0.8}`;
  const A = new Set(a.map(k)); const B = new Set(b.map(k));
  const out = new Set();
  for (const n of a) if (!B.has(k(n))) out.add(Math.floor(n.t / tpb));
  for (const n of b) if (!A.has(k(n))) out.add(Math.floor(n.t / tpb));
  return [...out].sort((x, y) => x - y);
}

// Applies a batch. Returns { rev, applied: [ops], refused: [{ op, why }], inverse: [ops] }.
// Lock refusals refuse only that op ("the rest goes through"); any other error refuses the whole batch.
export async function applyOps(root, id, ops, { ladder = null } = {}) {
  const loaded = loadSong(root, id);
  if (!loaded) throw new UserError('no song yet: import the score first (mortiflix music <project> --song)');
  const song = clone(loaded.song);
  let text = loaded.text;
  const tpb = ticksPerBar(song.meter[0]);
  const layout = sectionLayout(song);
  const rowOf = (pattern) => layout.findIndex((x) => x.id === pattern);
  const applied = []; const refused = []; const inverse = [];
  const rev = song.rev + 1;

  for (const op0 of ops) {
    const op = { by: 'hand', ...op0 };
    if (!['hand', 'ai', 'import'].includes(op.by)) throw new UserError(`"by" must be hand, ai or import (got ${op.by})`);

    // ---- notes: through the ladder into the row's text ----
    if (['note.add', 'note.set', 'note.del', 'strudel.set'].includes(op.op)) {
      const [pattern, channel] = op.op === 'strudel.set' ? String(op.region || '').split('.') : [op.pattern, op.channel];
      const p = song.patterns.find((x) => x.id === pattern);
      const c = song.channels.find((x) => x.id === channel);
      if (!p || !c) throw new UserError(`there is no pattern ${pattern} with channel ${channel}`);
      const row = rowOf(pattern);
      const block = parseSongFile(text).blocks.find((b) => b.channel === c.midiChannel);
      if (!block || !block.rows[row]) throw new UserError(`song.strudel.js has no row for ${pattern} in ${c.name}`);
      const before = block.rows[row].text;
      // the file leaves defaults out: velocity 0.8 is put back before comparing
      const old = (p.notes[channel] || []).map((n) => ({ ...n, vel: n.vel ?? 0.8 }));
      let wanted = null; let after = null;
      if (op.op === 'strudel.set') {
        if (op.before !== undefined && op.before !== before) throw new UserError(`${pattern} ${c.name} changed since it was read: read it again`);
        after = String(op.after);
      } else {
        const byName = new Map(old.map((n) => [noteName(n), n]));
        if (op.op === 'note.add') wanted = [...old, ...(op.notes || []).map((n) => ({ t: n.t, len: n.len, key: n.key, vel: n.vel ?? 0.8 }))];
        if (op.op === 'note.del') { for (const nm of op.ids || []) if (!byName.has(nm)) throw new UserError(`${pattern} ${c.name} has no note ${nm}`); wanted = old.filter((n) => !(op.ids || []).includes(noteName(n))); }
        if (op.op === 'note.set') {
          for (const nm of op.ids || []) if (!byName.has(nm)) throw new UserError(`${pattern} ${c.name} has no note ${nm}`);
          wanted = old.map((n) => ((op.ids || []).includes(noteName(n)) ? { ...n, ...op.fields } : n));
        }
        for (const n of wanted) {
          if (!Number.isInteger(n.t) || n.t < 0 || n.t >= p.bars * tpb) throw new UserError(`a note at tick ${n.t} is outside ${pattern} (${p.bars} bars)`);
          if (!Number.isInteger(n.key) || n.key < 0 || n.key > 127) throw new UserError(`key ${n.key} is outside MIDI's 0–127`);
          if (!Number.isInteger(n.len) || n.len < 1) throw new UserError('a note needs a length of at least 1 tick');
        }
      }
      const bars = wanted ? changedBars(old, wanted, tpb) : null;
      if (op.by !== 'hand') {
        const scope = { pattern, channel, ...(bars?.length ? { fromTick: bars[0] * tpb, toTick: (bars.at(-1) + 1) * tpb } : {}) };
        const hit = blockingLocks(song, op, scope);
        if (hit.length) { refused.push({ op, why: hit.map((l) => lockWords(song, l)).join('; ') }); continue; }
      }
      let notes;
      const lad = ladder || ladderFor(root);
      if (wanted) {
        const r = await lad.edit(before, wanted, { bars: p.bars, tpb });
        after = r.code; notes = r.notes;
      } else {
        notes = (await lad.notes(after, { bars: p.bars, tpb })).notes;
      }
      notes = notes.map(({ t, len, key, vel, fine }) => ({ t, len, key, vel, ...(fine ? { fine } : {}) }));
      const touched = bars || changedBars(old, notes, tpb);
      text = setRow(text, c.midiChannel, row, after);
      if (notes.length) p.notes[channel] = notes; else delete p.notes[channel];
      song.strudel.regions[`${pattern}.${channel}`] = { sha: sha16(after) };
      inverse.unshift({ op: 'strudel.set', region: `${pattern}.${channel}`, before: after, after: before, by: 'hand', undo: true });
      if (op.by === 'hand' && !op.undo && touched.length) {
        const { added, removed } = addHandLock(song, pattern, channel, touched[0] * tpb, (touched.at(-1) + 1) * tpb, rev);
        inverse.unshift({ op: 'lock.restore', remove: added.id, put: removed, undo: true });
      }
      applied.push({ ...op, region: `${pattern}.${channel}`, before, after });
      continue;
    }

    // ---- locks ----
    if (op.op === 'lock.add') {
      const n = Math.max(0, ...song.locks.map((l) => Number(String(l.id).slice(1)) || 0)) + 1;
      const lock = { id: `L${String(n).padStart(3, '0')}`, scope: op.scope, by: op.lockBy || 'ok', at: new Date().toISOString(), rev };
      song.locks.push(lock);
      inverse.unshift({ op: 'lock.del', id: lock.id, undo: true });
      applied.push({ ...op, id: lock.id });
      continue;
    }
    if (op.op === 'lock.del') {
      const l = song.locks.find((x) => x.id === op.id);
      if (!l) throw new UserError(`there is no lock ${op.id}`);
      song.locks = song.locks.filter((x) => x !== l);
      inverse.unshift({ op: 'lock.restore', remove: null, put: [l], undo: true });
      applied.push(op);
      continue;
    }
    if (op.op === 'lock.restore') {
      song.locks = song.locks.filter((x) => x.id !== op.remove).concat(op.put || []);
      applied.push(op);
      continue;
    }

    // ---- fields: patterns, channels, playlist items, mixer inserts, markers, maps ----
    const lists = { 'pattern.set': song.patterns, 'channel.set': song.channels, 'item.set': song.playlist.items, 'insert.set': song.mixer.inserts, 'marker.set': song.markers };
    if (lists[op.op]) {
      const x = lists[op.op].find((y) => y.id === op.id);
      if (!x) throw new UserError(`there is no ${op.op.split('.')[0]} ${op.id}`);
      if (op.by !== 'hand') {
        const kind = op.op.split('.')[0];
        const hit = blockingLocks(song, op, kind === 'pattern' ? { pattern: op.id } : kind === 'channel' ? { channel: op.id } : { [kind]: op.id });
        if (hit.length) { refused.push({ op, why: hit.map((l) => lockWords(song, l)).join('; ') }); continue; }
      }
      const prev = Object.fromEntries(Object.keys(op.fields || {}).map((k) => [k, x[k] === undefined ? null : clone(x[k])]));
      if ('notes' in (op.fields || {}) || 'id' in (op.fields || {})) throw new UserError('notes and ids are not changed this way');
      for (const [k, v] of Object.entries(op.fields || {})) { if (v === null) delete x[k]; else x[k] = v; }
      inverse.unshift({ op: op.op, id: op.id, fields: prev, by: 'hand', undo: true });
      applied.push(op);
      continue;
    }
    if (op.op === 'marker.add') {
      const n = Math.max(0, ...song.markers.map((m) => Number(String(m.id).replace(/\D/g, '')) || 0)) + 1;
      const m = { id: `k${String(n).padStart(2, '0')}`, kind: 'marker', from: op.by === 'hand' ? 'owner' : 'ai', ...op.marker };
      song.markers.push(m);
      inverse.unshift({ op: 'marker.del', id: m.id, undo: true });
      applied.push({ ...op, id: m.id });
      continue;
    }
    if (op.op === 'marker.del') {
      const m = song.markers.find((x) => x.id === op.id);
      if (!m) throw new UserError(`there is no marker ${op.id}`);
      song.markers = song.markers.filter((x) => x !== m);
      inverse.unshift({ op: 'marker.add', marker: m, by: 'hand', undo: true });
      applied.push(op);
      continue;
    }
    if (op.op === 'loop.set' || op.op === 'tempo.set' || op.op === 'meter.set') {
      const key = op.op.split('.')[0];
      inverse.unshift({ op: op.op, value: clone(song[key]), by: 'hand', undo: true });
      song[key] = op.value;
      applied.push(op);
      continue;
    }
    throw new UserError(`unknown operation "${op.op}"`);
  }

  if (!applied.length) return { rev: song.rev, applied, refused, inverse: [] };
  const saved = saveSong(root, id, song, text);
  // the undo log: one line per batch, newest last
  const sp = songPaths(root, id);
  mkdirSync(join(sp.music, 'daw'), { recursive: true });
  if (!ops.every((o) => o.undo)) appendFileSync(join(sp.music, 'daw', 'undo.jsonl'), JSON.stringify({ rev: saved, at: new Date().toISOString(), ops: inverse }) + '\n');
  return { rev: saved, applied, refused, inverse };
}

// Undo the last batch: its inverse ops, applied by hand (adding no locks of their own).
export async function undo(root, id, opts = {}) {
  const f = join(songPaths(root, id).music, 'daw', 'undo.jsonl');
  const lines = existsSync(f) ? readFileSync(f, 'utf8').split('\n').filter(Boolean) : [];
  if (!lines.length) throw new UserError('nothing to undo');
  const last = JSON.parse(lines.pop());
  const r = await applyOps(root, id, last.ops.map((o) => ({ ...o, undo: true })), opts);
  writeFileSync(f, lines.length ? lines.join('\n') + '\n' : '');
  return r;
}
