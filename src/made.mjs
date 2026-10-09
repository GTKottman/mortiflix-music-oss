// What a step made, for the owner to read: every step, reviewed or not. A step's `work` tags (pipeline.json) say what
// kind of work it does, and each tag knows where that work lands in the project folder and how to say it in plain
// words: the genre research, the plan, the score and its check, the MIDI pack, the instruments, the audio.
//
//   madeBy(root, id, stepKey) → { step: { key, name, describe, state, note, checks }, summary: [lines], pieces: [...] }
//
// A piece is { kind: 'markdown' | 'notes' | 'audio' | 'files' | 'table' | 'code' | 'facts', title, … }. Files are
// served from the project folder through `madeFile` (only readable music outputs: .md, .mid, .wav, .mp3).
import { existsSync, readFileSync, readdirSync, statSync, realpathSync } from 'node:fs';
import { join, extname, resolve } from 'node:path';
import { isInside } from './studio.mjs';
import { projectPaths, projectPipeline, loadProject } from './projects.mjs';
import * as gates from './gates.mjs';

const readJ = (f) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return null; } };
const readT = (f) => { try { return readFileSync(f, 'utf8'); } catch { return null; } };
const mmss = (s) => `${Math.floor(s / 60)}:${String(Math.round(s % 60)).padStart(2, '0')}`;
const list = (xs, n = 6) => (xs.length > n ? `${xs.slice(0, n).join(', ')} and ${xs.length - n} more` : xs.join(', '));

export const MADE_FILE = /^(music|out\/music)\/[\w ./-]+\.(md|mid|wav|mp3)$/;

export function madeBy(root, id, stepKey) {
  const pp = projectPaths(root, id);
  const pipeline = projectPipeline(root, id);
  const def = pipeline.steps.find((s) => s.key === stepKey);
  if (!def) throw new Error(`no step ${stepKey}`);
  const project = loadProject(root, id);
  const view = gates.stepView(project, pipeline).find((s) => s.key === stepKey);
  const rec = project.steps?.[stepKey] || {};
  const w = (f) => join(pp.work, f);
  const summary = []; const pieces = [];
  const file = (rel) => (existsSync(w(rel)) ? rel : null);

  for (const tag of def.work || []) {
    if (tag === 'research') {
      const g = readJ(w('music/genre.json'));
      if (g) {
        summary.push(`${g.name || 'The genre'}${g.summary ? `: ${g.summary}` : ''}`);
        summary.push([g.tempo ? `${g.tempo.min}-${g.tempo.max} BPM (usually ${g.tempo.typical})` : null, (g.meters || []).join(', ') || null,
          g.length ? `tracks run ${mmss(g.length.min_sec)}-${mmss(g.length.max_sec)}` : null].filter(Boolean).join(' · '));
        if (g.lineup?.length) summary.push(`Its usual band: ${list(g.lineup.map((r) => r.role), 8)}.`);
        if (g.unforgettable?.length) summary.push(`What makes it unforgettable: ${list(g.unforgettable.map((e) => e.element), 5)}.`);
        if (g.sources?.length) summary.push(`Researched from ${g.sources.length} sources.`);
      }
      if (file('music/GENRE.md')) pieces.push({ kind: 'markdown', title: 'The genre research', file: 'music/GENRE.md', text: readT(w('music/GENRE.md')) });
      if (g?.sources?.length) pieces.push({ kind: 'links', title: 'Sources read', links: g.sources.filter((s) => /^https?:\/\//.test(s.url || '')).map((s) => ({ url: s.url, title: s.title || s.url })) });
    }
    if (tag === 'plan') {
      const b = readJ(w('music/blueprint.json'));
      if (b) {
        const bpb = (() => { const [n, d] = String(b.meter || '4/4').split('/').map(Number); return (n * 4) / d; })();
        summary.push(`"${b.title || project.title}": ${b.bars} bars at ${b.bpm} BPM in ${b.meter || '4/4'}, ${b.scale || ''}${b.bars && b.bpm ? `, ${mmss((b.bars * bpb * 60) / b.bpm)} long` : ''}.`);
        if (b.dramatic_reading) summary.push(`Its story: ${b.dramatic_reading}.`);
        if (b.roles?.length) summary.push(`${b.roles.length} parts: ${list(b.roles.map((r) => r.name), 10)}.`);
        if (b.sections?.length) summary.push(`${b.sections.length} sections: ${list(b.sections.map((s) => s.name), 8)}.`);
        if (b.techniques?.length) summary.push(`Techniques: ${list(b.techniques.map((t) => t.name), 8)}.`);
      }
      const md = file('music/BLUEPRINT.md');
      if (md) pieces.push({ kind: 'markdown', title: 'The blueprint', file: md, text: readT(w(md)) });
    }
    if (tag === 'music') {
      const c = readJ(w('music/check.json'));
      const notes = readJ(w('music/notes.json'));
      if (c) {
        summary.push(c.ok ? 'The score passed its check: the harmony is locked, the energy follows the story, and every planned technique is in the notes.' : `The score's check found ${c.errors.length} problem${c.errors.length === 1 ? '' : 's'}.`);
        if (c.parts?.length) summary.push(`${c.parts.length} parts, ${c.parts.reduce((s, p) => s + p.notes, 0)} notes: ${list(c.parts.map((p) => `${p.name} (${p.notes})`), 10)}.`);
        const heard = (c.techniques || []).filter((t) => t.found);
        if (heard.length) summary.push(`Techniques heard in the notes: ${list(heard.map((t) => t.name), 8)}.`);
        if (c.intensity?.length) pieces.push({ kind: 'table', title: 'Energy, section by section: what the plan asked (0-10) and what the notes measure', head: ['Section', 'Bars', 'Asked', 'Measured'],
          rows: c.intensity.map((s) => [s.name, s.bars ? `${s.bars[0]}-${s.bars[1]}` : '', String(s.asked ?? ''), String(s.measured ?? '')]) });
        if (c.warnings?.length || c.errors?.length) pieces.push({ kind: 'facts', title: 'What the check said', facts: [...(c.errors || []).map((e) => `✖ ${e}`), ...(c.warnings || []).map((x) => `! ${x}`)] });
      }
      if (notes) pieces.push({ kind: 'notes', title: 'The notes', notes: true });
      const score = file('music/score.strudel.js');
      if (score) pieces.push({ kind: 'code', title: 'The score (Strudel)', text: readT(w(score)) });
    }
    if (tag === 'midi') {
      const dir = w('out/music/midi');
      if (existsSync(dir)) {
        const all = walk(dir);
        const sections = all.filter((f) => f.startsWith('sections/') && f.endsWith('-all.mid'));
        summary.push(`The MIDI pack: ${all.filter((f) => !f.includes('/') && f.endsWith('.mid')).length} files at the top (the whole arrangement and every channel on its own)${sections.length ? `, plus a clip for each of ${sections.length} sections` : ''}.`);
        pieces.push({ kind: 'files', title: 'The MIDI pack', files: all.filter((f) => f.endsWith('.mid') || f.endsWith('.md')).map((f) => `out/music/midi/${f}`), zip: existsSync(w('out/music/midi.zip')) });
        const cue = file('out/music/midi/CUE-SHEET.md');
        if (cue) pieces.push({ kind: 'markdown', title: 'The cue sheet', file: cue, text: readT(w(cue)) });
      }
    }
    if (tag === 'instruments') {
      const inst = readJ(w('music/instruments.json'));
      if (inst?.parts?.length) {
        if (inst.concept) summary.push(`The sound: ${inst.concept}`);
        pieces.push({ kind: 'table', title: 'A sound for every channel', head: ['Channel', 'Part', 'Sound', 'Level'],
          rows: inst.parts.map((p) => [String(p.channel), p.role || '', p.kit ? `drum kit: ${Object.entries(p.kit).map(([k, v]) => `${k} ${v.sound}`).join(', ')}` : `${p.sound}${p.set || ''}`, String(p.gain ?? 0.8)]) });
      }
      const chk = readJ(w('music/instruments-check.json'));
      if (chk?.warnings?.length) pieces.push({ kind: 'facts', title: 'Notes on the instruments', facts: chk.warnings.map((x) => `! ${x}`) });
      const aud = w('out/music/audition');
      if (existsSync(aud)) pieces.push({ kind: 'audio', title: 'Audition: every instrument\'s lowest, middle and highest note', files: walk(aud).filter((f) => f.endsWith('.wav')).map((f) => `out/music/audition/${f}`) });
    }
    if (tag === 'audio') {
      const m = readJ(w('music/master.json'));
      if (m?.master) summary.push(`Mastered: ${m.master.lufs} LUFS, true peak ${m.master.true_peak} dBTP, ${mmss(m.master.seconds || 0)}.`);
      const mixes = ['out/music/master.mp3', 'out/music/master.wav', 'out/music/score.wav'].filter((f) => existsSync(w(f)));
      const first = mixes.find((f) => f.endsWith('.mp3')) || mixes[0];
      if (first) pieces.push({ kind: 'audio', title: first.includes('master') ? 'The master' : 'The sketch (the score on its stand-in sounds)', files: [first] });
      const stems = w('out/music/stems');
      if (existsSync(stems)) pieces.push({ kind: 'audio', title: 'Stems: every channel on its own', files: walk(stems).filter((f) => f.endsWith('.wav')).map((f) => `out/music/stems/${f}`) });
    }
  }

  // A reviewed step: what was sent, version by version, is in the review room.
  const subs = gates.submissions(root, id).filter((s) => s.step === stepKey);
  return {
    step: { key: def.key, name: def.name, describe: def.describe || null, review: def.review, state: view?.state || null, done_at: rec.done_at || null, note: rec.note || subs.at(-1)?.note || null,
      checks: (rec.error_checks || subs.at(-1)?.error_checks || []).map((c) => ({ id: c.id, result: c.result, note: c.note || null })), versions: subs.map((s) => s.version) },
    summary: summary.filter(Boolean), pieces,
  };
}

function walk(dir, pre = '') {
  return readdirSync(dir, { withFileTypes: true }).flatMap((d) => (d.isDirectory() ? walk(join(dir, d.name), `${pre}${d.name}/`) : [`${pre}${d.name}`])).sort();
}

// A readable output from the project folder, or null: only music outputs (MADE_FILE), resolved inside the folder.
export function madeFile(root, id, rel) {
  if (!MADE_FILE.test(rel) || rel.includes('..')) return null;
  const base = realpathSync(projectPaths(root, id).work);
  const f = resolve(base, rel);
  if (!isInside(base, f) || !existsSync(f)) return null;
  const real = realpathSync(f);
  if (!isInside(base, real) || !statSync(real).isFile()) return null;
  return real;
}

export const madeKind = (f) => ({ '.md': 'text', '.mid': 'midi', '.wav': 'audio', '.mp3': 'audio' }[extname(f).toLowerCase()]);
