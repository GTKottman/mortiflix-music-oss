// The eight Mortiflix Studio stages over a pipeline's steps (owner, 2026-10-18): 1 Script · 2 Voice · 3 Style frames ·
// 4 Transitions · 5 Animatic · 6 Music · 7 Edit · 8 Render. Nothing of a stage is made before the stage before it
// passed its gate (gates.mjs). Each step belongs to one stage (its `stage`, else its key, else what it
// makes). A stage's status is worked out from its steps and its pieces on the canvas, never stored; only the owner's
// Next click ("stage done") and the last stage looked at are kept, in project.json `mfx`.
import { loadProject, projectPipeline, update, event, now } from '../projects.mjs';
import { stepView, respond, answerQuestion, reopen, GateError } from '../gates.mjs';
import { canvasView, loadOwner, nodeAction } from '../canvas.mjs';
import { APPS, MIDI, STAGED, VIDEO_STAGES, VIDEO_STAGE_NAMES, APP_STAGES, APP_STAGE_NAMES, MIDI_STAGES, MIDI_STAGE_NAMES } from '../product.mjs';

// The app studio has its own ten stages and the MIDI studio its own nine (src/product.mjs), and neither has the video's
// Music, Edit or Render.
export const STAGES = APPS ? APP_STAGES : MIDI ? MIDI_STAGES : VIDEO_STAGES;
export const STAGE_NAMES = APPS ? APP_STAGE_NAMES : MIDI ? MIDI_STAGE_NAMES : VIDEO_STAGE_NAMES;
// The six stages before 2026-10-18 (script, look, shots, voiceMusic, edit, render) → the eight: a project saved then
// opens on the same work.
export const OLD_STAGE = [0, 2, 4, 5, 6, 7];
const OLD_DONE = { look: ['styleFrames'], shots: ['transitions', 'animatic'], voiceMusic: ['voice', 'music'] };
export const STAGE_EDIT = STAGED ? -1 : 6;
export const STAGE_RENDER = STAGED ? -1 : 7;
export const STAGE_MUSIC = STAGED ? -1 : 5;
const DONE = ['approved', 'done', 'skipped'];

const BY_KEY = {
  brief: 0, script: 0, genre: 0, blueprint: 0,
  voice: 1, narration: 1,
  'style-frames': 2, directions: 2, look: 2,
  transitions: 3,
  animatic: 4, shots: 4,
  music: 5, score: 5, instruments: 5, mix: 5,
  build: 6,
  final: 7,
};
// The canvas pieces each stage shows (doc 35): the node kinds counted for its "made / total" (shots only on stage 3:
// the transition board never inflates the shot count, owner's first run "0/8" → "3/11"), and the kinds whose pieces
// can wait for the owner there.
const KINDS = STAGED ? STAGES.map(() => []) : [['beat'], ['line'], ['frame'], ['transition'], ['shot'], ['cue'], [], []];
const REVIEW_KINDS = KINDS;
const NOUN = STAGED ? STAGES.map(() => '') : ['scenes', 'voice lines', 'style frames', 'transitions', 'shots', 'cues', '', ''];

export function stageOf(step) {
  if (step.stage && STAGES.includes(step.stage)) return STAGES.indexOf(step.stage);
  if (STAGED) return 0;
  if (BY_KEY[step.key] !== undefined) return BY_KEY[step.key];
  if (step.delivers) return STAGE_RENDER;
  if (step.canvas === 'line') return 1;
  if (step.canvas === 'frame') return 2;
  if (step.canvas === 'transition') return 3;
  if (step.canvas === 'shot') return 4;
  if (step.canvas === 'beat') return 0;
  if ((step.work || []).some((w) => ['music', 'instruments'].includes(w))) return STAGE_MUSIC;
  if (step.waits_for_approval) return STAGE_EDIT;
  return 0;
}

export const stageIndex = (v) => (Number.isInteger(v) && v >= 0 && v < STAGES.length ? v : STAGES.indexOf(v));

// A project's `mfx` saved with the six stages, in the eight (stage index and the stages pressed Next on). Idempotent.
export function upgradeMfx(mfx) {
  if (!mfx || mfx.stages === STAGES.length) return false;
  // app projects never had the six video stages
  if (STAGED) { mfx.stages = STAGES.length; return true; }
  if (Number.isInteger(mfx.stage)) mfx.stage = OLD_STAGE[mfx.stage] ?? 0;
  const done = {};
  for (const [k, at] of Object.entries(mfx.stages_done || {})) for (const n of OLD_DONE[k] || [k]) done[n] = at;
  mfx.stages_done = done;
  mfx.stages = STAGES.length;
  return true;
}
// A project's `mfx` as the eight stages see it, without saving (reads).
export function mfxView(project) {
  const m = structuredClone(project?.mfx || {});
  upgradeMfx(m);
  return m;
}

// What the stage bar shows, one StageStatus per stage (protocol.md). `running`: a session is working on this project.
export function stageStatuses(root, id, { running = false, graph = null } = {}) {
  const project = loadProject(root, id);
  const pipeline = projectPipeline(root, id);
  const view = stepView(project, pipeline);
  let g = graph;
  if (!g) { try { g = canvasView(root, id); } catch { g = { nodes: {} }; } }
  const nodes = Object.values(g.nodes || {}).filter((n) => !n.missing);
  const done = mfxView(project).stages_done || {};
  const stopped = ['paused', 'cancelled'].includes(project.state) && !(project.state === 'paused' && project.needs_you && !project.needs_you.by_you);
  return STAGES.map((key, i) => {
    const steps = view.filter((s) => stageOf(s) === i);
    const pieces = nodes.filter((n) => KINDS[i].includes(n.kind));
    // a project made before its pipeline had this stage's own step (voice lines from the animatic) still has the stage
    const present = steps.length > 0 || i >= STAGE_EDIT || pieces.length > 0;
    const live = pieces.filter((n) => n.state !== 'cut');
    const reviewable = nodes.filter((n) => REVIEW_KINDS[i].includes(n.kind) && n.state !== 'cut');
    const made = live.filter((n) => !n.planned && n.versions?.length);
    const questions = project.questions.filter((q) => !q.answered_at && steps.some((s) => s.key === q.step)).length;
    // Pieces waiting for the owner: a failed remake, or a piece of a step in review that has no OK / Cut yet.
    const reviewKinds = steps.filter((s) => s.state === 'in_review' && s.canvas).map((s) => s.canvas);
    const undecided = reviewable.filter((n) => reviewKinds.includes(n.kind) && !n.owner && !n.planned && n.state !== 'generating').length;
    const failed = reviewable.filter((n) => n.state === 'needs you').length;
    const docsInReview = steps.filter((s) => s.state === 'in_review' && !s.canvas).length;
    const need = questions + failed + (done[key] ? 0 : undecided + docsInReview);
    const making = steps.some((s) => ['working', 'changes'].includes(s.state) || (s.state === 'ready' && project.state === 'queued'))
      || pieces.some((n) => n.state === 'generating' && !n.planned) || (running && steps.some((s) => s.state === 'ready'));
    const allDone = steps.length > 0 && steps.every((s) => DONE.includes(s.state));
    const reviewedOrDone = steps.length > 0 && steps.every((s) => DONE.includes(s.state) || s.state === 'in_review');
    let kind = 'notStarted';
    if (!present) kind = 'notStarted';
    else if (stopped && (making || steps.some((s) => s.state === 'ready'))) kind = 'stopped';
    else if (project.state === 'paused' && project.needs_you && !project.needs_you.by_you && steps.some((s) => ['working', 'ready', 'changes'].includes(s.state))) kind = 'needsYou';
    else if (making) kind = 'making';
    else if (need > 0) kind = 'needsYou';
    else if (done[key] || allDone) kind = 'done';
    else if (reviewedOrDone) kind = 'ready';
    // its pieces are all made and nothing waits: ready to check, never "not started" (owner's run: "not started 17/17")
    const allMade = live.length > 0 && made.length === live.length;
    if (kind === 'notStarted' && present && allMade) kind = 'ready';
    const total = pieces.length ? live.length : steps.length;
    const doneCount = pieces.length ? made.length : steps.filter((s) => DONE.includes(s.state) || s.state === 'in_review').length;
    const text = !present ? 'not in this preset'
      : need ? `${need} need${need === 1 ? 's' : ''} you`
        : pieces.length ? `${made.length}/${live.length} ${NOUN[i]}${allMade && kind === 'ready' ? ' ready' : ''}` : '';
    return { kind, done: doneCount, total, need, text, key, present, steps: steps.map((s) => ({ key: s.key, name: s.name, state: s.state, version: s.version })) };
  });
}

// The owner's Next on a stage: everything in it is kept as it is now. Steps in review are approved (pieces without an
// OK are OK'd as they are; questions take their defaults). `then`: the stage the owner goes to.
export function stageDone(root, id, from, { reason = 'Next' } = {}) {
  const i = stageIndex(from);
  if (i < 0) throw new GateError(`stage must be 0-${STAGES.length - 1} or a stage key`);
  const pipeline = projectPipeline(root, id);
  const kept = [];
  const left = [];
  // Questions of this stage's steps take their defaults.
  for (const q of loadProject(root, id).questions.filter((x) => !x.answered_at)) {
    const step = pipeline.steps.find((s) => s.key === q.step);
    if (!step || stageOf(step) !== i) continue;
    if (q.default === null) { left.push({ question: q.id, why: 'has no default: answer it in the chat' }); continue; }
    answerQuestion(root, id, q.id, null);
    kept.push({ question: q.id, answer: q.default });
  }
  for (const s of stepView(loadProject(root, id), pipeline).filter((x) => stageOf(x) === i && x.state === 'in_review')) {
    try {
      if (s.canvas) {
        const g = canvasView(root, id);
        const owner = loadOwner(root, id);
        for (const n of Object.values(g.nodes).filter((x) => x.kind === s.canvas && !x.missing && !x.planned && x.versions?.length && !owner.decisions[x.id])) {
          nodeAction(root, id, n.id, { action: 'ok' });
        }
      }
      const now2 = stepView(loadProject(root, id), pipeline).find((x) => x.key === s.key);
      if (now2.state === 'in_review') respond(root, id, s.key, now2.version, { verdict: 'approve', overall: `${reason}: kept as it is.` });
      kept.push({ step: s.key, version: s.version });
    } catch (e) {
      left.push({ step: s.key, why: e.message });
    }
  }
  update(root, id, (p) => {
    p.mfx ||= {};
    upgradeMfx(p.mfx);
    p.mfx.stages_done = { ...(p.mfx.stages_done || {}), [STAGES[i]]: now() };
  });
  event(root, id, { event: 'STAGE_DONE', actor: 'you', details: `${STAGE_NAMES[i]}: ${reason}` });
  return { stage: i, key: STAGES[i], kept, left };
}

// Leaving Music for Edit (Edit the video) or Render (Get the video): the app builds the edit from the
// graph and renders it with Mortiflix Studio's export (docs 38, 39), so the session never builds or renders a final. Those
// steps are skipped, marked as done in the app.
export function handOverToApp(root, id, how) {
  const pipeline = projectPipeline(root, id);
  return update(root, id, (p) => {
    const skipped = [];
    for (const s of pipeline.steps) {
      if (stageOf(s) < STAGE_EDIT || DONE.includes(p.steps[s.key]?.state)) continue;
      p.steps[s.key] = { ...(p.steps[s.key] || {}), state: 'skipped', by: 'app', how };
      skipped.push(s.key);
    }
    p.mfx ||= {};
    p.mfx.handover = { how, at: now() };
    event(root, id, { event: how === 'get' ? 'GET_THE_VIDEO' : 'EDIT_THE_VIDEO', actor: 'you', details: skipped.length ? `done in the app: ${skipped.join(', ')}` : '' });
    return { how, skipped };
  });
}

// Before going back (doc 30 "going back lists exactly what gets remade"): if something in `stage` changes, which later
// steps and pieces are made again.
export function remakesIf(root, id, stage) {
  const i = stageIndex(stage);
  if (i < 0) throw new GateError(`stage must be 0-${STAGES.length - 1} or a stage key`);
  const pipeline = projectPipeline(root, id);
  const view = stepView(loadProject(root, id), pipeline);
  const start = new Set(view.filter((s) => stageOf(s) === i).map((s) => s.key));
  const after = new Set(start);
  let grew = true;
  while (grew) { grew = false; for (const s of pipeline.steps) if (!after.has(s.key) && s.after.some((a) => after.has(a))) { after.add(s.key); grew = true; } }
  let g = { nodes: {} };
  try { g = canvasView(root, id); } catch { /* no canvas yet */ }
  return view.filter((s) => after.has(s.key) && !start.has(s.key) && s.version > 0).map((s) => ({
    step: s.key, name: s.name, stage: STAGES[stageOf(s)],
    pieces: s.canvas ? Object.values(g.nodes).filter((n) => n.kind === s.canvas && !n.missing && n.versions?.length).length : 0,
  }));
}

// Going back to change a stage: its last reviewed step goes back for changes with the owner's words (gates.reopen),
// and the stages from it on are no longer "done".
export function reopenStage(root, id, stage, text) {
  const i = stageIndex(stage);
  if (i < 0) throw new GateError(`stage must be 0-${STAGES.length - 1} or a stage key`);
  const pipeline = projectPipeline(root, id);
  const view = stepView(loadProject(root, id), pipeline).filter((s) => stageOf(s) === i && s.review !== 'internal' && ['approved', 'done'].includes(s.state));
  const step = view.at(-1);
  if (!step) throw new GateError(`nothing in ${STAGE_NAMES[i]} is approved yet: say what to change in the chat`);
  const r = reopen(root, id, step.key, { overall: String(text || '').slice(0, 4000) });
  // The MIDI studio: every later stage is built on this one, so each is made again (owner confirmed in the chat first).
  if (MIDI) {
    const later = pipeline.steps.slice(pipeline.steps.findIndex((s) => s.key === step.key) + 1).map((s) => s.key);
    update(root, id, (p) => { for (const k of later) delete p.steps[k]; });
    r.remade = later.map((k) => pipeline.steps.find((s) => s.key === k).name);
  }
  update(root, id, (p) => {
    p.mfx ||= {};
    upgradeMfx(p.mfx);
    const d = { ...(p.mfx.stages_done || {}) };
    for (const k of STAGES.slice(i)) delete d[k];
    p.mfx.stages_done = d;
  });
  return { ...r, stage: i, key: STAGES[i] };
}

export function saveLastStage(root, id, stage) {
  return update(root, id, (p) => { p.mfx ||= {}; upgradeMfx(p.mfx); p.mfx.stage = stage; return stage; });
}
