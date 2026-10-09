// The studio's player: hears a piece's MIDI in the browser with a small built-in synth and drum kit (Web Audio, no
// samples, nothing loaded from elsewhere), or plays an audio file (your master). One piece at a time, shared by the
// player bar, the library and every piano roll.
//
//   player.load({ id, title, href, piece })   piece: { bpm, beatsPerBar, tracks: [{ name, channel, kind?, notes }], markers }
//   player.loadAudio({ id, title, href, url })
//   player.play() · pause() · toggle() · seek(seconds) · close()
//   player.on((state) => …)                    called on every change and every frame while playing; returns an unsubscribe
//
// It's a sketch of the music, not its sound: the instruments are picked from each part's name (bass, pad, keys, lead,
// pluck, drums), so the shape, the harmony and the groove come through. The real sounds come in your DAW.

const LOOKAHEAD = 0.15;   // seconds of notes scheduled ahead
const TICK_MS = 25;

const listeners = new Set();
let ctx = null;
let out = null;           // master bus: gain → compressor → speakers
let verb = null;          // a shared reverb send
let noise = null;         // one second of white noise for the drums
let src = null;           // { kind: 'midi', id, title, href, events, duration, markers, bpm, bpb } | { kind: 'audio', id, title, href, el }
let playing = false;
let startedAt = 0;        // ctx time at which the piece's time 0 would have sounded
let offset = 0;           // where the piece is while paused (seconds)
let cursor = 0;           // the next event to schedule
let timer = null;
let frame = null;
const voices = new Set(); // sounding nodes, so pause and seek can silence them

function emit() { const s = state(); for (const fn of listeners) fn(s); }

export function on(fn) { listeners.add(fn); fn(state()); return () => listeners.delete(fn); }

export function state() {
  if (!src) return { id: null, playing: false, time: 0, duration: 0 };
  const time = now();
  return { id: src.id, title: src.title, href: src.href, kind: src.kind, playing, time, duration: src.duration, section: sectionAt(time), markers: src.markers || [] };
}

function now() {
  if (!src) return 0;
  if (src.kind === 'audio') return src.el.currentTime || 0;
  // Clamped: the audio clock can stand still for a moment while the sound device starts.
  return playing ? Math.max(0, Math.min(src.duration, ctx.currentTime - startedAt)) : offset;
}

function sectionAt(sec) {
  if (!src?.markers?.length) return null;
  let name = null;
  for (const m of src.markers) if (m.sec <= sec + 1e-3) name = m.text;
  return name;
}

// ---------- loading ----------

export function load({ id, title, href = null, piece }) {
  if (src?.id === id) return;
  close(false);
  const bpm = piece.bpm || 120;
  const bpb = piece.beatsPerBar || piece.beats_per_bar || 4;
  const spb = 60 / bpm;
  const events = [];
  for (const t of piece.tracks || []) {
    const inst = instrumentFor(t);
    for (const n of t.notes || []) events.push({ at: n.start * spb, dur: Math.max(0.03, n.dur * spb), pitch: n.pitch, vel: n.velocity ?? 0.8, inst });
  }
  events.sort((a, b) => a.at - b.at);
  const lastBeat = Math.max(0, ...(piece.tracks || []).flatMap((t) => (t.notes || []).map((n) => n.start + n.dur)), (piece.bars || 0) * bpb);
  const markers = (piece.markers || (piece.sections || []).map((x) => ({ beat: (x.start_bar - 1) * bpb, text: x.name })))
    .map((m) => ({ sec: m.beat * spb, text: m.text })).sort((a, b) => a.sec - b.sec);
  src = { kind: 'midi', id, title, href, events, markers, bpm, bpb, duration: Math.ceil(lastBeat / bpb) * bpb * spb };
  offset = 0; cursor = 0;
  emit();
}

export function loadAudio({ id, title, href = null, url }) {
  if (src?.id === id) return;
  close(false);
  const el = new Audio(url);
  el.preload = 'metadata';
  src = { kind: 'audio', id, title, href, el, duration: 0, markers: [] };
  el.addEventListener('loadedmetadata', () => { if (src?.el === el) { src.duration = el.duration; emit(); } });
  el.addEventListener('ended', () => { if (src?.el === el) { playing = false; stopFrames(); emit(); } });
  emit();
}

export function close(notify = true) {
  pause(false);
  if (src?.kind === 'audio') { src.el.pause(); src.el.removeAttribute('src'); src.el.load(); }
  src = null; offset = 0; cursor = 0;
  if (notify) emit();
}

// ---------- transport ----------

export async function play() {
  if (!src || playing) return;
  if (src.kind === 'audio') {
    if (src.duration && src.el.currentTime >= src.duration - 0.05) src.el.currentTime = 0;
    try { await src.el.play(); } catch { return; }
    playing = true; startFrames(); emit();
    return;
  }
  ensureContext();
  if (ctx.state === 'suspended') await ctx.resume();
  if (offset >= src.duration - 0.01) offset = 0;
  startedAt = ctx.currentTime + 0.05 - offset;
  cursor = firstAt(offset);
  playing = true;
  schedule();
  timer = setInterval(schedule, TICK_MS);
  startFrames();
  emit();
}

export function pause(notify = true) {
  if (!src || !playing) return;
  if (src.kind === 'audio') src.el.pause();
  else { offset = now(); clearInterval(timer); timer = null; silence(); }
  playing = false;
  stopFrames();
  if (notify) emit();
}

export function toggle() { return playing ? pause() : play(); }

export function seek(sec) {
  if (!src) return;
  const t = Math.max(0, Math.min(sec, src.duration || 0));
  if (src.kind === 'audio') { src.el.currentTime = t; emit(); return; }
  if (playing) { silence(); startedAt = ctx.currentTime + 0.03 - t; cursor = firstAt(t); schedule(); }
  else offset = t;
  emit();
}

function firstAt(sec) {
  let lo = 0; let hi = src.events.length;
  while (lo < hi) { const mid = (lo + hi) >> 1; if (src.events[mid].at < sec - 1e-4) lo = mid + 1; else hi = mid; }
  return lo;
}

function schedule() {
  if (!playing || src?.kind !== 'midi') return;
  const until = ctx.currentTime + LOOKAHEAD;
  const ev = src.events;
  while (cursor < ev.length && startedAt + ev[cursor].at < until) {
    const e = ev[cursor++];
    const when = Math.max(ctx.currentTime, startedAt + e.at);
    if (voices.size < 96) e.inst(when, e.pitch, e.dur, e.vel);
  }
  if (ctx.currentTime - startedAt >= src.duration) { offset = src.duration; clearInterval(timer); timer = null; playing = false; stopFrames(); emit(); }
}

function startFrames() {
  stopFrames();
  const loop = () => { emit(); frame = requestAnimationFrame(loop); };
  frame = requestAnimationFrame(loop);
}
function stopFrames() { if (frame) cancelAnimationFrame(frame); frame = null; }

function silence() {
  const t = ctx.currentTime;
  for (const v of voices) {
    try { v.gain.gain.cancelScheduledValues(t); v.gain.gain.setTargetAtTime(0, t, 0.01); v.nodes.forEach((n) => n.stop(t + 0.06)); } catch { /* already stopped */ }
  }
  voices.clear();
}

// ---------- the sound ----------

function ensureContext() {
  if (ctx) return;
  ctx = new (window.AudioContext || window.webkitAudioContext)();
  const comp = ctx.createDynamicsCompressor();
  comp.threshold.value = -14; comp.ratio.value = 3; comp.attack.value = 0.005; comp.release.value = 0.2;
  out = ctx.createGain(); out.gain.value = 0.7;
  out.connect(comp).connect(ctx.destination);
  // A short, dark room for the pads and keys: decaying noise as the impulse.
  const len = Math.floor(ctx.sampleRate * 1.8);
  const ir = ctx.createBuffer(2, len, ctx.sampleRate);
  for (let c = 0; c < 2; c += 1) { const d = ir.getChannelData(c); for (let i = 0; i < len; i += 1) d[i] = (Math.random() * 2 - 1) * (1 - i / len) ** 3; }
  const conv = ctx.createConvolver(); conv.buffer = ir;
  const tone = ctx.createBiquadFilter(); tone.type = 'lowpass'; tone.frequency.value = 3500;
  verb = ctx.createGain(); verb.gain.value = 0.35;
  verb.connect(conv).connect(tone).connect(out);
  noise = ctx.createBuffer(1, ctx.sampleRate, ctx.sampleRate);
  const d = noise.getChannelData(0); for (let i = 0; i < d.length; i += 1) d[i] = Math.random() * 2 - 1;
}

const hz = (pitch) => 440 * 2 ** ((pitch - 69) / 12);

// One voice: oscillators (or noise) → filter → an envelope, released after `dur`. `send` is the reverb amount.
function voice(when, { oscs, filter = null, a = 0.005, d = 0.1, s = 0.7, r = 0.1, dur, peak, send = 0 }) {
  const g = ctx.createGain();
  g.gain.setValueAtTime(0, when);
  g.gain.linearRampToValueAtTime(peak, when + a);
  g.gain.setTargetAtTime(peak * s, when + a, d / 3);
  const end = when + Math.max(dur, a);
  g.gain.setTargetAtTime(0, end, r / 3);
  let head = g;
  if (filter) { filter.connect(g); head = filter; }
  for (const o of oscs) { o.connect(head); o.start(when); o.stop(end + r * 2 + 0.05); }
  g.connect(out);
  if (send) { const sg = ctx.createGain(); sg.gain.value = send; g.connect(sg).connect(verb); }
  const v = { gain: g, nodes: oscs };
  voices.add(v);
  oscs[0].onended = () => voices.delete(v);
  return v;
}

function osc(type, freq, detune = 0) {
  const o = ctx.createOscillator(); o.type = type; o.frequency.value = freq; o.detune.value = detune; return o;
}

function lowpass(freq, q = 0.7) { const f = ctx.createBiquadFilter(); f.type = 'lowpass'; f.frequency.value = freq; f.Q.value = q; return f; }

const INSTRUMENTS = {
  bass(when, p, dur, vel) {
    const f = lowpass(180 + vel * 500, 2);
    f.frequency.setValueAtTime(900 * vel + 200, when); f.frequency.setTargetAtTime(220, when, 0.12);
    voice(when, { oscs: [osc('sawtooth', hz(p)), osc('sine', hz(p) / 2)], filter: f, a: 0.004, d: 0.25, s: 0.55, r: 0.06, dur, peak: 0.28 * vel });
  },
  pad(when, p, dur, vel) {
    voice(when, { oscs: [osc('sawtooth', hz(p), -7), osc('sawtooth', hz(p), 7)], filter: lowpass(1400), a: Math.min(0.35, dur / 3), d: 0.6, s: 0.8, r: 0.7, dur, peak: 0.06 * vel, send: 0.8 });
  },
  keys(when, p, dur, vel) {
    voice(when, { oscs: [osc('triangle', hz(p)), osc('sine', hz(p) * 2)], filter: lowpass(2600), a: 0.004, d: 0.9, s: 0.25, r: 0.25, dur, peak: 0.14 * vel, send: 0.35 });
  },
  lead(when, p, dur, vel) {
    voice(when, { oscs: [osc('square', hz(p)), osc('sawtooth', hz(p), 5)], filter: lowpass(2200, 1.5), a: 0.01, d: 0.2, s: 0.6, r: 0.12, dur, peak: 0.07 * vel, send: 0.25 });
  },
  pluck(when, p, dur, vel) {
    const f = lowpass(4000, 1); f.frequency.setValueAtTime(4000, when); f.frequency.setTargetAtTime(500, when, 0.08);
    voice(when, { oscs: [osc('sawtooth', hz(p))], filter: f, a: 0.002, d: 0.25, s: 0, r: 0.1, dur: Math.min(dur, 0.4), peak: 0.12 * vel, send: 0.3 });
  },
  drums(when, p, dur, vel) { drum(when, p, vel); },
};

// General MIDI drums, drawn from oscillators and noise.
function drum(when, p, vel) {
  const hit = (node, peak, decay, filterNode = null) => {
    const g = ctx.createGain(); g.gain.setValueAtTime(peak * vel, when); g.gain.exponentialRampToValueAtTime(0.0008, when + decay);
    (filterNode ? node.connect(filterNode) : node).connect(g).connect(out);
    node.start(when); node.stop(when + decay + 0.02);
    const v = { gain: g, nodes: [node] }; voices.add(v); node.onended = () => voices.delete(v);
  };
  const noiseSrc = () => { const n = ctx.createBufferSource(); n.buffer = noise; return n; };
  const filt = (type, freq, q = 0.8) => { const f = ctx.createBiquadFilter(); f.type = type; f.frequency.value = freq; f.Q.value = q; return f; };
  if (p === 35 || p === 36) {                      // kick
    const o = osc('sine', 150); o.frequency.setValueAtTime(150, when); o.frequency.exponentialRampToValueAtTime(42, when + 0.12);
    hit(o, 0.9, 0.35);
  } else if (p === 38 || p === 40) {               // snare
    hit(noiseSrc(), 0.35, 0.18, filt('highpass', 1500));
    const o = osc('triangle', 190); o.frequency.setValueAtTime(190, when); o.frequency.exponentialRampToValueAtTime(140, when + 0.08); hit(o, 0.3, 0.1);
  } else if (p === 37 || p === 39) {               // rim, clap
    hit(noiseSrc(), 0.35, p === 39 ? 0.14 : 0.04, filt('bandpass', p === 39 ? 1200 : 2500, 2));
  } else if (p === 42 || p === 44) {               // closed hat, pedal
    hit(noiseSrc(), 0.16, 0.05, filt('highpass', 8000));
  } else if (p === 46) {                           // open hat
    hit(noiseSrc(), 0.14, 0.32, filt('highpass', 7500));
  } else if (p === 49 || p === 52 || p === 55 || p === 57) { // crashes
    hit(noiseSrc(), 0.14, 1.2, filt('highpass', 5000));
  } else if (p === 51 || p === 53 || p === 59) {   // ride
    hit(noiseSrc(), 0.08, 0.45, filt('bandpass', 6000, 1.5));
  } else if (p >= 41 && p <= 50) {                 // toms
    const f0 = 80 + (p - 41) * 18; const o = osc('sine', f0 * 1.6); o.frequency.setValueAtTime(f0 * 1.6, when); o.frequency.exponentialRampToValueAtTime(f0, when + 0.15);
    hit(o, 0.5, 0.3);
  } else if (p === 54 || p === 69 || p === 70 || p === 82) { // tambourine, cabasa, maracas, shaker
    hit(noiseSrc(), 0.1, 0.08, filt('highpass', 6000));
  } else {                                         // anything else: a short wood tick
    const o = osc('square', hz(p)); hit(o, 0.08, 0.05, filt('bandpass', hz(p) * 2, 3));
  }
}

// Which instrument plays a part: drums by channel 10 (General MIDI) or kind, the rest by the part's name.
export function instrumentKind(t) {
  if (t.drums || t.kind === 'drums' || t.channel === 10) return 'drums';
  const n = `${t.name || ''} ${t.kind || ''}`.toLowerCase();
  if (/bass|808|sub\b/.test(n)) return 'bass';
  if (/pad|string|choir|atmo|drone|texture|swell|ambien/.test(n)) return 'pad';
  if (/pluck|arp|bell|mallet|marimba|harp|guitar|kalimba|chop/.test(n)) return 'pluck';
  if (/lead|melody|hook|vocal|vox|synth|flute|horn|brass|sax|whistle/.test(n)) return 'lead';
  return 'keys';
}

function instrumentFor(t) {
  const fn = INSTRUMENTS[instrumentKind(t)];
  return (when, p, dur, vel) => fn(when, p, dur, vel);
}

export const clock = (sec) => `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`;
