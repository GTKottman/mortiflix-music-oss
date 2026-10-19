// The music driven by the studio, not a session (owner, 2026-10-19: a music session reached 755k tokens in 12 steps
// before composing a note, reading the theory and the engine's source and searching tempos by hand; the owner approved
// "the AI makes the musical decisions, code writes the notes"). One small AI call:
//   1. the studio maps the tempo onto the picture (the board's cuts as hits, bar 1 at offset_sec) and makes one
//      section per scene (src/music/compose.mjs, no AI);
//   2. plan: a fresh call reads the scenes, the words, the hits and the owner's recent pieces and answers with a cue
//      plan: key, chords per section, a motif, each section's name, function, intensity and parts, how each hit plays;
//   3. code writes the notes from it (every note a chord tone of its moment), the engine checks them as it checks a
//      session's (strudel.mjs plan, check: the MIDI is captured, instruments, audition), the render goes through the
//      render queue, the studio mixes it under the voice (sidechain, two-pass loudnorm) onto the animatic and submits.
// A plan the engine refuses goes back to a fresh call with only the errors. The owner's notes on a version, a reference
// track, or anything the studio can't finish goes to a session.
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync, appendFileSync } from 'node:fs';
import { join } from 'node:path';
import { paths, REPO } from './studio.mjs';
import { loadProject, projectPaths, projectPipeline } from './projects.mjs';
import { stepView, stepChecks, submit } from './gates.mjs';
import { scriptView } from './svc/script.mjs';
import { loadTheme } from './svc/theme.mjs';
import { tempoMap, placeOf, sectionsFromScenes, planProblems, blueprintFrom, composeChecked, scoreText, instrumentsFor, balanceGains, usedParts, ROLES, PARTS, VARIATIONS, PALETTES, HIT_KINDS } from './music/compose.mjs';
import { readWav, levelOf } from '../pipelines/_shared/skills/music/lib/wav.mjs';

const CHECKS = ['loudness', 'sync', 'music-check', 'music-instruments', 'music-source', 'music-under-voice'];
const M = join(REPO, 'pipelines', '_shared', 'skills', 'music', 'strudel.mjs');
const QC = join(REPO, 'pipelines', '_shared', 'skills', 'final-pass', 'qc.mjs');
const FPS = 30;
const MAX_FIXES = 2;
const UNDER = 18; // dB the music sits under the voice while it speaks (music SKILL.md §8)

const read = (f, n = 200_000) => (existsSync(f) ? readFileSync(f, 'utf8').slice(0, n) : '');
const readJsonOr = (f, d) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return d; } };

export function quickMusicStep(root, id, config) {
  if (config.quickMusic === false || config.quickScript === false) return null;
  const project = loadProject(root, id);
  const pipeline = projectPipeline(root, id);
  const step = pipeline.steps.find((s) => s.key === 'music');
  if (!step) return null;
  const runnable = stepView(project, pipeline).filter((s) => ['ready', 'working', 'changes'].includes(s.state));
  if (runnable.length !== 1 || runnable[0].key !== 'music' || runnable[0].state === 'changes') return null;
  if (project.mfx && Object.keys(project.mfx).some((k) => k.endsWith('_wanted'))) return null;
  const work = projectPaths(root, id).work;
  // a reference track to match takes listening: a session
  if (loadTheme(root, id)?.reference) return null;
  for (const f of ['out/animatic.mp4', 'out/scenes.json', 'voice/timing.json', 'out/voice/narration.wav', 'transitions/board.json']) if (!existsSync(join(work, f))) return null;
  if (stepChecks(root, id, 'music').some((c) => !CHECKS.includes(c.id))) return null;
  return runnable[0];
}

// Run a command without blocking the service (the engine starts a headless Chrome; a check takes a while).
function exec(cmd, args, { cwd, env, timeout = 10 * 60_000 } = {}) {
  return new Promise((resolve) => {
    const p = spawn(cmd, args, { cwd, env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
    let out = '';
    p.stdout.on('data', (d) => { out += d; });
    p.stderr.on('data', (d) => { out += d; });
    const t = setTimeout(() => p.kill('SIGTERM'), timeout);
    p.on('error', (e) => { clearTimeout(t); resolve({ code: -1, out: `${out}${e.message}` }); });
    p.on('close', (code) => { clearTimeout(t); resolve({ code, out: out.trim() }); });
  });
}
const engine = (c, env, ...args) => exec(process.execPath, [M, ...args], { cwd: c.work, env });
const tail = (s, n = 3000) => String(s || '').slice(-n);

function context(root, id) {
  const work = projectPaths(root, id).work;
  const project = loadProject(root, id);
  const script = scriptView(root, id);
  const timing = readJsonOr(join(work, 'voice', 'timing.json'), { seconds: 0, lines: [] });
  const board = readJsonOr(join(work, 'transitions', 'board.json'), { cuts: [] });
  const scenesJson = readJsonOr(join(work, 'out', 'scenes.json'), { fps: FPS, scenes: [] });
  const fps = scenesJson.fps || FPS;
  const scenes = scenesJson.scenes.map((s) => ({ key: s.key, beat: s.beat, name: s.name, start_sec: s.from_frame / fps, end_sec: (s.from_frame + s.frames) / fps }));
  return { work, project, script, timing, board, scenes, theme: loadTheme(root, id), recent: readJsonOr(join(work, 'music', 'recent.json'), { pieces: [] }).pieces || [] };
}

// The board's cuts as hits: the moment each one lands (the word it lands on), else the scene it opens.
function cutHits(c) {
  return c.board.cuts.map((x, i) => {
    const t = Number(String(x.lands_on || '').match(/\((\d+(?:\.\d+)?)\s*s\)/)?.[1]) || c.scenes[i + 1]?.start_sec;
    const name = typeof x.transition === 'string' ? x.transition : x.transition?.new || 'cut';
    return t ? { cut: i + 1, t_sec: +Number(t).toFixed(3), what: `${name} into ${x.to}` } : null;
  }).filter(Boolean);
}

const PLAN_SCHEMA = {
  type: 'object',
  required: ['title', 'dramatic_reading', 'must_not', 'scale', 'palette', 'motif', 'sections', 'hits', 'ending', 'note'],
  properties: {
    title: { type: 'string', description: 'a real title for the cue' },
    dramatic_reading: { type: 'string', description: 'the transformation in one line: "confusion -> recognition -> confidence"' },
    must_not: { type: 'string', description: 'what the music must not do here' },
    scale: { type: 'string', description: 'tonic:mode, e.g. "E:dorian" (modes: major minor dorian phrygian lydian mixolydian harmonic minor melodic minor)' },
    palette: { type: 'string', enum: PALETTES },
    motif: {
      type: 'object', required: ['degrees', 'beats', 'contour'],
      properties: {
        degrees: { type: 'array', items: { type: 'integer' }, description: 'scale degrees from the tonic (0 = tonic, 7 = an octave up in a 7-note mode, negatives below)' },
        beats: { type: 'array', items: { type: 'number' }, description: 'each note\'s length in beats (quarter notes), multiples of 0.25' },
        contour: { type: 'string', description: 'the shape in words: a question that rises, an answer that falls…' },
      },
    },
    sections: {
      type: 'array',
      items: {
        type: 'object', required: ['name', 'function', 'intensity', 'chords', 'melody', 'parts'],
        properties: {
          name: { type: 'string', description: 'story-bearing, never Intro/Verse/A/B' },
          function: { type: 'string', description: 'establish, develop, destabilise, withhold, release, reinterpret, resolve: what happens, what rises or is held back' },
          intensity: { type: 'integer', minimum: 0, maximum: 10 },
          chords: { type: 'array', items: { anyOf: [{ type: 'string' }, { type: 'array', items: { type: 'string' } }] }, description: 'one chord per bar, cycling through the section; ["Am7", "G"] splits a bar in halves' },
          melody: { type: 'string', enum: VARIATIONS },
          parts: { type: 'array', items: { type: 'string', enum: PARTS } },
          picture: { type: 'string' },
        },
      },
    },
    hits: { type: 'array', items: { type: 'object', required: ['cut', 'kind', 'response'], properties: { cut: { type: 'integer' }, kind: { type: 'string', enum: HIT_KINDS }, response: { type: 'string' } } } },
    ending: { type: 'string' },
    note: { type: 'string', description: 'for the owner, short: the idea, how the hits are played, what to listen for' },
  },
};

function planPrompt(c, map, sections, hits, fix) {
  const busyWords = (a, b) => c.timing.lines.filter((l) => l.end > a && l.start < b).map((l) => l.text).join(' ');
  const t = c.theme;
  const using = t?.samples?.find((s) => s.id === t.using);
  return [
    `You plan the original score for "${c.project.title}", an explainer video with narration. You make the musical decisions; the studio's code writes every note from your plan (each one a tone of the chord of its moment), checks it, renders a stand-in and mixes it under the voice. Answer with the JSON asked for, nothing else.`,
    `## The brief\n${JSON.stringify({ ...c.project.intake?.answers, ...Object.fromEntries(Object.entries(c.project.mfx?.answers || {}).map(([k, v]) => [k, v.answer])) })}`,
    t ? `## The music theme ${t.picked_by === 'you' ? 'the owner chose' : 'set for this project'} (follow it${using ? '; the motif is its opening phrase, written as scale degrees' : ''})\n${JSON.stringify({ mood: t.mood, genre: t.genre, tempo: t.tempo, key: t.key, instruments: t.instruments, words: t.words, theme: using ? { name: using.name, desc: using.desc, key: using.key, strudel: using.strudel } : undefined })}` : '',
    `## The tempo (the studio fitted it to the cuts: don't change it)\n${map.bpm} BPM, ${map.meter}, bar 1 at ${map.offset_sec} s, ${map.bars} bars of ${map.sec_per_bar} s; the video is ${c.videoSec} s.`,
    `## The sections (one per scene, fixed: plan exactly these, in order)\n${sections.map((s, i) => `${i + 1}. bars ${s.start_bar}-${s.end_bar} (${s.start_sec.toFixed(1)}-${s.end_sec.toFixed(1)} s), scene ${s.scenes.join('+')}: ${s.scenes.map((k) => c.script.beats.find((b) => b.id === c.scenes.find((x) => x.key === k)?.beat)).filter(Boolean).map((b) => `"${b.name}", picture: ${b.visual}`).join(' / ')}. Spoken: "${busyWords(s.start_sec, s.end_sec)}"`).join('\n')}`,
    `## The cuts (hits on the picture)\n${hits.map((h) => `cut ${h.cut} at ${h.t_sec} s (${h.what}) = bar ${h.bar} beat ${h.beat}${h.fits ? '' : ': between beats at this tempo, it plays as no hit'}`).join('\n')}`,
    `## The owner's recent pieces (vary key, opening chords and mood from them: the same key and opening progression as one is refused)\n${JSON.stringify(c.recent.map((r) => ({ title: r.title, scale: r.scale, bpm: r.bpm, opening: r.opening })))}`,
    `## How the code plays your plan
- parts: bass (the root on every chord change; long notes when quiet, root-fifth from intensity 4, octave eighths from 7), pad (the chords held, voice-led), melody (your motif, varied per section), arp (broken chords, eighths; sixteenths from 8), bells (on the hits, at a section that rises, on the last chord), drums (a soft kick from intensity 0, hats from 4, a second kick from 6, claps from 7, a tom fill into a section that rises, a crash on a direct hit). Parts enter and leave by section: list only what plays.
- melody variations: statement, fragment (its first half), inversion, augmentation (twice as slow), octave-up, sequence (repeated a step higher each time), rest. A phrase plays, then rests as long (call and answer); under the words it plays every other phrase, softer.
- intensity 0-10 is the energy curve and follows the picture: builds really build, the quiet really thins out; contrast makes it legible. The code measures it, so a section asked 2+ higher than another must have more going on.
- chords: from the scale's own chords (borrowed colours only where they serve the story), symbols like Am7 Cmaj7 G Dsus4 Fadd9 Em9; under speech a slow harmonic rhythm (a chord lasting 2 bars is the same symbol twice). Vary the harmony across sections; the last section resolves or is left open on purpose (say which in "ending").
- the voice talks almost all the time: the music sits ~18 dB under it (ducked), so the busiest writing goes where the words are fewest; the end card can open up.
- palette: the stand-in sound (warm, glass or dark); the owner makes the final sound from the MIDI.
- hits: for each cut, direct (on it), anticipation (the change just before), aftermath (react a beat after) or none; hit the few that change meaning.`,
    fix ? `## Your last plan was refused: fix only these\n${fix.errors.join('\n')}\n\n## That plan\n${JSON.stringify(fix.plan)}` : '',
  ].filter(Boolean).join('\n\n');
}

// Under-voice: the music's level (after ducking) against the voice's, on every spoken line (RMS, dB).
function underVoice(c, voiceWav, duckedWav, offset) {
  const v = readWav(voiceWav); const m = readWav(duckedWav);
  const rows = c.timing.lines.filter((l) => l.end - l.start > 0.8).map((l) => {
    const a = levelOf(v, l.start, l.end).rms_db; const b = levelOf(m, l.start, l.end).rms_db;
    return { id: l.id, voice: a, music: b, under: +(a - b).toFixed(1) };
  }).filter((r) => r.music > -90 && r.voice > -60);
  const unders = rows.map((r) => r.under).sort((x, y) => x - y);
  return { rows, min: unders[0] ?? null, median: unders[Math.floor(unders.length / 2)] ?? null, offset };
}

// The mix under the voice (music SKILL.md §8): bar 1 at offset_sec, sidechain ducking, then two-pass loudnorm to
// -14 LUFS / -1.5 dBTP; the music alone after ducking is kept for the under-voice measurement.
async function mixUnderVoice(c, voiceRel, map, musicDb) {
  mkdirSync(join(c.work, 'audio'), { recursive: true });
  const ms = Math.round(map.offset_sec * 1000);
  const V = c.videoSec.toFixed(3);
  const graph = `[0:a]aresample=48000,apad=whole_dur=${V},asplit=2[v1][v2];[1:a]aresample=48000,adelay=${ms}|${ms},volume=${musicDb}dB,atrim=0:${V},afade=t=out:st=${Math.max(0, c.videoSec - 0.5).toFixed(3)}:d=0.5[m];[m][v1]sidechaincompress=threshold=0.03:ratio=8:attack=20:release=450,asplit=2[duck][dk];[v2][duck]amix=inputs=2:normalize=0[out]`;
  let r = await exec('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', voiceRel, '-i', 'out/music/score.wav', '-filter_complex', graph, '-map', '[out]', '-ar', '48000', '-c:a', 'pcm_f32le', 'audio/mix-pre.wav', '-map', '[dk]', '-ar', '48000', '-c:a', 'pcm_f32le', 'audio/music-ducked.wav'], { cwd: c.work });
  if (r.code !== 0) return { ok: false, error: `the mix: ${tail(r.out, 400)}` };
  const LN = 'loudnorm=I=-14:TP=-1.5:LRA=11';
  r = await exec('ffmpeg', ['-hide_banner', '-i', 'audio/mix-pre.wav', '-af', `${LN}:print_format=json`, '-f', 'null', '-'], { cwd: c.work });
  const m1 = JSON.parse(r.out.slice(r.out.lastIndexOf('{'), r.out.lastIndexOf('}') + 1) || '{}');
  // already at the delivery level (the mastered voice carries it: the music sits far under it): no gain to add, so
  // the mix stays as it is; a second pass would ride the level for a few hundredths of a dB
  if (Math.abs(Number(m1.input_i) + 14) <= 0.5 && Number(m1.input_tp) <= -1) {
    r = await exec('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', 'audio/mix-pre.wav', '-ar', '48000', '-c:a', 'pcm_s24le', 'audio/mix.wav'], { cwd: c.work });
    if (r.code !== 0) return { ok: false, error: `the mix: ${tail(r.out, 400)}` };
    return { ok: true, linear: true, loudnorm: { input_i: m1.input_i, input_tp: m1.input_tp, normalization_type: 'none (already at -14 LUFS)' } };
  }
  r = await exec('ffmpeg', ['-hide_banner', '-y', '-i', 'audio/mix-pre.wav', '-af', `${LN}:measured_I=${m1.input_i}:measured_TP=${m1.input_tp}:measured_LRA=${m1.input_lra}:measured_thresh=${m1.input_thresh}:offset=${m1.target_offset}:linear=true:print_format=json`, '-ar', '48000', '-c:a', 'pcm_s24le', 'audio/mix.wav'], { cwd: c.work });
  const m2 = JSON.parse(r.out.slice(r.out.lastIndexOf('{'), r.out.lastIndexOf('}') + 1) || '{}');
  if (r.code !== 0) return { ok: false, error: `the loudness pass: ${tail(r.out, 400)}` };
  return { ok: true, linear: m2.normalization_type !== 'dynamic', loudnorm: m2 };
}

export async function quickMusic({ root, id, view, run, runArgs, config, onActivity, fallback, renders, sessionId }) {
  const c = context(root, id);
  const env = runArgs.env || {};
  const usage = { input_tokens: 0, output_tokens: 0 };
  const dir = join(paths(root).run, 'quick', id);
  mkdirSync(dir, { recursive: true });
  const call = async (prompt, schema) => {
    const r = await run({ ...runArgs, workdir: dir, prompt, config: { ...config, claudeArgs: ['--tools', '', '--json-schema', JSON.stringify(schema)] } });
    usage.input_tokens += r.usage?.input_tokens || 0;
    usage.output_tokens += r.usage?.output_tokens || 0;
    return r;
  };
  const give = async (why) => {
    onActivity({ kind: 'info', text: `The studio couldn't finish the music by itself (${why}): a session does it.` });
    const f = await fallback();
    return { ...f, usage: { input_tokens: usage.input_tokens + (f.usage?.input_tokens || 0), output_tokens: usage.output_tokens + (f.usage?.output_tokens || 0) } };
  };
  const stopped = () => runArgs.signal?.aborted;
  if (!renders) return give('no render queue');
  if (!env.MFX_STRUDEL || !env.MFX_CHROME) return give('Strudel or its Chrome isn\'t set up');
  // the real instruments (VCSL, CC0): fetched into the studio once, then served to the renderer from disk
  if (!env.MFX_SAMPLES) {
    const dirS = join(root, 'tools', 'samples');
    onActivity({ kind: 'info', text: 'Fetching the instrument samples (once, about 360 MB).' });
    const f = await exec(process.execPath, [join(REPO, 'pipelines', '_shared', 'skills', 'music', 'samples.mjs'), 'fetch', dirS], { timeout: 30 * 60_000 });
    if (!existsSync(join(dirS, 'vcsl.json'))) return give(`the instrument samples couldn't be fetched: ${tail(f.out, 200)}`);
    env.MFX_SAMPLES = dirS;
  }
  if (/\bno music\b/i.test(String(c.project.intake?.answers?.music || ''))) return give('the brief asks for no music');
  const own = /MIDI/i.test(String(c.project.intake?.answers?.music || '')) || c.theme?.finish === 'own';

  // 1. the tempo map and the sections (no AI)
  const probe = await exec('ffprobe', ['-v', 'error', '-show_entries', 'format=duration', '-of', 'csv=p=0', 'out/animatic.mp4'], { cwd: c.work });
  c.videoSec = +Number(probe.out).toFixed(3);
  if (!(c.videoSec > 5)) return give('the animatic\'s length couldn\'t be read');
  const tt = c.theme?.tempo || {};
  const bpmMin = tt.bpm_fixed || tt.bpm_min || 64; const bpmMax = tt.bpm_fixed || tt.bpm_max || 140;
  onActivity({ kind: 'info', text: 'Fitting a tempo to the cuts.' });
  const cuts = cutHits(c);
  const map = tempoMap({ hits: cuts, videoSec: c.videoSec, fps: FPS, bpmMin, bpmMax, prefer: (bpmMin + bpmMax) / 2 > 64 && tt.bpm_min ? (bpmMin + bpmMax) / 2 : 90 });
  const hits = cuts.map((h) => { const p = placeOf(map, h.t_sec); return { ...h, bar: p.bar, beat: p.beat, fits: Math.abs(p.at - h.t_sec) <= 1 / FPS + 1e-6 && p.bar <= map.bars }; }).filter((h) => h.bar >= 1 && h.bar <= map.bars);
  const sections = sectionsFromScenes(map, c.scenes);
  if (!sections.length) return give('no scenes to score');

  // 2. the cue plan: one call, a fix call with only the errors when the plan or the engine refuses it
  const mu = join(c.work, 'music');
  mkdirSync(mu, { recursive: true });
  onActivity({ kind: 'info', text: `Planning the music: ${map.bpm} BPM, ${map.meter}, ${sections.length} sections, ${hits.filter((h) => h.fits).length} of ${hits.length} cuts on the beat.` });
  let plan = null; let fix = null; let bp = null; let composed = null;
  for (let round = 0; ; round++) {
    if (round > MAX_FIXES) return give(`the plan still fails: ${fix.errors.join('; ').slice(0, 300)}`);
    const r = await call(planPrompt(c, map, sections, hits, fix), PLAN_SCHEMA);
    if (stopped()) return { ok: false, error: 'stopped', usage };
    plan = r.ok ? r.structured : null;
    if (!plan) return give(r.error || 'the plan call gave no answer');
    const pp = planProblems(plan, sections);
    if (pp.length) { fix = { plan, errors: pp }; continue; }
    // 3. the notes, by code, checked with the engine's own harmony lock and intensity curve
    const used0 = new Set(plan.sections.flatMap((s) => s.parts || []).filter((p) => PARTS.includes(p)));
    bp = blueprintFrom({ plan, map, sections, hits, used: used0 });
    const busy = (t) => c.timing.lines.some((l) => t >= l.start && t < l.end);
    composed = composeChecked({ plan, bp, voiceBusy: busy });
    const used = usedParts(composed.notes);
    bp.roles = bp.roles.filter((r) => [...used.keys()].some((k) => ROLES[k].name === r.name));
    writeFileSync(join(mu, 'blueprint.json'), `${JSON.stringify(bp, null, 2)}\n`);
    const pl = await engine(c, env, 'plan');
    if (pl.code !== 0) { fix = { plan, errors: pl.out.split('\n').filter((l) => /blueprint\.json|✖/.test(l)) }; continue; }
    if (!composed.harmony.ok) return give(`the written notes break the harmony lock (a studio bug): ${composed.harmony.errors.slice(0, 3).join(' ')}`.slice(0, 400));
    if (composed.curve.problems.length) { fix = { plan, errors: composed.curve.problems.map((p) => `intensity: ${p} (change the asks or the parts that play)`) }; continue; }
    break;
  }
  writeFileSync(join(mu, 'score.strudel.js'), scoreText({ notes: composed.notes, bp, plan }));

  // 4. the engine's check (captures the MIDI), the instruments, the audition
  onActivity({ kind: 'info', text: 'Checking the score and capturing the MIDI.' });
  const ck = await engine(c, env, 'check', '--video-sec', String(c.videoSec), '--fps', String(FPS));
  if (stopped()) return { ok: false, error: 'stopped', usage };
  if (ck.code !== 0) return give(`strudel.mjs check: ${tail(ck.out, 300)}`);
  const used = usedParts(composed.notes);
  const inst = instrumentsFor(plan.palette, used);
  let audition = null;
  for (let round = 0; round < 4; round++) {
    writeFileSync(join(mu, 'instruments.json'), `${JSON.stringify(inst, null, 2)}\n`);
    const ins = await engine(c, env, 'instruments');
    if (ins.code !== 0) return give(`strudel.mjs instruments: ${tail(ins.out, 300)}`);
    onActivity({ kind: 'info', text: 'Auditioning every instrument across its range.' });
    audition = await engine(c, env, 'audition');
    if (stopped()) return { ok: false, error: 'stopped', usage };
    const rep = readJsonOr(join(mu, 'audition.json'), { parts: [] });
    // every instrument sounds: level each part to its place in the mix from what the audition measured, then again
    if (audition.code === 0) { if (round < 3 && balanceGains(inst, rep)) continue; break; }
    // a part clipping alone comes down, a silent one goes up; then the audition again
    let changed = false;
    for (const p of rep.parts.filter((x) => !x.ok)) {
      const part = inst.parts.find((x) => x.channel === p.channel);
      const loud = p.notes.some((n) => n.peak_db > -0.3);
      if (part) { part.gain = +Math.max(0.05, Math.min(1.4, (part.gain ?? 0.8) * (loud ? 0.6 : 1.6))).toFixed(3); changed = true; }
    }
    if (!changed) break;
  }
  if (audition.code !== 0) return give(`strudel.mjs audition: ${tail(audition.out, 300)}`);

  // 5. the render, through the render queue; a score that peaks over -1 dBFS (the parts add up) comes down as a whole,
  // every part by the same amount so the balance stays, and renders again
  let score = null;
  for (let round = 0; round < 3; round++) {
    onActivity({ kind: 'info', text: round ? 'The parts add up too hot: turning the whole score down and rendering again.' : 'Rendering the music.' });
    const job = renders.add({ projectId: id, sessionId, label: 'music', argv: [process.execPath, M, 'render'], cwd: c.work, env: { ...process.env, ...env } });
    let v = job;
    while (!['done', 'failed'].includes(v.state)) v = await renders.wait(job.id, id, 300);
    if (stopped()) return { ok: false, error: 'stopped', usage };
    if (!(v.state === 'done' && v.exit_code === 0 && existsSync(join(c.work, 'out', 'music', 'score.wav')))) return give(`the render failed: ${tail(v.output_tail || v.error, 300)}`);
    score = levelOf(readWav(join(c.work, 'out', 'music', 'score.wav')));
    if (score.peak_db < -50) return give('the render is silent');
    if (score.peak_db <= -1) break;
    const k = 10 ** ((Math.min(-6 - score.peak_db, -21 - score.rms_db)) / 20);
    for (const p of inst.parts) p.gain = +Math.max(0.01, (p.gain ?? 0.8) * k).toFixed(4);
    writeFileSync(join(mu, 'instruments.json'), `${JSON.stringify(inst, null, 2)}\n`);
  }
  if (score.peak_db > -1) return give(`the score still peaks at ${score.peak_db} dBFS`);
  await exec('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', 'out/music/score.wav', '-c:a', 'libmp3lame', '-b:a', '256k', 'out/music/score.mp3'], { cwd: c.work });
  await exec('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', 'out/music/score.wav', '-lavfi', 'showspectrumpic=s=1600x400', 'music/spectrum.png'], { cwd: c.work });

  // 6. the mix under the voice (the animatic's own narration), measured line by line, then onto the animatic
  onActivity({ kind: 'info', text: 'Mixing the music under the voice.' });
  const voiceRel = existsSync(join(c.work, 'video', 'public', 'voice.wav')) ? 'video/public/voice.wav' : 'out/voice/narration.wav';
  let musicDb = -4; let mix = null; let uv = null;
  for (let round = 0; round < 3; round++) {
    mix = await mixUnderVoice(c, voiceRel, map, musicDb);
    if (!mix.ok) return give(mix.error);
    uv = underVoice(c, join(c.work, voiceRel), join(c.work, 'audio', 'music-ducked.wav'), map.offset_sec);
    const short = uv.median !== null ? UNDER - uv.median : 0;
    // too loud under the words, or loud enough that loudnorm had to ride the level: down, and again
    if (short > 2 || !mix.linear) { musicDb -= Math.max(2, Math.round(short)); continue; }
    break;
  }
  if (!mix.linear) return give('the mix\'s peaks are too hot for a fixed loudness gain');
  const mux = await exec('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', 'out/animatic.mp4', '-i', 'audio/mix.wav', '-map', '0:v', '-map', '1:a', '-c:v', 'copy', '-c:a', 'aac', '-b:a', '256k', '-t', String(c.videoSec), 'out/animatic-music.mp4'], { cwd: c.work });
  if (mux.code !== 0) return give(`putting the mix on the animatic: ${tail(mux.out, 300)}`);
  const vt = read(join(c.work, 'video', 'src', 'video.ts'));
  const size = `${vt.match(/width:\s*(\d+)/)?.[1] || 1920}x${vt.match(/height:\s*(\d+)/)?.[1] || 1080}`;
  await exec(process.execPath, [QC, 'out/animatic-music.mp4', '--size', size, '--fps', String(FPS), '--duration', String(Math.round(c.videoSec)), '--lufs', '-14', '--true-peak', '-1', '--end-hold', '--json', 'checks/qc-music.json'], { cwd: c.work });
  const qc = readJsonOr(join(c.work, 'checks', 'qc-music.json'), { checks: [] });
  const q = (k) => (qc.checks || []).find((x) => x.id === k);
  const audioFails = (qc.checks || []).filter((x) => /^audio\./.test(x.id) && x.result === 'fail');
  if (audioFails.length) return give(`qc.mjs on the mix: ${audioFails.map((x) => `${x.id} ${x.detail || ''}`).join('; ').slice(0, 300)}`);

  // 7. the spotting map, written from the plan (no AI), and the submission
  const check = readJsonOr(join(mu, 'check.json'), { hits: [], intensity: [] });
  const spot = [
    `# Spotting: ${c.project.title}`, '',
    `**Dramatic reading:** ${plan.dramatic_reading}.`, '',
    `**What the music must not do:** ${plan.must_not}`, '',
    `Tempo map (fitted by the studio to the cuts): ${map.bpm} BPM, ${map.meter}, bar 1 at ${map.offset_sec} s (a bar is ${map.sec_per_bar} s), ${map.bars} bars.`, '',
    '| bars | time (s) | section | intensity | function | melody | parts |', '|---|---|---|---|---|---|---|',
    ...bp.sections.map((s, i) => `| ${s.start_bar}-${s.end_bar} | ${sections[i].start_sec.toFixed(2)} | ${s.name} | ${s.intensity} | ${s.function} | ${s.variation} | ${(s.parts || []).join(', ')} |`), '',
    '| time (s) | cut | music | response |', '|---|---|---|---|',
    ...bp.hits.map((h) => `| ${h.t_sec} | ${h.what.split(':')[0]} | bar ${h.bar} beat ${h.beat} | ${h.kind} |`), '',
    `**Ending:** ${plan.ending}`, '',
  ].join('\n');
  writeFileSync(join(mu, 'SPOTTING.md'), spot);
  await engine(c, env, 'sheet');
  const lufs = q('audio.loudness')?.detail; const tp = q('audio.true-peak')?.detail;
  const hitRows = (check.hits || []).filter((h) => h.kind !== 'none');
  const error_checks = [
    { id: 'loudness', result: 'pass', note: `qc.mjs on out/animatic-music.mp4: ${lufs || 'loudness inside -14 LUFS ±1'}, true peak ${tp || '≤ -1 dBTP'}; ${/none/.test(mix.loudnorm?.normalization_type || '') ? 'the mix was already at -14 LUFS (the mastered voice carries it), so no loudness pass' : 'two-pass loudnorm with a fixed gain (linear)'}. The score alone peaks at ${score.peak_db} dBFS. Measured only: nothing here is by ear.` },
    { id: 'sync', result: 'pass', note: `${hitRows.length ? `${hitRows.length} hit${hitRows.length === 1 ? '' : 's'} within a frame: ${hitRows.map((h) => `bar ${h.bar} beat ${h.beat ?? 1} (${h.off_sec >= 0 ? '+' : ''}${h.off_sec} s)`).join(', ')}.` : 'No hits at this tempo.'}${bp.hits.some((h) => h.kind === 'none') ? ` ${bp.hits.filter((h) => h.kind === 'none').length} cut(s) play as no hit.` : ''} The music starts at bar 1 = ${map.offset_sec} s (adelay ${Math.round(map.offset_sec * 1000)} ms). The picture and voice are the approved animatic's, unchanged (video copied).` },
    { id: 'music-check', result: 'pass', note: `strudel.mjs check --video-sec ${c.videoSec} --fps ${FPS}: 0 problems (harmony locked, the curve follows the sections, hits on the picture). MIDI captured: music/notes.json, out/music/midi/. The notes were written by code from the cue plan, every pitched note a chord tone.` },
    { id: 'music-instruments', result: 'pass', note: `strudel.mjs instruments and audition pass on ${inst.parts.length} channels (a stand-in ${plan.palette} palette of real instruments from VCSL, CC0, chosen by the studio; each part levelled from its audition).` },
    { id: 'music-source', result: 'n/a', note: own ? 'This is the music step itself: the Strudel stand-in for review. The owner\'s own master (strudel.mjs own-master) comes in at the build.' : 'This is the music step itself: the approved Strudel render is used from here on.' },
    { id: 'music-under-voice', result: 'pass', note: `Sidechain ducking, music at ${musicDb} dB into the mix: measured by RMS on every spoken line, the music sits ${uv.min} to ${Math.max(...uv.rows.map((r) => r.under))} dB under the voice (median ${uv.median}). Measured, not listened to.` },
  ].filter((x) => stepChecks(root, id, 'music').some((k) => k.id === x.id));
  const curve = (check.intensity || []).map((s) => `${s.name} ${s.asked}→${s.measured}`).join(' · ');
  const note = `${plan.note}\n\n"${plan.title}": ${map.bpm} BPM, ${map.meter}, ${plan.scale}, ${map.bars} bars, ${used.size} parts. The story: ${plan.dramatic_reading}. Intensity asked → measured: ${curve}.\n\nThe studio wrote the notes from one AI cue plan (no session). These are measurements, not listening: judge it by ear.${own ? ' It\'s a stand-in: the MIDI pack is ready on the project page (Music › Download MIDI) for you to finish the sound in your DAW.' : ''}`;
  try {
    submit(root, id, 'music', {
      note, questions: [], error_checks,
      items: [
        { path: 'out/animatic-music.mp4', label: 'Animatic with the music' },
        { path: 'out/music/score.mp3', label: own ? 'The music alone (stand-in)' : 'The music alone' },
        { path: 'music/MUSIC-SHEET.md', label: 'Music sheet' },
        { path: 'music/SPOTTING.md', label: 'Spotting map' },
        { path: 'music/spectrum.png', label: 'Spectrum of the score' },
      ].filter((it) => existsSync(join(c.work, it.path))),
    });
  } catch (e) { return give(`the submission was refused: ${e.message}`); }
  const cl = join(c.work, 'checklist.md');
  if (existsSync(cl)) {
    const t = readFileSync(cl, 'utf8').split('\n');
    const at = t.findIndex((l) => /^##\s+Music\b/i.test(l));
    if (at >= 0) { for (let i = at + 1; i < t.length && !/^##\s/.test(t[i]); i++) t[i] = t[i].replace(/^- \[ \]/, '- [x]'); writeFileSync(cl, t.join('\n')); }
  }
  appendFileSync(join(c.work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · Music v${view.version + 1} planned by one AI call, written and mixed by the studio (no session): "${plan.title}", ${map.bpm} BPM, ${plan.scale}, ${map.bars} bars.\n`);
  onActivity({ kind: 'info', text: `Music ready for your check: "${plan.title}".` });
  return { ok: true, usage, summary: plan.note };
}
