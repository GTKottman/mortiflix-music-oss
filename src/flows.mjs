// Flows: the shape a piece takes. A pipeline decides what you get and where you review it; a flow decides how the music
// moves inside it (its energy curve and rules, the techniques that make it, what it avoids). A flow is a folder:
//
//   flow.json   name, "for" (song or kit), its bucket (by shape) and feels, the curve, rules and techniques
//   FLOW.md     the craft: how a session writes a piece in this flow
//
// Built-in flows live in flows/; the studio's own (saved from your pieces, or added from someone else's zip) in the
// studio's flows/, and the same slug there wins. A project pins a copy of its flow when it's created.
//
// Taste: rating a finished piece (love it, it's fine, not for me) teaches the studio which flows and techniques land
// with you. When the studio picks a flow it leans toward what you love, steers away from what you've had lately, and
// now and then tries one you haven't heard yet, so two studios grow apart as their owners use them.
import { existsSync, readdirSync, mkdirSync, cpSync, rmSync, writeFileSync, mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { REPO, paths, readJson, writeJson, UserError } from './studio.mjs';
import { hashDir } from './pipelines.mjs';
import { listProjects, loadProject, projectPaths, update, event, now } from './projects.mjs';
import { zip, unzip, dirEntries } from './zip.mjs';
import { TECHNIQUES, canonical, heard } from '../pipelines/_shared/skills/music/lib/techniques.mjs';
import { RULES } from '../pipelines/_shared/skills/music/lib/flow.mjs';

const KEY = /^[a-z0-9][a-z0-9_-]{0,47}$/;
export const KINDS = { song: 'songs and MIDI packs', kit: 'loop kits' };
// The shape buckets, in the order they're shown. A flow can name its own bucket; these are the built-in ones.
export const BUCKETS = {
  builds: 'Builds: the energy climbs to somewhere',
  cycles: 'Cycles: something keeps coming back',
  stories: 'Stories: something changes for good',
  forms: 'Forms: a classic way of handling an idea',
};
export const FEELS = ['hypnotic', 'cinematic', 'danceable', 'intimate', 'restless', 'uplifting', 'dark', 'playful', 'epic', 'melancholic', 'warm'];
export const RATINGS = { love: { label: 'Love it', score: 2 }, fine: { label: 'It\'s fine', score: 0.5 }, no: { label: 'Not for me', score: -1.5 } };
const KIT_ONLY = ['pairs', 'shared_drums', 'layers'];

export const builtInFlowsDir = () => join(REPO, 'flows');
const studioFlowsDir = (root) => join(paths(root).root, 'flows');
const tasteFile = (root) => join(paths(root).root, 'taste.json');

// ---- the flows on disk ----

export function listFlows(root, kind = null) {
  const found = new Map();
  for (const base of [builtInFlowsDir(), root ? studioFlowsDir(root) : null]) {
    if (!base || !existsSync(base)) continue;
    for (const slug of readdirSync(base).sort()) {
      const dir = join(base, slug);
      if (!existsSync(join(dir, 'flow.json'))) continue;
      const source = base === builtInFlowsDir() ? 'built-in' : 'yours';
      try { found.set(slug, { ...loadFlow(dir), dir, source }); } catch (e) { found.set(slug, { slug, dir, source, error: e.message }); }
    }
  }
  return [...found.values()].filter((f) => !kind || f.error || f.for === kind);
}

export function findFlow(root, slug, kind = null) {
  const f = listFlows(root).find((x) => x.slug === slug);
  if (!f) throw new UserError(`no flow "${slug}" (see: mortiflix-music flows)`);
  if (f.error) throw new UserError(`flow "${slug}" is invalid: ${f.error}`);
  if (kind && f.for !== kind) throw new UserError(`"${f.name}" is a flow for ${KINDS[f.for]}, not ${KINDS[kind]}`);
  return f;
}

export function loadFlow(dir) {
  const def = readJson(join(dir, 'flow.json'));
  const errors = validateFlow(def, dir);
  if (errors.length) throw new Error(errors.join('; '));
  return { ...def, feels: def.feels || [], techniques: { required: [], avoid: [], free: 0, ...def.techniques }, shape: { rules: [], ...def.shape }, hash: hashDir(dir) };
}

export function validateFlow(def, dir) {
  const errors = [];
  const need = (c, m) => { if (!c) errors.push(m); };
  need(def && typeof def === 'object', 'flow.json is not a JSON object');
  if (errors.length) return errors;
  need(KEY.test(def.slug || ''), 'slug: lowercase letters, digits, - or _');
  need(typeof def.name === 'string' && def.name.trim(), 'name is required');
  need(Object.keys(KINDS).includes(def.for), `"for" must be ${Object.keys(KINDS).join(' or ')}`);
  need(KEY.test(def.bucket || ''), 'bucket: one word (builds, cycles, stories, forms, or your own)');
  need(Array.isArray(def.feels ?? []) && (def.feels ?? []).length <= 5 && (def.feels ?? []).every((f) => /^[a-z][a-z -]{1,23}$/.test(f)), 'feels: up to 5 lowercase words');
  need(typeof def.summary === 'string' && def.summary.trim().length >= 10 && def.summary.length <= 400, 'summary: one or two sentences (10-400 characters)');
  const sh = def.shape || {};
  if (sh.curve !== undefined) {
    const ok = Array.isArray(sh.curve) && sh.curve.length >= 2 && sh.curve.every((p, i) => Array.isArray(p) && p.length === 2 && p[0] >= 0 && p[0] <= 1 && p[1] >= 0 && p[1] <= 10 && (!i || p[0] > sh.curve[i - 1][0]));
    need(ok, 'shape.curve: 2+ points [position 0-1, intensity 0-10], positions rising');
  }
  for (const r of sh.rules || []) {
    need(RULES[r], `shape.rules: unknown rule "${r}" (${Object.keys(RULES).join(', ')})`);
    if (RULES[r] && def.for === 'song') need(!KIT_ONLY.includes(r), `shape.rules: "${r}" is for loop kits`);
    if (RULES[r] && def.for === 'kit') need(RULES[r].kit, `shape.rules: "${r}" is for songs`);
  }
  if (def.sections !== undefined) need(Number.isInteger(def.sections.min) && Number.isInteger(def.sections.max) && def.sections.min >= 1 && def.sections.min <= def.sections.max && def.sections.max <= 32, 'sections: { "min", "max" } whole numbers, 1-32');
  const t = def.techniques || {};
  for (const k of ['required', 'avoid']) {
    need(Array.isArray(t[k] ?? []), `techniques.${k}: a list of names`);
    for (const name of t[k] || []) need(canonical(name), `techniques.${k}: "${name}" isn't on the technique list`);
  }
  const req = new Set((t.required || []).map(canonical));
  for (const name of t.avoid || []) need(!req.has(canonical(name)), `"${name}" is both required and avoided`);
  need(t.free === undefined || (Number.isInteger(t.free) && t.free >= 0 && t.free <= 8), 'techniques.free: 0-8 (how many techniques the piece picks itself)');
  if (dir) need(existsSync(join(dir, 'FLOW.md')), 'FLOW.md is missing');
  return errors;
}

export function snapshotFlow(flow, dest) {
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });
  cpSync(flow.dir, dest, { recursive: true, dereference: true });
}

// ---- taste ----

export function readTaste(root) {
  const t = existsSync(tasteFile(root)) ? readJson(tasteFile(root)) : null;
  return { ratings: {}, ...t };
}

// Every piece the studio made with a flow, newest first (drafts and cancelled pieces don't count).
function history(root) {
  return listProjects(root).filter((p) => p.flow && !['draft', 'cancelled'].includes(p.state));
}

// What each flow has meant to you so far: how often it was used, how you rated it.
export function flowStats(root) {
  const taste = readTaste(root); const stats = {};
  const get = (slug) => (stats[slug] ||= { used: 0, love: 0, fine: 0, no: 0, score: 0 });
  for (const p of history(root)) get(p.flow.slug).used++;
  for (const r of Object.values(taste.ratings)) { if (!r.flow) continue; const s = get(r.flow); s[r.rating]++; s.score += RATINGS[r.rating].score; }
  return stats;
}

// Techniques you love (in pieces you loved), techniques that weren't for you, and the ones used lately.
export function techniqueTaste(root) {
  const taste = readTaste(root); const score = new Map();
  for (const r of Object.values(taste.ratings)) for (const t of r.techniques || []) score.set(t, (score.get(t) || 0) + RATINGS[r.rating].score);
  const recent = [...new Set(history(root).slice(0, 3).flatMap((p) => plannedTechniques(root, p.id)))];
  return {
    loved: [...score.entries()].filter(([, s]) => s >= 2).sort((a, b) => b[1] - a[1]).map(([t]) => t),
    disliked: [...score.entries()].filter(([, s]) => s <= -1.5).sort((a, b) => a[1] - b[1]).map(([t]) => t),
    recent,
  };
}
function plannedTechniques(root, id) {
  try {
    const bp = readJson(join(projectPaths(root, id).work, 'music', 'blueprint.json'));
    return [...new Set((bp.techniques || []).map((t) => canonical(t.name)).filter(Boolean))];
  } catch { return []; }
}

// The studio's pick: weighted by your ratings, away from what you've had lately, with room to try something new.
// among: null, { bucket } or { feel }. rng is injectable for tests. Returns { flow, why }.
export function pickFlow(root, kind, { among = null, rng = Math.random } = {}) {
  let cands = listFlows(root, kind).filter((f) => !f.error);
  if (among?.bucket) cands = cands.filter((f) => f.bucket === among.bucket);
  if (among?.feel) cands = cands.filter((f) => f.feels.includes(among.feel));
  if (!cands.length) throw new UserError(`no flow for ${KINDS[kind]}${among?.bucket ? ` in "${among.bucket}"` : among?.feel ? ` that feels ${among.feel}` : ''}`);
  const stats = flowStats(root);
  const all = listFlows(root, kind).filter((f) => !f.error);
  const recent = history(root).map((p) => p.flow.slug).filter((s) => all.some((f) => f.slug === s));
  // A feel you keep loving lifts every flow that shares it.
  const feelScore = {};
  for (const f of all) for (const feel of f.feels || []) feelScore[feel] = (feelScore[feel] || 0) + (stats[f.slug]?.score || 0);
  const weighed = cands.map((f) => {
    const s = stats[f.slug] || { used: 0, score: 0 };
    let w = 1 + s.score + (s.used ? 0 : 0.75) + 0.3 * Math.max(0, ...f.feels.map((x) => feelScore[x] || 0));
    const age = recent.indexOf(f.slug);
    if (age === 0 || age === 1) w *= 0.15; else if (age >= 2 && age <= 5) w *= 0.5;
    return { f, w: Math.max(0.05, w), s, age };
  });
  const total = weighed.reduce((a, x) => a + x.w, 0);
  let roll = rng() * total; let chosen = weighed.at(-1);
  for (const x of weighed) { roll -= x.w; if (roll <= 0) { chosen = x; break; } }
  const { f, s, age } = chosen;
  const reasons = [];
  if (s.love) reasons.push(`you loved it ${s.love === 1 ? 'once' : `${s.love} times`}`);
  if (!s.used) reasons.push('you haven\'t heard it yet');
  if (s.used && (age < 0 || age > 5)) reasons.push('not in your last few pieces');
  if (!reasons.length) reasons.push(s.no ? 'a second chance' : 'a fresh turn');
  return { flow: f, why: `${reasons.join('; ')}${among?.bucket ? ` (from ${among.bucket})` : among?.feel ? ` (feels ${among.feel})` : ''}` };
}

// What the brief asked for → the flow. choice: a slug, "studio" (or nothing), "shape:<bucket>", "feel:<feel>", "none".
export function resolveFlow(root, kind, choice, { rng } = {}) {
  if (!kind) return null;
  const c = String(choice ?? 'studio').trim();
  if (c === 'none') return null;
  if (!listFlows(root, kind).some((f) => !f.error)) return null;
  if (c === 'studio' || c === '') return { ...pickFlow(root, kind, { rng }), picked: 'studio' };
  const m = c.match(/^(shape|bucket|feel):(.+)$/);
  if (m) return { ...pickFlow(root, kind, { among: m[1] === 'feel' ? { feel: m[2] } : { bucket: m[2] }, rng }), picked: 'studio' };
  return { flow: findFlow(root, c, kind), why: 'your choice', picked: 'you' };
}

// Rating a piece: love, fine or no. Remembered with its flow and the techniques it planned.
export function rateProject(root, id, rating) {
  if (!RATINGS[rating]) throw new UserError(`rate it ${Object.keys(RATINGS).join(', ')}`);
  const p = loadProject(root, id);
  const taste = readTaste(root);
  taste.ratings[id] = { rating, flow: p.flow?.slug || null, techniques: plannedTechniques(root, id), title: p.title, at: now() };
  writeJson(tasteFile(root), taste);
  update(root, id, (x) => { x.rating = rating; });
  event(root, id, { event: 'RATED', actor: 'you', details: `${RATINGS[rating].label}${p.flow ? ` (flow: ${p.flow.name})` : ''}` });
  return taste.ratings[id];
}

// ---- making a flow from a piece you made ----

// The piece's shape (its sections' intensities as a curve), the techniques it actually plays, its motif and ending,
// as a new flow in the studio. It keeps the bucket and feels of the flow it came from unless you name new ones.
export function saveFlowFrom(root, id, { name, bucket = null, feels = null, replace = false } = {}) {
  const p = loadProject(root, id);
  const work = projectPaths(root, id).work;
  const bp = existsSync(join(work, 'music', 'blueprint.json')) ? readJson(join(work, 'music', 'blueprint.json')) : null;
  if (!bp?.sections?.length) throw new UserError('this piece has no blueprint yet: a flow is made from a planned piece');
  const title = String(name || '').trim();
  if (title.length < 2) throw new UserError('name the flow');
  const slug = title.toLowerCase().normalize('NFKD').replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40);
  if (!KEY.test(slug)) throw new UserError('name the flow with letters or digits');
  const dest = join(studioFlowsDir(root), slug);
  if (existsSync(dest) && !replace) throw new UserError(`you already have a flow "${slug}" (replace it, or pick another name)`);
  if (!replace && existsSync(join(builtInFlowsDir(), slug))) throw new UserError(`"${slug}" is a built-in flow's name: pick another`);
  const from = p.flow ? (() => { try { return loadFlow(join(projectPaths(root, id).state, 'flow')); } catch { return null; } })() : null;
  const kind = bp.loops ? 'kit' : 'song';
  const n = bp.sections.length;
  const curve = bp.sections.map((s, i) => [+(bp.loops ? (n === 1 ? 0 : i / (n - 1)) : (s.start_bar - 1 + (s.end_bar - s.start_bar + 1) / 2) / bp.bars).toFixed(3), Math.max(0, Math.min(10, Number(s.intensity) || 0))]);
  if (curve.length === 1) curve.push([1, curve[0][1]]);
  if (curve[0][0] > 0) curve.unshift([0, curve[0][1]]);
  if (curve.at(-1)[0] < 1) curve.push([1, curve.at(-1)[1]]);
  // The techniques it really plays: the planned ones the check heard, then whatever else the notes show.
  const check = existsSync(join(work, 'music', 'check.json')) ? readJson(join(work, 'music', 'check.json')) : null;
  const notes = existsSync(join(work, 'music', 'notes.json')) ? readJson(join(work, 'music', 'notes.json')) : null;
  const playedPlanned = (check?.techniques || []).filter((t) => t.found && !['sound', 'lock'].includes(t.kind)).map((t) => t.name);
  let required = [...new Set(playedPlanned)];
  if (required.length < 3 && notes) {
    for (const h of heard(notes, bp)) if (!required.includes(h.name) && !['Repetition'].includes(h.name)) required.push(h.name);
  }
  required = required.slice(0, 5).filter((t) => TECHNIQUES[t].kind !== 'motif' || bp.motif?.at);
  const asked = bp.sections.map((s) => Number(s.intensity) || 0);
  const pk = asked.indexOf(Math.max(...asked));
  const rules = [];
  if (asked.every((v, i) => !i || v >= asked[i - 1])) rules.push('rising');
  if (kind === 'song' && (bp.sections[pk].start_bar - 1) / bp.bars >= 0.6) rules.push('peak_late');
  if (kind === 'song' && (bp.sections[pk].start_bar - 1) / bp.bars <= 0.3) rules.push('peak_first');
  const def = {
    slug, name: title, for: kind,
    bucket: KEY.test(bucket || '') ? bucket : from?.bucket || 'yours',
    feels: (Array.isArray(feels) ? feels : from?.feels || []).filter((f) => /^[a-z][a-z -]{1,23}$/.test(f)).slice(0, 5),
    summary: `The shape of "${p.title}"${bp.dramatic_reading ? `: ${bp.dramatic_reading}` : ''}.`.slice(0, 400),
    from: { project: id, title: p.title, flow: p.flow?.slug || null },
    shape: { curve, rules },
    ...(kind === 'song' ? { sections: { min: Math.max(1, n - 1), max: n + 1 } } : {}),
    techniques: { required, avoid: from?.techniques?.avoid?.filter((t) => !required.includes(canonical(t))) || [], free: 2 },
    motif: bp.motif ? [bp.motif.role, bp.motif.rhythm, bp.motif.contour].filter(Boolean).join('; ') : undefined,
    ending: bp.ending || undefined,
  };
  const md = [`# ${title}`, '',
    `Made from "${p.title}"${p.flow ? ` (which followed "${p.flow.name}")` : ''}. ${def.summary}`, '',
    '## How it moved, section by section', '',
    ...bp.sections.map((s) => `- **${s.name}** (${s.end_bar - s.start_bar + 1} bar${s.end_bar === s.start_bar ? '' : 's'}, intensity ${s.intensity}): ${s.function}`), '',
    required.length ? `## Its techniques\n\n${required.map((t) => { const w = (bp.techniques || []).find((x) => canonical(x.name) === t); return `- **${t}**${w?.how ? `: ${w.how}` : ''}`; }).join('\n')}\n` : '',
    bp.ending ? `## The ending\n\n${bp.ending}\n` : '',
    'Write a new piece in this shape: the same rise and fall, section by section, with new material. Keep the proportions, not the notes.', ''].join('\n');
  const errors = validateFlow(def, null);
  if (errors.length) throw new UserError(`couldn't make a flow from this piece: ${errors.join('; ')}`);
  rmSync(dest, { recursive: true, force: true });
  mkdirSync(dest, { recursive: true });
  writeFileSync(join(dest, 'flow.json'), JSON.stringify(def, null, 2) + '\n');
  writeFileSync(join(dest, 'FLOW.md'), md.replace(/\n{3,}/g, '\n\n'));
  event(root, id, { event: 'FLOW_SAVED', actor: 'you', details: `${title} (${slug})` });
  return { ...loadFlow(dest), dir: dest, source: 'yours' };
}

// ---- sharing ----

export function exportFlow(root, slug) {
  const f = findFlow(root, slug);
  return { slug: f.slug, name: f.name, zip: zip(dirEntries(f.dir, f.slug)) };
}

export function addFlow(root, buf, { replace = false } = {}) {
  const entries = unzip(buf, { maxBytes: 5 * 1024 * 1024 });
  const top = entries.find((e) => /^([^/]+\/)?flow\.json$/.test(e.name));
  if (!top) throw new UserError('no flow.json in that zip (a shared flow is a folder with flow.json and FLOW.md)');
  const prefix = top.name.slice(0, -'flow.json'.length);
  const tmp = mkdtempSync(join(tmpdir(), 'mfx-flow-'));
  try {
    for (const e of entries) {
      if (!e.name.startsWith(prefix)) continue;
      const rel = e.name.slice(prefix.length);
      if (!/^[A-Za-z0-9._-]+$/.test(rel)) throw new UserError(`a flow is two files (flow.json, FLOW.md); this zip has ${e.name}`);
      writeFileSync(join(tmp, rel), e.data);
    }
    const def = loadFlow(tmp);
    const dest = join(studioFlowsDir(root), def.slug);
    if (existsSync(dest) && !replace) throw new UserError(`this studio already has a flow "${def.slug}" (add --replace to swap it)`);
    rmSync(dest, { recursive: true, force: true });
    mkdirSync(dest, { recursive: true });
    cpSync(tmp, dest, { recursive: true });
    return { slug: def.slug, name: def.name, for: def.for, overrides_built_in: existsSync(join(builtInFlowsDir(), def.slug)) };
  } finally { rmSync(tmp, { recursive: true, force: true }); }
}

// The flow in words, for the session brief (CLAUDE.md): what it is, what it asks, and what the owner's taste adds.
export function flowBrief(root, project, flow) {
  if (!flow) return null;
  const tt = techniqueTaste(root);
  const lines = [
    `**${flow.name}**${project.flow?.picked === 'studio' ? ` (the studio's pick: ${project.flow.why})` : ' (the owner chose it)'}: ${flow.summary}`, '',
    `- **Read \`flow/FLOW.md\` before the blueprint.** Set \`"flow": "${flow.slug}"\` in it; \`strudel.mjs plan\` holds the plan to the flow and \`check\` holds the notes to it.`,
    flow.shape?.curve ? `- **Its energy curve** (position in the piece → intensity): ${flow.shape.curve.map(([x, y]) => `${Math.round(x * 100)}% → ${y}`).join(', ')}. The sections' intensities follow it.` : null,
    ...(flow.shape?.rules || []).map((r) => `- **${RULES[r]?.says || r}**`),
    flow.sections ? `- **${flow.sections.min}-${flow.sections.max} sections.**` : null,
    flow.techniques.required.length ? `- **Its techniques:** ${flow.techniques.required.join(', ')}: each in the blueprint's \`techniques\` with its part, where and how; the check listens for every one.` : null,
    flow.techniques.free ? `- **Pick ${flow.techniques.free} more yourself** (\`node .claude/skills/music/strudel.mjs techniques\` lists them and how each is heard).${tt.loved.length ? ` The owner loved pieces with: ${tt.loved.slice(0, 8).join(', ')}.` : ''}${tt.recent.length ? ` Used in their last pieces (reach past these): ${tt.recent.slice(0, 10).join(', ')}.` : ''}${tt.disliked.length ? ` Not for them: ${tt.disliked.slice(0, 6).join(', ')}.` : ''}` : null,
    flow.techniques.avoid.length ? `- **It avoids:** ${flow.techniques.avoid.join(', ')}.` : null,
    flow.motif ? `- **The motif:** ${flow.motif}` : null,
    flow.surprise ? `- **The surprise:** ${flow.surprise}` : null,
    flow.ending ? `- **The ending:** ${flow.ending}` : null,
  ];
  return lines.filter((x) => x !== null).join('\n');
}
