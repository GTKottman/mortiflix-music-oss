// The song lane (doc 19 §1–2, mockup A): the music drawn under the shots it plays over, read-only for now. One time
// axis in video seconds: bar 1 starts at the song's offsetSec, and the shots, the bar ruler, the section blocks, the
// narration and the cut marks all sit on it, so "the section under the shot it plays over" is true by construction.
// Drawn on a <canvas> (a 200-bar song has tens of thousands of notes); the words around it are DOM.

const RULER = 18;
const LABELS = 64;          // the part names on the left
const ROW = 28;             // one part's mini row
const ROW_GAP = 2;
const BLOCK_HEAD = 24;
const MAX_ROWS = 6;         // more parts fold into the last row as "+N more parts"
const VOICE = 18;
const CUTS = 16;

const css = (el, name, fallback) => getComputedStyle(el).getPropertyValue(name).trim() || fallback;

// Bars and ticks to video seconds (one tempo for now: the tempo map's first point).
export function timeAxis(song) {
  const bpm = song.tempo?.[0]?.bpm || 120;
  const m = song.meter?.[0] || { num: 4, den: 4 };
  const ppq = song.ppq || 960;
  const tpb = ppq * 4 * m.num / m.den;
  const secPerTick = 60 / bpm / ppq;
  const off = song.offsetSec || 0;
  return { tpb, tickSec: (tick) => off + tick * secPerTick, barSec: (bar) => off + (bar - 1) * tpb * secPerTick };
}

// The shots in the edit, back to back: [{ id, label, from, to }] in video seconds.
export function shotTimes(graph) {
  const n = graph?.nodes || {};
  let t = 0;
  const out = [];
  for (const sid of graph?.order?.shots || []) {
    const s = n[sid];
    if (!s || s.state === 'cut') continue;
    const v = (s.versions || []).find((x) => x.v === s.current);
    if (!v || v.to_sec === undefined) continue;
    const len = Math.max(0, v.to_sec - (v.from_sec || 0));
    out.push({ id: sid, label: s.label, colour: s.colour, from: t, to: t + len });
    t += len;
  }
  return out;
}

// What each cut gets: a hit that lands within one frame ("cut · hit"), one that misses ("missed by 0.21 s"), or none.
export function cutMarks(song, shots, fps = 30) {
  const ax = timeAxis(song);
  const hits = (song.markers || []).filter((m) => m.kind === 'hit').map((m) => ({ ...m, music: ax.tickSec(m.tick) }));
  return shots.slice(1).map((s) => {
    const at = s.from;
    const near = hits.filter((x) => Math.abs((x.sec ?? x.music) - at) <= 1).sort((a, b) => Math.abs(a.music - at) - Math.abs(b.music - at))[0];
    if (!near) return { at, kind: 'cut', text: 'cut' };
    const off = near.music - at;
    if (Math.abs(off) <= 1 / fps + 1e-6) return { at, kind: 'hit', text: 'cut · hit', hit: near.name };
    return { at, kind: 'miss', text: `missed by ${Math.abs(off).toFixed(2)} s`, hit: near.name };
  });
}

export function songLane({ h, song, graph, draft = false }) {
  const ax = timeAxis(song);
  const shots = shotTimes(graph);
  const fps = graph?.fps || 30;
  const cuts = cutMarks(song, shots, fps);
  const cueNode = (id) => graph?.nodes?.[id];
  const items = (song.playlist?.items || []).filter((i) => i.kind === 'pattern').sort((a, b) => a.at.tick - b.at.tick);
  const sections = items.map((i) => {
    const p = song.patterns.find((x) => x.id === i.ref) || { notes: {} };
    const startBar = i.at.tick / ax.tpb + 1;
    const bars = i.len / ax.tpb;
    const from = ax.barSec(startBar);
    const to = ax.barSec(startBar + bars);
    const under = shots.find((s) => from >= s.from - 1e-6 && from < s.to - 1e-6);
    return { id: p.id || i.ref, name: p.name || '', startTick: i.at.tick, startBar, bars, from, to, notes: p.notes || {}, under: under?.label || null, colour: cueNode(p.id)?.colour || null };
  });
  const lastBar = sections.length ? sections.at(-1).startBar + sections.at(-1).bars : 1;
  const lines = (graph?.order?.lines || []).map((l) => graph.nodes[l]).filter((l) => l && l.start_sec !== null && l.start_sec !== undefined && l.state !== 'cut');
  const end = Math.max(ax.barSec(lastBar), shots.at(-1)?.to || 0, ...lines.map((l) => l.end_sec || 0), 1);

  // parts that play anywhere, in channel order; up to MAX_ROWS rows, the rest folded
  const playing = song.channels.filter((c) => song.patterns.some((p) => p.notes?.[c.id]?.length));
  const shown = playing.length > MAX_ROWS ? playing.slice(0, MAX_ROWS - 1) : playing;
  const folded = playing.length > MAX_ROWS ? playing.slice(MAX_ROWS - 1) : [];
  const rows = shown.length + (folded.length ? 1 : 0);
  const range = new Map(playing.map((c) => {
    const keys = song.patterns.flatMap((p) => (p.notes?.[c.id] || []).map((n) => n.key));
    return [c.id, [Math.min(...keys), Math.max(...keys)]];
  }));
  const blockH = BLOCK_HEAD + rows * (ROW + ROW_GAP) + 4;
  const voiceY = RULER + 4 + blockH + 6;
  const cutsY = voiceY + VOICE + 4;
  const H = cutsY + CUTS + 4;

  const canvas = h('canvas', { class: 'sl-canvas', role: 'img', 'aria-label': `The music under the shots: ${sections.length} sections, ${playing.length} parts, ${Math.round(lastBar - 1)} bars` });
  const wrap = h('div', { class: 'sl-wrap' }, canvas);
  const list = h('ol', { class: 'sr-only' }, sections.map((s) => h('li', null, `${s.id.toUpperCase()} ${s.name}: bars ${s.startBar}–${s.startBar + s.bars - 1}${s.under ? `, under ${s.under}` : ''}`)));
  const el = h('div', { class: 'sl' },
    draft ? h('div', { class: 'sl-note' }, 'From the score. The song is made when the music is imported (mortiflix music <project> --song).') : null,
    wrap, list);

  function draw() {
    const width = Math.max(320, wrap.clientWidth || 900);
    const pps = (width - LABELS - 12) / end;      // fit the whole song
    const x = (sec) => LABELS + sec * pps;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * dpr); canvas.height = Math.round(H * dpr);
    canvas.style.width = `${width}px`; canvas.style.height = `${H}px`;
    const g = canvas.getContext('2d');
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, width, H);
    const faint = css(el, '--faint', '#6e6e6e');
    const muted = css(el, '--muted', '#b0b0b0');
    const line = css(el, '--line', '#303030');
    const warn = css(el, '--warn', '#f0f04f');
    const bad = css(el, '--bad', '#dc513d');
    const font = (px, weight = 400) => `${weight} ${px}px Inter, system-ui, sans-serif`;
    g.textBaseline = 'middle';

    // the bar ruler: a number every bar, or every 2 / 4 / 8 when bars are narrow
    g.font = font(10);
    g.fillStyle = faint;
    g.fillText('Bar', 0, RULER / 2);
    const barPx = (ax.barSec(2) - ax.barSec(1)) * pps;
    const every = [1, 2, 4, 8, 16, 32].find((k) => barPx * k >= 22) || 64;
    for (let b = 1; b <= lastBar; b++) {
      const bx = Math.round(x(ax.barSec(b))) + 0.5;
      g.strokeStyle = line;
      g.beginPath(); g.moveTo(bx, b % every === 1 || every === 1 ? 0 : RULER - 5); g.lineTo(bx, RULER); g.stroke();
      if ((b - 1) % every === 0 && b < lastBar) g.fillText(String(b), bx + 3, RULER / 2);
    }

    // the section blocks, each with a mini row per part
    const top = RULER + 4;
    for (const s of sections) {
      const x0 = x(s.from) + 1; const x1 = x(s.to) - 2;
      g.fillStyle = s.colour?.hex || '#3a3a3a';
      roundRect(g, x0, top, x1 - x0, blockH, 4); g.fill();
      g.save(); g.beginPath(); g.rect(x0, top, x1 - x0, blockH); g.clip();
      g.font = font(11, 600);
      const tag = s.id.toUpperCase();
      const tw = g.measureText(tag).width + 8;
      g.fillStyle = s.colour?.chip || 'rgba(255,255,255,.35)';
      roundRect(g, x0 + 5, top + 5, tw, 15, 3); g.fill();
      g.fillStyle = '#000'; g.fillText(tag, x0 + 9, top + 12.5);
      g.fillStyle = '#fff'; g.font = font(12);
      g.fillText(s.name, x0 + tw + 10, top + 12.5);
      const nameW = g.measureText(s.name).width;
      g.fillStyle = 'rgba(255,255,255,.65)'; g.font = font(11);
      g.fillText(`bars ${s.startBar}–${s.startBar + s.bars - 1}${s.under ? ` · under ${s.under}` : ''}`, x0 + tw + 16 + nameW, top + 12.5);
      // notes, small: x by time, y by pitch within the part's own range
      const rowsOf = [...shown.map((c) => [c]), ...(folded.length ? [folded] : [])];
      rowsOf.forEach((chs, r) => {
        const ry = top + BLOCK_HEAD + r * (ROW + ROW_GAP);
        g.fillStyle = 'rgba(0,0,0,.18)';
        g.fillRect(x0, ry, x1 - x0, ROW);
        g.fillStyle = 'rgba(255,255,255,.85)';
        for (const c of chs) {
          const [lo, hi] = range.get(c.id);
          const span = Math.max(1, hi - lo);
          for (const n of s.notes[c.id] || []) {
            const nx = x(ax.tickSec(s.startTick + n.t));
            const nw = Math.max(1.5, (ax.tickSec(s.startTick + n.t + n.len) - ax.tickSec(s.startTick + n.t)) * pps - 0.5);
            const ny = ry + 3 + (1 - (n.key - lo) / span) * (ROW - 8);
            g.fillRect(nx, ny, nw, 2);
          }
        }
      });
      g.restore();
    }
    // part names on the left
    g.font = font(11); g.fillStyle = muted;
    shown.forEach((c, r) => g.fillText(ellipsis(g, c.name, LABELS - 6), 0, top + BLOCK_HEAD + r * (ROW + ROW_GAP) + ROW / 2));
    if (folded.length) g.fillText(`+${folded.length} more`, 0, top + BLOCK_HEAD + shown.length * (ROW + ROW_GAP) + ROW / 2);

    // narration: one thin row
    g.fillStyle = muted; g.font = font(11);
    g.fillText('Voice', 0, voiceY + VOICE / 2);
    for (const l of lines) {
      const lx = x(l.start_sec); const lw = Math.max(3, (l.end_sec - l.start_sec) * pps - 1);
      g.fillStyle = css(el, '--clip-audio', '#2f6f4f');
      roundRect(g, lx, voiceY, lw, VOICE, 3); g.fill();
      g.fillStyle = '#fff'; g.font = font(10);
      g.save(); g.beginPath(); g.rect(lx, voiceY, lw, VOICE); g.clip();
      g.fillText(l.label, lx + 4, voiceY + VOICE / 2); g.restore();
    }

    // cut marks: dashed, coloured by whether the music lands on the cut
    g.setLineDash([3, 3]);
    for (const c of cuts) {
      const cx = Math.round(x(c.at)) + 0.5;
      const colour = c.kind === 'hit' ? warn : c.kind === 'miss' ? bad : faint;
      g.strokeStyle = colour; g.lineWidth = c.kind === 'cut' ? 1 : 2;
      g.beginPath(); g.moveTo(cx, top); g.lineTo(cx, cutsY + CUTS); g.stroke();
      g.fillStyle = colour; g.font = font(10);
      g.fillText(c.text, cx + 4, cutsY + CUTS / 2);
    }
    g.setLineDash([]); g.lineWidth = 1;
  }

  const ro = new ResizeObserver(() => draw());
  ro.observe(wrap);
  el.lane = { draw, sections, cuts, shots, end, stop: () => ro.disconnect() };
  return el;
}

function roundRect(g, x, y, w, h, r) {
  g.beginPath();
  g.moveTo(x + r, y); g.lineTo(x + w - r, y); g.quadraticCurveTo(x + w, y, x + w, y + r);
  g.lineTo(x + w, y + h - r); g.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  g.lineTo(x + r, y + h); g.quadraticCurveTo(x, y + h, x, y + h - r);
  g.lineTo(x, y + r); g.quadraticCurveTo(x, y, x + r, y); g.closePath();
}

function ellipsis(g, text, max) {
  if (g.measureText(text).width <= max) return text;
  let t = text;
  while (t.length > 1 && g.measureText(`${t}…`).width > max) t = t.slice(0, -1);
  return `${t}…`;
}
