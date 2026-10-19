// The look the owner asks for before any style frame is made (owner, 2026-10-18: "before generating the style frames,
// it should ask for a look"). Either one of their style packs, or a font, a colour scheme, a vibe and a look they pick.
// Kept in project.json `mfx.look_brief` and written to design/look-brief.json for the session; the style frames step
// stays blocked until it exists (gates.mjs).
//
// A style pack is a folder in the studio, style-packs/<name>/, with a pack.json ({name, fonts, palette, vibe, look,
// words}) and any pictures beside it; a preset's saved look (its words and pictures) is offered as a pack too.
import { existsSync, readdirSync, readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { join, extname } from 'node:path';
import { spawnSync } from 'node:child_process';
import { loadProject, projectPaths, update, event, now } from '../projects.mjs';
import { listPipelines } from '../pipelines.mjs';
import { UserError } from '../studio.mjs';

const HEX = /^#[0-9a-f]{6}$/i;
const PICTURE = new Set(['.png', '.jpg', '.jpeg', '.webp', '.svg', '.gif']);
const str = (v, n) => String(v ?? '').trim().slice(0, n);
const strs = (v, n, each) => (Array.isArray(v) ? v.map((x) => str(x, each)).filter(Boolean).slice(0, n) : []);

export const packsDir = (root) => join(root, 'style-packs');

function readPack(dir, id) {
  let def = {};
  try { def = JSON.parse(readFileSync(join(dir, 'pack.json'), 'utf8')); } catch { /* a folder of pictures is a pack too */ }
  const pictures = readdirSync(dir).filter((f) => PICTURE.has(extname(f).toLowerCase())).slice(0, 24).map((f) => join(dir, f));
  return { id, name: str(def.name, 80) || id, fonts: strs(def.fonts, 4, 60), palette: strs(def.palette, 8, 7).filter((c) => HEX.test(c)),
    vibe: str(def.vibe, 200), look: str(def.look, 200), words: str(def.words, 2000), pictures, source: 'pack' };
}

// look.packs: the owner's style packs (studio folder first, then presets with a saved look).
export function listPacks(root) {
  const out = [];
  const dir = packsDir(root);
  if (existsSync(dir)) {
    for (const name of readdirSync(dir, { withFileTypes: true }).filter((d) => d.isDirectory()).map((d) => d.name).sort().slice(0, 200)) {
      try { out.push(readPack(join(dir, name), name)); } catch { /* an unreadable folder is skipped */ }
    }
  }
  for (const p of listPipelines(root)) {
    if (p.source === 'built-in' || !p.look || !(p.look.words || p.look.pictures?.length)) continue;
    out.push({ id: `preset:${p.slug}`, name: `${p.name} (preset)`, fonts: [], palette: [], vibe: '', look: '', words: str(p.look.words, 2000),
      pictures: (p.look.pictures || []).slice(0, 24).map((f) => join(p.dir, f)), source: 'preset' });
  }
  return out;
}

// look.fonts: the font families on this computer (fontconfig), else a short list every system has.
let fontCache = null;
export function listFonts() {
  if (fontCache) return fontCache;
  const r = spawnSync('fc-list', [':', 'family'], { encoding: 'utf8', timeout: 10_000, maxBuffer: 16 * 1024 * 1024 });
  const found = r.status === 0 ? [...new Set(r.stdout.split('\n').map((l) => l.split(',')[0].trim()).filter((f) => f && !f.startsWith('.')))].sort((a, b) => a.localeCompare(b)) : [];
  fontCache = found.length ? found.slice(0, 2000) : ['Arial', 'Georgia', 'Helvetica', 'Courier New', 'Times New Roman', 'Verdana'];
  return fontCache;
}

export function cleanBrief(b = {}) {
  if (!b || typeof b !== 'object') throw new UserError('a look is an object');
  const out = { pack: b.pack ? str(b.pack, 120) : null, font: str(b.font, 80), heading_font: str(b.heading_font, 80),
    palette: strs(b.palette, 8, 7).filter((c) => HEX.test(c)), vibe: str(b.vibe, 200), look: str(b.look, 200), words: str(b.words, 2000) };
  if (!out.pack && !out.font && !out.palette.length && !out.vibe && !out.look && !out.words) throw new UserError('pick a style pack, or at least one of a font, colours, a vibe or a look');
  return out;
}

export function getBrief(root, id) {
  return loadProject(root, id).mfx?.look_brief || null;
}

// look.brief.set: save the look; the style frames can start (the runner is woken by the caller).
export function setBrief(root, id, params) {
  const brief = cleanBrief(params);
  const pack = brief.pack ? listPacks(root).find((p) => p.id === brief.pack) : null;
  if (brief.pack && !pack) throw new UserError(`no style pack "${brief.pack}"`);
  const saved = { ...brief, pack_files: pack ? { name: pack.name, fonts: pack.fonts, palette: pack.palette, vibe: pack.vibe, look: pack.look, words: pack.words, pictures: pack.pictures } : null, at: now() };
  update(root, id, (p) => { p.mfx ||= {}; p.mfx.look_brief = saved; });
  const design = join(projectPaths(root, id).work, 'design');
  mkdirSync(design, { recursive: true });
  writeFileSync(join(design, 'look-brief.json'), JSON.stringify(saved, null, 2));
  event(root, id, { event: 'LOOK_BRIEF', actor: 'you', details: pack ? `style pack ${pack.name}` : [brief.font, brief.vibe, brief.look].filter(Boolean).join(' · ') });
  return saved;
}
