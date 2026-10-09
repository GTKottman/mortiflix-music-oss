// Quick edits: the owner marks bars on the MIDI they're reviewing, says what they want there ("a different drum beat"),
// and the page changes in seconds. No session runs:
//
//   1. Strudel reads the score's notes with their source locations: what plays in those bars, and the code behind it;
//   2. the context engine (src/context.mjs) briefs Claude on the piece, the section, the bars around them and the
//      owner's saved differences; Claude writes only the new bars;
//   3. the music engine's edit splice (lib/edit.mjs) forces them in (a per-bar mask, the rest untouched), and the
//      studio checks that nothing outside the marked bars moved;
//   4. the score is captured again (`strudel.mjs capture`: notes.json, the MIDI pack) and the version under review gets
//      the new MIDI, so the review room redraws.
//
// Each edit is tried until the owner keeps it, undoes it or sends the review (which keeps whatever wasn't undone). A
// kept edit becomes a saved difference (context.mjs): the studio's memory of what the owner likes. Edits live in the
// project's music/edits.json and the engine puts them back into the score whenever it's checked, so a session can't
// lose them.
import { existsSync, readFileSync, writeFileSync, copyFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { spawn } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { REPO, UserError, readJson, writeJson } from './studio.mjs';
import { projectPaths, projectPipeline, loadProject, event, now } from './projects.mjs';
import * as gates from './gates.mjs';
import { sessionToolsEnv } from './setup.mjs';
import { editContext, saveDifference } from './context.mjs';
import { ask, jsonFrom } from './quickmodel.mjs';

const LIB = join(REPO, 'pipelines', '_shared', 'skills', 'music', 'lib');
const E = await import(join(LIB, 'edit.mjs'));

const readJ = (f) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return null; } };
// A bar's mini-notation goes inside a string in the score: only the characters mini-notation uses, never a quote,
// backslash or newline that could end the string.
const SAFE = /^[\w .#~*!@,[\]:?|()<>/+%-]{1,600}$/;

function files(root, id) {
  const w = projectPaths(root, id).work;
  return { work: w, score: join(w, 'music', 'score.strudel.js'), blueprint: join(w, 'music', 'blueprint.json'), notes: join(w, 'music', 'notes.json'), edits: join(w, 'music', 'edits.json'),
    engine: existsSync(join(w, '.claude', 'skills', 'music', 'strudel.mjs')) ? join(w, '.claude', 'skills', 'music', 'strudel.mjs') : join(REPO, 'pipelines', '_shared', 'skills', 'music', 'strudel.mjs') };
}

export function listEdits(root, id) {
  return readJ(files(root, id).edits)?.edits || [];
}

function saveEdits(root, id, edits) {
  writeJson(files(root, id).edits, { how_to_read: 'The owner\'s own edits, made in the studio. status: tried (on trial), kept, undone. The music engine puts every edit that isn\'t undone back into the score.', edits });
}

// ---------- one Strudel, kept warm ----------

let warm = null; let warmKey = null; let idle = null; let queue = Promise.resolve();
const serial = (fn) => { const run = queue.then(fn, fn); queue = run.catch(() => {}); return run; };

async function strudelFor(root) {
  const env = sessionToolsEnv(root);
  if (!env.MFX_STRUDEL || !env.MFX_CHROME) throw new UserError('The music engine isn\'t set up: Settings › Setup › Music engine (or `mortiflix-music setup music`).');
  const key = `${env.MFX_STRUDEL}|${env.MFX_CHROME}`;
  if (!warm || warmKey !== key) {
    await warm?.close().catch(() => {});
    const { HeadlessStrudel } = await import(join(LIB, 'headless.mjs'));
    warm = await new HeadlessStrudel({ strudel: env.MFX_STRUDEL, chrome: env.MFX_CHROME }).start();
    warmKey = key;
  }
  clearTimeout(idle);
  idle = setTimeout(() => closeStrudel(), 5 * 60_000);
  idle.unref?.();
  return warm;
}

export async function closeStrudel() {
  clearTimeout(idle);
  const s = warm; warm = null; warmKey = null;
  await s?.close().catch(() => {});
}

// ---------- the version under review ----------

// The MIDI review the owner is looking at: its step, its version and its submission. Quick edits change that version.
function reviewing(root, id) {
  const p = loadProject(root, id);
  const steps = gates.stepView(p, projectPipeline(root, id));
  for (const s of steps.filter((x) => x.state === 'in_review')) {
    const sub = gates.submissions(root, id).find((x) => x.step === s.key && x.version === s.version);
    if (sub?.items.some((it) => it.kind === 'midi') && !sub.items.some((it) => it.kind === 'audio')) return { step: s, sub };
  }
  return null;
}

// The version under review gets the freshly captured MIDI. Its items were copied from wherever the session put them
// (out/music/midi/, or a copy of it), so each is matched to the new pack by what it is: the whole arrangement
// (…-all.mid), one channel (NN-….mid) or a section clip (sections/…).
export function packFileFor(source, packFiles) {
  const rel = String(source || '').replace(/\\/g, '/');
  const inPack = /(^|\/)out\/music\/midi\/(.+)$/.exec(rel);
  if (inPack && packFiles.includes(inPack[2])) return inPack[2];
  const name = rel.split('/').pop();
  const section = /(^|\/)(sections\/[^/]+\/[^/]+)$/.exec(rel);
  if (section && packFiles.includes(section[2])) return section[2];
  const top = packFiles.filter((f) => !f.includes('/'));
  if (/-all\.mid$/i.test(name)) return top.find((f) => /-all\.mid$/i.test(f)) || null;
  const ch = /^(\d{2})-/.exec(name);
  if (ch) return top.find((f) => f.startsWith(`${ch[1]}-`) && f.endsWith('.mid')) || null;
  return null;
}

function refreshSubmission(root, id, r) {
  const pp = projectPaths(root, id);
  const pack = join(pp.work, 'out', 'music', 'midi');
  const list = (dir, pre = '') => (existsSync(dir) ? readdirSync(dir, { withFileTypes: true }).flatMap((d) => (d.isDirectory() ? list(join(dir, d.name), `${pre}${d.name}/`) : [`${pre}${d.name}`])) : []);
  // Newest first: a pack can still hold files from before (another title's …-all.mid), and the capture just wrote the
  // ones that count.
  const packFiles = list(pack).sort((x, y) => statSync(join(pack, y)).mtimeMs - statSync(join(pack, x)).mtimeMs);
  const midiItems = r.sub.items.filter((it) => it.kind === 'midi' && it.file);
  for (const it of midiItems) {
    let match = packFileFor(it.source, packFiles);
    if (!match && midiItems.length === 1) match = packFiles.find((f) => !f.includes('/') && /-all\.mid$/i.test(f)) || null;
    if (match) copyFileSync(join(pack, match), join(pp.state, it.file));
  }
  const f = join(pp.reviews, r.step.key, `v${r.step.version}`, 'submission.json');
  const sub = readJson(f);
  sub.owner_edits = listEdits(root, id).filter((e) => e.step === r.step.key && e.version === r.step.version && e.status !== 'undone').map((e) => e.id);
  writeJson(f, sub);
}

// ---------- capture ----------

function capture(root, id) {
  const f = files(root, id);
  return new Promise((ok, fail) => {
    const p = spawn(process.execPath, [f.engine, 'capture'], { cwd: f.work, env: { ...process.env, ...sessionToolsEnv(root) }, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    p.stdout.on('data', (d) => { out += d; });
    p.stderr.on('data', (d) => { out += d; });
    p.on('error', fail);
    p.on('close', (code) => (code === 0 ? ok(out) : fail(new Error(`the score didn't capture: ${out.trim().split('\n').filter((l) => l.startsWith('✖')).join(' ') || out.trim().slice(-400)}`))));
  });
}

const warningsOf = (out) => {
  const i = out.indexOf('Captured anyway');
  if (i < 0) return [];
  const lines = out.slice(i).split('\n').slice(1);
  const end = lines.findIndex((l) => !l.trim() || l.trim().startsWith('✔'));
  return (end < 0 ? lines : lines.slice(0, end)).map((l) => l.trim()).filter(Boolean).slice(0, 6);
};

// ---------- making an edit ----------

export function makeEdit(root, id, opts) { return serial(() => doEdit(root, id, opts)); }

async function doEdit(root, id, { from, to, channel = null, ask: request, retryOf = null }) {
  const t0 = Date.now();
  from = Number(from); to = Number(to ?? from); channel = channel === null || channel === undefined || channel === '' ? null : Number(channel);
  request = String(request || '').trim();
  if (!request) throw new UserError('Say what should change in those bars.');
  const f = files(root, id);
  const r = reviewing(root, id);
  if (!r) throw new UserError('Quick edits change MIDI you\'re reviewing: there\'s no MIDI review waiting for you on this project.');
  if (!existsSync(f.score)) throw new UserError('This piece has no score to edit (music/score.strudel.js).');
  const bp = readJ(f.blueprint);
  if (!bp?.bars) throw new UserError('This piece has no blueprint with its bars.');
  if (!(from >= 1 && to >= from && to <= bp.bars)) throw new UserError(`Mark bars between 1 and ${bp.bars}.`);
  if (to - from > 15) throw new UserError('Mark up to 16 bars at a time.');
  if (channel !== null && !(Number.isInteger(channel) && channel >= 1 && channel <= 16)) throw new UserError('channel must be 1-16');
  const total = bp.bars + 2;
  const bars = Array.from({ length: to - from + 1 }, (_, i) => from + i);

  const strudel = await strudelFor(root);
  const code = readFileSync(f.score, 'utf8');
  const manifest = readJ(f.notes) || { tracks: [], beats_per_bar: 4 };
  const before = (await strudel.haps(code, total)).haps;
  const located = E.locate(code, before, { from, to });

  let edits = listEdits(root, id);
  const prior = retryOf ? edits.find((e) => e.id === retryOf) : null;
  const group = prior?.group || randomBytes(4).toString('hex');
  const attempts = edits.filter((e) => e.group === group);
  if (prior && prior.status !== 'undone') { prior.status = 'undone'; prior.undone_at = now(); prior.why_undone = 'tried another'; }

  const ctx = editContext(root, id, { from, to, channel, ask: request, manifest, code, located, attempts });
  let answer = null; let check = null; let next = null; let lastProblem = null;
  for (let tries = 0; tries < 2 && !check?.ok; tries += 1) {
    const prompt = lastProblem ? `${ctx.prompt}\n\n## Your last answer didn't work\n${lastProblem}\nAnswer again with the JSON.` : ctx.prompt;
    const text = await ask(root, { system: ctx.system, prompt, backend: loadProject(root, id).backend || null, demo: () => demoAnswer({ channel, bars, manifest, request, attempts: attempts.length + tries + edits.filter((e) => e.status !== 'undone' && e.from <= to && e.to >= from).length }) });
    try { answer = cleanAnswer(jsonFrom(text), { channel, bars }); } catch (e) { lastProblem = e.message; continue; }
    const edit = { id: randomBytes(5).toString('hex'), group, channel: answer.channel, bars: answer.bars, from, to, ask: request, said: answer.said,
      step: r.step.key, version: r.step.version, status: 'tried', at: now() };
    next = E.renderEdits(code, [...edits.filter((e) => e.status !== 'undone'), edit], total);
    try {
      const after = (await strudel.haps(next.code, total)).haps;
      check = E.verifyEdit(before, after, { channel: answer.channel, bars });
      if (!check.ok) lastProblem = `The studio refused it: ${check.problems.join('; ')}.`;
      else { edits.push(edit); answer.edit = edit; }
    } catch (e) { lastProblem = `It didn't evaluate in Strudel: ${e.message}`; check = null; }
  }
  if (!check?.ok) throw new UserError(`Couldn't make that change: ${lastProblem || 'no usable answer'}`);

  // Write it, capture it, and give the version under review its new MIDI.
  const edit = answer.edit;
  writeFileSync(f.score, next.code);
  saveEdits(root, id, edits);
  let out;
  try { out = await capture(root, id); } catch (e) {
    // Put everything back as it was: the owner's page never shows a half-made edit.
    writeFileSync(f.score, code);
    saveEdits(root, id, edits.filter((x) => x.id !== edit.id).map((x) => (x.id === prior?.id ? { ...x, status: 'tried', undone_at: undefined, why_undone: undefined } : x)));
    throw e;
  }
  const after = readJ(f.notes) || manifest;
  const drums = edit.channel === 10;
  const notesIn = (m) => (m.tracks.find((t) => t.channel === edit.channel)?.notes || []).filter((n) => n.start >= (from - 1) * (m.beats_per_bar || 4) - 1e-6 && n.start < to * (m.beats_per_bar || 4) - 1e-6);
  // Measured from the bars as they were before the first try, so "try another" still says what changed from the music.
  const base = prior?.channel === edit.channel && prior.from === from && prior.to === to && prior.base_notes ? prior.base_notes : notesIn(manifest);
  edit.base_notes = base;
  edit.facts = E.describeDifference(base, notesIn(after), { drums, bars: bars.length, beatsPerBar: after.beats_per_bar || 4 });
  edit.before = E.notesText(base, { drums, from, to, beatsPerBar: manifest.beats_per_bar || 4 });
  edit.after = E.notesText(notesIn(after), { drums, from, to, beatsPerBar: after.beats_per_bar || 4 });
  edit.part = after.tracks.find((t) => t.channel === edit.channel)?.name || `channel ${edit.channel}`;
  edit.section = ctx.sections[0] ? { name: ctx.sections[0].name, function: ctx.sections[0].function || null, intensity: ctx.sections[0].intensity } : null;
  edit.warnings = warningsOf(out);
  saveEdits(root, id, edits);
  if (prior) saveDifference(root, differenceOf(root, id, prior, 'undone'));
  refreshSubmission(root, id, r);
  event(root, id, { event: 'OWNER_EDIT', actor: 'you', step: r.step.key, version: r.step.version, details: `bars ${from}-${to}, ${edit.part}: "${request}" → ${edit.facts.join('; ')}` });
  return { edit, seconds: +((Date.now() - t0) / 1000).toFixed(1) };
}

export function cleanAnswer(a, { channel, bars }) {
  const ch = channel ?? Number(a.channel);
  if (!(Number.isInteger(ch) && ch >= 1 && ch <= 16)) throw new Error(`"channel" must be the MIDI channel you changed (1-16), not ${JSON.stringify(a.channel)}.`);
  const out = {};
  for (const bar of bars) {
    const v = a.bars?.[bar] ?? a.bars?.[String(bar)];
    if (!v || typeof v.notes !== 'string' || !v.notes.trim()) throw new Error(`"bars" needs an entry for bar ${bar} with "notes".`);
    const notes = v.notes.trim().replace(/^"|"$/g, '');
    const velocity = v.velocity === undefined || v.velocity === null || v.velocity === '' ? '.8' : String(v.velocity).trim();
    if (!SAFE.test(notes)) throw new Error(`bar ${bar}: "notes" has characters mini-notation doesn't use (no quotes, backslashes or new lines).`);
    if (!SAFE.test(velocity)) throw new Error(`bar ${bar}: "velocity" must be a number or a pattern of numbers.`);
    out[bar] = { notes, velocity };
  }
  return { channel: ch, bars: out, said: String(a.said || '').slice(0, 300) };
}

// ---------- keeping and undoing ----------

export function keepEdit(root, id, eid) {
  return serial(async () => {
    const edits = listEdits(root, id);
    const e = edits.find((x) => x.id === eid);
    if (!e) throw new UserError('no such edit');
    if (e.status === 'undone') throw new UserError('that edit was undone');
    if (e.status !== 'kept') { e.status = 'kept'; e.kept_at = now(); saveEdits(root, id, edits); saveDifference(root, differenceOf(root, id, e, 'kept')); }
    return { edit: e };
  });
}

export function undoEdit(root, id, eid) {
  return serial(async () => {
    const f = files(root, id);
    const edits = listEdits(root, id);
    const e = edits.find((x) => x.id === eid);
    if (!e) throw new UserError('no such edit');
    if (e.status === 'undone') return { edit: e };
    const r = reviewing(root, id);
    if (!r || r.step.key !== e.step || r.step.version !== e.version) throw new UserError('that version isn\'t waiting for your review any more');
    const code = readFileSync(f.score, 'utf8');
    const bp = readJ(f.blueprint);
    e.status = 'undone'; e.undone_at = now();
    writeFileSync(f.score, E.renderEdits(code, edits, bp.bars + 2).code);
    saveEdits(root, id, edits);
    try { await capture(root, id); } catch (err) {
      e.status = 'tried'; delete e.undone_at; saveEdits(root, id, edits); writeFileSync(f.score, code); throw err;
    }
    saveDifference(root, differenceOf(root, id, e, 'undone'));
    refreshSubmission(root, id, r);
    event(root, id, { event: 'OWNER_EDIT_UNDONE', actor: 'you', step: e.step, version: e.version, details: `bars ${e.from}-${e.to}, ${e.part}` });
    return { edit: e };
  });
}

// What the review room needs: the edits on this project, and whether the version under review can be edited now.
export function editState(root, id) {
  const r = reviewing(root, id);
  const scored = existsSync(files(root, id).score);
  return { edits: listEdits(root, id), can: Boolean(r && scored), step: r?.step.key || null, version: r?.step.version || null,
    why: !r ? 'only while a MIDI review is waiting for you' : !scored ? 'this piece has no score to edit' : null };
}

// gates.respond, then settleEdits: every way of sending a review (web, CLI) goes through here.
export function respond(root, id, step, version, body) {
  const r = gates.respond(root, id, step, version, body);
  settleEdits(root, id, step, version);
  return r;
}

// Sending the review keeps every edit on it that wasn't undone: approving takes them, and asking for changes hands
// them to the next session as the owner's own (the engine keeps them in the score).
export function settleEdits(root, id, step, version) {
  const edits = listEdits(root, id);
  let n = 0;
  for (const e of edits) {
    if (e.step === step && e.version === version && e.status === 'tried') { e.status = 'kept'; e.kept_at = now(); saveDifference(root, differenceOf(root, id, e, 'kept')); n += 1; }
  }
  if (n) saveEdits(root, id, edits);
  return n;
}

function differenceOf(root, id, e, status) {
  const p = loadProject(root, id);
  return { id: e.id, status, project: id, title: p.title, genre: p.intake?.answers?.genre || null, channel: e.channel, kind: e.channel === 10 ? 'drums' : 'pitched',
    part: e.part || null, section: e.section || null, bars: [e.from, e.to], ask: e.ask, said: e.said || null, facts: e.facts || [], before: e.before || null, after: e.after || null, at: now() };
}

// ---------- the demo ----------

// No Claude in the demo: a fixed set of variations, a new one each try, so the whole loop can be tried for free.
function demoAnswer({ channel, bars, manifest, request, attempts }) {
  const drums = channel === 10 || (channel === null && /beat|drum|groove|kick|snare|hat|fill/i.test(request)) || (channel === null && !manifest.tracks.some((t) => t.channel !== 10));
  const ch = drums ? 10 : channel ?? manifest.tracks.find((t) => t.channel !== 10)?.channel ?? 1;
  const BEATS = ['[36 ~ ~ 36 ~ 36 38 ~], [42*16]', '[36 ~ 38 ~ ~ 36 38 36], [42 ~ 46 ~]*2', '[36 36 ~ 38 ~ 36 [38 38] ~], [42 42 ~ 42]*2', '[36 ~ 38 36 ~ 36 38 ~], [44 42]*4'];
  const out = {};
  for (const bar of bars) {
    if (drums) { out[bar] = { notes: BEATS[(attempts + bar) % BEATS.length], velocity: '.85' }; continue; }
    const bpb = manifest.beats_per_bar || 4;
    const t = manifest.tracks.find((x) => x.channel === ch);
    const ps = [...new Set((t?.notes || []).filter((n) => n.start >= (bar - 1) * bpb && n.start < bar * bpb).map((n) => n.pitch))].sort((a, b) => a - b);
    const names = (ps.length ? ps : [57, 60, 64]).map((p) => `${['c', 'c#', 'd', 'd#', 'e', 'f', 'f#', 'g', 'g#', 'a', 'a#', 'b'][p % 12]}${Math.floor(p / 12) - 1}`);
    const arp = attempts % 2 ? [...names].reverse() : names;
    out[bar] = { notes: `[${[...arp, ...arp].slice(0, 8).join(' ')}]`, velocity: '.7' };
  }
  return JSON.stringify({ channel: ch, bars: out, said: `Demo: a placeholder ${drums ? 'beat' : 'figure'} (no Claude in the demo).` });
}
