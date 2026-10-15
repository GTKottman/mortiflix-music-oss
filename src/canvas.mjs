// The canvas's project graph: every piece of a project (beats of the script, style frames, shots, voice lines, music
// sections, transitions) as one node with a stable id, its beat's colour and every version it has had. The shot board,
// the node view and the FilmCraft hand-off all read this one graph.
//
// Sessions write plain files in their working folder (script/beats.json, design/frames.json, out/scenes.json,
// voice/lines.json, music/blueprint.json, transitions/board.json). `syncCanvas` reads them, copies each new version's
// media into the project's record (state/<id>/canvas/media/, so a later re-render can't change what the owner saw) and
// saves the graph in state/<id>/canvas/graph.json. Ids are forever: a node keeps its id and colour for life.
import { createHash } from 'node:crypto';
import { existsSync, readFileSync, writeFileSync, mkdirSync, copyFileSync, realpathSync, statSync } from 'node:fs';
import { join, extname, dirname } from 'node:path';
import { projectPaths } from './projects.mjs';
import { withLock, isInside } from './studio.mjs';

// One FilmCraft label colour per beat, in order (crates/project Label::rgb / marker_rgb, v0.8.0). Their marker colours
// all differ; Yellow is left out because text on its fill fails contrast. The beat number always sits beside it.
export const BEAT_COLOURS = [
  { label: 'Violet', hex: '#380ea7', chip: '#a98caf' },
  { label: 'Mango', hex: '#803f17', chip: '#da7639' },
  { label: 'Teal', hex: '#1f4d45', chip: '#72f0d7' },
  { label: 'Rose', hex: '#801836', chip: '#c13c3d' },
  { label: 'Blue', hex: '#192d94', chip: '#568bf4' },
  { label: 'Forest', hex: '#404a11', chip: '#758542' },
];

const sha = (buf) => createHash('sha256').update(buf).digest('hex');

export function canvasPaths(root, id) {
  const dir = join(projectPaths(root, id).state, 'canvas');
  return { dir, graph: join(dir, 'graph.json'), media: join(dir, 'media'), lock: join(dir, '.lock') };
}

export function loadGraph(root, id) {
  const f = canvasPaths(root, id).graph;
  try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return null; }
}

// A file a session wrote, only if it really is inside its working folder (no symlink out).
function workFile(work, rel) {
  if (!rel || typeof rel !== 'string') return null;
  const f = join(work, rel);
  if (!existsSync(f)) return null;
  const real = realpathSync(f);
  return isInside(realpathSync(work), real) && statSync(real).isFile() ? real : null;
}

function readJson(work, rel, problems) {
  const f = workFile(work, rel);
  if (!f) return null;
  try { return JSON.parse(readFileSync(f, 'utf8')); } catch (e) { problems.push({ file: rel, text: `can't be read: ${e.message}` }); return null; }
}

// The beats: script/beats.json, or the script's own "B01 · words" paragraphs.
function readBeats(work, problems) {
  const j = readJson(work, 'script/beats.json', problems);
  if (j?.beats?.length) return j.beats.filter((b) => /^b\d{2,3}$/.test(b.id)).map((b) => ({ id: b.id, words: String(b.words || ''), visual: b.visual ? String(b.visual) : null, target_sec: b.target_sec ?? null }));
  const f = workFile(work, 'script.md');
  if (!f) return [];
  const out = [];
  for (const para of readFileSync(f, 'utf8').split(/\n\s*\n/)) {
    const m = para.trim().match(/^B(\d{2,3})\s*·\s*([\s\S]*)$/);
    if (!m) continue;
    const [words, ...rest] = m[2].split('\n');
    out.push({ id: `b${m[1]}`, words: words.trim(), visual: rest.join(' ').replace(/^\*?Visual:\s*/i, '').replace(/\*$/, '').trim() || null, target_sec: null });
  }
  return out;
}

export function syncCanvas(root, id) {
  const pp = projectPaths(root, id);
  const cp = canvasPaths(root, id);
  mkdirSync(cp.media, { recursive: true });
  return withLock(cp.lock, () => {
    const prev = loadGraph(root, id) || { nodes: {}, colours: {} };
    const before = JSON.parse(JSON.stringify(prev));
    const problems = [];
    const nodes = prev.nodes || {};
    const colours = prev.colours || {};
    const seen = new Set();
    const work = pp.work;

    const colourOf = (beat) => {
      if (!beat) return null;
      if (colours[beat] === undefined) colours[beat] = Object.keys(colours).length;
      return BEAT_COLOURS[colours[beat] % BEAT_COLOURS.length];
    };
    // Add a version when the content behind a node changed (its media's or its text's hash).
    const put = (nid, kind, fields, version) => {
      seen.add(nid);
      const n = nodes[nid] || { id: nid, kind, versions: [] };
      Object.assign(n, fields, { kind, colour: colourOf(fields.beat) });
      if (!n.versions.some((v) => v.sha === version.sha)) n.versions.push({ v: n.versions.length + 1, made_at: new Date().toISOString(), ...version });
      n.current = n.versions.at(-1).v;
      nodes[nid] = n;
      return n;
    };
    // Copy a media file into the record once, named by its content.
    const snapshot = (rel, nid) => {
      const f = workFile(work, rel);
      if (!f) { problems.push({ file: rel, text: `doesn't exist (${nid})` }); return null; }
      const buf = readFileSync(f);
      const h = sha(buf);
      const name = `${h.slice(0, 16)}${extname(f).toLowerCase()}`;
      const dest = join(cp.media, nid, name);
      if (!existsSync(dest)) { mkdirSync(dirname(dest), { recursive: true }); copyFileSync(f, dest); }
      return { sha: h, file: `canvas/media/${nid}/${name}` };
    };

    const beats = readBeats(work, problems);
    beats.forEach((b, i) => {
      put(b.id, 'beat', { label: b.id.toUpperCase(), beat: b.id, order: i, title: b.words, visual: b.visual, target_sec: b.target_sec },
        { sha: sha(`${b.words}\n${b.visual || ''}`), text: b.words, visual: b.visual });
    });
    const beatIds = new Set(beats.map((b) => b.id));

    const frames = readJson(work, 'design/frames.json', problems)?.frames || [];
    frames.forEach((f, i) => {
      if (!/^sf\d{2,3}$/.test(f.id)) return problems.push({ file: 'design/frames.json', text: `frame id "${f.id}" should look like sf01` });
      const s = snapshot(f.file, f.id);
      if (s) put(f.id, 'frame', { label: f.label || f.id.toUpperCase(), beat: (f.beats || [])[0] || null, beats: f.beats || [], order: i }, s);
    });

    // Shots: one video cut into scenes. The video is snapshotted once; each shot plays its own seconds of it.
    const scenes = readJson(work, 'out/scenes.json', problems);
    if (scenes?.scenes?.length) {
      const src = snapshot(scenes.made_from, 'video');
      const fps = Number(scenes.fps) || 30;
      if (src) {
        scenes.scenes.forEach((sc, i) => {
          if (!/^s\d{2,3}$/.test(sc.key)) return problems.push({ file: 'out/scenes.json', text: `scene key "${sc.key}" should look like s01` });
          if (sc.beat && !beatIds.has(sc.beat)) problems.push({ file: 'out/scenes.json', text: `${sc.key}: beat ${sc.beat} isn't in the script` });
          const from = +(sc.from_frame / fps).toFixed(3);
          const to = +((sc.from_frame + sc.frames) / fps).toFixed(3);
          put(sc.key, 'shot', { label: sc.key.toUpperCase(), title: sc.name || sc.key, beat: sc.beat || null, order: i, look: frames.find((f) => (f.beats || []).includes(sc.beat))?.id || null },
            { sha: `${src.sha}:${sc.from_frame}:${sc.frames}`, file: src.file, from_sec: from, to_sec: to });
        });
      }
    } else {
      // Before the animatic: one planned shot per beat, its style frame as the still.
      beats.forEach((b, i) => {
        const look = frames.find((f) => (f.beats || []).includes(b.id));
        const sid = `s${b.id.slice(1)}`;
        if (nodes[sid]?.versions?.some((v) => v.file)) { seen.add(sid); return; }
        seen.add(sid);
        nodes[sid] = { ...(nodes[sid] || {}), id: sid, kind: 'shot', label: sid.toUpperCase(), title: b.words, beat: b.id, order: i, look: look?.id || null, colour: colourOf(b.id), planned: true, versions: [], current: null };
      });
    }

    const lines = readJson(work, 'voice/lines.json', problems);
    for (const [i, l] of (Array.isArray(lines) ? lines : lines?.lines || []).entries()) {
      if (!l?.id) continue;
      const beat = String(l.id).match(/^(b\d{2,3})-/)?.[1] || null;
      const clip = l.file || `voice/clips/${l.id}.wav`;
      const s = workFile(work, clip) ? snapshot(clip, l.id) : null;
      put(l.id, 'line', { label: l.id, title: String(l.script || l.text || ''), beat, order: i }, s || { sha: sha(String(l.script || l.text || '')), file: null });
    }

    const bp = readJson(work, 'music/blueprint.json', problems);
    (bp?.sections || []).forEach((sec, i) => {
      const mid = sec.id || `m${String(i + 1).padStart(2, '0')}`;
      put(mid, 'cue', { label: mid.toUpperCase(), title: sec.name, beat: sec.beat || null, order: i, bars: [sec.start_bar, sec.end_bar], intensity: sec.intensity ?? null }, { sha: sha(JSON.stringify(sec)) });
    });

    const board = readJson(work, 'transitions/board.json', problems);
    (board?.cuts || []).forEach((c, i) => {
      const tid = c.id || `t${String(i + 1).padStart(2, '0')}`;
      put(tid, 'transition', { label: tid.toUpperCase(), title: c.transition || c.name || c.style || 'cut', from: c.from_scene || null, to: c.to_scene || null, order: i, beat: null }, { sha: sha(JSON.stringify(c)) });
    });

    // Ids are forever: a node that disappeared from the files stays, marked missing, never deleted.
    for (const [nid, n] of Object.entries(nodes)) n.missing = !seen.has(nid) || undefined;

    const same = before.rev && JSON.stringify({ nodes: before.nodes, problems: before.problems, colours: before.colours }) === JSON.stringify({ nodes, problems, colours });
    if (same) return before;
    const order = (kind) => Object.values(nodes).filter((n) => n.kind === kind && !n.missing).sort((a, b) => a.order - b.order).map((n) => n.id);
    const graph = {
      format: 'mortiflix.canvas', version: 1, project: id, rev: (prev.rev || 0) + 1, built_at: new Date().toISOString(),
      order: { beats: order('beat'), shots: order('shot'), frames: order('frame'), lines: order('line'), cues: order('cue'), transitions: order('transition') },
      colours, nodes, problems,
    };
    writeFileSync(cp.graph, JSON.stringify(graph, null, 2));
    return graph;
  });
}
