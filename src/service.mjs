// The Mortiflix service: the AI side of Mortiflix Studio. The desktop app (Rust) runs
// `node bin/mortiflix-service.mjs` and talks JSON lines over stdin/stdout, no port.
// Protocol: docs/mortiflix/protocol.md in the Mortiflix Studio app repo, with every message this file handles listed
// in docs/service-protocol.md here.
//   request  {id, type, params}
//   reply    {id, ok: true, result} | {id, ok: false, error: {code, message}}
//   event    {event, data}
// One handler per message type; the work lives in src/svc/*.mjs over the studio's engine (projects, gates, canvas,
// runner). The service runs the studio's runner itself, so the AI works ahead while the app is open.
import { readFileSync } from 'node:fs';
import { join, isAbsolute } from 'node:path';
import { REPO, studioRoot, ensureStudio, loadConfig, paths, UserError } from './studio.mjs';
import { createProject, loadProject, projectPaths, listProjects, startProject, readEvents, projectPipeline, update as updateProject, event as projectEvent } from './projects.mjs';
import { Runner } from './runner.mjs';
import { findPipeline } from './pipelines.mjs';
import { canvasView, nodeAction, canvasPaths } from './canvas.mjs';
import * as gates from './gates.mjs';
import { RenderQueue } from './renderq.mjs';
import { voiceOverview, saveVoice } from './voice/index.mjs';
import { boothStatus, addTake, keepTake } from './booth.mjs';
import { musicScore } from './svc/score.mjs';
import { listPacks, listFonts, getBrief, setBrief } from './svc/lookbrief.mjs';
import { listAllPacks, packKinds } from './svc/packs.mjs';
import { getMotionBrief, setMotionBrief, getTransitionBrief, setTransitionBrief } from './svc/motionbrief.mjs';
import { STAGES, STAGE_NAMES, stageIndex, stageStatuses, stageDone, handOverToApp, remakesIf, reopenStage, saveLastStage, stageOf, mfxView, STAGE_MUSIC, STAGE_EDIT, STAGE_RENDER } from './svc/stages.mjs';
import { listPresets, getPreset, savePresetFull, deletePreset } from './svc/presets.mjs';
import { addAsset, updateAsset, replaceAsset, removeAsset, useAsset, listAssets } from './svc/assets.mjs';
import { loadTheme, saveTheme, themeSamples, setCuePlan, cleanTheme } from './svc/theme.mjs';
import * as graph from './svc/graph.mjs';
import { placeSubtitles } from './svc/subplace.mjs';
import { scriptView, applyScriptEdits, remakesFor, scriptActionText, scriptLogged, saveDraft } from './svc/script.mjs';
import { lookView, useLook, moreLooks } from './svc/look.mjs';
import { appendChat, chatHistory, translateEvents, typedMove, quickReply, applyMove, initChat, actionText, askAI } from './svc/chat.mjs';
import { nameBeats, unnamed } from './svc/beatnames.mjs';
import { activeWork } from './svc/work.mjs';
import { tokenRuns, tokenRun } from './svc/tokens.mjs';
import { exportTarget, deliverFromApp, postPlan, postSend } from './svc/export.mjs';
import * as setup from './svc/setup.mjs';
import { componentsStatus, installComponents, parseComponents } from './components.mjs';
import { setupHandlers } from './service-setup.mjs';

export const PROTOCOL = 1;
export { STAGES };

const version = () => {
  try { return JSON.parse(readFileSync(join(REPO, 'package.json'), 'utf8')).version; } catch { return '0.0.0'; }
};

class ProtocolError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}
const bad = (message) => { throw new ProtocolError('bad_request', message); };
const NODE_ID = /^[a-z][a-z0-9-]{1,23}$/;
const nodeId = (v, what = 'node') => (NODE_ID.test(String(v || '')) ? String(v) : bad(`${what} must be a piece id like s01`));
// What the chat can be "talking about" (protocol.md `chat.context`): the whole video, one piece (s01, b02, sf01,
// b01-2, m01, music, a01), the cut between two shots that has no transition yet (`cut:s02:s03`, the node area's cut
// pills) or a stage (`stage:look`). Ids are matched case-insensitively ("S03" is s03).
const CONTEXT = /^(?:video|[a-z][a-z0-9-]{1,23}|cut:[a-z][a-z0-9-]{1,23}:[a-z][a-z0-9-]{1,23}|stage:[a-z]{3,20})$/;
const contextOf = (v) => {
  if (v === undefined || v === null || v === '' || v === 'video') return 'video';
  const c = String(v).trim().toLowerCase();
  return CONTEXT.test(c) ? c : bad('context must be "video", a piece id like s01, a cut like cut:s02:s03 or a stage like stage:look');
};
const assetId = (v) => (/^a\d{2,3}$/.test(String(v || '')) ? String(v) : bad('asset must be an id like a01'));
const opt = (v, n = 4000) => (v === undefined || v === null ? undefined : String(v).slice(0, n));

// `runner`: run the studio's runner in this process (the app's service does; tests mostly don't).
// `tickMs`: how often the open project is looked at (graph, chat, statuses) while nothing else pokes it.
export function createService({ root = studioRoot(), send, runner: wantRunner = false, tickMs = 1500 } = {}) {
  const state = {
    project: null,
    stage: 0,
    stages: STAGES.map(() => ({ kind: 'notStarted' })),
    line: '',
    context: 'video',
  };
  const emit = (event, data) => send({ event, data });
  let installing = null; // component ids being installed (components.install)
  const activity = new Map(); // project id -> recent activity rows
  let runner = null;
  let runnerNote = '';
  let queue = null;
  let lastStatus = '';
  let lastNodes = new Map();
  let lastProjectState = '';
  let lastWork = '';
  let work = null; // the last work.active data for the open project
  let timer = null;
  let interval = null;
  let closed = false;

  const onJob = (job) => { try { emit('render.job', queue.view(job)); } catch { /* gone */ } };
  if (wantRunner) {
    try {
      runner = new Runner(root, { log: (l) => console.error(l) });
      runner.acquire();
      queue = runner.renders;
      const inner = queue.onChange;
      queue.onChange = (job) => { inner(job); onJob(job); };
      runner.on('change', () => poke());
      runner.on('activity', (a) => onActivity(a));
      runner.loop({ watch: true }).catch((e) => { runnerNote = `The AI stopped working: ${e.message}`; console.error(e); poke(); });
    } catch (e) {
      runnerNote = String(e.message || e);
      runner = null;
      console.error(`mortiflix-service: no runner: ${runnerNote}`);
    }
  }
  if (!queue) queue = new RenderQueue({ logDir: join(paths(root).run, 'renders'), onChange: onJob });

  function onActivity(a) {
    const list = activity.get(a.project) || [];
    list.push(a);
    if (list.length > 300) list.splice(0, list.length - 300);
    activity.set(a.project, list);
    if (a.project !== state.project?.id) return;
    emit('activity', a);
    if (['text', 'tool'].includes(a.kind)) poke();
  }

  const projectId = () => state.project?.id || null;
  const needProject = (params) => {
    const id = params?.project || state.project?.id;
    if (!id) throw new ProtocolError('no_project', 'open or make a project first');
    return String(id);
  };

  // The AI's one line for the stage bar hint and the chat's Now line (doc 37: one source).
  function liveLine(p) {
    if (!p) return runnerNote || '';
    if (p.state === 'paused' && p.needs_you) return `Needs you: ${p.needs_you.text}`.slice(0, 300);
    // what the AI is doing, in plain words with progress (work.active `doing`)
    if (p.session?.running && work?.project === p.id && work.doing) return work.doing.slice(0, 300);
    if (p.status?.text) return p.status.text;
    const last = (activity.get(p.id) || []).filter((a) => ['text', 'tool'].includes(a.kind)).at(-1);
    if (p.session?.running && last) return String(last.text).split('\n')[0].slice(0, 200);
    if (runnerNote && p.state === 'queued') return runnerNote.slice(0, 300);
    // Edit the video skips the AI's build and final (the app makes the video): nothing is delivered until Render.
    const inApp = p.state === 'delivered' && Object.values(p.steps || {}).some((s) => s?.state === 'skipped') && ['build', 'final'].every((k) => !p.steps?.[k] || p.steps[k].state === 'skipped');
    if (inApp) return 'The AI\'s part is done: finish in Edit, then Render.';
    return { draft: 'Not started yet.', queued: 'Starting…', waiting: 'Waiting for you.', delivered: 'Done: the video is delivered.', cancelled: 'Cancelled.', paused: 'Paused.' }[p.state] || '';
  }

  function status(force = false) {
    const id = projectId();
    let p = null;
    if (id) {
      try {
        p = loadProject(root, id);
        state.project = p;
        state.stages = stageStatuses(root, id, { running: Boolean(p.session?.running) });
      } catch (e) { console.error(`status: ${e.message}`); }
    }
    state.line = liveLine(p);
    const data = { line: state.line, stage: state.stage, project: id, stages: state.stages, state: p?.state ?? null, needs_you: p?.needs_you ?? null, title: p?.title ?? null };
    const key = JSON.stringify(data);
    if (force || key !== lastStatus) { lastStatus = key; emit('status', data); }
    if (p && p.state !== lastProjectState) { lastProjectState = p.state; emit('project.changed', { id, state: p.state, needs_you: p.needs_you, deliverables: p.deliverables }); }
  }

  // Policies the app's flow needs (docs 30, 37): questions never block (a questions step whose questions all have a
  // default goes on with them; a later answer is a chat message), and a stage the owner moved on from keeps what
  // arrives "as it is".
  function policies(id) {
    const config = loadConfig(root);
    const p = loadProject(root, id);
    if (!['queued', 'waiting'].includes(p.state)) return;
    const pipeline = projectPipelineOf(id);
    const done = mfxView(p).stages_done || {};
    for (const s of gates.stepView(p, pipeline).filter((x) => x.state === 'in_review')) {
      try {
        if (s.review === 'questions' && config.mfxQuestionsBlock !== true) {
          const sub = gates.submissions(root, id).find((x) => x.step === s.key && x.version === s.version);
          if (sub && sub.questions.every((q) => q.default !== null)) {
            gates.respond(root, id, s.key, s.version, { verdict: 'approve', overall: 'Going on with the defaults (questions never block; the owner can change any answer in the chat).' });
            const row = appendChat(root, id, { role: 'ai', text: `Going with: ${sub.questions.map((q) => `${q.text} → ${q.default}`).join(' · ')}. Change any of these in the chat.`, card: { kind: 'defaults', questions: sub.questions } });
            emit('chat.message', row);
            continue;
          }
        }
        if (done[STAGES[stageOf(s)]]) stageDone(root, id, stageOf(s), { reason: 'Moved on earlier' });
      } catch (e) { console.error(`policy ${s.key}: ${e.message}`); }
    }
  }

  // Beats without a name get one from the user's own AI (once per words; the demo titles them from the words). A
  // failed call is not tried again for 10 minutes; the words' own title shows meanwhile.
  const naming = { busy: false, failedAt: new Map() };
  function nameBeatsSoon(id) {
    if (naming.busy || Date.now() - (naming.failedAt.get(id) || 0) < 600_000) return;
    let todo;
    try { todo = unnamed(root, id); } catch { return; }
    if (!todo.length) return;
    naming.busy = true;
    nameBeats(root, id, { ask: (prompt) => askAI(root, id, prompt) })
      .then((r) => { if (r.named.length && id === projectId()) emit('node.changed', { project: id, rev: null, nodes: r.named.map((b) => ({ id: b, kind: 'beat', named: true })) }); })
      .catch((e) => { naming.failedAt.set(id, Date.now()); console.error(`beat names: ${e.message}`); })
      .finally(() => { naming.busy = false; });
  }

  function tick() {
    timer = null;
    if (closed) return;
    const id = projectId();
    if (id) {
      try {
        for (const row of translateEvents(root, id)) emit('chat.message', row);
        policies(id);
        nameBeatsSoon(id);
        const g = canvasView(root, id);
        const changed = [];
        const seen = new Map();
        for (const [nid, n] of Object.entries(g.nodes)) {
          const sig = `${n.current}|${n.state}|${n.owner}|${n.pending?.id || ''}|${n.missing ? 1 : 0}`;
          seen.set(nid, sig);
          if (lastNodes.get(nid) !== sig) changed.push({ id: nid, kind: n.kind, current: n.current, state: n.state, owner: n.owner, missing: Boolean(n.missing) });
        }
        if (lastNodes.size && changed.length) emit('node.changed', { project: id, rev: g.rev, nodes: changed });
        else if (!lastNodes.size && seen.size) emit('node.changed', { project: id, rev: g.rev, nodes: [], all: true });
        lastNodes = seen;
        refreshWork(id, g);
      } catch (e) { console.error(`tick: ${e.message}`); }
    }
    status();
  }
  // What the AI is working on (svc/work.mjs): sent as `work.active` whenever it changes (and on hello).
  function refreshWork(id, g, force = false) {
    let p;
    try { p = loadProject(root, id); } catch { return; }
    const render = queue.list().find((j) => j.project === id && j.state === 'running') || null;
    work = activeWork(root, id, { running: Boolean(p.session?.running), g, activity: activity.get(id) || [], render });
    const key = JSON.stringify(work);
    if (force || key !== lastWork) { lastWork = key; emit('work.active', work); }
  }

  function poke(ms = 150) {
    if (closed) return;
    if (timer && ms > 0) return;
    if (timer) clearTimeout(timer);
    timer = setTimeout(tick, ms);
    timer.unref?.();
  }
  if (tickMs > 0) { interval = setInterval(() => poke(0), tickMs); interval.unref?.(); }

  const projectPipelineOf = (id) => projectPipeline(root, id);

  function openProject(p, stage = null) {
    state.project = p;
    state.stage = stage ?? mfxView(p).stage ?? 0;
    lastNodes = new Map();
    lastProjectState = '';
    lastWork = '';
    work = null;
    poke(0);
  }

  function chatRow(id, row) {
    const r = appendChat(root, id, row);
    emit('chat.message', r);
    return r;
  }

  // Copy files in the background (big footage takes a while): progress and the result come as events.
  function copyAssets(id, list, { then = null } = {}) {
    const run = async () => {
      const added = [];
      for (const a of list) {
        try {
          const asset = await addAsset(root, id, { path: a.path, use: a.use, where: a.where, note: a.note, onProgress: (pr) => emit('assets.progress', { project: id, ...pr }) });
          added.push(asset);
          emit('assets.added', { project: id, asset });
          chatRow(id, { role: 'action', text: `Added ${asset.name} (${asset.id})` });
        } catch (e) {
          emit('assets.failed', { project: id, path: String(a.path || '').slice(0, 1000), message: e.message });
        }
      }
      if (then) {
        try { await then(added); } catch (e) { emit('assets.failed', { project: id, message: e.message }); chatRow(id, { role: 'system', text: `Couldn't start: ${e.message}` }); }
      }
      poke();
    };
    run().catch((e) => console.error(e));
  }

  const cleanAssetList = (p) => {
    const raw = Array.isArray(p?.assets) ? p.assets : Array.isArray(p?.paths) ? p.paths.map((x) => ({ path: x })) : p?.path ? [{ path: p.path }] : [];
    return raw.slice(0, 200).map((a) => (typeof a === 'string' ? { path: a } : { path: a?.path, use: a?.use ?? p?.use, where: a?.where ?? p?.where, note: a?.note ?? p?.note }))
      .filter((a) => typeof a.path === 'string' && a.path);
  };

  const handlers = {
    hello: (p) => {
      const result = { service: 'mortiflix-service', version: version(), protocol: PROTOCOL, studio: root, client: String(p?.client || ''), runner: Boolean(runner), runner_note: runnerNote || null, types: Object.keys(handlers).sort() };
      setImmediate(() => { status(true); if (projectId()) { try { refreshWork(projectId(), canvasView(root, projectId()), true); } catch { /* no canvas yet */ } } });
      return result;
    },
    'state.get': () => ({ project: state.project, stage: state.stage, stages: state.stages, line: state.line, context: state.context }),

    // ---- projects ----
    'project.list': () => listProjects(root).map((p) => ({ id: p.id, title: p.title, state: p.state, pipeline: p.pipeline, created_at: p.created_at, updated_at: p.updated_at ?? null, stage: mfxView(p).stage ?? 0, delivered: p.state === 'delivered' })),
    'project.new': (p) => {
      const slug = String(p?.preset || p?.pipeline || 'explainer');
      const pipeline = findPipeline(root, slug);
      const answers = {};
      for (const q of pipeline.intake) if (q.type !== 'files' && q.default !== undefined && q.default !== null && q.default !== '') answers[q.id] = q.default;
      Object.assign(answers, p?.answers && typeof p.answers === 'object' ? p.answers : {});
      const ideaField = (pipeline.intake.find((q) => q.type === 'long' && q.required) || pipeline.intake.find((q) => q.type === 'long'))?.id;
      if (p?.idea !== undefined && ideaField) answers[ideaField] = String(p.idea).slice(0, 20_000);
      const assets = cleanAssetList(p);
      const start = p?.start !== false;
      if (start) {
        for (const q of pipeline.intake.filter((x) => x.required)) {
          if (q.type === 'files' ? !assets.length : !String(answers[q.id] ?? '').trim()) bad(`${q.label || q.id}: ${q.type === 'files' ? 'add a file under Assets' : 'this is needed'}`);
        }
      }
      if (p?.backend !== undefined && p.backend !== 'demo') bad('backend: only "demo" can be pinned per project');
      const project = createProject(root, { pipeline: slug, title: p?.title ?? p?.name, answers, backend: p?.backend === 'demo' ? 'demo' : null });
      initChat(root, project.id);
      if (answers[ideaField]) appendChat(root, project.id, { role: 'user', text: answers[ideaField] });
      const theme = p?.music?.theme || p?.music_theme || null;
      if (theme) saveTheme(root, project.id, { ...theme, picked_by: 'you' });
      else if (pipeline.music_theme) saveTheme(root, project.id, { ...pipeline.music_theme, picked_by: 'you' }, { by: 'ai' });
      saveLastStage(root, project.id, 0);
      openProject(loadProject(root, project.id), 0);
      const go = async () => {
        if (!start) return;
        startProject(root, project.id);
        runner?.wake();
        poke();
      };
      if (assets.length) copyAssets(project.id, assets, { then: go });
      else go().catch((e) => chatRow(project.id, { role: 'system', text: `Couldn't start: ${e.message}` }));
      return { project: loadProject(root, project.id), assets_pending: assets.length };
    },
    'project.open': (p) => {
      if (!p?.id) bad('project.open needs an id');
      const project = loadProject(root, String(p.id));
      openProject(project);
      return { project, stage: state.stage };
    },
    'project.close': () => { state.project = null; state.stage = 0; lastNodes = new Map(); work = null; lastWork = ''; emit('work.active', { project: null, running: false, step: null, nodes: [], current: null, doing: '', progress: null, shots_eta_min: null, before_shots: [] }); status(true); return { closed: true }; },
    // What the AI is working on now (also the work.active event): the pieces it makes or remakes, in plain words.
    'work.get': (p) => { const id = needProject(p); const pr = loadProject(root, id); return activeWork(root, id, { running: Boolean(pr.session?.running), g: canvasView(root, id), activity: activity.get(id) || [], render: queue.list().find((j) => j.project === id && j.state === 'running') || null }); },
    'project.start': (p) => { const r = startProject(root, needProject(p)); runner?.wake(); poke(); return { project: r }; },
    'project.pause': (p) => { const id = needProject(p); gates.pause(root, id); runner?.stopProject(id); poke(); return { paused: true }; },
    'project.resume': (p) => { gates.resume(root, needProject(p)); runner?.wake(); poke(); return { resumed: true }; },
    'project.cancel': (p) => { const id = needProject(p); gates.cancel(root, id); runner?.stopProject(id); poke(); return { cancelled: true }; },
    'project.events': (p) => readEvents(root, needProject(p), { limit: Math.min(2000, Number(p?.limit) || 200) }),
    // Token usage (owner, 2026-10-18): every Claude run newest first, and one run's bars and what it pulled in.
    'tokens.runs': (p) => { const id = needProject(p); const pr = loadProject(root, id); return tokenRuns(root, id, { running: pr.session?.running ? pr.session.id : null, limit: p?.limit }); },
    'tokens.run': (p) => {
      const id = needProject(p);
      const pr = loadProject(root, id);
      if (!p?.run) bad('tokens.run needs a run');
      return tokenRun(root, id, String(p.run), { running: pr.session?.running ? pr.session.id : null, bars: p?.bars, pulled: p?.pulled });
    },

    // ---- stages ----
    'stage.set': (p) => {
      const stage = stageIndex(Number.isInteger(p?.stage) ? p.stage : p?.key);
      if (stage < 0) bad('stage must be 0-7 or a stage key');
      const from = stageIndex(p?.from);
      let out = {};
      const id = projectId();
      if (p?.done === true && from >= 0) {
        if (id) {
          out = stageDone(root, id, from);
          // Leaving Music: the app builds the edit (Edit the video) or renders straight away (Get the video).
          if (from === STAGE_MUSIC && stage >= STAGE_EDIT) out.handover = handOverToApp(root, id, stage === STAGE_RENDER ? 'get' : 'edit');
          chatRow(id, { role: 'action', text: `Next: ${STAGE_NAMES[stage]} (${STAGE_NAMES[from]} kept as it is)` });
        } else state.stages[from] = { kind: 'done' };
      }
      if (id && stage !== state.stage) chatRow(id, { role: 'stage', text: STAGE_NAMES[stage], stage });
      state.stage = stage;
      if (id) saveLastStage(root, id, stage);
      status(true);
      return { stage, key: STAGES[stage], ...out };
    },
    'stage.status': (p) => stageStatuses(root, needProject(p)),
    'stage.remakes': (p) => ({ remade: remakesIf(root, needProject(p), p?.stage ?? p?.key) }),
    'stage.reopen': (p) => { if (!String(p?.text || '').trim()) bad('say what to change'); const r = reopenStage(root, needProject(p), p?.stage ?? p?.key, p.text); runner?.wake(); poke(); return r; },
    'stage.handover': (p) => { const how = p?.how === 'get' ? 'get' : p?.how === 'edit' ? 'edit' : bad('how: edit or get'); const r = handOverToApp(root, needProject(p), how); poke(); return r; },

    // ---- presets (doc 31) ----
    'presets.list': () => listPresets(root),
    'presets.get': (p) => getPreset(root, String(p?.slug || bad('presets.get needs a slug'))),
    'presets.save': (p) => savePresetFull(root, p || {}),
    'presets.delete': (p) => deletePreset(root, String(p?.slug || bad('presets.delete needs a slug'))),
    'voice.list': () => { const v = voiceOverview(root); return { engine: v.engine, voice: v.voice ?? null, engines: ['none', 'elevenlabs', 'qwen', 'own'], presets: v.presets, elevenlabs_key: v.elevenlabs_key, local: v.local }; },
    // Settings › Mortiflix › Narration: which engine narrates new lines (ElevenLabs is paid; This computer is local)
    'voice.set': (p) => {
      const engine = String(p?.engine || '');
      if (!['none', 'elevenlabs', 'qwen', 'own'].includes(engine)) bad('engine must be none, elevenlabs, qwen or own');
      saveVoice(root, { engine });
      return handlers['voice.list']();
    },

    // ---- assets (doc 32) ----
    'assets.list': (p) => listAssets(root, needProject(p)),
    'assets.add': (p) => { const id = needProject(p); const list = cleanAssetList(p); if (!list.length) bad('assets.add needs paths'); copyAssets(id, list); return { queued: list.length }; },
    'assets.update': (p) => { const id = needProject(p); const a = updateAsset(root, id, assetId(p?.asset), { use: p.use, where: p.where, note: p.note }); poke(); return { asset: a }; },
    'assets.replace': (p) => {
      const id = needProject(p);
      const aid = assetId(p?.asset);
      if (!p?.path) bad('assets.replace needs a path');
      replaceAsset(root, id, aid, { path: String(p.path), onProgress: (pr) => emit('assets.progress', { project: id, ...pr }) })
        .then((asset) => { emit('assets.added', { project: id, asset, replaced: true }); chatRow(id, { role: 'action', text: `Replaced ${asset.id} with ${asset.name} (v${asset.version}; the old one is kept)` }); })
        .catch((e) => emit('assets.failed', { project: id, asset: aid, message: e.message }));
      return { queued: 1 };
    },
    'assets.remove': (p) => { const id = needProject(p); const a = removeAsset(root, id, assetId(p?.asset)); chatRow(id, { role: 'action', text: `Removed ${a.name} (${a.id}); the file is kept` }); return { asset: a }; },
    // Drag an asset onto a piece: it's used there, and (fix, default true) the AI is asked to put it in.
    'assets.use': (p) => {
      const id = needProject(p);
      const aid = assetId(p?.asset);
      const node = nodeId(p?.node);
      const a = useAsset(root, id, aid, node, { how: p.how, remove: Boolean(p.remove) });
      let edit = null;
      if (!p.remove && p.fix !== false) {
        try { edit = nodeAction(root, id, node, { action: 'fix', text: `Use the owner's asset ${a.id} (${a.file}) in this piece${p.how ? `: ${String(p.how).slice(0, 300)}` : ''}.` }).edit; } catch (e) { if (!/no piece/.test(e.message)) throw e; }
      }
      chatRow(id, { role: 'action', text: p.remove ? `${a.name} no longer used in ${node.toUpperCase()}` : `Use ${a.name} in ${node.toUpperCase()}${edit ? ` (${edit.id})` : ''}`, refs: [node], ...(edit ? { edit: edit.id } : {}) });
      runner?.wake();
      poke();
      return { asset: a, edit };
    },

    // ---- music theme and cues (doc 33) ----
    // paths in results are absolute: a session writes its theme samples' WAVs relative to its working folder
    'music.theme.get': (p) => {
      const id = needProject(p);
      const t = loadTheme(root, id);
      const work = projectPaths(root, id).work;
      if (t?.samples) t.samples = t.samples.map((x) => (x?.wav && !isAbsolute(x.wav) ? { ...x, wav: join(work, x.wav) } : x));
      return { theme: t };
    },
    'music.theme.set': (p) => { const id = needProject(p); const t = saveTheme(root, id, { ...(p?.theme || bad('music.theme.set needs a theme')), picked_by: 'you' }); chatRow(id, { role: 'action', text: `Music theme: ${[t.genre, ...t.mood].filter(Boolean).join(', ') || 'changed'}` }); poke(); return { theme: t }; },
    // Before a project exists (New project › Music › Choose…) pass no project: the demo makes samples in the studio.
    'music.theme.samples': async (p) => themeSamples(root, p?.project || projectId() || null, p?.theme || {}),
    'music.theme.use': (p) => {
      const id = needProject(p);
      const t = loadTheme(root, id) || bad('no theme yet');
      const sample = t.samples?.find((s) => s.id === p?.sample) || bad(`no theme sample ${p?.sample}`);
      const before = t.using;
      t.using = p.sample;
      const theme = saveTheme(root, id, t);
      // a theme picked after the music is written remakes the music in it (a Fix on the score; versions kept). Run 2:
      // the pick changed nothing you could hear
      const music = canvasView(root, id).nodes.music;
      let edit = null;
      if (before !== p.sample && music?.versions?.length) {
        edit = nodeAction(root, id, 'music', { action: 'fix', text: `The owner picked the music theme "${sample.name}" (music/theme.json, using ${sample.id}): rewrite the score in it, keeping the cue timing and the mix under the voice.` }).edit;
        runner?.wake();
      }
      chatRow(id, { role: 'action', text: `Music theme: ${sample.name}${edit ? ' · remaking the music in it' : ''}`, ...(edit ? { edit: edit.id, refs: ['music'] } : {}) });
      poke();
      return { theme, edit };
    },
    'music.theme.check': (p) => ({ theme: cleanTheme(p?.theme || {}) }),
    'music.cues': (p) => graph.voiceMusicView(root, needProject(p)).cues,
    'music.score': (p) => musicScore(root, needProject(p)),
    // A cue's feel or energy: before the score it changes the plan; once scored it's a Fix on that cue (only it changes).
    'music.cue.set': (p) => {
      const id = needProject(p);
      const cue = String(p?.cue || '');
      if (!/^m\d{2,3}$/.test(cue)) bad('cue must be an id like m01');
      const g = canvasView(root, id);
      if (g.nodes[cue] && !g.nodes[cue].missing) {
        const words = [p.feel ? `Feel: ${String(p.feel).slice(0, 300)}` : null, p.energy ? `energy ${Number(p.energy)} of 5` : null, p.text ? String(p.text).slice(0, 2000) : null].filter(Boolean).join('; ');
        if (!words) bad('say what to change');
        const r = nodeAction(root, id, cue, { action: 'fix', text: words, bars: p.bars ?? null, part: p.part ?? null });
        chatRow(id, { role: 'action', text: actionText('fix', g.nodes[cue].label, words), edit: r.edit.id, refs: [cue] });
        runner?.wake();
        poke();
        return { edit: r.edit };
      }
      return { plan: setCuePlan(root, id, cue, { feel: p.feel, energy: p.energy }) };
    },

    // ---- voice & music stage (doc 34) ----
    'voicemusic.view': (p) => graph.voiceMusicView(root, needProject(p)),
    'voice.lines': (p) => graph.voiceMusicView(root, needProject(p)).voice,
    'mix.set': (p) => { const id = needProject(p); const r = graph.setMix(root, id, nodeId(p?.node), p || {}); if (r.edit) runner?.wake(); poke(); return r; },
    'voice.line.words': (p) => {
      const id = needProject(p);
      const line = nodeId(p?.line, 'line');
      const text = String(p?.text || '').trim() || bad('the new words are empty');
      const r = nodeAction(root, id, line, { action: 'fix', text: `Change the words to: "${text.slice(0, 2000)}". Write them to voice/lines.json and the script beat, then make a new take.` });
      chatRow(id, { role: 'action', text: `Remake line ${line}: "${text.slice(0, 200)}"`, edit: r.edit.id, refs: [line] });
      runner?.wake();
      return { edit: r.edit };
    },
    // ---- stage 2 Look: the looks to pick from (the Look panel + the chat's looks card) ----
    'look.get': (p) => lookView(root, needProject(p)),
    'packs.list': (p) => listAllPacks(root, p?.kind ? String(p.kind).slice(0, 40) : null),
    'packs.kinds': () => packKinds(root),
    'transition.brief.get': (p) => getTransitionBrief(root, needProject(p)),
    'transition.brief.set': (p) => { const r = setTransitionBrief(root, needProject(p), p || {}); runner?.wake(); poke(); return r; },
    'motion.brief.get': (p) => getMotionBrief(root, needProject(p)),
    'motion.brief.set': (p) => { const r = setMotionBrief(root, needProject(p), p || {}); runner?.wake(); poke(); return r; },
    'look.packs': () => listPacks(root),
    'look.fonts': () => listFonts(),
    'look.brief.get': (p) => getBrief(root, needProject(p)),
    'look.brief.set': (p) => { const r = setBrief(root, needProject(p), p || {}); runner?.wake(); poke(); return r; },
    // Picking another look asks first when shots exist (no `confirm`: nothing changes, the answer lists what would be
    // remade); confirmed, the shots made in the old look are remade in the new one (a Fix each), one grey chat line.
    'look.use': (p) => {
      const id = needProject(p);
      const r = useLook(root, id, { frame: p?.frame, confirm: p?.confirm === true });
      if (r.changed) {
        const name = lookView(root, id).frames.find((f) => f.id === r.using)?.name || r.using;
        const n = r.remaking.length;
        chatRow(id, { role: 'action', text: `Look: now ${name}${n ? ` · remaking ${n === 1 ? '1 shot' : `${n} shots`} in it` : ''}`, refs: [r.using, ...r.remaking.map((x) => x.node)].slice(0, 40) });
        emit('look.changed', { project: id, using: r.using, from: r.from, remaking: r.remaking });
        if (n) runner?.wake();
        poke();
      }
      return r;
    },
    'look.more': (p) => {
      const id = needProject(p);
      const r = moreLooks(root, id, { count: p?.count === undefined ? 2 : Number(p.count), text: p?.text });
      chatRow(id, { role: 'action', text: `Making ${r.making.length === 1 ? '1 more look' : `${r.making.length} more looks`} to pick from`, refs: r.making.map((x) => x.node) });
      emit('look.changed', { project: id, making: r.making });
      runner?.wake();
      poke();
      return r;
    },
    // ---- the script, line by line (Script panel + the chat's script card) ----
    'script.get': (p) => scriptView(root, needProject(p)),
    // The app's unsaved script changes, kept per project (null clears them); script.get hands them back.
    'script.draft': (p) => saveDraft(root, needProject(p), p?.draft),
    // Saves the owner's edits into the script (and the voice lines), then remakes only the pieces that used the
    // changed words: a Fix on each (the canvas's own path; versions kept). One grey chat line, one script.changed.
    'script.edit': (p) => {
      const id = needProject(p);
      const done = applyScriptEdits(root, id, p);
      const remade = [];
      for (const a of remakesFor(root, id, done)) {
        try {
          const r = nodeAction(root, id, a.node, { action: a.action, text: a.text });
          if (r.edit) remade.push({ node: a.node, kind: r.edit.kind, edit: r.edit.id });
        } catch (e) { console.error(`script.edit ${a.node}: ${e.message}`); }
      }
      const text = scriptActionText(done, remade);
      scriptLogged(root, id, text);
      chatRow(id, { role: 'action', text, refs: [...done.changed, ...done.added, ...remade.map((r) => r.node)].filter((x, i, l) => l.indexOf(x) === i).slice(0, 40) });
      const data = { project: id, beats: done.beats, changed: done.changed, added: done.added, keys: done.keys, deleted: done.deleted, restored: done.restored, renamed: done.renamed, remaking: remade };
      emit('script.changed', data);
      if (remade.length) runner?.wake();
      poke();
      return data;
    },
    'voice.line.take': (p) => {
      const id = needProject(p);
      const line = nodeId(p?.line, 'line');
      const r = nodeAction(root, id, line, { action: 'fix', text: `New take of this line${p?.text ? `: ${String(p.text).slice(0, 500)}` : ''} (keep it inside its slot).` });
      chatRow(id, { role: 'action', text: `New take: ${line}`, edit: r.edit.id, refs: [line] });
      runner?.wake();
      return { edit: r.edit };
    },
    'voice.booth.status': (p) => boothStatus(root, needProject(p)),
    // A recording (the app's own mic, or a file the owner picked) as a take; kept by default, so it plays at once.
    'voice.booth.import': async (p) => {
      const id = needProject(p);
      const line = String(p?.line || '');
      const file = String(p?.path || bad('voice.booth.import needs a path'));
      const wav = await toWav(file);
      const take = addTake(root, id, line, wav, { source: p?.source === 'booth' ? 'booth' : 'import' });
      const kept = p?.keep === false ? null : keepTake(root, id, line, take.take_no);
      chatRow(id, { role: 'action', text: `Recorded line ${line} (take ${take.take_no}${kept ? ', kept' : ''})`, refs: [line] });
      poke();
      return { take, kept: Boolean(kept) };
    },
    'voice.booth.keep': (p) => { const id = needProject(p); const t = keepTake(root, id, String(p?.line || ''), Number(p?.take)); poke(); return { take: t }; },

    // ---- node area (doc 35) ----
    'node.list': (p) => {
      const view = canvasView(root, needProject(p));
      return { nodes: Object.entries(view.nodes).map(([id, n]) => ({ ...n, id })), open_edits: view.open_edits };
    },
    'graph.get': (p) => graph.graphView(root, needProject(p)),
    // OK / Fix / Cut it (and clear) on one node: the canvas's own nodeAction. Every press is a grey line in the chat.
    'node.edit': (p) => {
      const action = String(p?.action || '').toLowerCase();
      if (!['ok', 'fix', 'cut', 'clear'].includes(action)) bad('action must be ok, fix, cut or clear');
      if (!p?.node) bad('node.edit needs a node');
      const id = needProject(p);
      const r = nodeAction(root, id, String(p.node), { action, text: p.text, bars: p.bars ?? null, part: p.part ?? null });
      const label = canvasView(root, id).nodes[String(p.node)]?.label || String(p.node);
      chatRow(id, { role: 'action', text: actionText(action, label, action === 'fix' ? p.text : null), refs: [String(p.node)], ...(r.edit ? { edit: r.edit.id } : {}) });
      if (r.edit) runner?.wake();
      poke();
      return r;
    },
    'node.move': (p) => graph.moveNodes(root, needProject(p), p?.positions),
    'node.reorder': (p) => { const id = needProject(p); const r = graph.reorder(root, id, p?.order); chatRow(id, { role: 'action', text: `New film order: ${r.film.map((s) => s.toUpperCase()).join(' › ')}` }); poke(); return r; },
    'node.pick': (p) => { const id = needProject(p); const r = graph.pickVersion(root, id, nodeId(p?.node), p?.v === null || p?.v === undefined ? null : Number(p.v)); chatRow(id, { role: 'action', text: `${String(p.node).toUpperCase()} plays v${r.plays}`, refs: [p.node] }); poke(); return r; },
    'node.duplicate': (p) => { const id = needProject(p); const r = graph.newShot(root, id, { op: 'duplicate', after: nodeId(p?.node), text: opt(p?.text) || '' }); chatRow(id, { role: 'action', text: `Duplicate ${String(p.node).toUpperCase()} as ${r.node.toUpperCase()}`, edit: r.edit.id, refs: [r.node] }); runner?.wake(); poke(); return r; },
    'node.insert': (p) => { const id = needProject(p); const r = graph.newShot(root, id, { op: 'insert', after: nodeId(p?.after, 'after'), text: opt(p?.text) || '' }); chatRow(id, { role: 'action', text: `New shot ${r.node.toUpperCase()} after ${String(p.after).toUpperCase()}: ${String(p.text).slice(0, 200)}`, edit: r.edit.id, refs: [r.node] }); runner?.wake(); poke(); return r; },
    'node.find': (p) => graph.findNodes(root, needProject(p), p?.query),

    // ---- loose preview (doc 36) ----
    'preview.plan': (p) => graph.previewPlan(root, needProject(p), { scope: ['all', 'from', 'shot', 'beat'].includes(p?.scope) ? p.scope : 'all', node: p?.node ? String(p.node) : null }),

    // ---- into Edit (doc 38) ----
    'handoff.plan': async (p) => {
      const id = needProject(p);
      const plan = graph.handoffPlan(root, id);
      // subtitles that would cover the picture's own words go to the top (the first time reads the shots' video)
      const shots = plan.clips.filter((c) => c.kind === 'shot');
      try { plan.subtitles_moved = await placeSubtitles({ captions: plan.captions, shots, cacheDir: join(canvasPaths(root, id).dir, 'ink') }); } catch { /* placed at the bottom as before */ }
      return plan;
    },
    'handoff.done': (p) => { const id = needProject(p); const r = graph.saveHandoff(root, id, p?.map || p); chatRow(id, { role: 'stage', text: STAGE_NAMES[STAGE_EDIT], stage: STAGE_EDIT }); poke(); return r; },
    'handoff.get': (p) => ({ map: graph.loadHandoff(root, needProject(p)) }),
    'handoff.swaps': (p) => graph.handoffSwaps(root, needProject(p)),
    'handoff.swapped': (p) => graph.markSwapped(root, needProject(p), nodeId(p?.node), p?.version, { clip: p?.clip, item: p?.item }),

    // ---- chat (doc 37) ----
    'chat.history': (p) => chatHistory(root, needProject(p), { limit: p?.limit, before: p?.before || null }),
    'chat.context': (p) => { state.context = contextOf(p?.context); emit('chat.context', { context: state.context }); return { context: state.context }; },
    // A button press the app wants in the history as a grey line.
    'chat.action': (p) => chatRow(needProject(p), { role: 'action', text: String(p?.text || bad('chat.action needs text')).slice(0, 2000), context: p?.context, refs: Array.isArray(p?.refs) ? p.refs.map(String).filter((x) => NODE_ID.test(x)) : [] }),
    'chat.answer': (p) => {
      const id = needProject(p);
      const qid = String(p?.question || '');
      if (!loadProject(root, id).questions.some((x) => x.id === qid)) {
        // A question that went on with its default (the `defaults` card): the owner's answer is kept with the
        // project, said in the chat (the AI reads the chat) and logged; nothing waits on it.
        const sub = [...gates.submissions(root, id)].reverse().find((s) => (s.questions || []).some((q) => q?.id === qid));
        const q = sub?.questions.find((x) => x?.id === qid);
        if (!q) bad(`no question ${qid}`);
        const value = String(p?.value ?? '').trim().slice(0, 4000) || q.default;
        if (value === null || value === undefined) bad('this question needs an answer');
        updateProject(root, id, (pr) => { pr.mfx = pr.mfx || {}; pr.mfx.answers = { ...(pr.mfx.answers || {}), [qid]: { step: sub.step, text: String(q.text).slice(0, 400), answer: value, at: new Date().toISOString() } }; });
        projectEvent(root, id, { event: 'ANSWERED', actor: 'you', step: sub.step, details: `${String(q.text).slice(0, 120)} → ${value}` });
        chatRow(id, { role: 'action', text: `${q.text} → ${value}` });
        runner?.wake();
        poke();
        return { question: qid, answer: value, was_default: true };
      }
      const r = gates.answerQuestion(root, id, qid, p?.value ?? null);
      const q = loadProject(root, id).questions.find((x) => x.id === p.question);
      chatRow(id, { role: 'action', text: `${q?.text || p.question} → ${q?.answer ?? p.value}` });
      runner?.wake();
      poke();
      return r;
    },
    'questions.list': (p) => loadProject(root, needProject(p)).questions,
    // The service is the source of truth: the user's own message comes back as a chat.message too. In a node
    // context it becomes a Fix there; otherwise the user's own Claude Code answers (and may pick a move).
    'chat.send': (p) => {
      const text = String(p?.text ?? '').trim().slice(0, 20_000);
      if (!text) bad('say something');
      const id = projectId() || p?.project;
      if (!id) {
        emit('chat.message', { role: 'user', text, at: new Date().toISOString() });
        emit('chat.message', { role: 'system', text: 'Open or make a project first: the chat talks about one video.', at: new Date().toISOString() });
        return { queued: true };
      }
      const context = p?.context === undefined ? state.context : contextOf(p.context);
      chatRow(id, { role: 'user', text, context: context !== 'video' ? context : undefined });
      const typed = typedMove(root, id, { text, context, confirm: p?.confirm === true });
      if (typed) {
        chatRow(id, typed.reply);
        if (typed.move) { runner?.wake(); poke(); }
        return { queued: true, move: typed.move || null };
      }
      emit('chat.typing', { on: true });
      quickReply(root, id, { text, context })
        .then((r) => {
          const done = applyMove(root, id, r.move);
          chatRow(id, { role: 'ai', text: r.reply, refs: r.move?.node ? [String(r.move.node)] : [], ...(context !== 'video' ? { context } : {}) });
          if (done?.row) chatRow(id, done.row);
          if (done?.edit) runner?.wake();
        })
        .catch((e) => chatRow(id, { role: 'system', text: `The AI couldn't answer: ${e.message}` }))
        .finally(() => { emit('chat.typing', { on: false }); poke(); });
      return { queued: true };
    },
    'activity.list': (p) => (activity.get(needProject(p)) || []).slice(-Math.min(300, Number(p?.limit) || 100)),

    // ---- render queue and export (doc 39) ----
    'render.list': (p) => { const id = p?.project || null; return queue.list().filter((j) => !id || j.project === id); },
    'render.add': (p) => queue.addExternal({ projectId: p?.project || projectId(), label: opt(p?.label, 120) || 'Export', kind: opt(p?.kind, 40) || 'export' }),
    'render.progress': (p) => queue.report(Number(p?.id), { done: Number(p?.done), total: Number(p?.total), phase: p?.phase || 'rendering' }),
    'render.finish': (p) => queue.finish(Number(p?.id), { ok: p?.ok === true, file: p?.file || null, error: p?.error || null }),
    'render.cancel': (p) => queue.cancel(Number(p?.id)),
    'export.target': (p) => exportTarget(root, needProject(p), { ext: p?.ext, name: p?.name, stems: Boolean(p?.stems), create: Boolean(p?.create) }),
    'export.done': (p) => { const id = needProject(p); const r = deliverFromApp(root, id, { file: p?.file, preset: opt(p?.preset, 120) || null, timeline_rev: p?.timeline_rev ?? null }); poke(); return r; },
    'publish.plan': (p) => postPlan(root, needProject(p), p || {}),
    'publish.send': async (p) => postSend(root, needProject(p), p || {}),

    // ---- setup (part 09) ----
    'setup.status': () => setup.setupStatus(root),
    'setup.backend': (p) => setup.setBackend(root, p?.backend === undefined ? undefined : String(p.backend), p?.claudeBin),
    'setup.done': (p) => setup.setupDone(root, p?.done !== false),
    'setup.claude.test': async () => setup.claudeTest(root),
    'keys.list': () => setup.keysList(root),
    'keys.set': (p) => setup.keySet(root, String(p?.id || ''), p?.value ?? null),
    'keys.remove': (p) => setup.keySet(root, String(p?.id || ''), null),
    'keys.check': async (p) => setup.keyCheck(root, String(p?.id || '')),
    // ---- the installer's components (packaging): status, and installing skipped ones later ----
    'components.status': async () => ({ components: await componentsStatus(root), installing }),
    'components.install': (p) => {
      if (installing) throw new ProtocolError('busy', `already installing ${installing.join(', ')}`);
      let ids;
      try { ids = parseComponents(Array.isArray(p?.ids) ? p.ids.join(',') : p?.ids ?? 'default'); } catch (e) { bad(e.message); }
      installing = ids;
      installComponents(root, ids, { onEvent: (e) => emit('components.progress', e) })
        .then((results) => emit('components.done', { results }))
        .catch((e) => emit('components.done', { results: [], error: e.message }))
        .finally(() => { installing = null; });
      return { started: ids };
    },
  };
  // Setup downloads and installs (part 09): setup.download / setup.install / setup.cancel, setup.progress events.
  Object.assign(handlers, setupHandlers({ root, emit, ProtocolError }));

  async function handle(msg) {
    if (!msg || typeof msg !== 'object' || Array.isArray(msg)) return send({ id: null, ok: false, error: { code: 'bad_request', message: 'a request is a JSON object' } });
    const { id = null, type, params = {} } = msg;
    const h = typeof type === 'string' && Object.hasOwn(handlers, type) ? handlers[type] : null;
    if (!h) return send({ id, ok: false, error: { code: 'unknown_type', message: `unknown type ${JSON.stringify(type)}` } });
    try {
      const result = await h(params && typeof params === 'object' && !Array.isArray(params) ? params : {});
      send({ id, ok: true, result: result ?? null });
    } catch (e) {
      const code = e instanceof ProtocolError ? e.code : 'failed';
      // A bug (not a refusal) goes to the log with its stack; the app gets the message either way.
      if (e instanceof TypeError || e instanceof ReferenceError || e instanceof RangeError || e instanceof SyntaxError) console.error(e);
      send({ id, ok: false, error: { code, message: String(e.message || e).slice(0, 2000) } });
    }
  }

  async function close() {
    closed = true;
    if (timer) clearTimeout(timer);
    if (interval) clearInterval(interval);
    if (runner) { await runner.stop(); runner.release(); }
  }

  return { handle, handlers, state, status, close, poke, get runner() { return runner; }, get queue() { return queue; } };
}

// Any recording to a 48 kHz mono 16-bit WAV for the booth (ffmpeg for everything that isn't WAV already).
async function toWav(file) {
  const { existsSync, statSync, readFileSync: read } = await import('node:fs');
  if (!existsSync(file) || !statSync(file).isFile()) throw new UserError('no such recording');
  if (/\.wav$/i.test(file)) return read(file);
  const { spawnSync } = await import('node:child_process');
  const r = spawnSync('ffmpeg', ['-v', 'error', '-i', file, '-ac', '1', '-ar', '48000', '-c:a', 'pcm_s16le', '-f', 'wav', 'pipe:1'], { maxBuffer: 200 * 1024 * 1024 });
  if (r.status !== 0) throw new UserError(`couldn't read that recording: ${String(r.stderr).trim().slice(-200)}`);
  return r.stdout;
}

// Run on stdin/stdout. Nothing but protocol lines goes to stdout: console.log goes to stderr.
// The studio's runner runs here unless MFX_SERVICE_NO_RUNNER=1.
export function runStdio({ input = process.stdin, output = process.stdout, root = studioRoot(), runner = process.env.MFX_SERVICE_NO_RUNNER !== '1' } = {}) {
  console.log = console.error;
  ensureStudio(root);
  const send = (obj) => output.write(JSON.stringify(obj) + '\n');
  const svc = createService({ root, send, runner });
  let buf = '';
  let chain = Promise.resolve();
  input.setEncoding('utf8');
  input.on('data', (chunk) => {
    buf += chunk;
    if (buf.length > 16 * 1024 * 1024 && !buf.includes('\n')) { buf = ''; console.error('mortiflix-service: dropped a line over 16 MB'); return; }
    let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line) continue;
      let msg;
      try { msg = JSON.parse(line); } catch { send({ id: null, ok: false, error: { code: 'bad_json', message: 'not JSON' } }); continue; }
      chain = chain.then(() => svc.handle(msg));
    }
  });
  // The app closed our stdin (it quit or restarts us): finish what is running and go.
  input.on('end', () => chain.finally(() => svc.close().finally(() => process.exit(0))));
  return svc;
}
