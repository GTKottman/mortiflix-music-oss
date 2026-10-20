// The style frames driven by the studio, not a session (owner, 2026-10-19, "B"): a session took 2.07M tokens in 26
// steps, most of it the same context re-read while 13 full-size frame pictures and its own patch scripts piled up.
// Here every AI call starts fresh and small:
//   1. write: the script, the voice's length, the look the owner picked, the format, the two skills and the template
//      → the look's code (src/video.ts, src/ui.tsx, src/scenes/frames.tsx), the frame list and LOOK.md;
//   2. the studio renders the stills through the render queue and makes ONE contact sheet of them, and measures the
//      contrast of the text colours the AI says it used;
//   3. review: a fresh call looks at the sheet with the check results and the code, and says ok or hands back fixed
//      files (a render error goes back the same way); at most a few rounds;
//   4. the studio writes design/frames.json and submits with the checks.
// Anything it can't finish (the owner's notes on a version, a render that keeps failing) goes to a session.
import { runAs } from './models.mjs';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync, copyFileSync, rmSync } from 'node:fs';
import { join, dirname, normalize } from 'node:path';
import { paths } from './studio.mjs';
import { loadProject, projectPaths, projectPipeline } from './projects.mjs';
import { stepView, stepChecks, submit } from './gates.mjs';
import { scriptView } from './svc/script.mjs';

const CHECKS = ['spelling-and-facts', 'text-safe-area', 'no-collisions', 'text-contrast'];
const FILES = ['src/video.ts', 'src/ui.tsx', 'src/scenes/frames.tsx'];
const MAX_FIX = 3;
const MAX_REVIEW = 3;

export function quickFramesStep(root, id, config) {
  if (config.quickFrames === false || config.quickScript === false) return null;
  const project = loadProject(root, id);
  const pipeline = projectPipeline(root, id);
  const step = pipeline.steps.find((s) => s.key === 'style-frames');
  if (!step || step.review !== 'frames') return null;
  const runnable = stepView(project, pipeline).filter((s) => ['ready', 'working', 'changes'].includes(s.state));
  if (runnable.length !== 1 || runnable[0].key !== 'style-frames' || runnable[0].state === 'changes') return null;
  if (project.mfx && Object.keys(project.mfx).some((k) => k.endsWith('_wanted'))) return null;
  const work = projectPaths(root, id).work;
  if (!existsSync(join(work, 'design', 'look-brief.json'))) return null;
  if (!existsSync(join(work, '.claude', 'skills', 'remotion-motion', 'setup.mjs'))) return null;
  if (stepChecks(root, id, 'style-frames').some((c) => !CHECKS.includes(c.id))) return null;
  return runnable[0];
}

const read = (f, n = 200_000) => (existsSync(f) ? readFileSync(f, 'utf8').slice(0, n) : '');
const readJsonOr = (f, d) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return d; } };

// WCAG contrast of two #rrggbb colours (1..21).
export function contrast(a, b) {
  const lum = (h) => {
    const m = /^#?([0-9a-f]{6})$/i.exec(String(h).trim());
    if (!m) return null;
    const c = [0, 2, 4].map((i) => parseInt(m[1].slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
  };
  const x = lum(a), y = lum(b);
  if (x === null || y === null) return null;
  return Math.round(((Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05)) * 100) / 100;
}

const FORMAT = { '16:9': [1920, 1080], '9:16': [1080, 1920], '1:1': [1080, 1080], '4:5': [1080, 1350] };

const FILE_SCHEMA = { type: 'array', items: { type: 'object', required: ['path', 'content'], properties: { path: { type: 'string', enum: FILES }, content: { type: 'string' } } } };
const PAIRS_SCHEMA = { type: 'array', description: 'every text colour on its background, per frame', items: { type: 'object', required: ['frame', 'fg', 'bg', 'px'], properties: { frame: { type: 'string' }, fg: { type: 'string', description: '#rrggbb' }, bg: { type: 'string', description: '#rrggbb' }, px: { type: 'number', description: 'font size in px' } } } };
const WRITE_SCHEMA = {
  type: 'object',
  required: ['files', 'frames', 'look_md', 'text_pairs', 'note', 'facts_note'],
  properties: {
    files: FILE_SCHEMA,
    frames: { type: 'array', items: { type: 'object', required: ['id', 'label', 'beats'], properties: { id: { type: 'string', description: 'SF01, SF02, … (the export name in frames)' }, label: { type: 'string', description: 'SF01 · Opening' }, beats: { type: 'array', items: { type: 'string' } } } } },
    look_md: { type: 'string', description: 'design/LOOK.md: palette (hex), type, shape language, texture, and why' },
    text_pairs: PAIRS_SCHEMA,
    note: { type: 'string', description: 'for the owner: what each frame is and what to look at' },
    facts_note: { type: 'string', description: 'every on-screen word, name and number checked against the script' },
  },
};
const REVIEW_SCHEMA = {
  type: 'object',
  required: ['ok', 'problems', 'checks'],
  properties: {
    ok: { type: 'boolean', description: 'true when every frame passes every check and reads as its scene' },
    problems: { type: 'array', items: { type: 'string' } },
    files: { ...FILE_SCHEMA, description: 'only when not ok: the files to replace, whole' },
    text_pairs: PAIRS_SCHEMA,
    checks: { type: 'object', required: ['text-safe-area', 'no-collisions', 'spelling-and-facts'], properties: { 'text-safe-area': { type: 'string' }, 'no-collisions': { type: 'string' }, 'spelling-and-facts': { type: 'string' } } },
  },
};

function context(root, id) {
  const pp = projectPaths(root, id);
  const work = pp.work;
  const project = loadProject(root, id);
  const script = scriptView(root, id);
  const timing = readJsonOr(join(work, 'voice', 'timing.json'), { seconds: null, lines: [] });
  const look = readJsonOr(join(work, 'design', 'look-brief.json'), {});
  const aspect = project.intake?.answers?.aspect || '16:9';
  const [w, h] = FORMAT[aspect] || FORMAT['16:9'];
  const scenes = script.beats.map((b) => {
    const ls = timing.lines.filter((l) => l.id.startsWith(`${b.id}-`));
    return { id: b.id, name: b.name, words: b.lines.map((l) => l.words).join(' '), visual: b.visual || '', from_sec: ls[0]?.start ?? null, to_sec: ls.at(-1)?.end ?? null };
  });
  const row = read(join(work, 'pipeline', 'PIPELINE.md')).split('\n').find((l) => l.startsWith('| `style-frames`')) || '';
  return { pp, work, project, script, timing, look, aspect, w, h, scenes, row };
}

function writePrompt(root, id, c) {
  const tpl = (f) => read(join(c.work, 'video', f), 20_000);
  const skill = (n) => read(join(c.work, '.claude', 'skills', n, 'SKILL.md'), 20_000);
  const checks = stepChecks(root, id, 'style-frames');
  const pack = c.look.pack_files ? JSON.stringify(c.look.pack_files).slice(0, 4000) : '';
  return [
    `You design the style frames for "${c.project.title}", a ${projectPipeline(root, id).makes} made in the Mortiflix studio. You write the look's code for a Remotion project; the studio renders the frames and shows them back to you. Answer with the JSON asked for, nothing else.`,
    `## Format\n${c.w}×${c.h} (${c.aspect}), 30 fps, the narration runs ${c.timing.seconds ?? '?'} s.`,
    `## The look the owner picked (use it as given: the palette exactly)\n${JSON.stringify({ ...c.look, pack_files: undefined }, null, 1)}${pack ? `\nStyle pack files: ${pack}` : ''}`,
    `## The approved script, scene by scene (with when each is spoken)\n${JSON.stringify(c.scenes, null, 1)}`,
    `## The step\n${c.row}`,
    `## Checks every frame must pass\n${checks.map((x) => `- ${x.id}: ${x.how}`).join('\n')}`,
    `## Skill: motion design\n${skill('motion-design')}`,
    `## Skill: Remotion (the studio renders for you: ignore the mfx commands)\n${skill('remotion-motion')}`,
    `## The project as it is (template)\n### src/video.ts\n${tpl('src/video.ts')}\n### src/Main.tsx (leave it: the animatic writes it)\n${tpl('src/Main.tsx')}`,
    `## What to write
- Exactly these files, whole: src/video.ts (format from above, the look's palette and fonts in \`look\`), src/ui.tsx (the look's shared pieces: backgrounds, shapes, labels), src/scenes/frames.tsx.
- src/scenes/frames.tsx exports \`export const frames: Record<string, React.FC> = { SF01, SF02, … }\`: 3 to 5 stills, each a real moment of the script with its real words, at full resolution. The studio's src/Root.tsx registers each as a composition with that id and renders frame 0.
- Imports only from 'react', 'remotion', '@remotion/google-fonts/<Family>' (call loadFont at the top of ui.tsx) and your own files. No network, no randomness except random('seed') from remotion, no images from outside.
- text_pairs: every text colour you used, on the background behind it, per frame, with its size.`,
  ].join('\n\n');
}

function reviewPrompt(c, state, sheetName, renderError) {
  const code = FILES.map((f) => `### ${f}\n${read(join(c.work, 'video', f), 60_000)}`).join('\n');
  return [
    renderError
      ? `The style frames for "${c.project.title}" didn't render. Fix the code so they do.\n\n## The render's error\n${renderError.slice(-4000)}`
      : `Look at the style frames for "${c.project.title}": open ./${sheetName} with Read. It's one contact sheet, ${state.frames.length} frames left to right then down, in this order: ${state.frames.map((f) => f.label).join(' · ')}. Each frame is ${c.w}×${c.h}, shown smaller.`,
    `## The scenes they show\n${JSON.stringify(c.scenes.map((s) => ({ id: s.id, words: s.words, visual: s.visual })), null, 1)}`,
    renderError ? '' : `## Measured by the studio\nText contrast (WCAG, needs 4.5:1, 3:1 for text 36 px and up): ${state.contrast.map((p) => `${p.frame} ${p.fg} on ${p.bg} ${p.px}px = ${p.ratio ?? '?'}:1${p.ok ? '' : ' FAILS'}`).join('; ') || '(no text colours given)'}`,
    `## Checks\n- spelling-and-facts: every on-screen word matches the script\n- text-safe-area: all text at least 5% inside every edge\n- no-collisions: no text or shape overlaps another it shouldn't\n- text-contrast: the measured ratios above pass`,
    `## The code now\n${code}`,
    renderError
      ? 'Answer with ok: false, problems (what was wrong), files (the fixed files, whole) and checks (one line each: "not rendered yet").'
      : 'If every frame passes and reads as its scene, answer ok: true and a one-line note per check (what you looked at). Otherwise ok: false, the problems, and the files to replace (whole), plus text_pairs if colours changed.',
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

// The studio's Root: the template's Main plus every style frame as its own composition.
function writeRoot(work) {
  writeFileSync(join(work, 'video', 'src', 'Root.tsx'), `import { Composition } from 'remotion';
import { Main } from './Main';
import { video } from './video';
import { frames } from './scenes/frames';

// One composition per deliverable. Size, rate and length come from video.ts. Each style frame is its own composition.
export const Root = () => (
  <>
    <Composition id="Main" component={Main} width={video.width} height={video.height} fps={video.fps} durationInFrames={Math.round(video.seconds * video.fps)} />
    {Object.entries(frames).map(([id, F]) => (
      <Composition key={id} id={id} component={F} width={video.width} height={video.height} fps={video.fps} durationInFrames={30} />
    ))}
  </>
);
`);
}

async function renderStills({ renders, projectId, sessionId, work, env, frames }) {
  rmSync(join(work, 'out', 'frames'), { recursive: true, force: true });
  mkdirSync(join(work, 'out', 'frames'), { recursive: true });
  const loop = frames.map((f, i) => `npx remotion still src/index.ts ${f.id} ../out/frames/sf${String(i + 1).padStart(2, '0')}.png --frame=0 || exit 1`).join(' && ');
  const job = renders.add({ projectId, sessionId, label: `style frames (${frames.length})`, argv: ['bash', '-c', loop], cwd: join(work, 'video'), env: { ...process.env, ...env } });
  let v = job;
  while (!['done', 'failed'].includes(v.state)) v = await renders.wait(job.id, projectId, 120);
  return v.state === 'done' && v.exit_code === 0 ? { ok: true } : { ok: false, error: v.output_tail || v.error || 'the render failed' };
}

function sheet(work, n, dest) {
  const cols = n <= 3 ? n : 2;
  const r = spawnSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-start_number', '1', '-i', join(work, 'out', 'frames', 'sf%02d.png'), '-vf', `scale=960:-2,tile=${cols}x${Math.ceil(n / cols)}:padding=8:color=0x303030`, '-frames:v', '1', dest], { encoding: 'utf8' });
  return r.status === 0 && existsSync(dest);
}

const measurePairs = (pairs) => (pairs || []).slice(0, 80).map((p) => {
  const ratio = contrast(p.fg, p.bg);
  return { ...p, ratio, ok: ratio !== null && ratio >= (Number(p.px) >= 36 ? 3 : 4.5) };
});

export async function quickFrames({ root, id, view, run, runArgs, config, onActivity, fallback, renders, sessionId }) {
  const c = context(root, id);
  const usage = { input_tokens: 0, output_tokens: 0 };
  const dir = join(paths(root).run, 'quick', id);
  mkdirSync(dir, { recursive: true });
  const call = async (prompt, schema, tools = '', role = 'write') => {
    const r = await runAs(run, role, { ...runArgs, workdir: dir, prompt, config: { ...config, claudeArgs: ['--tools', tools, ...(tools ? ['--allowedTools', tools] : []), '--json-schema', JSON.stringify(schema)] } });
    usage.input_tokens += r.usage?.input_tokens || 0;
    usage.output_tokens += r.usage?.output_tokens || 0;
    return r;
  };
  const give = async (why) => {
    onActivity({ kind: 'info', text: `The studio couldn't finish the style frames by itself (${why}): a session does it.` });
    const f = await fallback();
    return { ...f, usage: { input_tokens: usage.input_tokens + (f.usage?.input_tokens || 0), output_tokens: usage.output_tokens + (f.usage?.output_tokens || 0) } };
  };
  if (!renders) return give('no render queue');

  if (!existsSync(join(c.work, 'video', 'node_modules'))) {
    onActivity({ kind: 'info', text: 'Setting up the video project.' });
    const s = spawnSync(process.execPath, [join(c.work, '.claude', 'skills', 'remotion-motion', 'setup.mjs')], { cwd: c.work, env: { ...process.env, ...(runArgs.env || {}) }, encoding: 'utf8', timeout: 15 * 60_000 });
    if (s.status !== 0) return give(`video setup: ${(s.stderr || s.stdout || '').slice(-300)}`);
  }

  onActivity({ kind: 'info', text: 'Designing the look: one call writes the style frames.' });
  const w = await call(writePrompt(root, id, c), WRITE_SCHEMA);
  if (runArgs.signal?.aborted) return { ok: false, error: 'stopped', usage };
  const s0 = w.ok ? w.structured : null;
  if (!s0?.files?.length || !s0.frames?.length) return give(w.ok ? 'the design call gave no frames' : w.error || 'no answer');
  const state = { frames: s0.frames.slice(0, 5).map((f) => ({ id: String(f.id).replace(/[^A-Za-z0-9]/g, '').slice(0, 20), label: String(f.label).slice(0, 80), beats: (f.beats || []).filter((b) => /^b\d{2,3}$/.test(b)) })), pairs: s0.text_pairs, contrast: [], checks: null };
  writeFiles(c.work, s0.files);
  writeRoot(c.work);
  mkdirSync(join(c.work, 'design'), { recursive: true });
  writeFileSync(join(c.work, 'design', 'LOOK.md'), String(s0.look_md || ''));

  let fixes = 0;
  let reviewed = 0;
  for (;;) {
    onActivity({ kind: 'info', text: `Rendering ${state.frames.length} style frames.` });
    const r = await renderStills({ renders, projectId: id, sessionId, work: c.work, env: runArgs.env || {}, frames: state.frames });
    if (runArgs.signal?.aborted) return { ok: false, error: 'stopped', usage };
    let rv;
    if (!r.ok) {
      if (++fixes > MAX_FIX) return give('the frames kept failing to render');
      onActivity({ kind: 'info', text: 'The frames didn\'t render: fixing the code.' });
      rv = await call(reviewPrompt(c, state, null, r.error), REVIEW_SCHEMA, '', 'codefix');
    } else {
      const name = `sheet-${reviewed + 1}.png`;
      if (!sheet(c.work, state.frames.length, join(dir, name))) return give('could not make the contact sheet');
      copyFileSync(join(dir, name), join(c.work, 'out', 'frames', 'sheet.png'));
      state.contrast = measurePairs(state.pairs);
      if (++reviewed > MAX_REVIEW) break;
      onActivity({ kind: 'info', text: `Checking the style frames (round ${reviewed}).` });
      rv = await call(reviewPrompt(c, state, name, null), REVIEW_SCHEMA, 'Read', 'review');
    }
    if (runArgs.signal?.aborted) return { ok: false, error: 'stopped', usage };
    const s = rv.ok ? rv.structured : null;
    if (!s) return give(rv.error || 'the check call gave no answer');
    if (s.text_pairs?.length) state.pairs = s.text_pairs;
    if (r.ok && s.ok && state.contrast.every((p) => p.ok)) { state.checks = s.checks; break; }
    if (!s.files?.length) {
      const bad = state.contrast.filter((p) => !p.ok).map((p) => `${p.frame} ${p.fg} on ${p.bg} ${p.ratio}:1`);
      return give(`the check found problems but gave no fix: ${[...(s.problems || []), ...bad].join('; ').slice(0, 200)}`);
    }
    writeFiles(c.work, s.files);
    state.problems = s.problems;
  }
  if (!state.checks) return give(`still not passing after ${MAX_REVIEW} checks: ${(state.problems || []).join('; ').slice(0, 300)}`);

  // design/frames.json (the canvas) and the submission
  // `composition`: the export in src/scenes/frames.tsx (the transition board and the animatic draw the frame from it)
  const frames = state.frames.map((f, i) => ({ id: `sf${String(i + 1).padStart(2, '0')}`, file: `out/frames/sf${String(i + 1).padStart(2, '0')}.png`, label: f.label, beats: f.beats, composition: f.id }));
  writeFileSync(join(c.work, 'design', 'frames.json'), JSON.stringify({ frames }, null, 2));
  const version = view.version + 1;
  const pairs = state.contrast.map((p) => `${p.frame} ${p.fg} on ${p.bg} (${p.px}px) ${p.ratio}:1`).join('; ');
  const error_checks = [
    { id: 'spelling-and-facts', result: 'pass', note: `${s0.facts_note} Checked again on the rendered frames: ${state.checks['spelling-and-facts']}` },
    { id: 'text-safe-area', result: 'pass', note: state.checks['text-safe-area'] },
    { id: 'no-collisions', result: 'pass', note: state.checks['no-collisions'] },
    { id: 'text-contrast', result: 'pass', note: `Measured by the studio (WCAG): ${pairs || 'no text'}; every pair passes (4.5:1, 3:1 for 36 px and up).` },
  ].filter((x) => stepChecks(root, id, 'style-frames').some((n) => n.id === x.id));
  try {
    submit(root, id, 'style-frames', {
      note: s0.note,
      questions: [],
      items: [...frames.map((f) => ({ path: f.file, label: f.label })), { path: 'design/LOOK.md', label: 'Look system' }],
      error_checks,
    });
  } catch (e) { return give(`the submission was refused: ${e.message}`); }
  const ck = join(c.work, 'checklist.md');
  if (existsSync(ck)) {
    const t = readFileSync(ck, 'utf8').split('\n');
    const at = t.findIndex((l) => /^##\s+Style frames\b/i.test(l));
    if (at >= 0) { for (let i = at + 1; i < t.length && !/^##\s/.test(t[i]); i++) t[i] = t[i].replace(/^- \[ \]/, '- [x]'); writeFileSync(ck, t.join('\n')); }
  }
  appendFileSync(join(c.work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · Style frames v${version} designed and submitted by the studio (no session): ${frames.length} frames, ${reviewed} check round${reviewed === 1 ? '' : 's'}, ${fixes} render fix${fixes === 1 ? '' : 'es'}.\n`);
  onActivity({ kind: 'info', text: `Style frames ready for your check: ${frames.length} frames.` });
  return { ok: true, usage, summary: s0.note };
}
