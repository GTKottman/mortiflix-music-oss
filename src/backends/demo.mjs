// Backend: demo. A scripted stand-in for Claude that walks every gate with placeholder work (coloured style
// frames, a sample script, a test-pattern video made with ffmpeg). It goes through the same `mfx` bridge and the
// same gate rules as a real session, so you can try the whole review loop (and develop the UI) without an
// account or any cost. Nothing it makes is meant to be good.
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync, readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';
import { callStudio } from '../mfx.mjs';
import { loadProject, projectPipeline } from '../projects.mjs';
import { stepView } from '../gates.mjs';

export const name = 'demo';

export function available() {
  return { ok: true, detail: 'placeholder work, no Claude needed' };
}

const PALETTES = [['#0f172a', '#f97316'], ['#1e1b4b', '#22d3ee'], ['#052e16', '#facc15'], ['#3b0764', '#f472b6']];

// The demo's script: four numbered beats, so the canvas has beats, frames, shots and lines to line up.
const BEATS = [
  ['Open on a single shape that becomes the logo.', 'A dot grows out of the dark.'],
  ['Three ideas, one per beat, each on its own colour.', 'Three cards slide in from the left.'],
  ['The ideas join into one picture.', 'The cards fold together into a circle.'],
  ['Close on the title and a call to action.', 'The title settles; a link fades in.'],
];
const BEAT_SEC = 2;
const FPS = 24;

export async function run({ root, projectId, workdir, env, onActivity, signal, config }) {
  const mfx = (cmd, args) => callStudio(cmd, args, env);
  const pause = (ms = config.demoDelayMs ?? 400) => new Promise((ok) => setTimeout(ok, ms));
  const say = (text) => onActivity({ kind: 'text', text });
  const hasFfmpeg = spawnSync('ffmpeg', ['-version']).status === 0;
  const pipeline = projectPipeline(root, projectId);
  const statusKeys = Object.keys(pipeline.status_lines);
  let did = 0;

  for (let guard = 0; guard < 50; guard++) {
    if (signal?.aborted) return { ok: false, error: 'stopped' };
    // Edit requests from the canvas come first: remake just that piece, then report it.
    for (const e of await mfx('edits', {})) {
      say(`Remaking ${e.label} (${e.id}): ${e.text}`);
      await pause();
      const made = await remake(e);
      if (made !== true) { await mfx('edit-fail', { edit: e.id, note: made }); say(`${e.id} failed: ${made}`); continue; }
      await mfx('canvas-sync', {});
      await mfx('edit-done', { edit: e.id, note: `Demo: remade ${e.label} for "${e.text.slice(0, 80)}".` });
      say(`${e.label} remade (${e.id}).`);
      did++;
    }
    const project = loadProject(root, projectId);
    if (project.state !== 'queued' || project.questions.some((q) => !q.answered_at)) break;
    const step = stepView(project, pipeline).find((s) => ['ready', 'working', 'changes'].includes(s.state));
    if (!step) break;
    const version = step.version + 1;
    say(`Working on ${step.name}${step.state === 'changes' ? ` (v${version}, answering your notes)` : ''}.`);
    if (statusKeys.length) await mfx('status', { key: statusKeys[did % statusKeys.length], mode: 'working' });
    await pause();
    const checks = (await mfx('checks', { step: step.key })).map((c) => ({ id: c.id, result: 'pass' }));

    if (step.review === 'internal') {
      await mfx('step-done', { step: step.key, checks });
      say(`${step.name} done (internal).`);
      did++;
      continue;
    }

    const items = [];
    const dir = join(workdir, 'out', `${step.key}-v${version}`);
    mkdirSync(dir, { recursive: true });
    const questions = [];
    if (step.review === 'questions') {
      questions.push({ id: 'pace', text: 'Should it feel calm or energetic?', choices: ['Calm', 'Energetic'], default: 'Energetic' });
      questions.push({ id: 'ending', text: 'What should the last card say?', default: 'Your title and a link' });
    } else if (step.review === 'document') {
      const file = join(dir, 'script.md');
      const beats = BEATS.map(([words, visual], i) => ({ id: `b0${i + 1}`, words: version > 1 && i === 1 ? `${words} (v${version})` : words, visual, target_sec: BEAT_SEC }));
      writeFileSync(file, `# ${project.title}: ${step.name} v${version}\n\n${beats.map((b) => `B${b.id.slice(1)} · ${b.words}\n*Visual: ${b.visual}*`).join('\n\n')}\n`);
      mkdirSync(join(workdir, 'script'), { recursive: true });
      writeFileSync(join(workdir, 'script', 'beats.json'), JSON.stringify({ version, beats }, null, 2));
      items.push({ path: `out/${step.key}-v${version}/script.md`, label: `${step.name} v${version}` });
    } else if (step.review === 'frames') {
      const frames = [];
      for (let i = 0; i < BEATS.length; i++) {
        const [bg, fg] = PALETTES[(i + version) % PALETTES.length];
        const svg = `<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080" viewBox="0 0 1920 1080"><rect width="1920" height="1080" fill="${bg}"/><circle cx="${460 + i * 330}" cy="540" r="${160 + i * 30}" fill="${fg}" opacity="0.9"/><text x="120" y="980" font-family="sans-serif" font-size="64" fill="#fff">SF0${i + 1} · ${escapeXml(project.title).slice(0, 40)} · v${version}</text></svg>`;
        writeFileSync(join(dir, `sf0${i + 1}.svg`), svg);
        const label = `SF0${i + 1} · ${['Open', 'Ideas', 'Together', 'Close'][i]}`;
        items.push({ path: `out/${step.key}-v${version}/sf0${i + 1}.svg`, label });
        frames.push({ id: `sf0${i + 1}`, file: `out/${step.key}-v${version}/sf0${i + 1}.svg`, label, beats: [`b0${i + 1}`] });
      }
      if (step.key === 'transitions') {
        // The board's cuts: one between each pair of shots, so the canvas has transition nodes to show.
        const kinds = ['Crossfade', 'Match cut on the circle', 'Push left'];
        mkdirSync(join(workdir, 'transitions'), { recursive: true });
        writeFileSync(join(workdir, 'transitions', 'board.json'), JSON.stringify({ cuts: BEATS.slice(1).map((_, i) => ({ id: `t0${i + 1}`, from_scene: `s0${i + 1}`, to_scene: `s0${i + 2}`, transition: kinds[i % kinds.length] })) }, null, 2));
      } else {
        mkdirSync(join(workdir, 'design'), { recursive: true });
        writeFileSync(join(workdir, 'design', 'frames.json'), JSON.stringify({ frames }, null, 2));
      }
    } else if (step.review === 'video' || step.review === 'audio') {
      if (!hasFfmpeg) {
        await mfx('needs-you', { text: 'The demo needs ffmpeg to make its placeholder video. Install ffmpeg, then resume.' });
        await mfx('handoff', { text: 'Stopped: ffmpeg is missing.' });
        return { ok: true };
      }
      const file = step.review === 'video' ? 'preview.mp4' : 'preview.wav';
      const sec = BEATS.length * BEAT_SEC;
      const args = step.review === 'video'
        ? ['ffmpeg', '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', `testsrc2=size=1280x720:rate=${FPS}:duration=${sec}`, '-f', 'lavfi', '-i', `sine=frequency=${330 + 110 * version}:duration=${sec}`, '-shortest', '-pix_fmt', 'yuv420p', '-c:v', 'libx264', '-bf', '0', '-c:a', 'aac', join(dir, file)]
        : ['ffmpeg', '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', `sine=frequency=440:duration=${sec}`, join(dir, file)];
      const job = await mfx('render', { label: `${step.name} v${version}`, argv: args, cwd: workdir });
      let r = job;
      while (!['done', 'failed'].includes(r.state)) r = await mfx('render-wait', { id: job.id, seconds: 30 });
      if (r.state === 'failed') return { ok: false, error: `the placeholder render failed: ${r.output_tail}` };
      items.push({ path: `out/${step.key}-v${version}/${file}`, label: `${step.name} v${version}` });
      // The video's scenes, one per beat: the canvas cuts the shots out of it by these frames.
      if (step.review === 'video') {
        writeFileSync(join(workdir, 'out', 'scenes.json'), JSON.stringify({ made_from: `out/${step.key}-v${version}/${file}`, fps: FPS,
          scenes: BEATS.map(([words], i) => ({ key: `s0${i + 1}`, name: words.replace(/\.$/, ''), from_frame: i * BEAT_SEC * FPS, frames: BEAT_SEC * FPS, beat: `b0${i + 1}` })) }, null, 2));
      }
    }

    const fb = step.last_feedback;
    const pin_changes = fb?.verdict === 'changes'
      ? Array.from({ length: fb.notes }, (_, i) => ({ note: i + 1, change: `Demo: pretended to fix note ${i + 1}.`, status: 'done' }))
      : [];
    const submission = { note: `Demo ${step.name}, version ${version}. Placeholder work: try pinning a note, asking for changes, or approving.`, questions, items, error_checks: checks, pin_changes };
    writeFileSync(join(workdir, 'submission.json'), JSON.stringify(submission, null, 2));
    await mfx('submit', { step: step.key, submission });
    say(`Sent ${step.name} v${version} for review.`);
    did++;
  }
  await mfx('handoff', { text: did ? `Demo session: moved ${did} step(s) forward.` : 'Demo session: nothing to do.' });
  return { ok: true, usage: { input_tokens: 0, output_tokens: 0 } };

  // One piece, remade alone: a shot gets its own clip, a frame a new picture, a beat new words. A note with the word
  // "fail" fails on purpose, so the failure path can be tried too.
  async function remake(e) {
    if (/\bfail\b/i.test(e.text)) return 'Demo: failed on purpose (the note said "fail").';
    const dir = join(workdir, 'edits', e.id);
    mkdirSync(dir, { recursive: true });
    const readJ = (rel) => JSON.parse(readFileSync(join(workdir, rel), 'utf8'));
    if (e.kind === 'shot') {
      if (!hasFfmpeg || !existsSync(join(workdir, 'out', 'scenes.json'))) return 'Demo: needs ffmpeg and the animatic to remake a shot.';
      const sc = readJ('out/scenes.json');
      const scene = sc.scenes.find((x) => x.key === e.node);
      if (!scene) return `Demo: ${e.node} isn't in out/scenes.json.`;
      const file = `edits/${e.id}/${e.node}.mp4`;
      const job = await mfx('render', { label: `${e.label} (${e.id})`, argv: ['ffmpeg', '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', `smptebars=size=1280x720:rate=${FPS}:duration=${scene.frames / FPS}`, '-pix_fmt', 'yuv420p', '-c:v', 'libx264', '-bf', '0', join(workdir, file)], cwd: workdir });
      let r = job;
      while (!['done', 'failed'].includes(r.state)) r = await mfx('render-wait', { id: job.id, seconds: 30 });
      if (r.state === 'failed') return `Demo: the render failed: ${r.output_tail}`;
      scene.file = file;
      writeFileSync(join(workdir, 'out', 'scenes.json'), JSON.stringify(sc, null, 2));
      return true;
    }
    if (e.kind === 'frame') {
      const fr = readJ('design/frames.json');
      const f = fr.frames.find((x) => x.id === e.node);
      if (!f) return `Demo: ${e.node} isn't in design/frames.json.`;
      f.file = `edits/${e.id}/${e.node}.svg`;
      writeFileSync(join(workdir, f.file), `<svg xmlns="http://www.w3.org/2000/svg" width="1920" height="1080"><rect width="1920" height="1080" fill="#111"/><rect x="560" y="240" width="800" height="600" fill="#f97316"/><text x="120" y="980" font-family="sans-serif" font-size="64" fill="#fff">${escapeXml(f.label)} · remade (${e.id})</text></svg>`);
      writeFileSync(join(workdir, 'design', 'frames.json'), JSON.stringify(fr, null, 2));
      return true;
    }
    if (e.kind === 'beat') {
      const b = readJ('script/beats.json');
      const beat = b.beats.find((x) => x.id === e.node);
      if (!beat) return `Demo: ${e.node} isn't in script/beats.json.`;
      beat.words = `${beat.words.replace(/ \(changed.*\)$/, '')} (changed: ${e.text.slice(0, 40)})`;
      writeFileSync(join(workdir, 'script', 'beats.json'), JSON.stringify(b, null, 2));
      return true;
    }
    if (e.kind === 'transition') {
      const bd = readJ('transitions/board.json');
      const cut = bd.cuts.find((x) => x.id === e.node);
      if (!cut) return `Demo: ${e.node} isn't in transitions/board.json.`;
      cut.transition = (e.text.match(/"([^"]+)"/)?.[1] || e.text).slice(0, 80);
      writeFileSync(join(workdir, 'transitions', 'board.json'), JSON.stringify(bd, null, 2));
      return true;
    }
    return `Demo: can't remake a ${e.kind} yet.`;
  }
}

const escapeXml = (s) => String(s).replace(/[<>&"']/g, (c) => ({ '<': '&lt;', '>': '&gt;', '&': '&amp;', '"': '&quot;', "'": '&apos;' }[c]));
