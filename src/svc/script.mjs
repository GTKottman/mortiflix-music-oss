// The script, line by line, for the app's Script panel and the chat's script card (doc 11 §9.1, doc 30 stage 1).
//
// The script is the session's own files: script/beats.json (beats with their words) and voice/lines.json (the
// narration lines `b01-1`, `b01-2` …, once the voice exists). This module reads them as beats → lines, and writes the
// owner's edits back into those same files, so the canvas graph (src/canvas.mjs) picks the change up as it always
// does: the beat gets a new version, the voice line its new words. Line ids are the voice line ids and are forever
// (a deleted line is kept, marked deleted, so it can be brought back).
//
// What a line feeds is the graph's own `says` edges (svc/graph.mjs): its voice line and the shots of its beat. An
// edit to a line's words remakes only those pieces that already exist (a Fix on each, so versions are kept and the AI
// runs ahead as usual); pieces not made yet will be made from the new words anyway.
import { readFileSync, writeFileSync, mkdirSync, renameSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { canvasView, canvasPaths, readBeats, loadOwner, CanvasError } from '../canvas.mjs';
import { projectPaths, loadProject, projectPipeline, event } from '../projects.mjs';
import { stepView } from '../gates.mjs';
import { withLock } from '../studio.mjs';
import { filmOrder, loadLayout } from './graph.mjs';
import { beatNames } from './beatnames.mjs';

const BEAT = /^b\d{2,3}$/;
const LINE = /^b\d{2,3}-\d{1,3}$/;
const MAX_WORDS = 2000;
const MAX_EDITS = 200;

const readOr = (f, d) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return d; } };

// A short name for a beat that has none of its own: its first few words.
export function autoName(words) {
  const w = String(words || '').replace(/\s+/g, ' ').trim().split(' ').slice(0, 4).join(' ');
  return w.replace(/[.,;:!?…]+$/, '');
}

// voice/lines.json is a list, or {lines: [...]}: read it as a list and write it back the same way.
function readVoice(work) {
  const raw = readOr(join(work, 'voice', 'lines.json'), null);
  if (!raw) return { raw: null, list: [] };
  const list = Array.isArray(raw) ? raw : Array.isArray(raw.lines) ? raw.lines : [];
  return { raw, list: list.filter((l) => l && typeof l === 'object') };
}
function writeVoice(work, raw, list) {
  const out = Array.isArray(raw) ? list : { ...raw, lines: list };
  writeFileSync(join(work, 'voice', 'lines.json'), JSON.stringify(out, null, 2));
}

// The script as stored: script/beats.json, else the beats read from script.md (the first edit writes beats.json).
function readScript(work) {
  const j = readOr(join(work, 'script', 'beats.json'), null);
  if (j?.beats?.length) return { ...j, beats: j.beats.filter((b) => b && BEAT.test(b.id)) };
  return { version: 1, beats: readBeats(work, []) };
}

// A stored line with only the fields the script keeps: a deleted one also keeps the OK its voice line had (`was`), so
// bringing it back puts the same voice line back as it was, OK and all, without remaking it.
const keepLine = (l) => ({ id: l.id, words: String(l.words || ''), ...(l.deleted ? { deleted: true } : {}), ...(l.deleted && l.was ? { was: l.was } : {}),
  ...(l.changed_at ? { changed_at: l.changed_at } : {}) });

// One beat's lines: its own `lines` once the owner edited it, then any voice line of the beat it doesn't have yet;
// before that the voice lines of the beat; before the voice exists, the beat's words as its one line.
function linesOf(beat, voice) {
  const mine = voice.filter((l) => String(l.id || '').startsWith(`${beat.id}-`) && LINE.test(l.id))
    .map((l) => ({ id: l.id, words: String(l.script || l.text || '') }));
  if (Array.isArray(beat.lines)) {
    const own = beat.lines.filter((l) => l && LINE.test(l.id)).map(keepLine);
    return [...own, ...mine.filter((m) => !own.some((o) => o.id === m.id))];
  }
  if (mine.length) return mine;
  return beat.words ? [{ id: `${beat.id}-1`, words: String(beat.words) }] : [];
}

// Is the AI still writing the script? (Its step is being worked on and the project is running.)
function writing(root, id) {
  try {
    const p = loadProject(root, id);
    const step = stepView(p, projectPipeline(root, id)).find((s) => s.key === 'script');
    return Boolean(step && ['ready', 'working', 'changes'].includes(step.state) && p.state === 'queued');
  } catch { return false; }
}

// What each beat's lines feed: the beat's voice line (by id) and the beat's shots, in film order.
function feedsOf(g, film) {
  const shotsOf = (beat) => film.filter((s) => g.nodes[s]?.beat === beat && !g.nodes[s]?.missing);
  return (beat, line) => {
    const out = [];
    const v = g.nodes[line];
    if (v && v.kind === 'line' && !v.missing) out.push(v);
    for (const s of shotsOf(beat)) out.push(g.nodes[s]);
    return out.map((n) => ({ node: n.id, kind: n.kind, label: n.label, state: n.state, current: n.current ?? null, made: Boolean(n.versions?.some((x) => x.file)), remaking: Boolean(n.pending) }));
  };
}

// script.get: beats → lines with ids, what each line feeds and whether those pieces are being remade.
export function scriptView(root, id) {
  const work = projectPaths(root, id).work;
  const g = canvasView(root, id);
  const film = filmOrder(g, loadLayout(root, id));
  const feeds = feedsOf(g, film);
  const { list: voice } = readVoice(work);
  const s = readScript(work);
  const busy = writing(root, id);
  let names = {};
  try { names = beatNames(root, id, readBeats(work, [])); } catch { /* the words alone */ }
  const beats = s.beats.map((b, i) => {
    const node = g.nodes[b.id];
    const all = linesOf(b, voice);
    // Deleted lines are kept (ids are forever): the app shows the ones it deleted with Undo, in their place (`after` =
    // the line shown before it, null = first in the beat).
    const deleted = all.map((l, i) => ({ l, after: all.slice(0, i).filter((x) => !x.deleted).at(-1)?.id ?? null })).filter((x) => x.l.deleted)
      .slice(-50).map(({ l, after }) => ({ id: l.id, words: l.words, after, changed_at: l.changed_at || null }));
    const lines = all.filter((l) => !l.deleted).map((l) => {
      const f = feeds(b.id, l.id);
      const remaking = f.filter((x) => x.remaking);
      return { id: l.id, words: l.words, feeds: f, changed: Boolean(l.changed_at), changed_at: l.changed_at || null,
        remaking: { shots: remaking.filter((x) => x.kind === 'shot').length, lines: remaking.filter((x) => x.kind === 'line').length } };
    });
    return { id: b.id, no: i + 1, name: b.name || names[b.id]?.name || autoName(b.words), name_auto: !b.name, name_from: b.name ? 'script' : names[b.id]?.from || 'words', colour: node?.colour || null,
      visual: b.visual || null, target_sec: b.target_sec ?? null, state: node?.state || null, lines, deleted };
  });
  // While the AI writes, the newest line is the one being written: read-only until it is done.
  const last = busy ? beats.at(-1)?.lines.at(-1) : null;
  for (const b of beats) for (const l of b.lines) l.read_only = l === last;
  const d = readDraft(root, id);
  return { project: id, rev: g.rev, writing: busy, version: s.version ?? null, beats, draft: d?.draft ?? null, draft_at: d?.at ?? null };
}

// ---- the owner's unsaved script changes (the app's draft), kept per project so nothing typed is lost ----
// The app sends its draft whenever it changes (`script.draft {draft}`, null = none); it lives beside the canvas in
// state/<id>/canvas/script-draft.json, so it survives a service restart, a project switch and a crash. The service
// never reads into it: it is the app's own shape, handed back as is by script.get. Saving the script clears it.
const MAX_DRAFT = 256 * 1024;
const draftFile = (root, id) => join(canvasPaths(root, id).dir, 'script-draft.json');

export function readDraft(root, id) {
  const d = readOr(draftFile(root, id), null);
  return d && typeof d === 'object' && d.draft && typeof d.draft === 'object' && !Array.isArray(d.draft) ? d : null;
}

export function saveDraft(root, id, draft) {
  const f = draftFile(root, id);
  if (draft === null || draft === undefined) {
    rmSync(f, { force: true });
    return { project: id, saved: false, cleared: true };
  }
  if (typeof draft !== 'object' || Array.isArray(draft)) throw new CanvasError('the draft is an object (or null)');
  const text = JSON.stringify({ draft, at: new Date().toISOString() });
  if (text.length > MAX_DRAFT) throw new CanvasError(`the draft is at most ${MAX_DRAFT / 1024} KB`);
  mkdirSync(canvasPaths(root, id).dir, { recursive: true });
  writeFileSync(`${f}.tmp`, text);
  renameSync(`${f}.tmp`, f);
  return { project: id, saved: true, cleared: false };
}

// script.edit: one edit, or {edits: [...]} (the app's Save sends every draft change at once). Each edit:
//   {beat, beat_name}                 rename the beat ("" = back to its first words)
//   {beat, line, words}               new words for a line
//   {beat, add_after, words, key?}    a new line after `add_after` (a line id, a `key` from this batch, or null = first)
//   {beat, line, delete: true|false}  delete a line (kept, hidden) or bring it back
// Writes script/beats.json (and voice/lines.json once the voice exists) and returns what changed; the caller asks for
// the remakes (requestRemakes) so they go through the canvas's own Fix path.
export function applyScriptEdits(root, id, params) {
  const list = Array.isArray(params?.edits) ? params.edits : [params];
  if (!list.length) throw new CanvasError('no edits');
  if (list.length > MAX_EDITS) throw new CanvasError(`at most ${MAX_EDITS} edits at once`);
  if (writing(root, id) && list.some((e) => !e?.beat_name && e?.line)) {
    // the line the AI is writing right now is read-only
    const v = scriptView(root, id);
    const busy = v.beats.flatMap((b) => b.lines).find((l) => l.read_only);
    if (busy && list.some((e) => e?.line === busy.id)) throw new CanvasError(`${busy.id} is still being written`);
  }
  const work = projectPaths(root, id).work;
  const cp = canvasPaths(root, id);
  mkdirSync(cp.dir, { recursive: true });
  const at = new Date().toISOString();
  const decisions = loadOwner(root, id).decisions || {};
  return withLock(join(cp.dir, '.script.lock'), () => {
    const s = readScript(work);
    if (!s.beats.length) throw new CanvasError('there is no script yet');
    const { raw: voiceRaw, list: voice } = readVoice(work);
    const out = { changed: [], added: [], keys: {}, deleted: [], restored: [], renamed: [] };
    const touched = new Set();
    let voiceDirty = false;
    for (const e of list) {
      if (!e || typeof e !== 'object') throw new CanvasError('each edit is an object');
      const beat = s.beats.find((b) => b.id === String(e.beat || ''));
      if (!beat) throw new CanvasError(`no beat ${String(e.beat || '').slice(0, 20) || '(none)'} in the script`);
      if (!Array.isArray(beat.lines)) beat.lines = linesOf(beat, voice);
      const words = e.words === undefined || e.words === null ? null : String(e.words).replace(/\s+/g, ' ').trim();
      if (words !== null && words.length > MAX_WORDS) throw new CanvasError(`a line is at most ${MAX_WORDS} characters`);
      if (e.beat_name !== undefined) {
        const name = String(e.beat_name ?? '').replace(/\s+/g, ' ').trim().slice(0, 80);
        if (name) beat.name = name; else delete beat.name;
        out.renamed.push(beat.id);
        touched.add(beat.id);
        if (!e.line && e.add_after === undefined) continue;
      }
      if (e.add_after !== undefined) {
        if (!words) throw new CanvasError('a new line needs its words');
        const after = e.add_after === null || e.add_after === '' ? null : out.keys[String(e.add_after)] || String(e.add_after);
        const at_ = after === null ? -1 : beat.lines.findIndex((l) => l.id === after);
        if (after !== null && at_ < 0) throw new CanvasError(`no line ${after} in ${beat.id}`);
        // ids are forever: the next number after every line the beat ever had (deleted ones and voice lines too)
        const used = [...beat.lines, ...voice.filter((l) => String(l.id || '').startsWith(`${beat.id}-`))].map((l) => Number(String(l.id).split('-')[1]) || 0);
        const nid = `${beat.id}-${Math.max(0, ...used) + 1}`;
        beat.lines.splice(at_ + 1, 0, { id: nid, words, changed_at: at });
        if (e.key !== undefined) out.keys[String(e.key).slice(0, 40)] = nid;
        out.added.push(nid);
        // once the beat has voice lines, the new line is one too (in the beat's place in voice/lines.json)
        const beatVoice = voice.filter((l) => String(l.id || '').startsWith(`${beat.id}-`));
        if (voiceRaw && beatVoice.length) {
          const prev = after && voice.findIndex((l) => l.id === after);
          const idx = prev !== null && prev >= 0 ? prev + 1 : voice.indexOf(beatVoice[0]);
          voice.splice(idx, 0, { id: nid, script: words });
          voiceDirty = true;
        }
        touched.add(beat.id);
        continue;
      }
      const lid = out.keys[String(e.line || '')] || String(e.line || '');
      const line = beat.lines.find((l) => l.id === lid);
      if (!line) throw new CanvasError(`no line ${lid.slice(0, 20) || '(none)'} in ${beat.id}`);
      if (e.delete === true || e.delete === false) {
        if (e.delete) {
          if (line.deleted) continue;
          line.deleted = true;
          if (decisions[lid]?.verdict === 'ok') line.was = 'ok';
          out.deleted.push(lid);
        } else {
          if (!line.deleted) continue;
          out.restored.push(lid);
          // remakesFor reads `was` from this list (the file no longer has it once the line is back)
          out.was = { ...(out.was || {}), ...(line.was ? { [lid]: line.was } : {}) };
          delete line.deleted;
          delete line.was;
        }
        line.changed_at = at;
        touched.add(beat.id);
        continue;
      }
      if (!words) throw new CanvasError('the new words are empty (delete the line instead)');
      if (words === line.words) continue;
      line.words = words;
      line.changed_at = at;
      if (!out.changed.includes(lid)) out.changed.push(lid);
      const v = voice.find((l) => l.id === lid);
      if (v) { v.script = words; if ('text' in v) v.text = words; voiceDirty = true; }
      touched.add(beat.id);
    }
    // The beat's words are its lines as one paragraph (the canvas's beat node, the chat card, the brief).
    for (const b of s.beats.filter((x) => touched.has(x.id))) {
      b.lines = b.lines.map(keepLine);
      b.words = b.lines.filter((l) => !l.deleted).map((l) => l.words).join(' ');
    }
    mkdirSync(join(work, 'script'), { recursive: true });
    writeFileSync(join(work, 'script', 'beats.json'), JSON.stringify(s, null, 2));
    if (voiceDirty) writeVoice(work, voiceRaw, voice);
    // saved: the draft those changes came from is done (the app sends what is still unsaved again)
    rmSync(draftFile(root, id), { force: true });
    out.beats = [...touched];
    return out;
  });
}

// The remakes an applied edit asks for, as canvas actions: [{node, action: fix|cut|clear, text}]. Only pieces that
// were already made are remade; a shot used by several changed lines gets one Fix naming them all.
export function remakesFor(root, id, done) {
  const g = canvasView(root, id);
  const film = filmOrder(g, loadLayout(root, id));
  const feeds = feedsOf(g, film);
  const work = projectPaths(root, id).work;
  const s = readScript(work);
  const wordsOf = (lid) => s.beats.flatMap((b) => b.lines || []).find((l) => l.id === lid)?.words || '';
  const beatOf = (lid) => lid.split('-')[0];
  const fixes = new Map();
  const add = (node, text) => { const f = fixes.get(node) || []; f.push(text); fixes.set(node, f); };
  const actions = [];
  for (const lid of done.changed) {
    for (const f of feeds(beatOf(lid), lid).filter((x) => x.made)) {
      add(f.node, f.kind === 'line'
        ? `The owner changed this line's words in the script to: "${wordsOf(lid)}". They are already saved in script/beats.json and voice/lines.json: make a new take with exactly these words (keep it inside its slot).`
        : `The narration over this shot changed (line ${lid}, now: "${wordsOf(lid)}"). Remake this shot so it fits the new words; keep its look and length.`);
    }
  }
  for (const lid of done.added) {
    const v = g.nodes[lid];
    if (v?.kind === 'line') add(lid, `A new line the owner added to the script: "${wordsOf(lid)}". Record it in the same voice, right after the line before it.`);
  }
  for (const [node, texts] of fixes) actions.push({ node, action: 'fix', text: texts.join(' ') });
  for (const lid of done.deleted) if (g.nodes[lid]?.kind === 'line' && g.nodes[lid].owner !== 'cut') actions.push({ node: lid, action: 'cut' });
  // Brought back: the same voice line as it was (its OK too), nothing remade.
  for (const lid of done.restored) if (g.nodes[lid]?.owner === 'cut') actions.push({ node: lid, action: done.was?.[lid] === 'ok' ? 'ok' : 'clear' });
  return actions;
}

// One plain sentence for the chat's grey line: "Script: 1 line changed · remaking 1 shot and 1 voice line".
export function scriptActionText(done, remade) {
  const n = (k, one, many) => (k === 1 ? `1 ${one}` : `${k} ${many}`);
  const parts = [];
  if (done.changed.length) parts.push(`${n(done.changed.length, 'line', 'lines')} changed`);
  const added = done.added.length;
  if (added) parts.push(`${n(added, 'line', 'lines')} added`);
  if (done.deleted.length) parts.push(`${n(done.deleted.length, 'line', 'lines')} deleted`);
  if (done.restored.length) parts.push(`${n(done.restored.length, 'line', 'lines')} back`);
  if (done.renamed.length) parts.push(`${n(done.renamed.length, 'beat', 'beats')} renamed`);
  const shots = remade.filter((r) => r.kind === 'shot').length;
  const lines = remade.filter((r) => r.kind === 'line').length;
  const what = [shots && n(shots, 'shot', 'shots'), lines && n(lines, 'voice line', 'voice lines')].filter(Boolean).join(' and ');
  return `Script: ${parts.join(', ') || 'saved'}${what ? ` · remaking ${what}` : ''}`;
}

export const scriptLogged = (root, id, text) => event(root, id, { event: 'SCRIPT_EDITED', actor: 'you', details: text.slice(0, 300) });
