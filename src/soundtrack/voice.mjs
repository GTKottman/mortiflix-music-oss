// Soundtrack v1, the voice (owner, 2026-10-19: "a finished voice should be perfect if it's coming from 11 labs"). The
// narration comes from ElevenLabs as lossless 48 kHz PCM with its word timings and is never processed: no speed-up, no
// compressor, no limiter, no EQ, no loudness gain (the mix sets one plain gain for the whole soundtrack). The only edit
// is cutting each line's silent edges (below -50 dBFS, keeping a 60 ms lead and a 100 ms tail) and joining the lines
// with the script's gaps. The picture is cut to this voice, never the voice to the picture.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { writeWav } from './wav.mjs';

const RATE = 48000;
const FORMAT = 'pcm_48000'; // lossless; the best ElevenLabs gives on the Creator plan (44.1 kHz PCM needs Pro)
const SILENT = 10 ** (-50 / 20);
const LEAD = 0.06;
const TAIL = 0.1;

async function voiceId(key, name) {
  const r = await fetch(`https://api.elevenlabs.io/v2/voices?search=${encodeURIComponent(name)}&page_size=20`, { headers: { 'xi-api-key': key } });
  const v = (await r.json()).voices?.find((x) => x.name === name) || null;
  if (!v) throw new Error(`no ElevenLabs voice named "${name}"`);
  return v.voice_id;
}

// Words and their times from ElevenLabs' character alignment; performance tags ([warm, unhurried]) are not words.
export function wordsFrom(alignment) {
  const ch = alignment?.characters || []; const st = alignment?.character_start_times_seconds || []; const en = alignment?.character_end_times_seconds || [];
  const words = []; let cur = null; let tag = 0;
  ch.forEach((c, i) => {
    if (c === '[') tag++;
    if (tag) { if (c === ']') tag--; return; }
    if (/\s/.test(c)) { if (cur) words.push(cur); cur = null; return; }
    if (!cur) cur = { text: '', start: st[i], end: en[i] };
    cur.text += c; cur.end = en[i];
  });
  if (cur) words.push(cur);
  return words.filter((w) => /[\p{L}\p{N}]/u.test(w.text));
}

export async function makeVoice({ work, key, voice = 'Tracy K', model = 'eleven_v4', stability = 0.5, similarity = 0.75, log = () => {} }) {
  const lines = JSON.parse(readFileSync(join(work, 'voice', 'lines.json'), 'utf8'));
  const dir = join(work, 'sound', 'voice');
  mkdirSync(join(dir, 'lines'), { recursive: true });
  const id = await voiceId(key, voice);
  const clips = [];
  for (const l of lines) {
    const pcm = join(dir, 'lines', `${l.id}.pcm`); const meta = join(dir, 'lines', `${l.id}.json`);
    if (!existsSync(pcm) || !existsSync(meta)) {
      log(`ElevenLabs: ${l.id}`);
      const r = await fetch(`https://api.elevenlabs.io/v1/text-to-speech/${id}/with-timestamps?output_format=${FORMAT}`, {
        method: 'POST', headers: { 'xi-api-key': key, 'Content-Type': 'application/json' },
        body: JSON.stringify({ text: l.text, model_id: model, voice_settings: { stability, similarity_boost: similarity } }),
      });
      if (!r.ok) throw new Error(`ElevenLabs ${r.status} on ${l.id}: ${(await r.text()).slice(0, 200)}`);
      const j = await r.json();
      writeFileSync(pcm, Buffer.from(j.audio_base64, 'base64'));
      writeFileSync(meta, JSON.stringify({ id: l.id, text: l.text, voice, voice_id: id, model, format: FORMAT, alignment: j.alignment }, null, 1));
    }
    const buf = readFileSync(pcm);
    const x = new Float32Array(buf.length / 2);
    for (let i = 0; i < x.length; i++) x[i] = buf.readInt16LE(i * 2) / 32768;
    const words = wordsFrom(JSON.parse(readFileSync(meta, 'utf8')).alignment);
    // cut only silence: from LEAD before the first sound to TAIL after the last
    let a = 0; while (a < x.length && Math.abs(x[a]) < SILENT) a++;
    let z = x.length - 1; while (z > a && Math.abs(x[z]) < SILENT) z--;
    const from = Math.max(0, a - Math.round(LEAD * RATE)); const to = Math.min(x.length, z + 1 + Math.round(TAIL * RATE));
    clips.push({ line: l, samples: x.subarray(from, to), words: words.map((w) => ({ text: w.text, start: w.start - from / RATE, end: w.end - from / RATE })) });
  }
  // join with the script's gaps
  const total = clips.reduce((s, c) => s + c.samples.length + Math.round((c.line.gap_after ?? 0.3) * RATE), 0);
  const out = new Float32Array(total);
  const timing = { seconds: 0, made_by: 'soundtrack v1: ElevenLabs pcm_48000, unprocessed', lines: [] };
  let at = 0;
  for (const c of clips) {
    out.set(c.samples, at);
    const t0 = at / RATE;
    timing.lines.push({ id: c.line.id, text: c.line.script || c.line.text, start: +(t0).toFixed(3), end: +(t0 + c.samples.length / RATE).toFixed(3),
      words: c.words.map((w) => ({ text: w.text, start: +(t0 + w.start).toFixed(3), end: +(t0 + w.end).toFixed(3) })) });
    at += c.samples.length + Math.round((c.line.gap_after ?? 0.3) * RATE);
  }
  timing.seconds = +(total / RATE).toFixed(3);
  writeWav(join(dir, 'voice.wav'), [out], RATE);
  writeFileSync(join(dir, 'timing.json'), JSON.stringify(timing, null, 1));
  return { file: join(dir, 'voice.wav'), timing, voice, model, format: FORMAT };
}
