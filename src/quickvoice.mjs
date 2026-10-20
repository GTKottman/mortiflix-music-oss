// The voice without an AI session (owner, 2026-10-19: Voice took 1.29M tokens in 24 steps, almost all of it the same
// context re-read while the session trimmed silence, measured loudness and checked word endings by hand). The voice is
// mechanical: the approved script's sentences, the studio's voice, the speech-to-text check, one track. So the studio
// does it: lines from the script, `vo.mjs speak` (the check retakes lines whose words don't match), one small AI call
// only to re-spell a line that keeps failing, `vo.mjs build` (clips trimmed to their words), the track set to -14
// LUFS, a clip per scene, and the submission. A line nothing could check (a voice with no speech-to-text) is marked
// for the owner to listen to: the Voice stage shows what was heard for every line, or that nobody listened yet.
import { runAs } from './models.mjs';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, writeFileSync, appendFileSync, copyFileSync } from 'node:fs';
import { join } from 'node:path';
import { paths, writeJson, REPO } from './studio.mjs';
import { loadProject, projectPaths, projectPipeline } from './projects.mjs';
import { stepView, stepChecks, submit } from './gates.mjs';
import { scriptView } from './svc/script.mjs';
import { voiceConfig } from './voice/index.mjs';
import { lengthSeconds } from './quickscript.mjs';
import { makeVoice } from './soundtrack/voice.mjs';
import { logVoiceUse } from './voiceusage.mjs';

const CHECKS = ['loudness', 'sync', 'music-source'];
const LUFS = -14;
const TRUE_PEAK = -1.5;
const GAP_IN_SCENE = 0.35;
const GAP_AFTER_SCENE = 0.7;

// The voice step, when it's the only work now and the studio can do it alone; otherwise null (a session does it).
export function quickVoiceStep(root, id, config) {
  if (config.quickVoice === false || config.quickScript === false) return null;
  const project = loadProject(root, id);
  const pipeline = projectPipeline(root, id);
  const step = pipeline.steps.find((s) => s.key === 'voice');
  if (!step || step.review !== 'audio') return null;
  const runnable = stepView(project, pipeline).filter((s) => ['ready', 'working', 'changes'].includes(s.state));
  // a new version after the owner's notes needs judgement: a session reads the notes
  if (runnable.length !== 1 || runnable[0].key !== 'voice' || runnable[0].state === 'changes') return null;
  if (project.mfx && Object.keys(project.mfx).some((k) => k.endsWith('_wanted'))) return null;
  if (!['elevenlabs', 'qwen'].includes(voiceConfig(root).engine)) return null;
  const work = projectPaths(root, id).work;
  // the owner's own recording to cut into lines
  if (existsSync(join(work, 'input')) && readdirSync(join(work, 'input')).length) return null;
  if (voiceConfig(root).engine !== 'elevenlabs' && !existsSync(VO)) return null;
  if (stepChecks(root, id, 'voice').some((c) => !CHECKS.includes(c.id))) return null;
  return runnable[0];
}

// Sentences, kept whole: "Dr. Who" and "3.5" don't split; a sentence ends at . ! ? (or …) before a capital or a quote.
export function sentences(text) {
  // titles and abbreviations keep their full stop without ending the sentence ("Mr. Smith", "e.g. this")
  const s = String(text).replace(/\s+/g, ' ').trim().replace(/\b(Mr|Mrs|Ms|Dr|Prof|St|Jr|Sr|vs|etc|e\.g|i\.e|U\.S|No)\.(?=\s)/gi, '$1\u0000');
  if (!s) return [];
  const out = s.split(/(?<=[.!?…])["”’)]?\s+(?=["“‘(]?[A-Z0-9])/u).map((x) => x.replace(/\u0000/g, '.').trim()).filter(Boolean);
  // a one-word piece ("Yes.") joins the sentence before it
  return out.reduce((acc, x) => { if (acc.length && x.split(' ').length < 2) acc[acc.length - 1] += ` ${x}`; else acc.push(x); return acc; }, []);
}

// voice/lines.json from the approved script: a beat's own lines when the owner edited them, else its sentences.
export function linesFromScript(view) {
  const list = [];
  for (const b of view.beats) {
    const own = b.lines.filter((l) => String(l.words || '').trim());
    const parts = own.length > 1 ? own.map((l) => ({ id: l.id, words: l.words })) : sentences(own[0]?.words || '').map((w, i) => ({ id: `${b.id}-${i + 1}`, words: w }));
    parts.forEach((p, i) => list.push({ id: p.id, text: p.words.trim(), script: p.words.trim(), gap_after: i === parts.length - 1 ? GAP_AFTER_SCENE : GAP_IN_SCENE }));
  }
  if (list.length) list.at(-1).gap_after = 0;
  return list;
}

// The studio's own voice tool (not the project's pinned copy: this is the studio's work, done with its current tool).
const VO = join(REPO, 'pipelines', '_shared', 'skills', 'voiceover', 'vo.mjs');

function vo(work, env, argv, timeoutMs = 20 * 60_000) {
  const r = spawnSync(process.execPath, [VO, ...argv], { cwd: work, env: { ...process.env, ...env }, encoding: 'utf8', timeout: timeoutMs, maxBuffer: 16 * 1024 * 1024 });
  return { ok: r.status === 0, out: `${r.stdout || ''}${r.stderr || ''}`.trim(), code: r.status };
}

function ffmpeg(args) {
  const r = spawnSync('ffmpeg', ['-hide_banner', '-nostdin', '-y', ...args], { encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 10 * 60_000 });
  return { ok: r.status === 0, err: r.stderr || '' };
}

// Integrated loudness and true peak of a file (EBU R128, ffmpeg's loudnorm in measure mode).
export function measure(file) {
  const r = ffmpeg(['-i', file, '-af', `loudnorm=I=${LUFS}:TP=${TRUE_PEAK}:LRA=11:print_format=json`, '-f', 'null', '-']);
  const m = r.err.match(/\{[^{}]*"input_i"[^{}]*\}/s);
  if (!m) return null;
  try { const j = JSON.parse(m[0]); return { i: Number(j.input_i), tp: Number(j.input_tp), lra: Number(j.input_lra), thresh: Number(j.input_thresh), offset: Number(j.target_offset) }; } catch { return null; }
}

// The narration at -14 LUFS, true peak ≤ -1.5 dBTP: two-pass loudnorm (linear when it can, so the voice isn't squashed).
function master(src, dst) {
  const m = measure(src);
  if (!m) return { ok: false, why: 'could not measure the loudness' };
  const f = `loudnorm=I=${LUFS}:TP=${TRUE_PEAK}:LRA=11:measured_I=${m.i}:measured_TP=${m.tp}:measured_LRA=${m.lra}:measured_thresh=${m.thresh}:offset=${m.offset}:linear=true`;
  const r = ffmpeg(['-i', src, '-af', `${f},aresample=48000`, '-ac', '1', '-c:a', 'pcm_s24le', dst]);
  if (!r.ok) return { ok: false, why: r.err.slice(-300) };
  return { ok: true, before: m, after: measure(dst) };
}

// One AI call for lines that keep failing the check: the spoken spelling (numbers, names, abbreviations as said),
// same words. `ask` is the claude-code run with no tools and a schema.
export const RESPELL_SCHEMA = { type: 'object', required: ['lines'], properties: { lines: { type: 'array', items: { type: 'object', required: ['id', 'text'], properties: { id: { type: 'string' }, text: { type: 'string', description: 'what the voice reads: the same words, spelled the way they are said' } } } } } };

export function respellPrompt(engine, bad) {
  return [`These narration lines failed a speech-to-text check after several takes (${engine === 'elevenlabs' ? 'ElevenLabs Eleven v4: you may use IPA between slashes inside quotes, "/ˈkoʊmæl/", for a hard name' : 'a local voice that reads plain words: spell hard names the way they sound'}).`,
    'For each, write `text` so the voice says exactly the script\'s words: numbers, dates, units and abbreviations written the way they are said, a long sentence split with a comma or ellipsis where a breath goes. Never change, add or drop words of the script. Answer with the JSON asked for, nothing else.',
    ...bad.map((b) => `- ${b.id}\n  script: ${b.script}\n  heard: ${b.transcript ?? '(nothing)'}\n  problems: ${(b.problems || []).join('; ')}`)].join('\n\n');
}

const r1 = (v) => Math.round(v * 10) / 10;

// Speak, check, build, master and submit the voice; `fallback()` runs a session when the studio can't finish it.
export async function quickVoice({ root, id, view, run, runArgs, config, onActivity, fallback }) {
  const pp = projectPaths(root, id);
  const work = pp.work;
  const env = runArgs.env || {};
  const engine = voiceConfig(root).engine;
  const usage = { input_tokens: 0, output_tokens: 0 };
  const give = async (why) => {
    onActivity({ kind: 'info', text: `The studio couldn't finish the voice by itself (${why}): a session does it.` });
    const f = await fallback();
    return { ...f, usage: { input_tokens: usage.input_tokens + (f.usage?.input_tokens || 0), output_tokens: usage.output_tokens + (f.usage?.output_tokens || 0) } };
  };
  const script = scriptView(root, id);
  const lines = linesFromScript(script);
  if (!lines.length) return give('the script has no words');
  mkdirSync(join(work, 'voice'), { recursive: true });
  writeFileSync(join(work, 'voice', 'lines.json'), JSON.stringify(lines, null, 2));
  // ElevenLabs: the soundtrack's voice (src/soundtrack/voice.mjs), lossless and never processed
  if (engine === 'elevenlabs') return soundtrackVoice({ root, id, view, work, script, lines, env, onActivity, give, usage });
  onActivity({ kind: 'info', text: `Recording the narration: ${lines.length} lines, checked as they're made.` });

  const spoke = Date.now();
  let sp = vo(work, env, ['speak', 'voice/lines.json']);
  const local = voiceConfig(root).qwen;
  logVoiceUse(work, { engine: 'qwen', voice: local.voice, model: `Qwen3-TTS ${local.model}`, lines: lines.length, seconds: (Date.now() - spoke) / 1000 });
  if (runArgs.signal?.aborted) return { ok: false, error: 'stopped', usage };
  const reportOf = () => { try { return JSON.parse(readFileSync(join(work, 'voice', 'speak-report.json'), 'utf8')); } catch { return []; } };
  let report = reportOf();
  if (!report.length) return give(sp.out.split('\n').slice(-3).join(' ').slice(0, 300) || 'the voice tool gave no report');
  if (/needs-you|ELEVENLABS_API_KEY is not set|no ElevenLabs voice chosen|ComfyUI/i.test(sp.out) && report.every((r) => !r.pass)) return give(sp.out.split('\n').at(-1).slice(0, 300));

  // lines that kept failing: re-spelled once by a small AI call, then spoken again
  const failing = () => report.filter((r) => !r.pass && r.problems?.length && !/ElevenLabs \d|upload|ComfyUI/i.test(r.problems.join(' ')));
  let bad = failing();
  if (bad.length && run) {
    onActivity({ kind: 'info', text: `${bad.length} line${bad.length === 1 ? '' : 's'} didn't say the script's words: re-spelling ${bad.length === 1 ? 'it' : 'them'}.` });
    const dir = join(paths(root).run, 'quick');
    mkdirSync(dir, { recursive: true });
    const r = await runAs(run, 'respell', { ...runArgs, workdir: dir, prompt: respellPrompt(engine, bad.map((b) => ({ ...b, script: lines.find((l) => l.id === b.id)?.script }))), config: { ...config, claudeArgs: ['--tools', '', '--json-schema', JSON.stringify(RESPELL_SCHEMA)] } });
    usage.input_tokens += r.usage?.input_tokens || 0;
    usage.output_tokens += r.usage?.output_tokens || 0;
    const fixes = (r.structured?.lines || []).filter((f) => lines.some((l) => l.id === f.id) && String(f.text || '').trim());
    for (const f of fixes) lines.find((l) => l.id === f.id).text = String(f.text).trim();
    if (fixes.length) {
      writeFileSync(join(work, 'voice', 'lines.json'), JSON.stringify(lines, null, 2));
      const again0 = Date.now();
      sp = vo(work, env, ['speak', 'voice/lines.json', '--only', fixes.map((f) => f.id).join(',')]);
      logVoiceUse(work, { engine: 'qwen', voice: local.voice, model: `Qwen3-TTS ${local.model}`, lines: fixes.length, seconds: (Date.now() - again0) / 1000 });
      // --only rewrites the report with those lines: put them back among the rest
      const again = reportOf();
      report = report.map((x) => again.find((y) => y.id === x.id) || x);
      writeFileSync(join(work, 'voice', 'speak-report.json'), JSON.stringify(report, null, 2));
    }
    bad = failing();
  }
  if (report.some((r) => !r.pass && !r.problems?.length)) return give('some lines have no take');

  const b = vo(work, env, ['build', 'voice/lines.json']);
  if (!b.ok || !existsSync(join(work, 'voice', 'voice.wav'))) return give(`build: ${b.out.slice(-300)}`);
  mkdirSync(join(work, 'out', 'voice'), { recursive: true });
  const m = master(join(work, 'voice', 'voice.wav'), join(work, 'out', 'voice', 'narration.wav'));
  if (!m.ok || !m.after) return give(`loudness: ${m.why || 'not measured'}`);

  // a clip per scene, from the track, for checking each scene's voice
  const timing = JSON.parse(readFileSync(join(work, 'voice', 'timing.json'), 'utf8'));
  const items = [{ path: 'out/voice/narration.wav', label: `Narration · full (${r1(timing.seconds)} s)`, section: 'Full' }];
  for (const beat of script.beats) {
    const ls = timing.lines.filter((l) => l.id.startsWith(`${beat.id}-`));
    if (!ls.length) continue;
    const from = Math.max(0, ls[0].start - 0.05), to = ls.at(-1).end + 0.15;
    const f = `out/voice/${beat.id}.wav`;
    if (ffmpeg(['-ss', String(from), '-t', String(to - from), '-i', join(work, 'out', 'voice', 'narration.wav'), '-c:a', 'pcm_s24le', join(work, f)]).ok) {
      items.push({ path: f, label: `${beat.id.toUpperCase()} · ${beat.name}`, section: 'Scenes' });
    }
  }

  // what the owner needs to know, and the checks
  const target = lengthSeconds(loadProject(root, id).intake?.answers?.length);
  const unchecked = report.filter((r) => r.transcript == null).map((r) => r.id);
  const untimed = timing.lines.filter((l) => l.timing !== 'words').map((l) => l.id);
  const retaken = report.filter((r) => (r.take || 1) > 1).map((r) => r.id);
  const who = report.find((r) => r.voice)?.voice;
  const note = [
    `The narration, read word for word from the approved script${who ? ` by ${who}` : ''} (${engine === 'elevenlabs' ? 'ElevenLabs' : 'the local voice'}): ${lines.length} lines, ${r1(timing.seconds)} s${target ? ` (the brief asks for about ${target} s)` : ''}. The full track first, then one clip per scene, so you can check each scene's voice before anything is drawn. Each clip is trimmed to its words; the track is set to ${LUFS} LUFS.`,
    retaken.length ? `Retaken until the words matched: ${retaken.join(', ')}.` : '',
    bad.length ? `Still not word-perfect after retakes (the best take is in): ${bad.map((x) => `${x.id} (${x.problems.join('; ')})`).join(' · ')}. Listen to ${bad.length === 1 ? 'it' : 'them'} and Fix with a note if a word is wrong.` : '',
    unchecked.length ? `Nobody listened to ${unchecked.length === report.length ? 'any line' : unchecked.join(', ')} yet: this voice has no speech-to-text check here, so listen and check the words before you OK ${unchecked.length === 1 ? 'it' : 'them'}.` : 'Every line was checked against the script by speech-to-text: the Voice stage shows what it heard.',
    target && Math.abs(timing.seconds - target) > target * 0.1 ? `It runs ${timing.seconds > target ? 'long' : 'short'} for ${target} s: Fix a scene (shorter words) or keep it as it is.` : '',
  ].filter(Boolean).join('\n\n');
  const error_checks = [
    { id: 'loudness', result: 'pass', note: `Measured by the studio: ${r1(m.before.i)} LUFS as recorded, set to ${r1(m.after.i)} LUFS integrated, true peak ${r1(m.after.tp)} dBTP (two-pass loudnorm). Clip edges are cut in silence with 20 ms fades.` },
    { id: 'sync', result: 'pass', note: `No picture yet. ${untimed.length ? `Word timings for every line but ${untimed.join(', ')} (timed per line)` : 'Every line has word timings'} in voice/timing.json; the animatic is timed to them.` },
    { id: 'music-source', result: 'n/a', note: 'Voice only: the music comes after the animatic.' },
  ].filter((c) => stepChecks(root, id, 'voice').some((n) => n.id === c.id));
  if (Number.isNaN(m.after.i) || Math.abs(m.after.i - LUFS) > 1 || m.after.tp > -1) return give(`loudness came out ${m.after.i} LUFS / ${m.after.tp} dBTP`);
  try {
    submit(root, id, 'voice', { note, questions: [], items, error_checks });
  } catch (e) { return give(`the submission was refused: ${e.message}`); }
  const ck = join(work, 'checklist.md');
  if (existsSync(ck)) {
    const t = readFileSync(ck, 'utf8').split('\n');
    const at = t.findIndex((l) => /^##\s+Voice\b/i.test(l));
    if (at >= 0) { for (let i = at + 1; i < t.length && !/^##\s/.test(t[i]); i++) t[i] = t[i].replace(/^- \[ \]/, '- [x]'); writeFileSync(ck, t.join('\n')); }
  }
  appendFileSync(join(work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · Voice v${view.version + 1} recorded and submitted by the studio (no session): ${lines.length} lines, ${r1(timing.seconds)} s, ${r1(m.after.i)} LUFS.\n`);
  writeJson(join(work, 'voice', 'quick-voice.json'), { lines: lines.length, seconds: timing.seconds, retaken, still_failing: bad.map((x) => x.id), unchecked, loudness: m });
  onActivity({ kind: 'info', text: `Voice ready for your check: ${r1(timing.seconds)} s, ${lines.length} lines.` });
  return { ok: true, usage, summary: note };
}

// The voice from Soundtrack v1 (owner, 2026-10-19: the old path saved 128 kbps MP3s and a session sped them up 4% and
// compressed them; "a finished voice should be perfect if it's coming from 11 labs"): ElevenLabs pcm_48000 with word
// timings, the silent edges cut, the lines joined with the script's gaps, and nothing else. The narration the later
// steps use (out/voice/narration.wav) is that file, untouched; the soundtrack's mix sets one plain gain for everything.
async function soundtrackVoice({ root, id, view, work, script, lines, env, onActivity, give, usage }) {
  const key = (() => { try { return JSON.parse(readFileSync(join(root, 'secrets.json'), 'utf8')).elevenlabs_api_key; } catch { return env.ELEVENLABS_API_KEY; } })();
  if (!key) return give('no ElevenLabs key');
  const el = voiceConfig(root).elevenlabs;
  onActivity({ kind: 'info', text: `Recording the narration: ${lines.length} lines from ElevenLabs, lossless, nothing done to it.` });
  let v;
  try {
    v = await makeVoice({ work, key, voice: el.voice_name || 'the studio voice', voiceId: el.voice_id, model: el.model_id, stability: el.stability, similarity: el.similarity_boost, speakerBoost: el.use_speaker_boost !== false,
      log: (t, at) => onActivity(at ? { kind: 'voice', engine: 'ElevenLabs', voice: el.voice_name || null, line: at.line, n: at.n, of: at.of, state: 'recording', text: `Recording line ${at.line.replace(/^b0*/, '')} (${at.n} of ${at.of}) on ElevenLabs${el.voice_name ? `, ${el.voice_name}` : ''}` } : { kind: 'info', text: t }) });
  } catch (e) { return give(`ElevenLabs: ${e.message}`); }
  const timing = v.timing;
  writeFileSync(join(work, 'voice', 'timing.json'), JSON.stringify(timing, null, 2));
  mkdirSync(join(work, 'out', 'voice'), { recursive: true });
  copyFileSync(v.file, join(work, 'voice', 'voice.wav'));
  copyFileSync(v.file, join(work, 'out', 'voice', 'narration.wav'));
  const m = measure(v.file);
  const items = [{ path: 'out/voice/narration.wav', label: `Narration · full (${r1(timing.seconds)} s)`, section: 'Full' }];
  for (const beat of script.beats) {
    const ls = timing.lines.filter((l) => l.id.startsWith(`${beat.id}-`));
    if (!ls.length) continue;
    const from = Math.max(0, ls[0].start - 0.05), to = ls.at(-1).end + 0.15;
    const f = `out/voice/${beat.id}.wav`;
    // a cut of the same samples (32-bit float: nothing rounded)
    if (ffmpeg(['-ss', String(from), '-t', String(to - from), '-i', join(work, 'out', 'voice', 'narration.wav'), '-c:a', 'pcm_f32le', join(work, f)]).ok) items.push({ path: f, label: `${beat.id.toUpperCase()} · ${beat.name}`, section: 'Scenes' });
  }
  const target = lengthSeconds(loadProject(root, id).intake?.answers?.length);
  const note = [
    `The narration, read word for word from the approved script by ${el.voice_name || 'the studio voice'} (ElevenLabs ${el.model_id}): ${lines.length} lines, ${r1(timing.seconds)} s${target ? ` (the brief asks for about ${target} s)` : ''}. The full track first, then one clip per scene.`,
    'It is exactly what ElevenLabs made, lossless (48 kHz PCM): no speed change, no compression, no limiter, no loudness change. Only the silence at each line\'s edges is cut. The soundtrack sets one plain gain for the whole mix at the music step.',
    'No speech-to-text check on this path: listen for the words before you OK it.',
    target && Math.abs(timing.seconds - target) > target * 0.1 ? `It runs ${timing.seconds > target ? 'long' : 'short'} for ${target} s: Fix a scene (fewer words) or keep it. The picture is cut to the voice; the voice is never sped up.` : '',
  ].filter(Boolean).join('\n\n');
  const error_checks = [
    { id: 'loudness', result: 'n/a', note: `Measured, not changed: ${m ? `${r1(m.i)} LUFS integrated, true peak ${r1(m.tp)} dBTP as ElevenLabs made it` : 'not measured'}. The level is set once for the whole soundtrack at the mix (Dubstage: one plain gain, no limiter).` },
    { id: 'sync', result: 'pass', note: 'No picture yet. Word timings for every line from ElevenLabs\' own alignment in voice/timing.json; the animatic is timed to them.' },
    { id: 'music-source', result: 'n/a', note: 'Voice only: the music comes after the animatic.' },
  ].filter((c) => stepChecks(root, id, 'voice').some((n) => n.id === c.id));
  try {
    submit(root, id, 'voice', { note, questions: [], items, error_checks });
  } catch (e) { return give(`the submission was refused: ${e.message}`); }
  const ck = join(work, 'checklist.md');
  if (existsSync(ck)) {
    const t = readFileSync(ck, 'utf8').split('\n');
    const at = t.findIndex((l) => /^##\s+Voice\b/i.test(l));
    if (at >= 0) { for (let i = at + 1; i < t.length && !/^##\s/.test(t[i]); i++) t[i] = t[i].replace(/^- \[ \]/, '- [x]'); writeFileSync(ck, t.join('\n')); }
  }
  appendFileSync(join(work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · Voice v${view.version + 1} from ElevenLabs (soundtrack v1, lossless, untouched): ${lines.length} lines, ${r1(timing.seconds)} s.\n`);
  onActivity({ kind: 'info', text: `Voice ready for your check: ${r1(timing.seconds)} s, ${lines.length} lines, untouched.` });
  return { ok: true, usage, summary: note };
}
