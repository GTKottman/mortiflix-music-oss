// Backend: demo. A scripted stand-in for Claude that walks every gate with placeholder work (a sample blueprint, a
// four-bar MIDI sketch, a test tone made with ffmpeg). It goes through the same `mfx` bridge and the same gate rules
// as a real session, so you can try the whole review loop (and develop the UI) without an account or any cost.
// Nothing it makes is meant to be good.
import { spawnSync } from 'node:child_process';
import { mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { callStudio } from '../mfx.mjs';
import { loadProject, projectPipeline } from '../projects.mjs';
import { stepView } from '../gates.mjs';
import { writeMidi } from '../../pipelines/_shared/skills/music/lib/midi.mjs';

export const name = 'demo';

export function available() {
  return { ok: true, detail: 'placeholder work, no Claude needed' };
}

// Four bars in A minor at 90 BPM: a pad, a bass and a beat, so the piano roll has something to show.
function demoMidi(version) {
  const chords = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
  const pad = chords.flatMap((c, bar) => c.map((pitch) => ({ pitch: pitch + (version % 2 ? 12 : 0), start: bar * 4, dur: 3.75, velocity: 0.6 })));
  const bass = chords.flatMap((c, bar) => [0, 1.5, 2.5].map((b) => ({ pitch: c[0] - 12, start: bar * 4 + b, dur: 0.5, velocity: 0.8 })));
  const drums = Array.from({ length: 16 }, (_, beat) => [{ pitch: beat % 2 ? 38 : 36, start: beat, dur: 0.25, velocity: 0.9 }, { pitch: 42, start: beat + 0.5, dur: 0.2, velocity: 0.5 }]).flat();
  return writeMidi({ bpm: 90, meter: '4/4', markers: [{ beat: 0, text: 'Demo loop' }], tracks: [
    { channel: 1, name: 'Bass', notes: bass }, { channel: 2, name: 'Pad', notes: pad }, { channel: 10, name: 'Drums', kind: 'drums', notes: drums }] });
}

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
      questions.push({ id: 'ending', text: 'How should it end?', default: 'On the fourth chord, unresolved' });
    } else if (step.review === 'document') {
      const file = join(dir, 'BLUEPRINT.md');
      writeFileSync(file, `# ${project.title}: ${step.name} v${version}\n\nA hazy loop in A minor at 90 BPM. This is placeholder copy from the demo studio.\n\nFour chords under a slow pad, a bass that answers the kick, and a beat that only arrives in the second half.\n\nIt ends on the fourth chord, unresolved.\n`);
      items.push({ path: `out/${step.key}-v${version}/BLUEPRINT.md`, label: `${step.name} v${version}` });
    } else if (step.review === 'midi' || step.review === 'audio') {
      // The MIDI (a piano roll in the review room), and for an audio step a placeholder tone to listen to.
      writeFileSync(join(dir, 'demo-all.mid'), demoMidi(version));
      items.push({ path: `out/${step.key}-v${version}/demo-all.mid`, label: `${step.name} v${version} · MIDI` });
      if (step.review === 'audio') {
        if (!hasFfmpeg) {
          await mfx('needs-you', { text: 'The demo needs ffmpeg to make its placeholder audio. Install ffmpeg, then resume.' });
          await mfx('handoff', { text: 'Stopped: ffmpeg is missing.' });
          return { ok: true };
        }
        const job = await mfx('render', { label: `${step.name} v${version}`, argv: ['ffmpeg', '-y', '-loglevel', 'error', '-f', 'lavfi', '-i', `sine=frequency=220:duration=${3 + version}`, join(dir, 'preview.wav')], cwd: workdir });
        let r = job;
        while (!['done', 'failed'].includes(r.state)) r = await mfx('render-wait', { id: job.id, seconds: 30 });
        if (r.state === 'failed') return { ok: false, error: `the placeholder render failed: ${r.output_tail}` };
        items.unshift({ path: `out/${step.key}-v${version}/preview.wav`, label: `${step.name} v${version}` });
      }
    }

    const fb = step.last_feedback;
    const pin_changes = fb?.verdict === 'changes'
      ? Array.from({ length: fb.notes }, (_, i) => ({ note: i + 1, change: `Demo: pretended to fix note ${i + 1}.`, status: 'done' }))
      : [];
    const submission = { note: `Demo ${step.name}, version ${version}. Placeholder work: try leaving a note, asking for changes, or approving.`, questions, items, error_checks: checks, pin_changes };
    writeFileSync(join(workdir, 'submission.json'), JSON.stringify(submission, null, 2));
    await mfx('submit', { step: step.key, submission });
    say(`Sent ${step.name} v${version} for review.`);
    did++;
  }
  await mfx('handoff', { text: did ? `Demo session: moved ${did} step(s) forward.` : 'Demo session: nothing to do.' });
  return { ok: true, usage: { input_tokens: 0, output_tokens: 0 } };
}

