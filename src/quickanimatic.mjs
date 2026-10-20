// The animatic driven by the studio, not a session (owner, 2026-10-19: an animatic session reached 2.14M tokens in 26
// steps before it was stopped, reading every file of the project first and re-reading all of it on every step).
// Every AI call starts fresh and small:
//   1. write: the approved frames' code, the cut list, the voice's word times, the scenes, the motion brief and the
//      skills → src/timing.ts, src/scenes/animatic.tsx and src/Main.tsx (and small exports in the frames' code);
//   2. the studio puts the mastered narration in video/public, renders the animatic through the render queue
//      and runs qc.mjs (format, black and frozen frames, loudness, a contact sheet);
//   3. review: a fresh call reads the sheet with the QC report and the code, and passes or hands back fixed files
//      (render errors go back the same way); blocking problems must be fixed, polish after 3 rounds goes to the owner;
//   4. the studio writes out/scenes.json (one scene per beat, from the voice's times) and submits.
// The owner's notes on a version, a motion or rule pack, or an animatic that keeps failing go to a session.
import { runAs } from './models.mjs';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync, copyFileSync } from 'node:fs';
import { join, dirname, normalize } from 'node:path';
import { paths, REPO } from './studio.mjs';
import { loadProject, projectPaths, projectPipeline } from './projects.mjs';
import { stepView, stepChecks, submit } from './gates.mjs';
import { scriptView } from './svc/script.mjs';
import { sceneGaps, gapsText } from './scenes.mjs';
import { measure } from './quickvoice.mjs';
import { plainGain } from './soundtrack/voice.mjs';

const CHECKS = ['spelling-and-facts', 'text-safe-area', 'no-collisions', 'text-contrast', 'frame-sheet', 'format', 'loudness', 'sync', 'music-source', 'transitions-carried'];
const FILES = ['src/timing.ts', 'src/scenes/animatic.tsx', 'src/Main.tsx', 'src/video.ts', 'src/ui.tsx', 'src/scenes/frames.tsx'];
const QC = join(REPO, 'pipelines', '_shared', 'skills', 'final-pass', 'qc.mjs');
const FPS = 30;
const MAX_ROUNDS = 3;
const LEAD = 0.25; // a scene starts this long before its first word

const read = (f, n = 200_000) => (existsSync(f) ? readFileSync(f, 'utf8').slice(0, n) : '');
const readJsonOr = (f, d) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return d; } };

export function quickAnimaticStep(root, id, config) {
  if (config.quickAnimatic === false || config.quickScript === false) return null;
  const project = loadProject(root, id);
  const pipeline = projectPipeline(root, id);
  const step = pipeline.steps.find((s) => s.key === 'animatic');
  if (!step || step.review !== 'video') return null;
  const runnable = stepView(project, pipeline).filter((s) => ['ready', 'working', 'changes'].includes(s.state));
  if (runnable.length !== 1 || runnable[0].key !== 'animatic' || runnable[0].state === 'changes') return null;
  if (project.mfx && Object.keys(project.mfx).some((k) => k.endsWith('_wanted'))) return null;
  const work = projectPaths(root, id).work;
  const brief = readJsonOr(join(work, 'design', 'motion-brief.json'), null);
  if (!brief || brief.motion_pack || (brief.rule_packs || []).length) return null;
  for (const f of ['video/src/scenes/frames.tsx', 'video/src/cuts.ts', 'voice/timing.json', 'out/voice/narration.wav', 'transitions/board.json']) if (!existsSync(join(work, f))) return null;
  if (stepChecks(root, id, 'animatic').some((c) => !CHECKS.includes(c.id))) return null;
  return runnable[0];
}

// One scene per beat, from the voice: each starts LEAD before its first word (the first at 0), and runs to the next.
export function scenesFromTiming(beats, timing, totalFrames) {
  const starts = beats.map((b) => {
    const ls = timing.lines.filter((l) => l.id.startsWith(`${b.id}-`));
    return ls.length ? Math.max(0, (ls[0].words?.[0]?.start ?? ls[0].start) - LEAD) : null;
  });
  const out = [];
  beats.forEach((b, i) => {
    if (starts[i] === null) return;
    const from = i === 0 ? 0 : Math.round(starts[i] * FPS);
    const next = starts.slice(i + 1).find((s) => s !== null);
    const to = next === undefined ? totalFrames : Math.round(next * FPS);
    out.push({ key: `s${String(i + 1).padStart(2, '0')}`, name: b.name, from_frame: from, frames: Math.max(1, to - from), beat: b.id });
  });
  return out;
}

const FILE_SCHEMA = { type: 'array', items: { type: 'object', required: ['path', 'content'], properties: { path: { type: 'string', enum: FILES }, content: { type: 'string' } } } };
const WRITE_SCHEMA = {
  type: 'object',
  required: ['files', 'seconds', 'note', 'facts_note'],
  properties: {
    files: FILE_SCHEMA,
    seconds: { type: 'number', description: 'the animatic\'s length in seconds (the narration plus a short end hold)' },
    note: { type: 'string', description: 'for the owner: the pace and order, what moves on which word, what to judge' },
    facts_note: { type: 'string', description: 'every on-screen word, name and number checked against the script' },
  },
};
const REVIEW_SCHEMA = {
  type: 'object',
  required: ['ok', 'blocking', 'problems', 'checks'],
  properties: {
    ok: { type: 'boolean' },
    blocking: { type: 'boolean', description: 'true only for a broken rule: a blank or frozen stretch, wrong words, a scene that doesn\'t show its beat, the voice out of step' },
    problems: { type: 'array', items: { type: 'string' } },
    files: { ...FILE_SCHEMA, description: 'only when not ok: the files to replace, whole' },
    seconds: { type: 'number' },
    checks: { type: 'object', required: ['text-safe-area', 'no-collisions', 'spelling-and-facts', 'frame-sheet', 'text-contrast'], properties: { 'text-safe-area': { type: 'string' }, 'no-collisions': { type: 'string' }, 'spelling-and-facts': { type: 'string' }, 'frame-sheet': { type: 'string' }, 'text-contrast': { type: 'string' } } },
  },
};

function context(root, id) {
  const work = projectPaths(root, id).work;
  const project = loadProject(root, id);
  const script = scriptView(root, id);
  const timing = readJsonOr(join(work, 'voice', 'timing.json'), { seconds: 0, lines: [] });
  const board = readJsonOr(join(work, 'transitions', 'board.json'), { cuts: [] });
  const frames = (readJsonOr(join(work, 'design', 'frames.json'), { frames: [] }).frames || []).map((f) => ({ composition: f.composition || String(f.id).toUpperCase(), label: f.label, beats: f.beats || [] }));
  const lines = timing.lines.map((l) => ({ id: l.id, text: l.text, start: l.start, end: l.end, words: (l.words || []).map((w) => [w.text, w.start]) }));
  return { work, project, script, timing, board, frames, lines, brief: readJsonOr(join(work, 'design', 'motion-brief.json'), {}) };
}

function writePrompt(root, id, c) {
  const code = (f, n = 40_000) => `### ${f}\n${read(join(c.work, 'video', f), n)}`;
  const skill = (n) => read(join(c.work, '.claude', 'skills', n, 'SKILL.md'), 20_000);
  const row = read(join(c.work, 'pipeline', 'PIPELINE.md')).split('\n').find((l) => l.startsWith('| `animatic`')) || '';
  return [
    `You build the animatic for "${c.project.title}": the approved style frames in motion, timed to the approved narration and cut with the approved board. You write the Remotion code; the studio renders it, runs the QC and shows it back to you. Answer with the JSON asked for, nothing else.`,
    `## The step\n${row}`,
    `## The scenes (one per beat; the studio cuts the video into shots at each beat's first word, minus ${LEAD} s)\n${JSON.stringify(c.script.beats.map((b) => ({ beat: b.id, name: b.name, visual: b.visual, words: b.lines.map((l) => l.words).join(' ') })))}`,
    `## The narration, word by word (seconds; video/public/voice.wav, already mastered: play it as it is)\n${JSON.stringify(c.lines)}`,
    `## The approved style frames (compositions in src/scenes/frames.tsx) and the beats each shows\n${JSON.stringify(c.frames)}`,
    `## The approved board (src/cuts.ts holds the same cuts: use them as they are, at these lengths, landing on these words)\n${JSON.stringify(c.board.cuts.map((x) => ({ from: x.from, to: x.to, transition: x.transition, frames: x.frames, lands_on: x.lands_on, carrier: x.carrier })))}`,
    `## The motion brief\n${JSON.stringify(c.brief)}`,
    `## The code now\n${code('src/video.ts')}\n${code('src/cuts.ts')}\n${code('src/scenes/board.tsx', 8000)}\n${code('src/ui.tsx')}\n${code('src/scenes/frames.tsx')}\n${code('src/Main.tsx', 8000)}`,
    `## Skill: motion design\n${skill('motion-design')}`,
    `## Skill: Remotion (the studio renders for you: ignore the mfx commands)\n${skill('remotion-motion')}`,
    `## What to write
- src/timing.ts (the clock: word starts from the narration above, frames at ${FPS} fps), src/scenes/animatic.tsx (the scenes in motion), src/Main.tsx (the whole video: the scenes in a TransitionSeries with the board's cuts from src/cuts.ts, and <Audio src={staticFile('voice.wav')} />, total length = video.seconds).
- Each cut exactly as src/scenes/board.tsx (the approved board) does it: \`import { toPresentation } from './transitions/core';\` and \`<TransitionSeries.Transition presentation={toPresentation(cut.t as any, cut.params as any)} timing={linearTiming({ durationInFrames: cut.frames })} />\`. Never write your own toPresentation.
- Build from the approved frames: reuse their components; you may add exports or props to src/ui.tsx and src/scenes/frames.tsx, but every frame keeps its approved look. Set video.seconds in src/video.ts to your length (send video.ts too).
- Simple motion, every move on its word: the owner judges pace and order here, not polish. Never a blank or frozen stretch longer than a breath; the last scene may settle and hold.
- Imports only from 'react', 'remotion', '@remotion/transitions', the project's own files and '@remotion/google-fonts/<Family>'.`,
  ].join('\n\n');
}

function reviewPrompt(c, files, sheetName, qcText, renderError) {
  const code = files.map((f) => `### ${f}\n${read(join(c.work, 'video', f), 60_000)}`).join('\n');
  return [
    renderError
      ? `The animatic for "${c.project.title}" didn't render. Fix the code so it does.\n\n## The render's error\n${renderError.slice(-4000)}${/error #130/.test(renderError) ? '\n(React #130: something rendered is undefined: a missing export, a default/named import mix-up, or a transition presentation not made with toPresentation from ./transitions/core as src/scenes/board.tsx does)' : ''}`
      : `Look at the animatic for "${c.project.title}": open ./${sheetName} with Read (a contact sheet, about one frame a second, left to right then down, from 0 s).`,
    renderError ? '' : `## qc.mjs\n${qcText.slice(0, 4000)}`,
    `## The narration, word by word (seconds)\n${JSON.stringify(c.lines.map((l) => [l.id, l.start, l.text]))}`,
    `## The scenes\n${JSON.stringify(c.script.beats.map((b) => ({ beat: b.id, name: b.name, visual: b.visual })))}`,
    `## Checks\n- spelling-and-facts: the on-screen words match the script\n- text-safe-area: settled text 5% inside the edges (motion may pass an edge for a moment)\n- no-collisions: nothing overlaps once settled\n- text-contrast: text stays on the approved backgrounds\n- frame-sheet: every second of the sheet shows its scene; nothing blank or broken\n- still stretches qc.mjs lists: a designed hold (a phrase that settles, the end) is fine; an undesigned freeze is blocking\n- the rest of the qc.mjs report passes (format, black frames, loudness); render flags are the studio's, not yours`,
    `## The code now\n${code}`,
    renderError ? 'Answer ok: false, blocking: true, the problems, the fixed files (whole) and checks ("not rendered yet").'
      : 'If it passes, ok: true and a one-line note per check (what you looked at). Otherwise ok: false, blocking (true only for a broken rule), the problems, and the files to replace (whole). The owner judges pace at the gate: polish is not blocking.',
  ].filter(Boolean).join('\n\n');
}

function writeFiles(work, files) {
  for (const f of files || []) {
    if (!FILES.includes(f.path)) continue;
    const dest = normalize(join(work, 'video', f.path));
    if (!dest.startsWith(normalize(join(work, 'video', 'src')))) continue;
    mkdirSync(dirname(dest), { recursive: true });
    writeFileSync(dest, String(f.content));
  }
}

async function renderAnimatic({ renders, projectId, sessionId, c, env, gl }) {
  mkdirSync(join(c.work, 'out'), { recursive: true });
  // full size: a --scale must give whole pixels (0.6667 of 1080 is 720.036, which Remotion refuses)
  const job = renders.add({ projectId, sessionId, label: 'animatic', argv: ['bash', '-c', `npx remotion render src/index.ts Main ../out/animatic.mp4 --codec=h264 --crf=20 --image-format=png --pixel-format=yuv420p --log=error${gl ? ' --gl=angle' : ''}`], cwd: join(c.work, 'video'), env: { ...process.env, ...env } });
  let v = job;
  while (!['done', 'failed'].includes(v.state)) v = await renders.wait(job.id, projectId, 300);
  return v.state === 'done' && v.exit_code === 0 && existsSync(join(c.work, 'out', 'animatic.mp4')) ? { ok: true } : { ok: false, error: v.output_tail || v.error || 'the render failed' };
}

function qc(c, seconds) {
  mkdirSync(join(c.work, 'checks'), { recursive: true });
  const vt = read(join(c.work, 'video', 'src', 'video.ts'));
  const size = `${vt.match(/width:\s*(\d+)/)?.[1] || 1920}x${vt.match(/height:\s*(\d+)/)?.[1] || 1080}`;
  const r = spawnSync(process.execPath, [QC, 'out/animatic.mp4', '--size', size, '--fps', String(FPS), '--duration', String(Math.round(seconds)), '--lufs', String(c.voiceLufs ?? -14), '--true-peak', '-1', '--end-hold', '--sheet', 'checks/animatic-sheet.png', '--json', 'checks/qc-animatic.json'], { cwd: c.work, encoding: 'utf8', timeout: 10 * 60_000, maxBuffer: 16 * 1024 * 1024 });
  const json = readJsonOr(join(c.work, 'checks', 'qc-animatic.json'), null);
  // still stretches are the reviewer's call (a designed hold is fine, an undesigned freeze is blocking): everything
  // else qc.mjs checks (format, black frames, loudness, the sheet) must pass outright
  const hard = (json?.checks || []).filter((x) => x.id !== 'frames.frozen' && x.result === 'fail');
  return { pass: Boolean(json) && !hard.length, text: `${r.stdout || ''}${r.stderr || ''}`.trim(), json };
}

export async function quickAnimatic({ root, id, view, run, runArgs, config, onActivity, fallback, renders, sessionId }) {
  const c = context(root, id);
  const env = runArgs.env || {};
  const usage = { input_tokens: 0, output_tokens: 0 };
  const dir = join(paths(root).run, 'quick', id);
  mkdirSync(dir, { recursive: true });
  const call = async (prompt, schema, tools, role = 'write') => {
    const r = await runAs(run, role, { ...runArgs, workdir: dir, prompt, config: { ...config, claudeArgs: ['--tools', tools, ...(tools ? ['--allowedTools', tools] : []), '--json-schema', JSON.stringify(schema)] } });
    usage.input_tokens += r.usage?.input_tokens || 0;
    usage.output_tokens += r.usage?.output_tokens || 0;
    return r;
  };
  const give = async (why) => {
    onActivity({ kind: 'info', text: `The studio couldn't finish the animatic by itself (${why}): a session does it.` });
    const f = await fallback();
    return { ...f, usage: { input_tokens: usage.input_tokens + (f.usage?.input_tokens || 0), output_tokens: usage.output_tokens + (f.usage?.output_tokens || 0) } };
  };
  if (!renders) return give('no render queue');
  mkdirSync(join(c.work, 'video', 'public'), { recursive: true });
  // the narration as the voice step made it, with one plain gain (toward -14 LUFS, stopping where its own peaks reach
  // -1 dBTP): the same for every sample, never a limiter (soundtrack v1); the QC expects the level that gain gives
  const vm = measure(join(c.work, 'out', 'voice', 'narration.wav'));
  const vgain = vm ? plainGain(vm.i, vm.tp) : 0;
  const vr = spawnSync('ffmpeg', ['-hide_banner', '-nostdin', '-y', '-i', join(c.work, 'out', 'voice', 'narration.wav'), '-af', `volume=${vgain.toFixed(2)}dB`, '-c:a', 'pcm_f32le', join(c.work, 'video', 'public', 'voice.wav')], { encoding: 'utf8' });
  if (vr.status !== 0) return give('the narration could not be copied for the animatic');
  c.voiceLufs = vm ? +(vm.i + vgain).toFixed(1) : -14;
  // transitions that need WebGL render with --gl=angle
  const lib = env.MFX_TRANSITIONS ? readJsonOr(join(env.MFX_TRANSITIONS, 'catalog.json'), { transitions: [] }) : { transitions: [] };
  const gl = c.board.cuts.some((x) => (lib.transitions.find((t) => t.id === x.transition)?.requires || []).includes('webgl2'));

  // config._reuseCode (tests): start from the code already written, at the review, instead of a new write call
  const reuse = config._reuseCode && ['src/timing.ts', 'src/scenes/animatic.tsx', 'src/Main.tsx'].every((f) => existsSync(join(c.work, 'video', f)));
  if (!reuse) onActivity({ kind: 'info', text: 'Building the animatic: one call writes the whole video.' });
  const w = reuse ? { ok: true, structured: { files: [], seconds: config._reuseCode.seconds, note: config._reuseCode.note, facts_note: config._reuseCode.facts_note } } : await call(writePrompt(root, id, c), WRITE_SCHEMA, '');
  if (runArgs.signal?.aborted) return { ok: false, error: 'stopped', usage };
  const s0 = w.ok ? w.structured : null;
  if (!reuse && !s0?.files?.length) return give(w.ok ? 'the build call gave no code' : w.error || 'no answer');
  writeFiles(c.work, s0.files);
  let seconds = Number(s0.seconds) || c.timing.seconds + 1;
  let checks = null, polish = [], lastProblems = [], lastBlocking = true, lastChecks = null, qcOut = null;
  for (let round = 1; ; round++) {
    // the type check first: a missing export or a wrong prop shows up in seconds, with its file and line
    const tsc = spawnSync('npx', ['tsc', '--noEmit', '-p', '.'], { cwd: join(c.work, 'video'), encoding: 'utf8', timeout: 5 * 60_000, maxBuffer: 16 * 1024 * 1024 });
    const typeErrors = tsc.status === 0 ? null : `${tsc.stdout || ''}${tsc.stderr || ''}`.trim().split('\n').filter((l) => /error TS/.test(l)).slice(0, 40).join('\n');
    onActivity({ kind: 'info', text: typeErrors ? 'The code has type errors: fixing them before rendering.' : `Rendering the animatic (${Math.round(seconds)} s).` });
    const r = typeErrors ? { ok: false, error: `TypeScript (npx tsc --noEmit):\n${typeErrors}` } : await renderAnimatic({ renders, projectId: id, sessionId, c, env, gl });
    if (runArgs.signal?.aborted) return { ok: false, error: 'stopped', usage };
    let sheetName = null;
    if (r.ok) {
      qcOut = qc(c, seconds);
      sheetName = `animatic-sheet-${round}.png`;
      if (!existsSync(join(c.work, 'checks', 'animatic-sheet.png'))) return give(`qc.mjs made no sheet: ${qcOut.text.slice(-200)}`);
      copyFileSync(join(c.work, 'checks', 'animatic-sheet.png'), join(dir, sheetName));
    }
    if (round > MAX_ROUNDS) {
      if (r.ok && qcOut.pass && !lastBlocking) { checks = lastChecks; polish = lastProblems; break; }
      return give(`still not passing after ${MAX_ROUNDS} rounds: ${[...lastProblems, r.ok && !qcOut.pass ? qcOut.text.slice(-200) : ''].join('; ').slice(0, 300)}`);
    }
    onActivity({ kind: 'info', text: r.ok ? `Checking the animatic (round ${round}).` : 'The animatic didn\'t render: fixing the code.' });
    // every file the video is built from (cuts.ts read-only: the approved board)
    const shown = [...FILES, 'src/cuts.ts', 'src/scenes/board.tsx'];
    const rv = await call(reviewPrompt(c, shown, sheetName, r.ok ? qcOut.text : '', r.ok ? null : r.error), REVIEW_SCHEMA, r.ok ? 'Read' : '', r.ok ? 'review' : 'codefix');
    if (runArgs.signal?.aborted) return { ok: false, error: 'stopped', usage };
    const s = rv.ok ? rv.structured : null;
    if (!s) return give(rv.error || 'the check call gave no answer');
    if (r.ok && qcOut.pass && s.ok) { checks = s.checks; break; }
    if (!s.files?.length) return give(`problems but no fix: ${[...(s.problems || []), r.ok && !qcOut.pass ? qcOut.text.slice(-200) : ''].join('; ').slice(0, 300)}`);
    writeFiles(c.work, s.files);
    if (Number(s.seconds)) seconds = Number(s.seconds);
    lastProblems = s.problems || [];
    lastBlocking = Boolean(s.blocking) || !r.ok;
    lastChecks = s.checks;
  }

  // one scene per beat, from the voice, and the submission
  const total = Math.round((qcOut.json?.duration ?? seconds) * FPS);
  const scenes = scenesFromTiming(c.script.beats, c.timing, total);
  writeFileSync(join(c.work, 'out', 'scenes.json'), JSON.stringify({ made_from: 'out/animatic.mp4', fps: FPS, scenes }, null, 2));
  const gaps = sceneGaps(c.work);
  if (gaps.length) return give(gapsText(gaps));
  const qcCheck = (k) => (qcOut.json?.checks || []).find((x) => x.id === k)?.detail || null;
  const lufs = qcCheck('audio.loudness');
  const tp = qcCheck('audio.true-peak');
  const error_checks = [
    { id: 'spelling-and-facts', result: 'pass', note: `${s0.facts_note} On the rendered sheet: ${checks['spelling-and-facts']}` },
    { id: 'text-safe-area', result: 'pass', note: checks['text-safe-area'] },
    { id: 'no-collisions', result: 'pass', note: checks['no-collisions'] },
    { id: 'text-contrast', result: 'pass', note: checks['text-contrast'] },
    { id: 'frame-sheet', result: 'pass', note: `checks/animatic-sheet.png (qc.mjs, about a frame a second) viewed: ${checks['frame-sheet']}` },
    { id: 'format', result: 'pass', note: `qc.mjs passes: the video's own size, ${FPS} fps, ${Math.round(seconds)} s, no black or frozen stretch but the designed end hold.` },
    { id: 'loudness', result: 'pass', note: `qc.mjs: ${lufs || `integrated loudness inside ${c.voiceLufs} LUFS ±1`}, true peak ${tp || '≤ -1 dBTP'}; the approved narration untouched but for one plain gain (toward -14 LUFS, stopped by its own peaks at -1 dBTP), no music yet.` },
    { id: 'sync', result: 'pass', note: `The approved narration plays as it is; src/timing.ts holds its word starts and every move is placed on one. Each beat's scene in out/scenes.json starts ${LEAD} s before its first word.` },
    { id: 'music-source', result: 'n/a', note: 'No music yet: the music step scores this timing.' },
    { id: 'transitions-carried', result: 'pass', note: `The approved board's ${c.board.cuts.length} cuts from src/cuts.ts, at their lengths, in src/Main.tsx's TransitionSeries.` },
  ].filter((x) => stepChecks(root, id, 'animatic').some((k) => k.id === x.id));
  const note = polish.length ? `${s0.note}\n\nStill to look at (polish the studio's check raised; QC passes): ${polish.join(' · ')}` : s0.note;
  try {
    submit(root, id, 'animatic', { note, questions: [], items: [{ path: 'out/animatic.mp4', label: `Animatic (${Math.round(seconds)} s)` }, { path: 'checks/animatic-sheet.png', label: 'Frame sheet' }], error_checks });
  } catch (e) { return give(`the submission was refused: ${e.message}`); }
  const ck = join(c.work, 'checklist.md');
  if (existsSync(ck)) {
    const t = readFileSync(ck, 'utf8').split('\n');
    const at = t.findIndex((l) => /^##\s+Animatic\b/i.test(l));
    if (at >= 0) { for (let i = at + 1; i < t.length && !/^##\s/.test(t[i]); i++) t[i] = t[i].replace(/^- \[ \]/, '- [x]'); writeFileSync(ck, t.join('\n')); }
  }
  appendFileSync(join(c.work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · Animatic v${view.version + 1} built and submitted by the studio (no session): ${Math.round(seconds)} s, ${scenes.length} scenes.\n`);
  onActivity({ kind: 'info', text: `Animatic ready for your check: ${Math.round(seconds)} s.` });
  return { ok: true, usage, summary: s0.note };
}
