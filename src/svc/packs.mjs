// Packs (owner, 2026-10-18): ready-made direction the owner picks per video, kept in the studio and, later, shared on
// mortiflix.com ("there are going to be many different packs. transitions packs, motion packs, ect."). Each pack is a
// folder: <studio>/packs/<kind>/<id>/ with a pack.json and anything it brings (pictures, fonts, RULES.md, MOTION.md…).
//
// pack.json, every kind:
//   { "format": "mortiflix.pack", "version": 1, "kind": "style", "name": "Noir receipts", "description": "…",
//     "author": "…", "license": "…", "source": "local" | "mortiflix.com" | "<where it came from>", "tags": [] }
// plus the kind's own fields (KINDS below). Any other folder under packs/ is a kind too, read with the common fields,
// so a new kind from mortiflix.com shows up before the app knows what to do with it.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, extname } from 'node:path';

export const PACK_FORMAT = 'mortiflix.pack';
const PICTURE = new Set(['.png', '.jpg', '.jpeg', '.webp', '.svg', '.gif']);
const HEX = /^#[0-9a-f]{6}$/i;
const str = (v, n) => String(v ?? '').trim().slice(0, n);
const strs = (v, n, each) => (Array.isArray(v) ? v.map((x) => str(x, each)).filter(Boolean).slice(0, n) : []);
const textFile = (dir, name, n = 20_000) => { try { const f = join(dir, name); return statSync(f).size <= 1024 * 1024 ? readFileSync(f, 'utf8').slice(0, n) : ''; } catch { return ''; } };

// The kinds the studio knows: where they're used and their own fields.
export const KINDS = {
  style: { used: 'Style frames: the look (fonts, colours, vibe, look)',
    read: (d, dir) => ({ fonts: strs(d.fonts, 6, 60), palette: strs(d.palette, 8, 7).filter((c) => HEX.test(c)), vibe: str(d.vibe, 200), look: str(d.look, 200), words: str(d.words, 2000) || textFile(dir, 'STYLE.md', 4000) }) },
  transition: { used: 'Transitions: the cuts the board picks from',
    read: (d, dir) => ({ transitions: (Array.isArray(d.transitions) ? d.transitions : []).slice(0, 200).map((t) => ({ name: str(t?.name, 80), sec: Number(t?.sec) || null, words: str(t?.words, 600) })).filter((t) => t.name), words: str(d.words, 2000) || textFile(dir, 'TRANSITIONS.md', 8000) }) },
  motion: { used: 'Animatic: how things move (moves, easing, pace)',
    read: (d, dir) => ({ moves: strs(d.moves, 60, 200), easing: str(d.easing, 200), pace: str(d.pace, 200), words: str(d.words, 4000) || textFile(dir, 'MOTION.md', 12_000) }) },
  rule: { used: 'Every step from the one it is picked on: rules the AI follows',
    read: (d, dir) => ({ rules: strs(d.rules, 200, 600), words: str(d.words, 4000) || textFile(dir, 'RULES.md', 12_000) }) },
  music: { used: 'Music: a theme or sound to score in', read: (d) => ({ words: str(d.words, 4000) }) },
  voice: { used: 'Voice: how the narration sounds', read: (d) => ({ words: str(d.words, 4000) }) },
};

export const packsRoot = (root) => join(root, 'packs');

function readPack(dir, kind, id) {
  let d = {};
  try { d = JSON.parse(readFileSync(join(dir, 'pack.json'), 'utf8')); } catch { /* a folder of pictures is a pack too */ }
  if (!d || typeof d !== 'object') d = {};
  const files = readdirSync(dir);
  const pictures = files.filter((f) => PICTURE.has(extname(f).toLowerCase())).sort().slice(0, 24).map((f) => join(dir, f));
  const fontsDir = join(dir, 'fonts');
  const fontFiles = existsSync(fontsDir) ? readdirSync(fontsDir).filter((f) => /\.(ttf|otf|woff2?)$/i.test(f)).slice(0, 40).map((f) => join(fontsDir, f)) : [];
  const own = KINDS[kind]?.read(d, dir) || { words: str(d.words, 4000) };
  return { id, kind, name: str(d.name, 80) || id, description: str(d.description, 600), author: str(d.author, 120), license: str(d.license, 200),
    source: str(d.source, 200) || 'local', tags: strs(d.tags, 20, 40), version: Number(d.version) || 1, dir, pictures, font_files: fontFiles, ...own };
}

function packsIn(dir, kind) {
  if (!existsSync(dir)) return [];
  const out = [];
  for (const e of readdirSync(dir, { withFileTypes: true }).filter((x) => x.isDirectory() && !x.name.startsWith('.')).sort((a, b) => a.name.localeCompare(b.name)).slice(0, 500)) {
    try { out.push(readPack(join(dir, e.name), kind, e.name)); } catch { /* an unreadable folder is skipped */ }
  }
  return out;
}

// packs.list {kind?}: the studio's packs, every kind (or one). `style-packs/` (the first place style packs lived) is
// read as style packs too.
export function listAllPacks(root, kind = null) {
  const base = packsRoot(root);
  const kinds = existsSync(base) ? readdirSync(base, { withFileTypes: true }).filter((x) => x.isDirectory()).map((x) => x.name) : [];
  const out = [];
  for (const k of [...new Set([...Object.keys(KINDS), ...kinds])]) {
    if (kind && k !== kind) continue;
    out.push(...packsIn(join(base, k), k));
  }
  if (!kind || kind === 'style') for (const p of packsIn(join(root, 'style-packs'), 'style')) if (!out.some((x) => x.kind === 'style' && x.id === p.id)) out.push(p);
  return out;
}

// packs.kinds: every kind, what it's for, its folder and how many packs it has.
export function packKinds(root) {
  const all = listAllPacks(root);
  const kinds = [...new Set([...Object.keys(KINDS), ...all.map((p) => p.kind)])];
  return kinds.map((k) => ({ kind: k, used: KINDS[k]?.used || '', dir: join(packsRoot(root), k), count: all.filter((p) => p.kind === k).length }));
}
