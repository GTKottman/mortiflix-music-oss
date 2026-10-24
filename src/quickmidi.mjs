// The MIDI studio's stages (Design Docs/MIDI Studio - Plan v1.md): the AI makes the musical decisions in one small call
// per stage (no tools, never a session), code writes and checks every note. The notes are plain Strudel: harmony parts
// play chord tones by index (`n("0 2 1").chord("<Am7 F>").voicing()`), melodies play scale degrees
// (`n("0 2 4").scale("A4:minor")`), so pitch is right by construction and a gate only asks about feel. Arrange, Feel and
// Export are code only (no tokens). Every call's tokens are added to the project's usage; a song stops at its budget.
import { runAs } from './models.mjs';
import { execFile } from 'node:child_process';
import { existsSync, readFileSync, writeFileSync, appendFileSync, mkdirSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { paths } from './studio.mjs';
import { loadProject, projectPaths, projectPipeline } from './projects.mjs';
import { stepView, submit, submissions } from './gates.mjs';
import { chatHistory } from './svc/chat.mjs';
import { createEvaluator } from './strudel/evaluator.mjs';
import { strudelDirOf } from './song-store.mjs';
import { writeSmf, PPQ } from './midi/smf.mjs';
import { scaleOf, chordTones, inScale, strudelProgression, strudelChord, STRUDEL_MODE, MODES } from './midi/theory.mjs';

export const MIDI_STEPS = ['questions', 'idea', 'genre', 'song-map', 'chords', 'shapes', 'melody', 'arrange', 'feel', 'export'];
// Tokens a song may use (input incl. cache reads + output) before the studio stops it; config.midiTokenBudget changes it.
export const TOKEN_BUDGET = 1_500_000;
const ROLES_FOR = { questions: 'brief', idea: 'brief', genre: 'brief', 'song-map': 'plan', chords: 'plan', shapes: 'plan', melody: 'plan' };
const MAX_FIXES = 2;

export const MIDI_QUESTIONS = [
  { id: 'purpose', text: 'What is the song for?' },
  { id: 'feel', text: 'What should it feel like?' },
  { id: 'length', text: 'How long should it be?' },
  { id: 'like', text: 'A song it should sound like?' },
];

const readJson = (f, d = null) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return d; } };
const mp = (root, id) => { const d = join(projectPaths(root, id).work, 'midi'); mkdirSync(d, { recursive: true }); return d; };
const tpbOf = (meter) => (meter === '3/4' ? 3 : 4) * PPQ;

// The MIDI step to do now, or null.
export function quickMidiStep(root, id) {
  const pipeline = projectPipeline(root, id);
  if (!pipeline.steps.some((s) => s.key === 'song-map')) return null;
  const runnable = stepView(loadProject(root, id), pipeline).filter((s) => ['ready', 'working', 'changes'].includes(s.state));
  return runnable.length === 1 && MIDI_STEPS.includes(runnable[0].key) ? runnable[0] : null;
}

// ---- what the AI sees -------------------------------------------------------------------------------------------

function ownerWords(root, id) {
  const project = loadProject(root, id);
  const parts = [`## The song's title (the owner's: keep it exactly)\n${project.title}`, `## The owner's idea (the new-project form)\n${JSON.stringify(project.intake?.answers || {}, null, 1)}`];
  const chat = chatHistory(root, id, { limit: 500 }).filter((r) => ['user', 'action'].includes(r.role) && !r.edit).slice(-30);
  if (chat.length) parts.push(`## The owner's words in the chat (newest last; they win over the form)\n${chat.map((r) => `- ${r.text}`).join('\n')}`);
  const subs = submissions(root, id);
  const answers = (subs.filter((s) => s.step === 'questions').at(-1)?.feedback?.answers || []).map((a) => `- ${a.text} → ${a.answer}`);
  if (answers.length) parts.push(`## The owner's answers\n${answers.join('\n')}`);
  return parts;
}

// The approved stages before this one, short.
function sofar(root, id, upTo) {
  const d = mp(root, id);
  const out = [];
  const add = (step, title, f, pick = (x) => x) => {
    if (MIDI_STEPS.indexOf(step) >= MIDI_STEPS.indexOf(upTo)) return;
    const j = readJson(join(d, f));
    if (j) out.push(`## ${title} (approved)\n${JSON.stringify(pick(j))}`);
  };
  add('idea', 'The brief', 'brief.json');
  add('genre', 'The genre profile', 'genre.json');
  add('song-map', 'The song map', 'songmap.json');
  add('chords', 'The chords per kind of section', 'chords.json');
  add('shapes', 'The shapes per part', 'shapes.json', (j) => j.parts.map((p) => ({ part: p.part, picked: p.shapes[p.picked]?.code })));
  return out;
}

// The owner's notes on the last version when they asked for changes.
function changesAsked(root, id, view, file) {
  if (view.state !== 'changes') return [];
  const last = submissions(root, id).filter((s) => s.step === view.key).at(-1);
  if (!last) return [];
  const prev = existsSync(file) ? readFileSync(file, 'utf8').slice(0, 20000) : last.note;
  return [`## Version ${last.version} (the owner asked for changes)\n${prev}\n\n## The owner's notes on it\n${last.feedback?.overall ? `Overall: ${last.feedback.overall}\n` : ''}${(last.feedback?.notes || []).map((n) => `${n.n}. ${n.text}`).join('\n')}\nChange what the notes ask for and keep the rest.`];
}

const STRUDEL_RULES = `Notes are written in plain Strudel only (no helper functions, no setcps, no samples beyond drum names, no .s() sounds on pitched parts). One Strudel cycle is one bar.
- Drums: s("bd ~ sd ~, hh*8") style mini-notation with these names only: bd sd rim cp hh oh lt mt ht cr rd cb sh perc. Use .velocity("…") for accents and ghost notes. Use <…> alternation for patterns longer than a bar.
- Harmony parts (chords, pads, keys, arps, bass): never name notes. Write the token <CHORDS> where the chord progression goes; the studio puts the section's progression there. Chords: chord("<CHORDS>").voicing() with .struct("…") for the rhythm. Arps and riffs: n("0 1 2 3").chord("<CHORDS>").voicing() (n picks chord tones by index, 0 = lowest). Bass: chord("<CHORDS>").rootNotes(2).struct("x ~ x [~ x]") or n("0 ~ 0 1").chord("<CHORDS>").mode("root").anchor("C2").voicing().
- Melodies: n("0 2 4 [5 4] 2 ~ 1 0").scale("<SCALE>") with scale degrees (0 = the tonic, negative numbers go below); the studio puts the song's scale with its octave there.
- Mini-notation: ~ rest, [a b] subdivide, a*2 repeat, a!2 replicate, a@3 lengthen, <a b> one per bar, a,b layers, a? sometimes (avoid ? in melodies).`;

// ---- schemas ------------------------------------------------------------------------------------------------------

const QSCHEMA = { type: 'object', required: MIDI_QUESTIONS.map((q) => q.id), properties: Object.fromEntries(MIDI_QUESTIONS.map((q) => [q.id, { type: 'string', description: `a suggested answer to "${q.text}", one short plain sentence` }])) };
const BRIEF = {
  type: 'object', required: ['title', 'summary', 'purpose', 'genre', 'feel', 'length_sec', 'like', 'must', 'never', 'note'],
  properties: {
    title: { type: 'string', description: 'the song\'s title exactly as the owner named it' }, summary: { type: 'string', description: 'one sentence: what the song is' },
    purpose: { type: 'string' }, genre: { type: 'string', description: 'the genre in a few words' }, feel: { type: 'string' },
    length_sec: { type: 'integer', minimum: 30, maximum: 600 }, like: { type: 'string', description: 'songs or artists it should sound like (never copy them)' },
    must: { type: 'array', items: { type: 'string' }, description: 'what the song must have' }, never: { type: 'array', items: { type: 'string' }, description: 'what it must never do' },
    note: { type: 'string', description: 'for the owner: what to check before pressing Next' },
  },
};
const GENRE = {
  type: 'object', required: ['name', 'summary', 'bpm_min', 'bpm_max', 'keys', 'meter', 'swing', 'drums', 'bass', 'harmony', 'melody', 'instruments', 'form', 'moves', 'sources', 'note'],
  properties: {
    name: { type: 'string' }, summary: { type: 'string', description: 'two plain sentences' }, bpm_min: { type: 'integer', minimum: 50, maximum: 200 }, bpm_max: { type: 'integer', minimum: 50, maximum: 200 },
    keys: { type: 'array', items: { type: 'string' }, description: 'typical keys like "A minor", "F dorian"' }, meter: { type: 'string', enum: ['4/4', '3/4'] },
    swing: { type: 'number', minimum: 0, maximum: 0.5, description: '0 = straight, 0.33 = triplet swing on the off 8ths/16ths' },
    drums: { type: 'string', description: 'the typical drum pattern in words' }, bass: { type: 'string' }, harmony: { type: 'string', description: 'typical chords and progressions' }, melody: { type: 'string' },
    instruments: { type: 'array', items: { type: 'object', required: ['role', 'sound', 'program'], properties: { role: { type: 'string' }, sound: { type: 'string' }, program: { type: 'integer', minimum: 0, maximum: 127, description: 'General MIDI program, 0-based' } } } },
    form: { type: 'string', description: 'the usual song form and section lengths in bars' }, moves: { type: 'array', items: { type: 'string' }, description: 'signature moves of the genre' },
    sources: { type: 'array', items: { type: 'string' }, description: 'where this comes from: well-known tracks, artists, books (from what you know; no links)' }, note: { type: 'string' },
  },
};
const ROLE_ENUM = ['drums', 'perc', 'bass', 'chords', 'pad', 'keys', 'arp', 'lead', 'counter'];
const MAP = {
  type: 'object', required: ['title', 'bpm', 'tonic', 'mode', 'meter', 'parts', 'sections', 'note'],
  properties: {
    title: { type: 'string' }, bpm: { type: 'integer', minimum: 50, maximum: 200 }, tonic: { type: 'string', description: 'C, F#, Bb …' }, mode: { type: 'string', enum: Object.keys(MODES) }, meter: { type: 'string', enum: ['4/4', '3/4'] },
    parts: { type: 'array', minItems: 6, items: { type: 'object', required: ['id', 'name', 'role', 'program', 'low', 'high'], properties: { id: { type: 'string', description: 'short id like bass, keys, lead' }, name: { type: 'string' }, role: { type: 'string', enum: ROLE_ENUM }, program: { type: 'integer', minimum: 0, maximum: 127 }, low: { type: 'integer', minimum: 20, maximum: 108, description: 'lowest MIDI note' }, high: { type: 'integer', minimum: 20, maximum: 108 } } } },
    sections: { type: 'array', minItems: 3, items: { type: 'object', required: ['name', 'kind', 'bars', 'energy', 'parts'], properties: { name: { type: 'string', description: 'e.g. Verse 1' }, kind: { type: 'string', description: 'intro, verse, pre, chorus, bridge, break, drop, outro …: sections of the same kind share chords and melody' }, bars: { type: 'integer', minimum: 2, maximum: 32 }, energy: { type: 'integer', minimum: 1, maximum: 5 }, parts: { type: 'array', items: { type: 'string' }, description: 'ids of the parts that play' } } } },
    note: { type: 'string' },
  },
};
const CHORDS = {
  type: 'object', required: ['kinds', 'note'],
  properties: {
    kinds: { type: 'array', items: { type: 'object', required: ['kind', 'chords', 'bars_per_chord'], properties: { kind: { type: 'string' }, chords: { type: 'array', items: { type: 'string' }, description: 'chord symbols like Am7 Fmaj7 C G Dm9 E7 Bbmaj7 Gsus4 (no slash chords: the bass note is dropped)' }, bars_per_chord: { type: 'integer', enum: [1, 2] }, borrowed: { type: 'array', items: { type: 'string' }, description: 'chords outside the key, on purpose' }, why: { type: 'string' } } } },
    note: { type: 'string' },
  },
};
const SHAPES = {
  type: 'object', required: ['parts', 'note'],
  properties: {
    parts: { type: 'array', items: { type: 'object', required: ['part', 'shapes', 'suggested'], properties: { part: { type: 'string' }, shapes: { type: 'array', minItems: 2, maxItems: 3, items: { type: 'object', required: ['name', 'code', 'why'], properties: { name: { type: 'string', description: 'two or three words' }, code: { type: 'string' }, why: { type: 'string' } } } }, suggested: { type: 'integer', minimum: 0, description: 'index of the shape you suggest' } } } },
    note: { type: 'string' },
  },
};
const MELODY = {
  type: 'object', required: ['kinds', 'octave', 'note'],
  properties: {
    octave: { type: 'integer', minimum: 3, maximum: 6, description: 'the octave of the scale\'s tonic for the lead' },
    kinds: { type: 'array', items: { type: 'object', required: ['kind', 'code', 'why'], properties: { kind: { type: 'string' }, code: { type: 'string', description: 'n("…").scale("<SCALE>") over the whole section (use <…> for bars)' }, why: { type: 'string' } } } },
    note: { type: 'string' },
  },
};
const FIX = { type: 'object', required: ['answer'], properties: { answer: { type: 'object', description: 'the corrected JSON, same shape as before' } } };

// ---- calls and budget ---------------------------------------------------------------------------------------------

// What a fix round is about, in the journal (to see where tokens go).
function fixLog(ctx, what, problems) {
  appendFileSync(join(projectPaths(ctx.root, ctx.id).work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · ${what}: fix round for ${problems.length} problem${problems.length === 1 ? '' : 's'}: ${problems.slice(0, 8).join('; ').slice(0, 900)}\n`);
}

async function ask(ctx, role, prompt, schema) {
  const { run, runArgs, config } = ctx;
  if (config.backend !== 'claude-code') return { ok: false, error: 'the MIDI studio runs on Claude Code: link it in Setup' };
  const r = await runAs(run, role, { ...runArgs, prompt, config: { ...config, claudeArgs: ['--tools', '', '--json-schema', JSON.stringify(schema)] } });
  ctx.spent.input += r.usage?.input_tokens || 0;
  ctx.spent.output += r.usage?.output_tokens || 0;
  ctx.calls += 1;
  return r;
}

export function usedTokens(root, id) {
  const u = loadProject(root, id).usage || {};
  return (u.input_tokens || 0) + (u.output_tokens || 0);
}

// ---- notes --------------------------------------------------------------------------------------------------------

let EV = null;
function evaluator(root) {
  if (!EV) EV = createEvaluator({ strudelDir: strudelDirOf(root), timeoutMs: 8000 });
  return EV;
}

// Stop the Strudel process (tests, and the service on exit).
export function closeMidi() {
  EV?.close();
  EV = null;
}

export const fill = (code, { chords, scale }) => String(code).replaceAll('<CHORDS>', chords ?? '<C>').replaceAll('<SCALE>', scale ?? 'C4:major');
const scaleText = (map, octave) => `${map.tonic}${octave}:${STRUDEL_MODE[map.mode]}`;

// The chord symbol sounding at tick t of a section's progression.
function chordAt(prog, t, tpb) {
  const flat = prog.chords.flatMap((c) => Array(prog.bars_per_chord || 1).fill(c));
  return flat[Math.floor(t / tpb) % flat.length];
}

// Problems with one part's notes in one section: chord tones for harmony parts, scale tones for melodies, the range.
export function noteProblems(notes, { role, part, map, prog, tpb, voiced = {} }) {
  const out = [];
  const scale = scaleOf(map.tonic, map.mode);
  if (role === 'drums' || role === 'perc') return out;
  let off = 0;
  let strongOff = 0;
  let strong = 0;
  for (const n of notes) {
    const sym = chordAt(prog, n.t, tpb);
    const ct = [...(chordTones(sym)?.tones || []), ...(voiced[sym] || [])];
    const pc = ((n.key % 12) + 12) % 12;
    if (['lead', 'counter'].includes(role)) {
      if (!inScale(n.key, scale) && !ct.includes(pc)) off += 1;
      if (n.t % (tpb / (tpb === 3 * PPQ ? 3 : 2)) === 0) { strong += 1; if (!ct.includes(pc)) strongOff += 1; }
    } else if (!ct.includes(pc)) off += 1;
  }
  if (off) out.push(`${part.id}: ${off} note${off === 1 ? '' : 's'} outside ${['lead', 'counter'].includes(role) ? 'the scale' : 'the chord'}`);
  if (strong >= 4 && strongOff / strong > 0.5) out.push(`${part.id}: only ${Math.round((1 - strongOff / strong) * 100)}% of the strong-beat notes are chord tones (aim for most)`);
  return out;
}

// Octaves moved so a part stays inside its instrument's range (by construction, not a problem to report).
export function fitRange(notes, part) {
  const lo = Math.min(part.low, part.high);
  const hi = Math.max(part.low, part.high);
  return notes.map((n) => {
    let k = n.key;
    while (k < lo && k + 12 <= 127) k += 12;
    while (k > hi && k - 12 >= 0) k -= 12;
    return k === n.key ? n : { ...n, key: k };
  });
}

// The pitch classes Strudel's own voicing plays for each chord symbol (so a tone it voices is never "outside the chord").
const VOICED = new Map();
export async function voicedTones(root, symbols) {
  const out = {};
  for (const sym of new Set(symbols)) {
    if (!VOICED.has(sym)) {
      const r = await evaluator(root).run(`chord("${strudelChord(sym)}").voicing()`, { bars: 1, ms: 8000 }).catch(() => null);
      VOICED.set(sym, r?.ok ? [...new Set(r.notes.map((n) => ((n.key % 12) + 12) % 12))] : []);
    }
    out[sym] = VOICED.get(sym);
  }
  return out;
}

async function notesOf(root, code, { bars, tpb, drums }) {
  const r = await evaluator(root).run(code, { bars, tpb, drums, ms: 8000 });
  if (!r.ok) return { error: String(r.error || 'it did not run').slice(0, 300) };
  // notes between grid ticks (triplets, quintuplets) are only rounded to the nearest tick: not a problem to fix
  return { notes: r.notes.map(({ t, len, key, vel }) => ({ t, len, key, vel })), problems: (r.problems || []).filter((p) => !/off the \d+ grid/.test(p)) };
}

// ---- documents ----------------------------------------------------------------------------------------------------

const list = (a) => (a || []).map((x) => `- ${x}`).join('\n');
const roleOf = (map, pid) => map.parts.find((p) => p.id === pid)?.role;

function writeDoc(root, id, file, md) {
  const f = join(mp(root, id), file);
  writeFileSync(f, md);
  return `midi/${file}`;
}

function submitDoc(root, id, step, { file, label, note, checks = [] }) {
  submit(root, id, step, { note, questions: [], items: [{ path: file, label }], error_checks: checks, pin_changes: [] });
  appendFileSync(join(projectPaths(root, id).work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · ${label} submitted.\n`);
}

const checksMd = (passed, problems) => `## Checks\n\n${passed.map((p) => `- ✓ ${p}`).join('\n')}${problems.length ? `\n${problems.map((p) => `- ✗ ${p}`).join('\n')}` : ''}\n`;

// ---- the stages ---------------------------------------------------------------------------------------------------

async function stepQuestions(ctx) {
  const { root, id } = ctx;
  const project = loadProject(root, id);
  const r = await ask(ctx, 'brief', [`You help the owner of "${project.title}", a song they are making in a MIDI studio, answer four questions about it. Suggest an answer to each from what they said, in their voice. Answer with the JSON asked for, nothing else.`, ...ownerWords(root, id), `## The questions\n${MIDI_QUESTIONS.map((q) => `- ${q.id}: ${q.text}`).join('\n')}`].join('\n\n'), QSCHEMA);
  if (!r.ok || !r.structured) return { ...r, ok: false, error: r.error || 'no suggestions came back' };
  const questions = MIDI_QUESTIONS.map((q) => ({ id: q.id, text: q.text, default: String(r.structured[q.id] || '').trim().slice(0, 1000) || null }));
  submit(root, id, 'questions', { note: 'Four questions about the song, each with a suggested answer from your idea.', questions, items: [], error_checks: [], pin_changes: [] });
  return r;
}

async function stepBrief(ctx) {
  const { root, id, view, version } = ctx;
  const d = mp(root, id);
  const r = await ask(ctx, 'brief', [`You write the one-page brief for a song the owner is making in a MIDI studio. Every later stage (genre, song map, chords, shapes, melody) is planned from it. Plain words. Never copy a real song; "sounds like" means the feel. Answer with the JSON asked for, nothing else.`, ...ownerWords(root, id), ...changesAsked(root, id, view, join(d, 'brief.md'))].join('\n\n'), BRIEF);
  const s = r.ok ? r.structured : null;
  if (!s?.summary) return { ...r, ok: false, error: r.error || 'the brief call gave no brief' };
  s.title = loadProject(root, id).title;
  writeFileSync(join(d, 'brief.json'), JSON.stringify(s, null, 1));
  const md = `# ${s.title} · Brief v${version}\n\n${s.summary}\n\n## What it's for\n\n${s.purpose}\n\n## Genre\n\n${s.genre}\n\n## The feel\n\n${s.feel}\n\n## Length\n\nAbout ${Math.round(s.length_sec / 60 * 10) / 10} minutes (${s.length_sec} s)\n\n## Sounds like\n\n${s.like}\n\n## It must\n\n${list(s.must)}\n\n## It must never\n\n${list(s.never)}\n`;
  submitDoc(root, id, 'idea', { file: writeDoc(root, id, 'brief.md', md), label: `Brief v${version}`, note: s.note || 'The brief.' });
  return r;
}

async function stepGenre(ctx) {
  const { root, id, view, version } = ctx;
  const d = mp(root, id);
  const r = await ask(ctx, 'brief', [`You write the genre profile for the song in the brief: how this genre is actually made (tempo, keys, drums, bass, harmony, melody, instruments as General MIDI programs, form, signature moves) and where that knowledge comes from. Be specific and true to the genre. Answer with the JSON asked for, nothing else.`, ...sofar(root, id, 'genre'), ...ownerWords(root, id), ...changesAsked(root, id, view, join(d, 'genre.md'))].join('\n\n'), GENRE);
  const g = r.ok ? r.structured : null;
  if (!g?.name) return { ...r, ok: false, error: r.error || 'the genre call gave no profile' };
  if (g.bpm_min > g.bpm_max) [g.bpm_min, g.bpm_max] = [g.bpm_max, g.bpm_min];
  writeFileSync(join(d, 'genre.json'), JSON.stringify(g, null, 1));
  const problems = [];
  if ((g.sources || []).length < 3) problems.push('fewer than 3 sources');
  const md = `# Genre: ${g.name} · v${version}\n\n${g.summary}\n\n| Tempo | Keys | Meter | Swing |\n|---|---|---|---|\n| ${g.bpm_min}-${g.bpm_max} BPM | ${g.keys.join(', ')} | ${g.meter} | ${g.swing ? `${Math.round(g.swing * 100)}%` : 'straight'} |\n\n## Drums\n\n${g.drums}\n\n## Bass\n\n${g.bass}\n\n## Harmony\n\n${g.harmony}\n\n## Melody\n\n${g.melody}\n\n## Instruments\n\n${list(g.instruments.map((i) => `${i.role}: ${i.sound} (GM ${i.program + 1})`))}\n\n## Song form\n\n${g.form}\n\n## Signature moves\n\n${list(g.moves)}\n\n## Sources\n\n${list(g.sources)}\n\n${checksMd([`tempo range ${g.bpm_min}-${g.bpm_max}`, `${g.sources.length} sources`], problems)}`;
  submitDoc(root, id, 'genre', { file: writeDoc(root, id, 'genre.md', md), label: `Genre v${version}`, note: g.note || 'The genre profile.' });
  return r;
}

export function mapProblems(m, { genre, brief, recent = [] }) {
  const out = [];
  const ids = new Set(m.parts.map((p) => p.id));
  if (m.parts.length < 6) out.push(`only ${m.parts.length} parts: a full song needs 6 or more`);
  if (ids.size !== m.parts.length) out.push('two parts share an id');
  if (!m.parts.some((p) => p.role === 'drums')) out.push('no drums part');
  if (!m.parts.some((p) => p.role === 'bass')) out.push('no bass part');
  if (!m.parts.some((p) => p.role === 'lead')) out.push('no lead part (the melody stage writes it)');
  if (scaleOf(m.tonic, m.mode) === null) out.push(`the key "${m.tonic} ${m.mode}" isn't one the studio knows`);
  for (const s of m.sections) for (const p of s.parts) if (!ids.has(p)) out.push(`${s.name} plays "${p}", which isn't a part`);
  for (const s of m.sections) if (s.bars % 2) out.push(`${s.name} is ${s.bars} bars: use an even number`);
  for (const p of m.parts) if (!m.sections.some((s) => s.parts.includes(p.id))) out.push(`${p.name} (${p.id}) never plays: give it the sections it plays in, or leave it out`);
  if (genre && (m.bpm < genre.bpm_min - 5 || m.bpm > genre.bpm_max + 5)) out.push(`${m.bpm} BPM is outside the genre's ${genre.bpm_min}-${genre.bpm_max}`);
  const sec = m.sections.reduce((a, s) => a + s.bars, 0) * (m.meter === '3/4' ? 3 : 4) * 60 / m.bpm;
  if (brief && Math.abs(sec - brief.length_sec) > brief.length_sec * 0.25) out.push(`the song is ${Math.round(sec)} s long; the brief asks for about ${brief.length_sec} s`);
  const same = recent.find((r) => r.tonic === m.tonic && r.mode === m.mode && Math.abs(r.bpm - m.bpm) <= 2);
  if (same) out.push(`same key and tempo as the recent song "${same.title}": pick another key or tempo`);
  return out;
}

const recentFile = (root) => join(paths(root).run, '..', 'midi-recent.json');

async function stepSongMap(ctx) {
  const { root, id, view, version } = ctx;
  const d = mp(root, id);
  const genre = readJson(join(d, 'genre.json'));
  const brief = readJson(join(d, 'brief.json'));
  const recent = readJson(recentFile(root), []);
  const base = [`You map the song: tempo, key, meter, the parts (instruments with General MIDI programs, 0-based, and their note ranges) and the sections in order, each with its bars, energy (1-5) and which parts play. Build energy like the genre does; parts enter and leave. Answer with the JSON asked for, nothing else.`, ...sofar(root, id, 'song-map'), ...ownerWords(root, id),
    recent.length ? `## Recent songs (never the same key and tempo)\n${recent.slice(-8).map((r) => `- ${r.title}: ${r.tonic} ${r.mode}, ${r.bpm} BPM`).join('\n')}` : '', ...changesAsked(root, id, view, join(d, 'songmap.md'))].filter(Boolean);
  let r = await ask(ctx, 'plan', base.join('\n\n'), MAP);
  let m = r.ok ? r.structured : null;
  let problems = m ? mapProblems(m, { genre, brief, recent }) : [];
  for (let i = 0; m && problems.length && i < MAX_FIXES; i++) {
    fixLog(ctx, 'Song map', problems);
    const f = await ask(ctx, 'fix', `The studio refused this song map. Fix only these problems and keep everything else. Answer with {"answer": <the whole corrected song map>}.\n\n## Problems\n${list(problems)}\n\n## The song map\n${JSON.stringify(m)}`, FIX);
    if (f.ok && f.structured?.answer?.sections) { m = f.structured.answer; problems = mapProblems(m, { genre, brief, recent }); }
  }
  if (!m?.sections) return { ...r, ok: false, error: r.error || 'the song map call gave no map' };
  m.title = loadProject(root, id).title;
  writeFileSync(join(d, 'songmap.json'), JSON.stringify(m, null, 1));
  const bars = m.sections.reduce((a, s) => a + s.bars, 0);
  const sec = Math.round(bars * (m.meter === '3/4' ? 3 : 4) * 60 / m.bpm);
  const grid = `| Section | Bars | Energy | ${m.parts.map((p) => p.id).join(' | ')} |\n|---|---|---|${m.parts.map(() => '---').join('|')}|\n${m.sections.map((s) => `| ${s.name} | ${s.bars} | ${'●'.repeat(s.energy)} | ${m.parts.map((p) => (s.parts.includes(p.id) ? '■' : '')).join(' | ')} |`).join('\n')}`;
  const md = `# ${m.title} · Song map v${version}\n\n${m.bpm} BPM · ${m.tonic} ${m.mode} · ${m.meter} · ${bars} bars · ${Math.floor(sec / 60)}:${String(sec % 60).padStart(2, '0')}\n\n## Sections\n\n${grid}\n\n## Parts\n\n${list(m.parts.map((p) => `${p.name} (${p.id}, ${p.role}): GM ${p.program + 1}, notes ${p.low}-${p.high}`))}\n\n${checksMd([`${m.parts.length} parts`, `${m.sections.length} sections`, `${sec} s long`], problems)}`;
  submitDoc(root, id, 'song-map', { file: writeDoc(root, id, 'songmap.md', md), label: `Song map v${version}`, note: m.note || 'The song map.' });
  return r;
}

export function chordProblems(c, map) {
  const out = [];
  const scale = scaleOf(map.tonic, map.mode);
  const kinds = [...new Set(map.sections.map((s) => s.kind))];
  for (const k of kinds) if (!c.kinds.some((x) => x.kind === k)) out.push(`no chords for the "${k}" sections`);
  for (const p of c.kinds) {
    for (const sym of p.chords) {
      const ch = chordTones(sym);
      if (!ch) { out.push(`${p.kind}: "${sym}" isn't a chord symbol the studio reads`); continue; }
      if (!inScale(ch.root, scale) && !(p.borrowed || []).includes(sym)) out.push(`${p.kind}: ${sym} is outside ${map.tonic} ${map.mode} (list it in borrowed if that's on purpose)`);
    }
    const len = p.chords.length * (p.bars_per_chord || 1);
    for (const s of map.sections.filter((x) => x.kind === p.kind)) if (s.bars % len) out.push(`${p.kind}: the progression is ${len} bars, which doesn't fit ${s.name}'s ${s.bars} bars`);
  }
  return out;
}

async function stepChords(ctx) {
  const { root, id, view, version } = ctx;
  const d = mp(root, id);
  const map = readJson(join(d, 'songmap.json'));
  const kinds = [...new Set(map.sections.map((s) => s.kind))];
  const base = [`You write the chord progression for each kind of section (${kinds.join(', ')}) in ${map.tonic} ${map.mode}, true to the genre. A progression loops through its sections, so its length in bars must divide each of their lengths. Answer with the JSON asked for, nothing else.`, ...sofar(root, id, 'chords'), ...ownerWords(root, id), ...changesAsked(root, id, view, join(d, 'chords.md'))];
  const r = await ask(ctx, 'plan', base.join('\n\n'), CHORDS);
  let c = r.ok ? r.structured : null;
  // every chord must voice in Strudel (a name its dictionary doesn't know plays nothing, without an error)
  const voiceProblems = async () => {
    const out = [];
    const tones = await voicedTones(root, c.kinds.flatMap((p) => p.chords));
    for (const p of c.kinds) for (const x of new Set(p.chords)) if (!tones[x]?.length) out.push(`${p.kind}: Strudel has no voicing for ${x} (it would play nothing): use another spelling or chord`);
    return out;
  };
  const allProblems = async () => (c?.kinds ? [...chordProblems(c, map), ...(await voiceProblems())] : []);
  let problems = await allProblems();
  for (let i = 0; c && problems.length && i < MAX_FIXES; i++) {
    fixLog(ctx, 'Chords', problems);
    const f = await ask(ctx, 'fix', `The studio refused these chords. Fix only these problems and keep everything else. Answer with {"answer": <the whole corrected JSON>}.\n\nKey: ${map.tonic} ${map.mode}. Sections: ${JSON.stringify(map.sections.map((s) => ({ name: s.name, kind: s.kind, bars: s.bars })))}\n\n## Problems\n${list(problems)}\n\n## The chords\n${JSON.stringify(c)}`, FIX);
    if (f.ok && f.structured?.answer?.kinds) { c = f.structured.answer; problems = await allProblems(); }
  }
  if (!c?.kinds) return { ...r, ok: false, error: r.error || 'the chords call gave no chords' };
  writeFileSync(join(d, 'chords.json'), JSON.stringify(c, null, 1));
  const md = `# ${map.title} · Chords v${version}\n\n${map.tonic} ${map.mode}\n\n${c.kinds.map((p) => `## ${p.kind}\n\n${p.chords.map((x) => `**${x}**`).join(' · ')}${p.bars_per_chord > 1 ? ` (${p.bars_per_chord} bars each)` : ''}\n\n${p.why || ''}${p.borrowed?.length ? `\n\nBorrowed on purpose: ${p.borrowed.join(', ')}` : ''}`).join('\n\n')}\n\n${checksMd(['every chord read and voiced', 'every progression fits its sections'], problems)}`;
  submitDoc(root, id, 'chords', { file: writeDoc(root, id, 'chords.md', md), label: `Chords v${version}`, note: c.note || 'The chords.' });
  return r;
}

// One shape checked over the first section that plays it: it runs, it has notes, every pitch fits.
export async function shapeCheck(root, code, part, map, chords) {
  const sec = map.sections.find((s) => s.parts.includes(part.id)) || map.sections[0];
  const prog = chords.kinds.find((k) => k.kind === sec.kind) || chords.kinds[0];
  const tpb = tpbOf(map.meter);
  const drums = ['drums', 'perc'].includes(part.role);
  const res = await notesOf(root, fill(code, { chords: strudelProgression(prog.chords, prog.bars_per_chord), scale: scaleText(map, 4) }), { bars: Math.min(sec.bars, 8), tpb, drums });
  if (res.error) return [`${part.id}: ${res.error}`];
  if (!res.notes.length) return [`${part.id}: the shape plays no notes`];
  if (!drums && !/<CHORDS>|<SCALE>/.test(code)) return [`${part.id}: a harmony part must use <CHORDS> (or <SCALE>) instead of naming notes`];
  const voiced = await voicedTones(root, prog.chords);
  return [...res.problems.slice(0, 2).map((p) => `${part.id}: ${p}`), ...noteProblems(res.notes, { role: part.role, part, map, prog, tpb, voiced })];
}

async function stepShapes(ctx) {
  const { root, id, view, version } = ctx;
  const d = mp(root, id);
  const map = readJson(join(d, 'songmap.json'));
  const chords = readJson(join(d, 'chords.json'));
  const parts = map.parts.filter((p) => p.role !== 'lead');
  const base = [`You write 2 or 3 short shapes (1, 2 or 4 bar loops) for each of these parts: ${parts.map((p) => `${p.id} (${p.role})`).join(', ')}. The lead is written later. Shapes are the genre's real patterns with character: the owner picks the ones they love and those repeat most. Suggest one per part.\n\n${STRUDEL_RULES}\n\nAnswer with the JSON asked for, nothing else.`, ...sofar(root, id, 'shapes'), ...ownerWords(root, id), ...changesAsked(root, id, view, join(d, 'shapes.md'))];
  const r = await ask(ctx, 'plan', base.join('\n\n'), SHAPES);
  let s = r.ok ? r.structured : null;
  if (!s?.parts) return { ...r, ok: false, error: r.error || 'the shapes call gave no shapes' };
  const check = async () => {
    const probs = [];
    for (const p of parts) if (!s.parts.some((x) => x.part === p.id)) probs.push(`no shapes for ${p.id}`);
    for (const x of s.parts) {
      const part = map.parts.find((p) => p.id === x.part);
      if (!part) { probs.push(`"${x.part}" isn't a part`); continue; }
      for (const [i, sh] of x.shapes.entries()) for (const p of await shapeCheck(root, sh.code, part, map, chords)) probs.push(`${p} (shape ${i + 1} "${sh.name}")`);
    }
    return probs;
  };
  let problems = await check();
  for (let i = 0; problems.length && i < MAX_FIXES; i++) {
    fixLog(ctx, 'Shapes', problems);
    const bad = s.parts.filter((x) => problems.some((p) => p.startsWith(`${x.part}:`)));
    const missing = parts.filter((p) => !s.parts.some((x) => x.part === p.id)).map((p) => `${p.id} (${p.role})`);
    const f = await ask(ctx, 'fix', `The studio refused some of these shapes. Answer with only the parts named below, fixed (all their shapes); the other parts stay as they are.\n\n${STRUDEL_RULES}\n\nKey ${map.tonic} ${map.mode}. Answer with {"answer": {"parts": [<only the fixed or missing parts>]}}.\n\n## Problems\n${list(problems)}\n\n## The parts to fix\n${JSON.stringify(bad)}${missing.length ? `\n\n## Missing: write shapes for\n${missing.join(', ')}` : ''}`, FIX);
    const got = f.ok ? f.structured?.answer?.parts : null;
    if (Array.isArray(got) && got.length) { s.parts = [...s.parts.filter((x) => !got.some((g) => g.part === x.part)), ...got]; problems = await check(); }
  }
  for (const x of s.parts) x.picked = Math.min(Math.max(0, x.suggested | 0), x.shapes.length - 1);
  writeFileSync(join(d, 'shapes.json'), JSON.stringify(s, null, 1));
  const md = `# ${map.title} · Shapes v${version}\n\nThe suggested shape of each part is marked ★ and is the one the song uses until you pick another.\n\n${s.parts.map((x) => `## ${x.part}\n\n${x.shapes.map((sh, i) => `${i === x.picked ? '★' : '·'} **${sh.name}**: ${sh.why}\n\n\`${sh.code}\``).join('\n\n')}`).join('\n\n')}\n\n${checksMd(['every shape runs in Strudel', 'every pitch is a chord tone (bass, chords, keys, arps) or a scale tone'], problems)}`;
  submitDoc(root, id, 'shapes', { file: writeDoc(root, id, 'shapes.md', md), label: `Shapes v${version}`, note: s.note || 'The shapes.' });
  return r;
}

async function stepMelody(ctx) {
  const { root, id, view, version } = ctx;
  const d = mp(root, id);
  const map = readJson(join(d, 'songmap.json'));
  const chords = readJson(join(d, 'chords.json'));
  const lead = map.parts.find((p) => p.role === 'lead');
  const kinds = [...new Set(map.sections.filter((s) => s.parts.includes(lead.id)).map((s) => s.kind))];
  const lens = Object.fromEntries(kinds.map((k) => [k, Math.min(...map.sections.filter((s) => s.kind === k).map((s) => s.bars))]));
  const base = [`You write the lead melody (${lead.name}) for each kind of section it plays in: ${kinds.map((k) => `${k} (${lens[k]} bars)`).join(', ')}. A melody that sings: a motif that comes back, mostly stepwise, leaps resolved, rests to breathe, strong beats on chord tones, a peak in the chorus. Write each one over its whole section with <…> bar by bar.\n\n${STRUDEL_RULES}\n\nAnswer with the JSON asked for, nothing else.`, ...sofar(root, id, 'melody'), ...ownerWords(root, id), ...changesAsked(root, id, view, join(d, 'melody.md'))];
  const r = await ask(ctx, 'plan', base.join('\n\n'), MELODY);
  let m = r.ok ? r.structured : null;
  if (!m?.kinds) return { ...r, ok: false, error: r.error || 'the melody call gave no melody' };
  // how stepwise each melody is: shown on the stage, never a fix round (blues and funk lines leap by nature)
  let notes = [];
  const check = async () => {
    const probs = [];
    notes = [];
    for (const k of kinds) if (!m.kinds.some((x) => x.kind === k)) probs.push(`no melody for the ${k}`);
    for (const x of m.kinds) {
      const prog = chords.kinds.find((c) => c.kind === x.kind);
      if (!prog) continue;
      if (!x.code.includes('<SCALE>')) { probs.push(`${x.kind}: the melody must use .scale("<SCALE>")`); continue; }
      const res = await notesOf(root, fill(x.code, { scale: scaleText(map, m.octave) }), { bars: lens[x.kind] || 8, tpb: tpbOf(map.meter), drums: false });
      if (res.error) { probs.push(`${x.kind}: ${res.error}`); continue; }
      if (!res.notes.length) probs.push(`${x.kind}: no notes`);
      probs.push(...noteProblems(res.notes, { role: 'lead', part: { ...lead, id: x.kind }, map, prog, tpb: tpbOf(map.meter), voiced: await voicedTones(root, prog.chords) }));
      const steps = res.notes.slice(1).map((n, i) => Math.abs(n.key - res.notes[i].key));
      if (steps.length > 6) notes.push(`${x.kind}: ${Math.round((steps.filter((s) => s <= 2).length / steps.length) * 100)}% of the moves are steps`);
    }
    return probs;
  };
  let problems = await check();
  for (let i = 0; problems.length && i < MAX_FIXES; i++) {
    fixLog(ctx, 'Melody', problems);
    const bad = m.kinds.filter((x) => problems.some((p) => p.startsWith(`${x.kind}:`)));
    const missing = kinds.filter((k) => !m.kinds.some((x) => x.kind === k));
    const f = await ask(ctx, 'fix', `The studio refused some of these melodies. Answer with only the melodies named below, fixed; the others stay as they are.\n\n${STRUDEL_RULES}\n\nKey ${map.tonic} ${map.mode}; chords per kind ${JSON.stringify(chords.kinds.map((c) => ({ kind: c.kind, chords: c.chords, bars_per_chord: c.bars_per_chord })))}; section lengths ${JSON.stringify(lens)}. Answer with {"answer": {"kinds": [<only the fixed or missing melodies>]}}.\n\n## Problems\n${list(problems)}\n\n## The melodies to fix\n${JSON.stringify(bad)}${missing.length ? `\n\n## Missing: write these\n${missing.join(', ')}` : ''}`, FIX);
    const got = f.ok ? f.structured?.answer?.kinds : null;
    if (Array.isArray(got) && got.length) { m.kinds = [...m.kinds.filter((x) => !got.some((g) => g.kind === x.kind)), ...got]; problems = await check(); }
  }
  writeFileSync(join(d, 'melody.json'), JSON.stringify(m, null, 1));
  const md = `# ${map.title} · Melody v${version}\n\n${lead.name}, tonic in octave ${m.octave}\n\n${m.kinds.map((x) => `## ${x.kind}\n\n${x.why}\n\n\`${x.code}\``).join('\n\n')}\n\n${checksMd(['every melody runs in Strudel', 'in the scale', 'strong beats mostly on chord tones'], problems)}${notes.length ? `\n## Shape\n\n${list(notes)}\n` : ''}`;
  submitDoc(root, id, 'melody', { file: writeDoc(root, id, 'melody.md', md), label: `Melody v${version}`, note: m.note || 'The melody.' });
  return r;
}

// The whole song as plain Strudel: one arrange() per part, every section's bars with the shape (or melody) and the
// section's own progression; silence where the part rests. The picked shape plays everywhere; a second shape the
// owner didn't skip plays in the high-energy sections, so the loved one repeats most.
export function arrangeCode(map, chords, shapes, melody) {
  const out = [];
  for (const part of map.parts) {
    const rows = [];
    for (const sec of map.sections) {
      if (!sec.parts.includes(part.id)) { rows.push(`  [${sec.bars}, silence], // ${sec.name}`); continue; }
      const prog = chords.kinds.find((k) => k.kind === sec.kind) || chords.kinds[0];
      const ch = strudelProgression(prog.chords, prog.bars_per_chord);
      let code;
      if (part.role === 'lead') {
        code = melody.kinds.find((k) => k.kind === sec.kind)?.code;
        if (!code) { rows.push(`  [${sec.bars}, silence], // ${sec.name} (no melody for ${sec.kind})`); continue; }
        code = fill(code, { scale: `${map.tonic}${melody.octave}:${STRUDEL_MODE[map.mode]}` });
      } else {
        const x = shapes.parts.find((p) => p.part === part.id);
        if (!x) { rows.push(`  [${sec.bars}, silence], // ${sec.name}`); continue; }
        const alt = x.shapes.findIndex((_, i) => i !== x.picked && !(x.skipped || []).includes(i));
        const use = sec.energy >= 4 && alt >= 0 ? alt : x.picked;
        code = fill(x.shapes[use].code, { chords: ch, scale: `${map.tonic}4:${STRUDEL_MODE[map.mode]}` });
      }
      rows.push(`  [${sec.bars}, ${code}], // ${sec.name}`);
    }
    out.push(`// ${part.id} · ${part.name}\n$${part.id}: arrange(\n${rows.join('\n')}\n)`);
  }
  return `// ${map.title}: ${map.bpm} BPM, ${map.tonic} ${map.mode}, ${map.meter}. One cycle = one bar.\n\n${out.join('\n\n')}\n`;
}

async function stepArrange(ctx) {
  const { root, id, version } = ctx;
  const d = mp(root, id);
  const [map, chords, shapes, melody] = ['songmap', 'chords', 'shapes', 'melody'].map((f) => readJson(join(d, `${f}.json`)));
  const tpb = tpbOf(map.meter);
  const total = map.sections.reduce((a, s) => a + s.bars, 0);
  const text = arrangeCode(map, chords, shapes, melody);
  writeFileSync(join(d, 'song.strudel.js'), text);
  const notes = {};
  const problems = [];
  for (const block of text.split(/\n\n(?=\/\/ )/).slice(1)) {
    const pid = /\$(\w[\w-]*):/.exec(block)?.[1];
    const part = map.parts.find((p) => p.id === pid);
    if (!part) continue;
    const code = block.slice(block.indexOf('arrange('));
    const res = await notesOf(root, code, { bars: total, tpb, drums: ['drums', 'perc'].includes(part.role) });
    if (res.error) { problems.push(`${pid}: ${res.error}`); notes[pid] = []; continue; }
    notes[pid] = ['drums', 'perc'].includes(part.role) ? res.notes : fitRange(res.notes, part);
  }
  writeFileSync(join(d, 'notes.json'), JSON.stringify({ tpb, parts: notes }));
  let at = 0;
  const rows = map.sections.map((s) => { const r = `| ${s.name} | ${at + 1}-${at + s.bars} | ${map.parts.map((p) => notes[p.id]?.filter((n) => n.t >= at * tpb && n.t < (at + s.bars) * tpb).length || '').join(' | ')} |`; at += s.bars; return r; });
  const md = `# ${map.title} · Arrangement v${version}\n\nThe whole song in plain Strudel (midi/song.strudel.js), ${total} bars. Notes per part and section:\n\n| Section | Bars | ${map.parts.map((p) => p.id).join(' | ')} |\n|---|---|${map.parts.map(() => '---').join('|')}|\n${rows.join('\n')}\n\n${checksMd([`${Object.values(notes).reduce((a, n) => a + n.length, 0)} notes`, 'every part ran', 'every part inside its range'], problems)}`;
  submitDoc(root, id, 'arrange', { file: writeDoc(root, id, 'arrange.md', md), label: `Arrangement v${version}`, note: 'The whole song, arranged from your shapes and the melody. Press Next for the feel.' });
  return { ok: true };
}

// Feel: velocity from each section's energy and the beat, swing from the genre, a small human timing drift, and a
// drum fill into every section that lifts. Seeded, so the same song always gets the same feel.
export function applyFeel(notesByPart, map, { swing = 0, seed = 1 } = {}) {
  const tpb = tpbOf(map.meter);
  let s = seed >>> 0;
  const rand = () => { s = (s * 1664525 + 1013904223) >>> 0; return s / 2 ** 32; };
  const bounds = [];
  let at = 0;
  for (const sec of map.sections) { bounds.push({ ...sec, start: at * tpb, end: (at + sec.bars) * tpb }); at += sec.bars; }
  const secAt = (t) => bounds.find((b) => t >= b.start && t < b.end) || bounds.at(-1);
  const out = {};
  const stats = { swung: 0, fills: 0 };
  for (const part of map.parts) {
    const drums = ['drums', 'perc'].includes(part.role);
    let ns = (notesByPart[part.id] || []).map((n) => {
      const sec = secAt(n.t);
      const inBeat = n.t % PPQ;
      const accent = inBeat === 0 ? (n.t % tpb === 0 ? 1.08 : 1.0) : 0.86;
      const vel = Math.min(1, Math.max(0.15, n.vel * (0.62 + 0.08 * sec.energy) * accent + (rand() - 0.5) * 0.06));
      let t = n.t;
      if (swing > 0 && inBeat === PPQ / 2) { t += Math.round(swing * PPQ / 2); stats.swung += 1; }
      if (swing > 0 && drums && inBeat % (PPQ / 2) === PPQ / 4) { t += Math.round(swing * PPQ / 4); stats.swung += 1; }
      t = Math.max(0, t + Math.round((rand() - 0.5) * (drums ? 8 : 20)));
      return { ...n, t, vel };
    });
    if (part.role === 'drums') {
      for (let i = 0; i + 1 < bounds.length; i++) {
        const [a, b] = [bounds[i], bounds[i + 1]];
        if (b.energy <= a.energy || !b.parts.includes(part.id)) continue;
        const from = a.end - 2 * PPQ;
        ns = ns.filter((n) => n.t < from || n.t >= a.end || ![38, 41, 45, 48].includes(n.key));
        for (let k = 0; k < 8; k++) ns.push({ t: from + k * (PPQ / 4), len: PPQ / 4, key: k < 4 ? 38 : [48, 45, 41, 41][k - 4], vel: 0.5 + k * 0.06 });
        ns.push({ t: b.start, len: PPQ, key: 49, vel: 0.95 });
        stats.fills += 1;
      }
    }
    // no two same notes overlapping on one part
    ns.sort((x, y) => x.t - y.t || x.key - y.key);
    const last = {};
    for (const n of ns) { const p = last[n.key]; if (p && p.t + p.len > n.t) p.len = Math.max(1, n.t - p.t); last[n.key] = n; }
    out[part.id] = ns.filter((n) => n.len > 0);
  }
  return { parts: out, stats };
}

async function stepFeel(ctx) {
  const { root, id, version } = ctx;
  const d = mp(root, id);
  const map = readJson(join(d, 'songmap.json'));
  const genre = readJson(join(d, 'genre.json'), {});
  const raw = readJson(join(d, 'notes.json'));
  const { parts, stats } = applyFeel(raw.parts, map, { swing: genre.swing || 0, seed: [...id].reduce((a, c) => a * 31 + c.charCodeAt(0), 7) });
  writeFileSync(join(d, 'final.json'), JSON.stringify({ tpb: raw.tpb, parts }));
  const vel = Object.values(parts).flat().map((n) => n.vel);
  const md = `# ${map.title} · Feel v${version}\n\n- Velocity follows each section's energy and the beat: downbeats strongest, off-beats softer (${Math.round(Math.min(...vel) * 127)}-${Math.round(Math.max(...vel) * 127)}).\n- Swing: ${genre.swing ? `${Math.round(genre.swing * 100)}% on the off-beats (${stats.swung} notes moved)` : 'straight, as the genre plays it'}.\n- Timing: a small human drift (drums ±4 ticks, the rest ±10 at 960 per beat).\n- Fills: ${stats.fills} drum fill${stats.fills === 1 ? '' : 's'} into the sections that lift, each with a crash on the downbeat.\n\n${checksMd(['no overlapping notes', 'velocities 1-127'], [])}`;
  submitDoc(root, id, 'feel', { file: writeDoc(root, id, 'feel.md', md), label: `Feel v${version}`, note: 'Velocity, swing, timing and fills. Press Next for the MIDI files.' });
  return { ok: true };
}

const sh = (cmd, args) => new Promise((ok) => execFile(cmd, args, { timeout: 5 * 60_000 }, (e, so, se) => ok({ code: e ? e.code || 1 : 0, out: `${so}${se}`.slice(-600) })));
export const SOUNDFONT = process.env.MFX_SOUNDFONT || '/mnt/storage/drive_1/mortiflix/soundfonts/GeneralUser-GS-9575028c.sf2';

async function stepExport(ctx) {
  const { root, id, version } = ctx;
  const d = mp(root, id);
  const map = readJson(join(d, 'songmap.json'));
  const fin = readJson(join(d, 'final.json'));
  const out = join(d, 'out');
  mkdirSync(join(out, 'parts'), { recursive: true });
  mkdirSync(join(out, 'sections'), { recursive: true });
  const tpb = fin.tpb;
  const slug = (s) => String(s).toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '') || 'song';
  const name = slug(map.title);
  const all = map.parts.map((p) => ({ name: p.name, program: p.program, drums: ['drums', 'perc'].includes(p.role), notes: fin.parts[p.id] || [] }));
  let at = 0;
  const markers = map.sections.map((s) => { const m = { t: at * tpb, text: s.name }; at += s.bars; return m; });
  writeFileSync(join(out, `${name}.mid`), writeSmf({ bpm: map.bpm, meter: map.meter, parts: all, markers }));
  for (const p of all) writeFileSync(join(out, 'parts', `${slug(p.name)}.mid`), writeSmf({ bpm: map.bpm, meter: map.meter, parts: [p] }));
  at = 0;
  for (const [i, s] of map.sections.entries()) {
    const [a, b] = [at * tpb, (at + s.bars) * tpb];
    at += s.bars;
    const parts = all.map((p) => ({ ...p, notes: p.notes.filter((n) => n.t >= a && n.t < b).map((n) => ({ ...n, t: n.t - a, len: Math.min(n.len, b - n.t) })) })).filter((p) => p.notes.length);
    writeFileSync(join(out, 'sections', `${String(i + 1).padStart(2, '0')}-${slug(s.name)}.mid`), writeSmf({ bpm: map.bpm, meter: map.meter, parts }));
  }
  const problems = [];
  // a preview to listen to: the General MIDI soundfont through FluidSynth, then MP3
  let preview = null;
  if (existsSync(SOUNDFONT)) {
    const wav = join(out, `${name}-preview.wav`);
    const r1 = await sh('fluidsynth', ['-ni', '-g', '0.6', '-F', wav, '-r', '44100', SOUNDFONT, join(out, `${name}.mid`)]);
    if (r1.code) problems.push(`the preview didn't render: ${r1.out}`);
    else {
      const songSec = map.sections.reduce((n, x) => n + x.bars, 0) * (map.meter === '3/4' ? 3 : 4) * 60 / map.bpm;
      // FluidSynth renders until every voice dies (a long tail of near-silence): cut at the song's end plus 4 s
      const r2 = await sh('ffmpeg', ['-y', '-loglevel', 'error', '-i', wav, '-t', (songSec + 4).toFixed(2), '-af', 'loudnorm=I=-14:TP=-1.5:LRA=11', '-ar', '44100', '-b:a', '256k', join(out, `${name}-preview.mp3`)]);
      if (r2.code) problems.push(`the preview MP3 failed: ${r2.out}`); else { preview = `midi/out/${name}-preview.mp3`; rmSync(wav, { force: true }); }
    }
  } else problems.push('no General MIDI soundfont for the preview');
  const recent = readJson(recentFile(root), []);
  recent.push({ title: map.title, tonic: map.tonic, mode: map.mode, bpm: map.bpm, at: new Date().toISOString() });
  writeFileSync(recentFile(root), JSON.stringify(recent.slice(-30), null, 1));
  const cue = `# ${map.title} · Into your DAW\n\n${map.bpm} BPM · ${map.tonic} ${map.mode} · ${map.meter}\n\n## FL Studio\n\n1. Set the tempo to ${map.bpm}.\n2. File › Import › MIDI file… › ${name}.mid. Tick "Start new project" off, "Import notes" on: each part becomes a channel; drums land on one channel.\n3. Load an instrument per channel (the names say which: ${map.parts.map((p) => p.name).join(', ')}).\n4. Strata on the Master.\n\n## REAPER\n\n1. Drag ${name}.mid into the arrange view; choose "one track per MIDI channel" and "import tempo map".\n2. Put an instrument on each track.\n\nSection markers are in the file (${map.sections.map((s) => s.name).join(' · ')}).\n`;
  writeFileSync(join(out, 'CUE-SHEET.md'), cue);
  const md = `# ${map.title} · Export v${version}\n\n- **${name}.mid**: the whole song, ${all.length} parts, section markers\n- **parts/**: one file per part (${all.length})\n- **sections/**: one file per section (${map.sections.length})\n- **CUE-SHEET.md**: how to bring it into FL Studio or REAPER\n${preview ? `- **${name}-preview.mp3**: a quick General MIDI preview to listen to (not the final sound)\n` : ''}\nFiles: midi/out/\n\n${checksMd(['the MIDI files are written', 'one per part and per section'], problems)}`;
  const items = [{ path: writeDoc(root, id, 'export.md', md), label: `Export v${version}` }];
  if (preview) items.push({ path: preview, label: 'Preview (General MIDI)' });
  submit(root, id, 'export', { note: 'The MIDI files and a preview. Approve to finish the song.', questions: [], items, error_checks: [], pin_changes: [] });
  return { ok: true };
}

// Make the notes, the feel and the files again from the approved decisions (song map, chords, shapes, melody): code
// only, no AI. For a song made before a fix to the studio's code; the stages' gates are left as they are.
export async function rebuildFromDecisions(root, id) {
  const ctx = { root, id, version: 1 };
  for (const fn of [stepArrange, stepFeel, stepExport]) {
    try { await fn(ctx); } catch (e) { if (!/already|waiting for review|approved/.test(e.message)) throw e; }
  }
  return readJson(join(mp(root, id), 'final.json'));
}

const STEP_FN = { questions: stepQuestions, idea: stepBrief, genre: stepGenre, 'song-map': stepSongMap, chords: stepChords, shapes: stepShapes, melody: stepMelody, arrange: stepArrange, feel: stepFeel, export: stepExport };

// Run the MIDI step that's next, after checking the song's token budget.
export async function quickMidi({ root, id, view, run, runArgs, config, onActivity }) {
  const budget = config.midiTokenBudget || TOKEN_BUDGET;
  const used = usedTokens(root, id);
  if (used >= budget) return { ok: false, pause: true, error: `this song has used ${used.toLocaleString('en')} of its ${budget.toLocaleString('en')} tokens. Raise the song's token limit (midiTokenBudget), then press Resume.` };
  const dir = join(paths(root).run, 'quick');
  mkdirSync(dir, { recursive: true });
  const ctx = { root, id, view, version: view.version + 1, run, runArgs: { ...runArgs, workdir: dir }, config, onActivity, spent: { input: 0, output: 0 }, calls: 0 };
  onActivity({ kind: 'info', text: `${view.name}: ${ROLES_FOR[view.key] ? 'one AI call, then the studio checks it' : 'the studio does this itself (no AI)'}.` });
  let r;
  try { r = await STEP_FN[view.key](ctx); } catch (e) { r = { ok: false, error: e.message }; }
  const line = `${view.name} v${ctx.version}: ${ctx.calls} call${ctx.calls === 1 ? '' : 's'}, ${ctx.spent.input.toLocaleString('en')} in / ${ctx.spent.output.toLocaleString('en')} out; song so far ${(used + ctx.spent.input + ctx.spent.output).toLocaleString('en')} of ${budget.toLocaleString('en')}.`;
  onActivity({ kind: 'info', text: line });
  appendFileSync(join(projectPaths(root, id).work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · ${line}${r?.ok ? '' : ` Failed: ${r?.error}`}\n`);
  return { ...r, usage: { input_tokens: ctx.spent.input, output_tokens: ctx.spent.output } };
}

// What the stage panel shows: the current stage's document and where it is.
export function midiView(root, id) {
  const steps = stepView(loadProject(root, id), projectPipeline(root, id));
  const d = mp(root, id);
  const docs = { idea: 'brief.md', genre: 'genre.md', 'song-map': 'songmap.md', chords: 'chords.md', shapes: 'shapes.md', melody: 'melody.md', arrange: 'arrange.md', feel: 'feel.md', export: 'export.md' };
  return {
    steps: steps.map((s) => ({ key: s.key, stage: s.stage, state: s.state, version: s.version || 0, doc: s.version && docs[s.key] && existsSync(join(d, docs[s.key])) ? readFileSync(join(d, docs[s.key]), 'utf8') : null })),
    usage: usedTokens(root, id),
    // why the song stopped, while it waits for Resume
    stopped: (() => { const p = loadProject(root, id); return p.state === 'paused' ? String(p.needs_you?.text || 'Paused.') : ''; })(),
    out: existsSync(join(d, 'out')) ? join(d, 'out') : null,
  };
}
