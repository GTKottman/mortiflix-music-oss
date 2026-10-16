// Playing the cut with the music (doc 19 §4). Without the sound engine the clock is an <audio> element playing the last
// render; the Monitor's video follows it, never leads: two <video> elements (A plays the current shot, B is readied
// 300 ms before the next cut and swapped in on the cut), each frame checked with requestVideoFrameCallback and nudged.

// What to do about one frame's error (seconds; positive = the picture is ahead of the sound). Pure, so it's tested.
//   under 10 ms (or within the frame on screen): play at 1; up to 120 ms: nudge the rate (0.95–1.05, catches up in about
//   a second); over: seek. `hold` is half a frame: a frame shown anywhere inside its own duration is on time.
export function syncAction(error, hold = 0) {
  const a = Math.abs(error);
  if (a < Math.max(0.010, hold)) return { rate: 1 };
  if (a <= 0.120) return { rate: Math.min(1.05, Math.max(0.95, 1 - error)) };
  return { seek: true };
}

// The clock: "heard time now" from the audio element. currentTime moves in steps, so only readings taken just after it
// moved count (a stale one would say the sound is earlier than it is); of those, the smallest offset (arrival − media
// time) in the last 2 s wins: the reading with the least delay. Before the sound has moved at all, the media time is
// the answer.
export function audioClock(audio) {
  let samples = [];
  let last = null;
  return {
    reset() { samples = []; last = null; },
    now() {
      const t = performance.now();
      const ct = audio.currentTime;
      if (audio.paused) { samples = []; last = null; return ct; }
      if (last !== null && ct !== last) samples.push([t, t - ct * 1000]);
      last = ct;
      while (samples.length > 1 && samples[0][0] < t - 2000) samples.shift();
      if (!samples.length) return ct;
      const off = Math.min(...samples.map((x) => x[1]));
      return Math.max(0, (t - off) / 1000);
    },
  };
}

const LEAD = 1.0;          // seconds before a cut that the next shot is readied (a seek can take that long)

export function createTransport({ h, audioUrl, shots, fps = 30, onTime = () => {}, onState = () => {} }) {
  const frame = 1 / fps;
  const audio = h('audio', { src: audioUrl, preload: 'auto' });
  const clock = audioClock(audio);
  const vids = [0, 1].map(() => h('video', { class: 'tp-video', muted: true, playsinline: true, preload: 'auto' }));
  for (const v of vids) { v.muted = true; }
  const stage = h('div', { class: 'tp-stage' }, ...vids);
  let active = 0;           // which video shows
  let shotIdx = -1;         // the shot in the active video
  let raf = 0;
  const stats = [];         // frame errors while playing (seconds), for the drift test
  const trace = [];         // [heard time, error, rate, shot] per frame, for the same
  const shotAt = (t) => shots.findIndex((s) => t >= s.from && t < s.to);
  // the next shot carries straight on in the same file (cut from one render, in order): no swap needed
  const continues = (i) => { const a = shots[i]; const b = shots[i + 1]; return a && b && a.src === b.src && Math.abs(b.srcFrom - (a.srcFrom + (a.to - a.from))) < 0.002; };

  function load(v, i, t) {
    const s = shots[i];
    if (!s) return;
    // t may be before the shot starts (a readied video running up to its cut): fine while the file has frames there
    const want = Math.max(0, s.srcFrom + (t - s.from));
    if (!v.src.endsWith(s.src)) v.src = s.src;
    v.__shot = i;
    if (Math.abs(v.currentTime - want) > 0.02) v.currentTime = want;
  }
  function show(i) {
    vids[active].classList.remove('on');
    active = i;
    vids[active].classList.add('on');
  }

  // each frame: compare its middle (a frame is on screen for its whole duration) with the clock at the moment it's
  // displayed. The shown video is logged and nudged; a readied one running ahead of its cut is nudged too.
  function watch(v) {
    if (!v.requestVideoFrameCallback) return;
    v.requestVideoFrameCallback((now, meta) => {
      const s = shots[v.__shot];
      if (!audio.paused && s && !v.paused && (v.__shot === shotIdx || v.__shot === shotIdx + 1)) {
        const pictureAt = s.from + (meta.mediaTime + frame / 2 - s.srcFrom);
        const heardAt = clock.now() + (meta.expectedDisplayTime - performance.now()) / 1000;
        const error = pictureAt - heardAt;
        const shown = v === vids[active] && v.__shot === shotIdx;
        if (shown) {
          stats.push(error);
          trace.push([heardAt, error, v.playbackRate, shotIdx]);
          if (stats.length > 4000) { stats.shift(); trace.shift(); }
        }
        const act = syncAction(error, frame / 4);
        // a rate change restarts the browser's own timing, so it's only made when it matters (> 0.003 apart)
        if (act.seek) load(v, v.__shot, heardAt + 0.05);
        else if (Math.abs(v.playbackRate - act.rate) > 0.003 || (act.rate === 1 && v.playbackRate !== 1)) v.playbackRate = act.rate;
      }
      watch(v);
    });
  }
  vids.forEach(watch);

  function tick() {
    const t = clock.now();
    onTime(t);
    const i = shotAt(t);
    if (i !== shotIdx && i >= 0) {
      if (i === shotIdx + 1 && continues(shotIdx)) {
        vids[active].__shot = i;              // the same video plays on through the cut
      } else {
        // the cut: show the readied video if it holds this shot, else load it now
        const other = 1 - active;
        if (vids[other].__shot === i) { show(other); } else load(vids[active], i, t);
        vids[active].play().catch(() => {});
        vids[1 - active].pause();
      }
      shotIdx = i;
    }
    // 300 ms before the next cut, ready the hidden video: running up to the cut when its file has the frames before
    // the shot (the same render), else waiting at the shot's first frame
    const next = shots[shotIdx + 1];
    if (next && !continues(shotIdx) && next.from - t < LEAD && vids[1 - active].__shot !== shotIdx + 1) {
      const v = vids[1 - active];
      const lead = next.from - t;
      v.playbackRate = 1;
      if (next.srcFrom - lead >= 0) { load(v, shotIdx + 1, t); v.play().catch(() => {}); } else { load(v, shotIdx + 1, next.from); v.pause(); }
    }
    if (!audio.paused) raf = requestAnimationFrame(tick);
  }

  async function play(from = null) {
    if (from !== null) audio.currentTime = Math.max(0, from);
    clock.reset();
    shotIdx = -1;
    stats.length = 0; trace.length = 0;
    const t = audio.currentTime;
    const i = Math.max(0, shotAt(t));
    load(vids[active], i, t);
    vids[active].classList.add('on');
    shotIdx = i;
    await vids[active].play().catch(() => {});   // the picture first: it takes longer to start than the sound
    await audio.play();
    onState(true);
    raf = requestAnimationFrame(tick);
  }
  function stop() {
    audio.pause();
    cancelAnimationFrame(raf);
    for (const v of vids) { v.pause(); v.playbackRate = 1; }
    onState(false);
    onTime(audio.currentTime);
  }
  function seek(t) {
    const playing = !audio.paused;
    audio.currentTime = Math.max(0, t);
    clock.reset();
    const i = Math.max(0, shotAt(t));
    shotIdx = -1;
    load(vids[active], i, t);
    onTime(t);
    if (playing) { shotIdx = -1; tick(); }
  }
  audio.addEventListener('ended', () => stop());

  return { stage, audio, play, stop, seek, get playing() { return !audio.paused; }, now: () => clock.now(), stats, trace, destroy() { stop(); audio.removeAttribute('src'); for (const v of vids) v.removeAttribute('src'); } };
}
