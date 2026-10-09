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
function demoPiece(version) {
  const chords = [[57, 60, 64], [53, 57, 60], [48, 52, 55], [55, 59, 62]];
  const pad = chords.flatMap((c, bar) => c.map((pitch) => ({ pitch: pitch + (version % 2 ? 12 : 0), start: bar * 4, dur: 3.75, velocity: 0.6 })));
  const bass = chords.flatMap((c, bar) => [0, 1.5, 2.5].map((b) => ({ pitch: c[0] - 12, start: bar * 4 + b, dur: 0.5, velocity: 0.8 })));
  const drums = Array.from({ length: 16 }, (_, beat) => [{ pitch: beat % 2 ? 38 : 36, start: beat, dur: 0.25, velocity: 0.9 }, { pitch: 42, start: beat + 0.5, dur: 0.2, velocity: 0.5 }]).flat();
  return { bpm: 90, meter: '4/4', tracks: [
    { channel: 1, name: 'Bass', notes: bass }, { channel: 2, name: 'Pad', notes: pad }, { channel: 10, name: 'Drums', kind: 'drums', notes: drums }] };
}

const demoMidi = (piece) => writeMidi({ ...piece, markers: [{ beat: 0, text: 'Demo loop' }] });

// The same piece as a Strudel score, one labelled block per part and one value per bar (16 steps a bar), so a quick
// edit in the review room has a real score to splice into (the music engine plays it when Strudel is set up).
export function demoScore(piece, bars = 4) {
  const lines = [`setcpm(${piece.bpm} / 4)`, `const BARS = ${bars}`, 'const cue = (p) => p.filterWhen((t) => t < BARS)', ''];
  for (const t of piece.tracks) {
    // The notes and their velocities in the same shape, so every note keeps its own dynamic.
    const perBar = []; const velBar = [];
    for (let bar = 0; bar < bars; bar += 1) {
      const at = new Map();
      for (const n of t.notes.filter((x) => x.start >= bar * 4 && x.start < bar * 4 + 4)) {
        const step = Math.round((n.start - bar * 4) * 4);
        if (!at.has(step)) at.set(step, { pitches: [], velocity: n.velocity, dur: Math.max(1, Math.round(n.dur * 4)) });
        at.get(step).pitches.push(n.pitch);
      }
      const steps = [...at.keys()].sort((x, y) => x - y);
      const tokens = []; const vels = [];
      const rest = (n) => { const r = n > 1 ? `~@${n}` : '~'; tokens.push(r); vels.push(r); };
      let pos = 0;
      steps.forEach((st, i) => {
        if (st > pos) rest(st - pos);
        const e = at.get(st); const nextAt = i + 1 < steps.length ? steps[i + 1] : 16;
        const len = Math.min(e.dur, nextAt - st);
        const p = e.pitches.length > 1 ? `[${e.pitches.join(',')}]` : String(e.pitches[0]);
        tokens.push(len > 1 ? `${p}@${len}` : p); vels.push(len > 1 ? `${e.velocity}@${len}` : String(e.velocity));
        if (len < nextAt - st) rest(nextAt - st - len);
        pos = nextAt;
      });
      if (!steps.length) rest(16);
      perBar.push(`[${tokens.join(' ')}]`); velBar.push(`[${vels.join(' ')}]`);
    }
    lines.push(`${t.name.toLowerCase().replace(/[^a-z0-9]+/g, '_')}: cue(note("<${perBar.join(' ')}>").velocity("<${velBar.join(' ')}>")).midichan(${t.channel})`);
  }
  return `${lines.join('\n')}\n`;
}

// Where the music engine leaves a passed score (music/notes.json, the blueprint, out/music/midi/), so the project
// page's MIDI panel and Download MIDI work in the demo too.
function demoMusic(workdir, title, piece) {
  mkdirSync(join(workdir, 'music'), { recursive: true });
  const midi = join(workdir, 'out', 'music', 'midi');
  mkdirSync(midi, { recursive: true });
  writeFileSync(join(workdir, 'music', 'blueprint.json'), JSON.stringify({ title, bpm: piece.bpm, meter: piece.meter, scale: 'A minor', bars: 4,
    sections: [{ name: 'Demo loop', start_bar: 1, end_bar: 4, function: 'a placeholder loop from the demo backend', intensity: 5 }],
    roles: [{ name: 'Bass', channel: 1, harmony: 'bass', job: 'the root under each chord' }, { name: 'Pad', channel: 2, harmony: 'chords', job: 'holds the chords' },
      { name: 'Drums', channel: 10, harmony: 'drums', job: 'the beat' }],
    chart: [{ bars: [1, 4], chords: ['Am', 'F', 'C', 'G'] }] }, null, 2));
  writeFileSync(join(workdir, 'music', 'notes.json'), JSON.stringify({ ...piece, beats_per_bar: 4, bars: 4 }, null, 2));
  writeFileSync(join(workdir, 'music', 'score.strudel.js'), demoScore(piece));
  writeFileSync(join(midi, 'demo-all.mid'), demoMidi(piece));
  for (const t of piece.tracks) writeFileSync(join(midi, `${String(t.channel).padStart(2, '0')}-${t.name.toLowerCase()}.mid`), writeMidi({ ...piece, tracks: [t] }));
  writeFileSync(join(midi, 'CUE-SHEET.md'), `# ${title}: cue sheet\n\nPlaceholder from the demo backend: 90 BPM, 4/4, A minor, 4 bars. Drums are on channel 10 (General MIDI).\n`);
}

// music/draft.json as strudel.mjs draft writes it: the parts written so far, the layers in order, the last check.
function writeDraft(workdir, piece, tracks, layers, check) {
  mkdirSync(join(workdir, 'music'), { recursive: true });
  const at = new Date().toISOString();
  writeFileSync(join(workdir, 'music', 'draft.json'), JSON.stringify({ ...piece, tracks, beats_per_bar: 4, bars: 4,
    sections: [{ name: 'Demo loop', start_bar: 1, end_bar: 4 }], at, layer: layers.at(-1)?.name || null, layers, check: check ? { ...check, at } : null }, null, 2));
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
      // Something readable for every kind of work, so the project page has a "what it made" to show.
      const work = step.work || pipeline.steps.find((x) => x.key === step.key)?.work || [];
      if (work.includes('research')) {
        mkdirSync(join(workdir, 'music'), { recursive: true });
        writeFileSync(join(workdir, 'music', 'GENRE.md'), `# ${project.intake.answers.genre || 'The genre'} (demo)\n\nPlaceholder research from the demo backend: no sources were read.\n\n## The beat\nA slow, steady backbeat.\n\n## The usual band\n- **Bass**: roots under each chord\n- **Pad**: holds the chords\n- **Drums**: the beat\n`);
      }
      if (work.includes('music') || work.includes('midi')) {
        // The score a layer at a time, as a real session writes it, so the project page shows it growing.
        const piece = demoPiece(1);
        const order = [...piece.tracks].sort((a, b) => (b.channel === 10) - (a.channel === 10) || a.channel - b.channel);
        const layers = [];
        for (let i = 0; i < order.length; i++) {
          const name = `the ${order[i].name.toLowerCase()}`;
          layers.push({ name, at: new Date().toISOString(), parts: [order[i].channel] });
          writeDraft(workdir, piece, order.slice(0, i + 1), layers, null);
          await mfx('draft', { layer: name });
          await pause();
        }
        demoMusic(workdir, project.title, piece);
        writeDraft(workdir, piece, order, layers, { ok: true, problems: 0 });
      }
      const notes = { research: 'Demo: a placeholder genre page (no research was done).', music: 'Demo: the same 4-bar placeholder loop as always (bass, pad, drums in A minor).',
        instruments: 'Demo: no instruments were chosen.', audio: 'Demo: no audio was made.', plan: 'Demo: a placeholder plan.' };
      await mfx('step-done', { step: step.key, checks, note: work.map((x) => notes[x]).filter(Boolean).join(' ') || `Demo: ${step.name}, placeholder work.` });
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
      const piece = demoPiece(version);
      writeFileSync(join(dir, 'demo-all.mid'), demoMidi(piece));
      demoMusic(workdir, project.title, piece);
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

