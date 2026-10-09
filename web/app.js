// Mortiflix Music studio (web). No framework: a tiny DOM builder, hash routes, and live updates over SSE.
// Everything the studio or a session wrote is inserted as text, never as HTML.

import * as player from './player.js';
import { h, fill, add } from './dom.js';
import { instrumentStrip, noteEditor, colorsFor } from './seq.js';

const $app = document.getElementById('app');
const $status = document.getElementById('top-status');
let studio = null;
let projectsCache = [];
let current = { name: null, refresh: null };
let leaveView = null; // a view that holds something open releases it here

// ---------- helpers ----------

async function api(path, { method = 'GET', body, raw } = {}) {
  const opts = { method, headers: {} };
  if (method !== 'GET') opts.headers['x-mortiflix'] = '1';
  if (raw) { opts.body = raw; opts.headers['content-type'] = 'application/octet-stream'; }
  else if (body !== undefined) { opts.body = JSON.stringify(body); opts.headers['content-type'] = 'application/json'; }
  const res = await fetch(path, opts);
  const data = await res.json().catch(() => ({}));
  if (!res.ok) throw Object.assign(new Error(data.error || `HTTP ${res.status}`), { status: res.status, data });
  return data;
}

function toast(text, bad = false) {
  const t = h('div', { class: `toast${bad ? ' bad' : ''}`, role: 'status' }, text);
  add(document.body, t);
  setTimeout(() => t.remove(), bad ? 6000 : 2600);
}

const ago = (iso) => {
  if (!iso) return '';
  const s = (Date.now() - new Date(iso).getTime()) / 1000;
  if (s < 60) return 'just now';
  if (s < 3600) return `${Math.round(s / 60)} min ago`;
  if (s < 86400) return `${Math.round(s / 3600)} h ago`;
  return new Date(iso).toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
};
const clock = (sec) => `${Math.floor(sec / 60)}:${String(Math.floor(sec % 60)).padStart(2, '0')}`;
const fileUrl = (id, file, download = false) => `/files/${id}/${file.split('/').map(encodeURIComponent).join('/')}${download ? '?download' : ''}`;
const store = {
  get(k) { try { return JSON.parse(localStorage.getItem(k)); } catch { return null; } },
  set(k, v) { try { localStorage.setItem(k, JSON.stringify(v)); } catch { /* private mode */ } },
  del(k) { try { localStorage.removeItem(k); } catch { /* private mode */ } },
};
const RATINGS = { love: 'Love it', fine: 'It\'s fine', no: 'Not for me' };
const cap = (s) => s.charAt(0).toUpperCase() + s.slice(1);
const BACKEND_NAMES = { 'claude-code': 'Claude Code', 'anthropic-api': 'Claude API', demo: 'Demo' };
const ICONS = { approved: ['✓', 'done'], done: ['✓', 'done'], skipped: ['–', ''], in_review: ['●', 'review'], changes: ['↺', 'changes'], working: ['…', ''], ready: ['○', ''], blocked: ['·', ''] };
const STATE_WORDS = { approved: 'approved', done: 'done', skipped: 'skipped', in_review: 'waiting for you', changes: 'changes asked', working: 'in progress', ready: 'up next', blocked: 'later' };
const icon = (state) => { const [g, c] = ICONS[state] || ['·', '']; return h('span', { class: `ico ${c}`, 'aria-hidden': 'true' }, g); };

// ---------- routing ----------

const routes = [
  [/^$/, studioView, 'studio'],
  [/^new(?:\/([a-z0-9_-]+))?$/, newView, 'new'],
  [/^p\/([a-z0-9-]+)$/, projectView, 'studio'],
  [/^p\/([a-z0-9-]+)\/review\/([a-z0-9_-]+)(?:\/(\d+))?$/, reviewView, 'studio'],
  [/^flows$/, flowsView, 'flows'],
  [/^pipelines$/, pipelinesView, 'pipelines'],
  [/^settings$/, settingsView, 'settings'],
];

async function render() {
  const path = location.hash.replace(/^#\/?/, '');
  const hit = routes.find(([re]) => re.test(path));
  const [re, view, nav] = hit || routes[0];
  for (const a of document.querySelectorAll('[data-nav]')) a.classList.toggle('on', a.dataset.nav === nav);
  if (leaveView) { try { leaveView(); } catch { /* already closed */ } leaveView = null; }
  unsavedNotes = 0;
  current = { name: view.name, refresh: null, args: path.match(re)?.slice(1) || [] };
  // The review room is wider; the header widens with it so both keep one left edge.
  document.body.classList.toggle('wide', view === reviewView);
  try {
    await view(...current.args);
  } catch (e) {
    fill($app, h('div', { class: 'empty' }, h('div', { class: 'big' }, 'Something went wrong'), e.message));
  }
}

function mount(...kids) {
  fill($app, ...kids);
}

window.addEventListener('hashchange', () => { window.scrollTo(0, 0); render(); });

// ---------- live updates ----------

let refreshTimer = null;
function connect() {
  const es = new EventSource('/api/events');
  es.addEventListener('change', () => {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(async () => {
      await loadStudio();
      if (current.refresh) current.refresh();
    }, 150);
  });
  es.addEventListener('activity', (e) => {
    const a = JSON.parse(e.data);
    const log = document.getElementById(`log-${a.project}`);
    if (log) appendLog(log, a);
  });
}

async function loadStudio() {
  try {
    studio = await api('/api/studio');
    projectsCache = await api('/api/projects');
  } catch { return; }
  const working = studio.runner ? projectsCache.find((p) => p.id === studio.runner.project) : null;
  fill($status,
    working ? h('span', { class: 'pulse', 'aria-hidden': 'true' }) : null,
    working ? h('span', null, `Working on ${working.title}`) : null,
    h('span', { class: 'chip' }, BACKEND_NAMES[studio.config.backend] || studio.config.backend),
  );
}

// ---------- the library (home) ----------

// Every piece you've made, newest first, each with a sketch of its notes and a play button; what's waiting on you
// stays on top.
async function studioView() {
  const projects = projectsCache = await api('/api/projects');
  const turn = projects.filter((p) => p.state === 'waiting' || p.state === 'paused');
  const pieces = projects.filter((p) => !['draft', 'cancelled'].includes(p.state));
  const other = projects.filter((p) => p.state === 'draft' || p.state === 'cancelled');
  const kinds = [...new Set(pieces.map((p) => p.pipeline.name))];
  let kind = store.get('library-kind');
  if (!kinds.includes(kind)) kind = null;

  const turnRow = (p, first) => {
    const what = p.state === 'paused'
      ? `Needs you: ${p.needs_you?.text || 'paused'}`
      : p.in_review.length ? `${p.in_review.map((s) => `${s.name} v${s.version}`).join(', ')} ready to review`
        : p.open_questions ? 'A question for you' : 'Waiting for you';
    const to = p.in_review.length ? `#/p/${p.id}/review/${p.in_review[0].key}` : `#/p/${p.id}`;
    return h('li', { class: 'row turn' },
      h('span', { class: `ico ${p.state === 'paused' ? 'bad' : 'review'}` }, p.state === 'paused' ? '!' : '●'),
      h('div', { class: 'main' }, h('a', { class: 'title', href: `#/p/${p.id}` }, p.title), h('div', { class: 'meta' }, what)),
      h('div', { class: 'end' }, h('a', { class: `btn${first ? ' primary' : ''}`, href: to }, p.state === 'paused' ? 'Open' : 'Review')));
  };

  const pieceRow = (p) => {
    const m = p.music || {};
    const playable = m.notes || m.master;
    const sketch = h('div', { class: 'sketch' });
    if (m.notes) notesFor(p).then((n) => fill(sketch, miniRoll(n, p.id))).catch(() => {});
    const end = p.state === 'delivered' ? [m.seconds ? clock(m.seconds) : null, h('span', { class: 'quiet' }, ago(p.delivered_at))]
      : p.state === 'queued' ? [p.working ? h('span', { class: 'pulse', 'aria-hidden': 'true' }) : null, `${p.done}/${p.total}`]
        : turn.includes(p) ? h('span', null, 'waiting for you') : p.state;
    return h('li', { class: 'row piece' },
      playable ? playButton(p) : h('span', { class: 'play-slot' }, p.working ? h('span', { class: 'pulse' }) : icon(p.state === 'delivered' ? 'done' : 'ready')),
      h('div', { class: 'main' }, h('a', { class: 'title', href: `#/p/${p.id}` }, p.title),
        h('div', { class: 'meta' }, [p.pipeline.name, p.genre, m.bpm ? `${m.bpm} BPM` : null, p.flow].filter(Boolean).join(' · '))),
      sketch,
      h('div', { class: 'end' }, end));
  };

  const listBox = h('ul', { class: 'rows' });
  const drawList = () => fill(listBox, pieces.filter((p) => !kind || p.pipeline.name === kind).map(pieceRow));
  drawList();

  mount(
    h('div', { class: 'head' }, h('h1', null, 'Library'), h('div', { class: 'actions' }, h('a', { class: 'btn solid', href: '#/new' }, 'New piece'))),
    projects.length ? null : h('section', null, h('div', { class: 'empty' },
      h('div', { class: 'big' }, 'Nothing here yet.'),
      h('p', null, 'Pick what to make, write the brief, and approve each stage as it comes in. Everything you make lands here, ready to play.'),
      h('div', { class: 'actions' }, h('a', { class: 'btn solid', href: '#/new' }, 'Make a song'),
        h('button', { class: 'btn', onclick: startDemo }, 'Try the demo'), h('span', { class: 'meta' }, 'a song with placeholder work, free')))),
    turn.length ? h('section', null, h('h2', null, 'Your turn', h('span', { class: 'count' }, turn.length)), h('ul', { class: 'rows' }, turn.map((p, i) => turnRow(p, i === 0)))) : null,
    pieces.length ? h('section', null, h('h2', null, 'Your music', h('span', { class: 'count' }, pieces.length)),
      kinds.length > 1 ? h('div', { class: 'filter' }, choiceChips(['All', ...kinds], kind || 'All', (v) => { kind = v === 'All' ? null : v; store.set('library-kind', kind); drawList(); })) : null,
      listBox) : null,
    other.length ? h('section', null, h('details', null, h('summary', null, `Drafts and cancelled (${other.length})`), h('ul', { class: 'rows' }, other.map((p) => h('li', { class: 'row' }, icon('blocked'),
      h('div', { class: 'main' }, h('a', { class: 'title', href: `#/p/${p.id}` }, p.title)), h('div', { class: 'end' }, p.state)))))) : null,
  );
  current.refresh = () => studioView();
}

// A piece's notes, fetched once per change to the project.
const notesCache = new Map();
function notesFor(p) {
  const key = `${p.id}@${p.updated_at}`;
  if (!notesCache.has(key)) notesCache.set(key, api(`/api/projects/${p.id}/music/notes`).catch((e) => { notesCache.delete(key); throw e; }));
  return notesCache.get(key);
}

// A play/pause button for one thing the player can play. `loadIt` puts it on the player the first time.
function playToggle(pid, title, loadIt) {
  const b = h('button', { type: 'button', class: 'play', 'aria-label': `Play ${title}`, onclick: async () => {
    if (player.state().id !== pid) {
      try { await loadIt(); } catch (e) { return toast(e.message, true); }
    }
    player.toggle();
  } }, playGlyph(false));
  watch({ id: pid, el: b, update: (s) => {
    const on = s.id === pid && s.playing;
    if (b.dataset.on === String(on)) return;
    b.dataset.on = String(on); fill(b, playGlyph(on)); b.setAttribute('aria-label', `${on ? 'Pause' : 'Play'} ${title}`);
  } });
  return b;
}

// Play a piece from the library: your master when you've imported one, otherwise its MIDI on the built-in synth.
// The ids match the ones the project page plays them under.
function playButton(p) {
  const href = `#/p/${p.id}`;
  if (p.music.master) return audioButton({ id: `${p.id}/master`, title: `${p.title} · your master`, href, url: `/api/projects/${p.id}/music/master?v=${encodeURIComponent(p.updated_at)}` });
  return playToggle(p.id, p.title, async () => player.load({ id: p.id, title: p.title, href, piece: await notesFor(p) }));
}

// An audio file (a master, a delivered mix) on the player bar.
const audioButton = ({ id, title, href, url }) => playToggle(id, title, () => player.loadAudio({ id, title, href, url }));

// A MIDI file on the player bar, read in the browser.
const midiButton = ({ id, title, href, url }) => playToggle(id, title, async () => {
  let piece;
  try { piece = readMidi(new Uint8Array(await (await fetch(url)).arrayBuffer())); } catch (e) { throw new Error(`Couldn't read this MIDI file: ${e.message}`); }
  player.load({ id, title, href, piece });
});

// The library's little piano roll: every note in faint ink, drums left out, and the playhead while it plays.
function miniRoll(piece, pid) {
  const tracks = (piece.tracks || []).filter((t) => t.notes?.length && !(t.drums || t.kind === 'drums' || t.channel === 10));
  const all = tracks.flatMap((t) => t.notes);
  const wrap = h('div', { class: 'mini' });
  if (!all.length) return wrap;
  const bpb = piece.beatsPerBar || piece.beats_per_bar || 4;
  const end = Math.ceil(Math.max(...all.map((n) => n.start + n.dur), ...(piece.tracks || []).flatMap((t) => t.notes.map((n) => n.start + n.dur))) / bpb) * bpb;
  const lo = Math.min(...all.map((n) => n.pitch)) - 1; const hi = Math.max(...all.map((n) => n.pitch)) + 1;
  const canvas = h('canvas', { 'aria-hidden': 'true' });
  const head = h('div', { class: 'playhead', hidden: true });
  add(wrap, canvas, head);
  const draw = () => {
    const w = canvas.clientWidth || 200; const ht = canvas.clientHeight || 34; const dpr = window.devicePixelRatio || 1;
    canvas.width = w * dpr; canvas.height = ht * dpr;
    const g = canvas.getContext('2d'); g.scale(dpr, dpr);
    g.fillStyle = getComputedStyle(document.documentElement).getPropertyValue('--muted').trim();
    const row = ht / (hi - lo + 1);
    for (const n of all) g.fillRect((n.start / end) * w, ht - (n.pitch - lo + 1) * row, Math.max(1, (n.dur / end) * w - 0.5), Math.max(1.5, row));
  };
  requestAnimationFrame(draw);
  watch({ id: pid, el: wrap, update: (s) => playheadAt(head, s, pid) });
  return wrap;
}

// Playheads and play buttons follow the player: one subscription, each element updated while it's on the page.
const watchers = new Set();
function watch(w) { watchers.add(w); w.update(player.state()); }
function playheadAt(el, s, pid) {
  const on = s.id === pid && s.duration > 0 && (s.playing || s.time > 0);
  el.hidden = !on;
  if (on) el.style.left = `${Math.min(100, (s.time / s.duration) * 100)}%`;
}

const SVG = 'http://www.w3.org/2000/svg';
function playGlyph(pausing) {
  const svg = document.createElementNS(SVG, 'svg');
  svg.setAttribute('viewBox', '0 0 16 16'); svg.setAttribute('width', '14'); svg.setAttribute('height', '14'); svg.setAttribute('aria-hidden', 'true');
  const path = document.createElementNS(SVG, 'path');
  path.setAttribute('d', pausing ? 'M4 2.5h3v11H4zM9 2.5h3v11H9z' : 'M4.5 2.2v11.6L13.6 8z');
  path.setAttribute('fill', 'currentColor');
  svg.append(path);
  return svg;
}

// The player bar: what's playing, its section, a scrubber with the sections marked, and the time. It stays put across
// pages; Space plays and pauses (outside text fields).
function playerBar() {
  const bar = document.getElementById('playerbar');
  const btn = h('button', { type: 'button', class: 'play big', 'aria-label': 'Play', onclick: () => player.toggle() }, playGlyph(false));
  const title = h('a', { class: 'pb-title' });
  const section = h('span', { class: 'pb-section' });
  const scrub = h('input', { type: 'range', min: 0, max: 1000, value: 0, step: 1, 'aria-label': 'Position', class: 'pb-scrub' });
  const ticks = h('div', { class: 'pb-ticks', 'aria-hidden': 'true' });
  const time = h('span', { class: 'pb-time' });
  const x = h('button', { type: 'button', class: 'pb-x', 'aria-label': 'Close the player', onclick: () => player.close() }, '×');
  let dragging = false;
  scrub.addEventListener('input', () => { dragging = true; const s = player.state(); time.textContent = `${clock((scrub.value / 1000) * s.duration)} / ${clock(s.duration)}`; });
  scrub.addEventListener('change', () => { dragging = false; player.seek((scrub.value / 1000) * player.state().duration); });
  add(bar, h('div', { class: 'pb-in' }, btn, h('div', { class: 'pb-what' }, title, section), h('div', { class: 'pb-track' }, ticks, scrub), time, x));
  let shown = null; let wasPlaying = null; let markKey = '';
  player.on((s) => {
    bar.hidden = !s.id;
    document.body.classList.toggle('has-player', Boolean(s.id));
    if (s.id) {
      if (shown !== s.id) { shown = s.id; title.textContent = s.title; if (s.href) title.href = s.href; else title.removeAttribute('href'); }
      if (wasPlaying !== s.playing) { wasPlaying = s.playing; fill(btn, playGlyph(s.playing)); btn.setAttribute('aria-label', s.playing ? 'Pause' : 'Play'); }
      section.textContent = s.section ? `· ${s.section}` : '';
      const key = `${s.id}:${s.duration}`;
      if (markKey !== key) { markKey = key; fill(ticks, s.duration ? s.markers.filter((m) => m.sec > 0).map((m) => h('span', { style: { left: `${(m.sec / s.duration) * 100}%` }, title: m.text })) : []); }
      if (!dragging) { scrub.value = s.duration ? Math.round((s.time / s.duration) * 1000) : 0; time.textContent = `${clock(s.time)} / ${clock(s.duration)}`; }
    } else shown = null;
    for (const w of watchers) { if (!w.el.isConnected) watchers.delete(w); else w.update(s); }
  });
  document.addEventListener('keydown', (e) => {
    if (e.key !== ' ' || e.repeat || !player.state().id) return;
    if (e.target.closest('input, textarea, select, button, a, [contenteditable], [role=radio]')) return;
    e.preventDefault();
    player.toggle();
  });
}

async function startDemo() {
  try {
    const { id } = await api('/api/projects', { method: 'POST', body: { pipeline: 'song', title: 'Demo song', answers: { genre: 'trip hop', topic: 'A night drive through a city that never sleeps', mood: 'hazy' }, backend: 'demo' } });
    await api(`/api/projects/${id}/start`, { method: 'POST' });
    location.hash = `#/p/${id}`;
  } catch (e) { toast(e.message, true); }
}

// The MIDI panel: the arrangement as a piano roll, the MIDI pack to download, and (when you finish it in your own
// DAW) the place to import your master and see how it fits the score.
function musicPanel(id, ms, title) {
  const own = ms.own; const e = ms.expect;
  const waiting = ms.finish === 'own' && !own;
  const input = h('input', { type: 'file', accept: '.wav,.aif,.aiff,.flac,.mp3,.m4a,.ogg,audio/*', style: { display: 'none' }, onchange: async (ev) => {
    const f = ev.target.files[0]; if (!f) return;
    toast(`Importing ${f.name}…`);
    try {
      const r = await api(`/api/projects/${id}/music/master?name=${encodeURIComponent(f.name)}`, { method: 'POST', raw: f });
      toast(r.resumed ? 'Imported: the studio carries on with your master' : 'Imported');
      current.refresh?.();
    } catch (err) { toast(err.data?.errors?.join(' · ') || err.message, true); }
  } });
  const facts = own ? `${own.source}: ${clock(own.measured.seconds)}, ${own.measured.lufs} LUFS, true peak ${own.measured.true_peak} dBTP` : e ? `${e.bars} bars at ${e.bpm} BPM (${e.meter}), ${clock(e.seconds)}${ms.daw ? ` · import steps for ${ms.daw}` : ''}` : '';
  const roll = packBench(id, title);
  return h('section', null, h('h2', null, 'MIDI'),
    h('div', { class: waiting ? 'turn-line' : 'state-line' },
      h('div', null,
        h('div', { class: 'what' }, waiting ? 'Finish it in your DAW' : own ? 'Your master is the music' : 'The MIDI is ready'),
        h('div', { class: 'why' }, waiting ? 'Every channel is its own MIDI file, with a clip per section and the tempo and sections as markers. Give each channel its sound, mix, export from bar 1, then import it here. ' : '', facts)),
      h('div', { class: 'actions' },
        h('a', { class: 'btn', href: `/api/projects/${id}/music/midi.zip` }, 'Download MIDI'),
        ms.masters || own ? [h('button', { class: waiting ? 'btn primary' : 'btn', onclick: () => input.click() }, own ? 'Replace master' : 'Import your master'), input] : null)),
    roll,
    own ? h('div', { class: 'row with-play' }, audioButton({ id: `${id}/master`, title: `${title} · your master`, href: `#/p/${id}`, url: `/api/projects/${id}/music/master?v=${encodeURIComponent(own.imported_at)}` }),
      h('div', { class: 'main' }, h('span', { class: 'title' }, 'Your master')), h('div', { class: 'end' }, clock(own.measured.seconds))) : null,
    own?.warnings?.length ? h('ul', { class: 'meta' }, own.warnings.map((w) => h('li', null, w))) : null);
}

// The piece's MIDI on the project page: the instruments, the whole piece and its notes, which can be changed by hand
// while the piece waits for you (src/edits.mjs, step 'pack'). The page redraws on every studio change, so the bench is
// kept and handed back while its notes are the same: a redraw never drops changes you haven't put in yet.
const benches = new Map(); // project id → { el, stale }
const packStates = new Map(); // project id → the note editor's copy
function packBench(id, title) {
  const kept = benches.get(id);
  if (kept && !kept.stale) return kept.el;
  const el = h('div', { class: 'roll-wrap' }, h('div', { class: 'meta' }, 'Reading the notes…'));
  const entry = { el, stale: false };
  benches.set(id, entry);
  const play = { id, title, href: `#/p/${id}` };
  Promise.all([api(`/api/projects/${id}/music/notes`), api(`/api/projects/${id}/music/edits`).catch(() => null)]).then(([piece, info]) => {
    const est = packStates.get(id) || {};
    packStates.set(id, est);
    const can = Boolean(info?.can && info.mode === 'pack');
    const wb = midiWorkbench({ id, piece, play, info, step: 'pack', version: 0, handEditable: can, est,
      after: async (what, bar) => {
        entry.stale = true;
        if (what !== 'keep') {
          packStates.delete(id);
          const fresh = await api(`/api/projects/${id}/music/notes`).catch(() => null);
          if (fresh) { player.load({ ...play, piece: fresh }, { force: true }); player.seek(Math.max(0, ((bar - 2) * (fresh.beats_per_bar || 4) * 60) / (fresh.bpm || 120))); player.play(); }
        }
        const next = packBench(id, title);
        el.replaceWith(next);
      } });
    fill(el, can ? null : h('p', { class: 'meta' }, info?.mode === 'review' ? 'Change the notes in the review room while it waits for you.' : 'The notes can be changed here while the piece waits for you (not while a session is writing it).'), wb.el);
  }).catch(() => el.remove());
  return el;
}

// ---------- a project ----------

async function projectView(id) {
  const d = await api(`/api/projects/${id}`);
  const p = d.project;
  const reviewing = d.steps.filter((s) => s.state === 'in_review');
  const openQs = p.questions.filter((q) => !q.answered_at);
  const currentStep = d.steps.find((s) => ['working', 'changes', 'ready'].includes(s.state));
  const act = (path, msg) => async () => { try { await api(`/api/projects/${id}/${path}`, { method: 'POST' }); toast(msg); } catch (e) { toast(e.message, true); } };

  let stateBlock;
  if (p.state === 'paused') {
    stateBlock = h('div', { class: 'needs' }, h('div', { class: 'what' }, p.needs_you?.by_you ? 'Paused.' : `Needs you: ${p.needs_you?.text || ''}`),
      h('div', { class: 'actions', style: { marginTop: '10px' } }, h('button', { class: 'btn solid', onclick: act('resume', 'Resumed') }, 'Resume')));
  } else if (reviewing.length) {
    const s = reviewing[0];
    const sub = d.submissions.filter((x) => x.step === s.key).at(-1);
    stateBlock = h('div', { class: 'turn-line' },
      h('div', null, h('div', { class: 'what' }, `${s.name} v${s.version} is ready for you`), h('div', { class: 'why' }, (sub?.note || '').slice(0, 160))),
      h('a', { class: 'btn primary', href: `#/p/${id}/review/${s.key}` }, 'Review'));
  } else if (openQs.length) {
    stateBlock = h('div', { class: 'turn-line' }, h('div', null, h('div', { class: 'what' }, openQs.length === 1 ? 'A question for you' : `${openQs.length} questions for you`), h('div', { class: 'why' }, 'Answer below; the studio carries on as soon as you do.')));
  } else if (p.state === 'queued') {
    stateBlock = h('div', { class: 'state-line' }, p.session?.running ? h('span', { class: 'pulse' }) : icon('ready'),
      p.session?.running ? `Claude is working on ${currentStep?.name || 'it'}` : 'Waiting for its turn',
      p.status ? h('span', { class: 'quiet' }, `· ${p.status.text}`) : null);
  } else if (p.state === 'delivered') {
    stateBlock = h('div', { class: 'state-line' }, icon('done'), `Delivered ${ago(p.delivered_at)}`);
  } else if (p.state === 'draft') {
    stateBlock = h('div', { class: 'state-line' }, 'Draft: not started.', h('button', { class: 'btn solid', onclick: act('start', 'Started') }, 'Start'));
  } else {
    stateBlock = h('div', { class: 'state-line' }, h('span', { class: 'quiet' }, 'Cancelled.'));
  }

  const ms = await api(`/api/projects/${id}/music`).catch(() => null);
  const musicBlock = ms?.midi?.length ? musicPanel(id, ms, p.title) : null;

  const deliverables = p.deliverables.length ? h('section', { class: 'deliver' }, h('h2', null, 'Delivered'),
    h('ul', { class: 'rows' }, p.deliverables.map((f) => h('li', { class: 'row' },
      f.kind === 'audio' ? audioButton({ id: `${id}/${f.file}`, title: `${p.title} · ${f.label}`, href: `#/p/${id}`, url: fileUrl(id, f.file) })
        : f.kind === 'midi' ? midiButton({ id: `${id}/${f.file}`, title: `${p.title} · ${f.label}`, href: `#/p/${id}`, url: fileUrl(id, f.file) }) : icon('done'),
      h('div', { class: 'main' }, h('span', { class: 'title' }, f.label)), h('div', { class: 'end' }, h('a', { href: fileUrl(id, f.file, true) }, 'Download')))))) : null;

  const questions = openQs.length ? h('section', null, h('h2', null, 'Questions'), openQs.map((q) => questionForm(id, q))) : null;

  // How it landed: a rating teaches the studio your taste; a piece you love can become a flow of your own.
  const tasteBlock = p.flow && (p.state === 'delivered' || p.rating) ? tasteSection(id, p, Boolean(ms?.expect)) : null;

  // The MIDI, once the score passes its check; and, when the brief chose to finish it in the owner's own DAW, the
  // place to import that master.
  const subsByStep = (key) => d.submissions.filter((s) => s.step === key);
  // Every step that has made something opens to show it: what Claude says it made, the summary, and the work itself.
  const openKey = `steps-open-${id}`;
  const opened = new Set(store.get(openKey) || []);
  const hasWork = (s) => ['approved', 'done', 'in_review', 'changes', 'working'].includes(s.state) || subsByStep(s.key).length;
  const steps = h('section', null, h('h2', null, 'Steps', h('span', { class: 'count' }, `${d.steps.filter((s) => ['approved', 'done', 'skipped'].includes(s.state)).length}/${d.steps.length}`)),
    h('ul', { class: 'rows' }, d.steps.map((s) => {
      const panel = h('li', { class: 'made-row', hidden: !opened.has(s.key) });
      if (opened.has(s.key)) fill(panel, stepMade(id, s.key));
      const toggle = hasWork(s) ? h('button', { type: 'button', class: 'btn link', 'aria-expanded': String(opened.has(s.key)), onclick: (e) => {
        const open = !opened.has(s.key);
        if (open) { opened.add(s.key); fill(panel, stepMade(id, s.key)); } else opened.delete(s.key);
        panel.hidden = !open; e.currentTarget.setAttribute('aria-expanded', String(open)); e.currentTarget.textContent = open ? 'Hide' : 'What it made';
        store.set(openKey, [...opened]);
      } }, opened.has(s.key) ? 'Hide' : 'What it made') : null;
      return [h('li', { class: 'row' }, icon(s.state),
        h('div', { class: 'main' }, h('span', { class: 'title' }, s.name),
          h('div', { class: 'meta' }, s.review === 'internal' ? 'made in the studio' : `you review ${s.review === 'questions' ? 'questions' : s.review === 'midi' ? 'the MIDI' : `the ${s.review}`}`,
            subsByStep(s.key).length ? ' · ' : '', subsByStep(s.key).map((x, i) => [i ? ' ' : '', h('a', { href: `#/p/${id}/review/${s.key}/${x.version}` }, `v${x.version}`)]))),
        h('div', { class: 'end' }, toggle, s.state === 'in_review' ? h('a', { href: `#/p/${id}/review/${s.key}` }, 'Review') : h('span', null, STATE_WORDS[s.state]))), panel];
    })));

  const log = h('div', { class: 'log', id: `log-${id}` });
  for (const a of d.activity.slice(-150)) appendLog(log, a, false);
  const activity = h('section', null, h('h2', null, 'Activity', d.renders.length ? h('span', { class: 'count' }, d.renders.map((r) => `${r.label}: ${r.state}`).join(' · ')) : null),
    d.activity.length ? log : h('div', { class: 'empty' }, 'Nothing yet. What the studio does shows up here as it happens.'));

  const usage = p.usage;
  const brief = h('section', null, h('details', { 'data-key': 'brief' }, h('summary', null, 'Brief'),
    h('ul', { class: 'rows' }, d.pipeline.intake.map((q) => h('li', { class: 'row', style: { gridTemplateColumns: '200px 1fr' } },
      h('span', { class: 'meta' }, q.label || q.id),
      h('span', { style: { whiteSpace: 'pre-wrap' } }, q.type === 'files' ? (p.intake.files.filter((f) => f.field === q.id).map((f) => f.name).join(', ') || '-') : (p.intake.answers[q.id] || '-')))))));
  const events = h('section', null, h('details', { 'data-key': 'log' }, h('summary', null, `Project log (${d.events.length})`),
    h('ul', { class: 'rows events' }, d.events.slice().reverse().map((e) => h('li', { class: 'row' },
      h('span', { class: 'meta' }, new Date(e.t).toLocaleString(undefined, { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' })),
      h('span', null, h('b', null, e.event.toLowerCase().replace(/_/g, ' ')), e.step ? ` ${e.step}${e.version ? ` v${e.version}` : ''}` : '', e.details ? h('span', { class: 'meta' }, ` · ${e.details}`) : null))))));
  const journal = d.journal ? h('section', null, h('details', { 'data-key': 'journal' }, h('summary', null, 'Studio journal'), h('pre', { class: 'journal' }, d.journal))) : null;

  const actions = h('div', { class: 'actions' },
    ['queued', 'waiting'].includes(p.state) ? h('button', { class: 'btn', onclick: act('pause', 'Paused') }, 'Pause') : null,
    !['delivered', 'cancelled'].includes(p.state) ? h('button', { class: 'btn link danger', onclick: async () => { if (confirmInline(actions, 'Cancel this project?')) await act('cancel', 'Cancelled')(); } }, 'Cancel') : null);

  const open = new Set([...document.querySelectorAll('details[open][data-key]')].map((x) => x.dataset.key));
  mount(
    h('a', { class: 'crumb', href: '#/' }, '← Library'),
    h('div', { class: 'head' }, h('div', null, h('h1', null, p.title), h('div', { class: 'sub' }, h('span', { class: 'chip' }, d.pipeline.name),
      p.flow ? h('a', { class: 'chip', href: '#/flows', title: p.flow.picked === 'you' ? 'Your choice' : `The studio's pick: ${p.flow.why}` }, `Flow: ${p.flow.name}`) : null,
      p.flow?.picked === 'studio' ? h('span', null, `the studio's pick: ${p.flow.why}`) : null,
      h('span', null, p.backend ? `${BACKEND_NAMES[p.backend]} backend` : ''))), actions),
    stateBlock, musicBlock, deliverables, tasteBlock, questions, steps, activity, brief, events, journal,
    h('p', { class: 'meta', style: { color: 'var(--faint)', fontSize: '12px', marginTop: '18px' } },
      `${usage.sessions} session${usage.sessions === 1 ? '' : 's'}${usage.output_tokens ? ` · ${usage.output_tokens.toLocaleString()} output tokens` : ''}${usage.cost_text ? ` · ${usage.cost_text}` : ''}`),
  );
  for (const det of document.querySelectorAll('details[data-key]')) if (open.has(det.dataset.key)) det.open = true;
  log.scrollTop = log.scrollHeight;
  current.refresh = () => projectView(id);
}

// ---------- what a step made ----------

function stepMade(id, key) {
  const box = h('div', { class: 'made' }, h('div', { class: 'meta' }, 'Loading…'));
  api(`/api/projects/${id}/steps/${key}/made`).then((m) => {
    const madeUrl = (f, download = false) => `/made/${id}/${f.split('/').map(encodeURIComponent).join('/')}${download ? '?download' : ''}`;
    const piece = (pc, i) => {
      let body = null;
      if (pc.kind === 'markdown') body = mdView(pc.text || '');
      else if (pc.kind === 'facts') body = h('ul', { class: 'facts' }, pc.facts.map((f) => h('li', null, f)));
      else if (pc.kind === 'links') body = h('ul', { class: 'facts' }, pc.links.map((l) => h('li', null, h('a', { href: l.url, target: '_blank', rel: 'noopener noreferrer' }, l.title))));
      else if (pc.kind === 'table') body = h('table', { class: 'grid' }, h('thead', null, h('tr', null, pc.head.map((x) => h('th', null, x)))), h('tbody', null, pc.rows.map((r) => h('tr', null, r.map((x) => h('td', null, x))))));
      else if (pc.kind === 'code') body = h('pre', { class: 'code' }, pc.text);
      else if (pc.kind === 'audio') body = h('ul', { class: 'rows' }, pc.files.map((f) => h('li', { class: 'row with-play' },
        audioButton({ id: `${id}/made/${f}`, title: `${m.step.name} · ${f.split('/').pop()}`, href: `#/p/${id}`, url: madeUrl(f) }),
        h('div', { class: 'main' }, h('span', { class: 'title' }, f.split('/').pop().replace(/\.(wav|mp3)$/, '').replace(/^\d+-/, ''))), h('div', { class: 'end' }, h('a', { href: madeUrl(f, true) }, 'Download')))));
      else if (pc.kind === 'files') body = h('div', null, pc.zip ? h('p', null, h('a', { class: 'btn', href: `/api/projects/${id}/music/midi.zip` }, 'Download the MIDI pack')) : null,
        h('ul', { class: 'facts files' }, pc.files.map((f) => h('li', null, h('a', { href: madeUrl(f, true) }, f.replace('out/music/midi/', ''))))));
      else if (pc.kind === 'notes') {
        body = h('div', { class: 'roll-wrap' });
        Promise.all([api(`/api/projects/${id}/music/notes`), api(`/api/projects/${id}/music/edits`).catch(() => ({ roles: [] }))]).then(([n, info]) => {
          // The instruments light up as they play, in the same colours as their notes.
          const play = { id, title: m.step.name, href: `#/p/${id}` };
          const tracks = n.tracks.filter((t) => t.notes?.length);
          const colors = colorsFor(tracks);
          let roll = null;
          const strip = instrumentStrip({ tracks, roles: info.roles || [], colors, pid: id, bpb: n.beats_per_bar || 4, load: () => { if (player.state().id !== id) player.load({ ...play, piece: n }); }, onFocus: (ch) => roll.setFocus(ch) });
          roll = pianoRoll(n, { play, colors, legend: false });
          fill(body, strip, roll);
        }).catch(() => body.remove());
      }
      // The first piece is open; long ones (the research, the code) can be opened when wanted.
      return h('details', { class: 'piece', open: i === 0 && pc.kind !== 'code' ? true : null }, h('summary', null, pc.title), body);
    };
    fill(box,
      m.step.describe ? h('p', { class: 'meta what-is' }, m.step.describe) : null,
      m.step.note ? h('blockquote', { class: 'said' }, h('div', { class: 'who' }, 'What Claude says it made'), m.step.note) : null,
      m.summary.length ? h('ul', { class: 'summary' }, m.summary.map((x) => h('li', null, x))) : null,
      m.pieces.map(piece),
      !m.step.note && !m.summary.length && !m.pieces.length ? h('p', { class: 'meta' }, m.step.state === 'done' || m.step.state === 'approved' ? 'This step left nothing to read in the project folder.' : 'Nothing yet: it shows here as soon as the step has made something.') : null,
      m.step.checks.length ? h('details', { class: 'piece' }, h('summary', null, `Studio checks: ${m.step.checks.filter((c) => c.result === 'pass').length} of ${m.step.checks.length} passed`),
        h('ul', { class: 'facts' }, m.step.checks.map((c) => h('li', null, `${c.result === 'pass' ? '✓' : '!'} ${c.id}${c.note ? `: ${c.note}` : ''}`)))) : null,
      m.step.versions.length ? h('p', { class: 'meta' }, 'Sent to you: ', m.step.versions.map((v, i) => [i ? ' · ' : '', h('a', { href: `#/p/${id}/review/${key}/${v}` }, `v${v}`)])) : null);
  }).catch((e) => fill(box, h('p', { class: 'err' }, e.message)));
  return box;
}

// A small Markdown reader for what the studio writes (GENRE.md, BLUEPRINT.md, cue sheets): headings, lists, tables,
// code, bold, code spans and links. It builds elements: nothing is ever inserted as HTML.
function mdView(text) {
  const root = h('div', { class: 'md' });
  const inline = (line) => {
    const out = [];
    const re = /\*\*([^*]+)\*\*|`([^`]+)`|\[([^\]]+)\]\((https?:\/\/[^)\s]+)\)/g;
    let last = 0; let m;
    while ((m = re.exec(line))) {
      if (m.index > last) out.push(line.slice(last, m.index));
      if (m[1]) out.push(h('b', null, m[1])); else if (m[2]) out.push(h('code', null, m[2])); else out.push(h('a', { href: m[4], target: '_blank', rel: 'noopener noreferrer' }, m[3]));
      last = re.lastIndex;
    }
    if (last < line.length) out.push(line.slice(last));
    return out;
  };
  const lines = String(text).replace(/\r/g, '').split('\n');
  for (let i = 0; i < lines.length;) {
    const l = lines[i];
    if (!l.trim()) { i += 1; continue; }
    if (/^```/.test(l)) { const buf = []; i += 1; while (i < lines.length && !/^```/.test(lines[i])) buf.push(lines[i++]); i += 1; add(root, h('pre', { class: 'code' }, buf.join('\n'))); continue; }
    const hd = /^(#{1,4})\s+(.*)$/.exec(l);
    if (hd) { add(root, h(`h${Math.min(4, hd[1].length + 2)}`, null, inline(hd[2]))); i += 1; continue; }
    if (/^\s*\|/.test(l)) {
      const rows = []; while (i < lines.length && /^\s*\|/.test(lines[i])) rows.push(lines[i++]);
      const cells = (r) => r.trim().replace(/^\||\|$/g, '').split('|').map((c) => c.trim());
      const body = rows.filter((r) => !/^\s*\|[\s:|-]+\|\s*$/.test(r));
      add(root, h('table', { class: 'grid' }, h('thead', null, h('tr', null, cells(body[0] || '').map((c) => h('th', null, inline(c))))), h('tbody', null, body.slice(1).map((r) => h('tr', null, cells(r).map((c) => h('td', null, inline(c))))))));
      continue;
    }
    if (/^\s*([-*]|\d+\.)\s+/.test(l)) {
      const ordered = /^\s*\d+\./.test(l); const items = [];
      while (i < lines.length && /^\s*([-*]|\d+\.)\s+/.test(lines[i])) {
        let item = lines[i++].replace(/^\s*([-*]|\d+\.)\s+/, '');
        while (i < lines.length && /^\s{2,}\S/.test(lines[i]) && !/^\s*([-*]|\d+\.)\s+/.test(lines[i])) item += ` ${lines[i++].trim()}`;
        items.push(h('li', null, inline(item)));
      }
      add(root, h(ordered ? 'ol' : 'ul', null, items)); continue;
    }
    const para = []; while (i < lines.length && lines[i].trim() && !/^(#{1,4}\s|```|\s*\||\s*([-*]|\d+\.)\s+)/.test(lines[i])) para.push(lines[i++].trim());
    add(root, h('p', null, inline(para.join(' '))));
  }
  return root;
}

function ratingChips(current, onPick) {
  return choiceChips(Object.values(RATINGS), current ? RATINGS[current] : null, (label) => onPick(Object.keys(RATINGS).find((k) => RATINGS[k] === label)));
}

function tasteSection(id, p, canSave) {
  const save = h('div');
  const openSave = () => {
    let name = ''; const err = h('div', { class: 'err' });
    const input = h('input', { type: 'text', placeholder: `A name, e.g. "${p.title} shape"`, oninput: (e) => { name = e.target.value; } });
    fill(save, h('div', { class: 'field' }, h('label', null, 'Name the flow'), h('div', null, input,
      h('div', { class: 'help' }, 'Its sections\' rise and fall, the techniques it really plays, its motif and ending become a flow you can pick for new pieces (the studio may pick it too).'),
      err, h('div', { class: 'actions', style: { marginTop: '10px' } }, h('button', { class: 'btn solid', onclick: async () => {
        try { const f = await api(`/api/projects/${id}/flow`, { method: 'POST', body: { name } }); toast(`${f.name} is one of your flows`); fill(save, h('p', { class: 'meta' }, `Saved as "${f.name}". Pick it in a new brief, or find it under Flows.`)); } catch (e) { err.textContent = e.message; }
      } }, 'Save flow'), h('button', { class: 'btn link', onclick: () => fill(save, saveLink) }, 'Cancel')))));
    input.focus();
  };
  const saveLink = h('button', { class: 'btn link', onclick: openSave }, 'Make this a flow of your own');
  if (canSave) fill(save, saveLink);
  return h('section', null, h('h2', null, 'How did it land?'),
    ratingChips(p.rating, async (rating) => { try { await api(`/api/projects/${id}/rate`, { method: 'POST', body: { rating } }); toast('Noted: the studio picks your next flows with it'); } catch (e) { toast(e.message, true); } }),
    h('p', { class: 'meta', style: { marginTop: '8px' } }, `It followed "${p.flow.name}". Your ratings steer which flows and techniques the studio reaches for next.`),
    canSave ? save : null);
}

// A second click confirms (no browser dialogs).
function confirmInline(_el, _msg) {
  const now = Date.now();
  if (confirmInline.armed && now - confirmInline.armed < 4000) { confirmInline.armed = 0; return true; }
  confirmInline.armed = now;
  toast('Click again to confirm');
  return false;
}

function appendLog(log, a, scroll = true) {
  const atEnd = log.scrollHeight - log.scrollTop - log.clientHeight < 40;
  const t = new Date(a.t).toLocaleTimeString(undefined, { hour: '2-digit', minute: '2-digit', hourCycle: 'h23' });
  const mark = { text: '│', tool: '›', error: '!', info: '●' }[a.kind] || ' ';
  add(log, h('div', { class: 'l' }, h('span', { class: 't' }, t), h('span', { class: 'k' }, mark), h('span', { class: a.kind }, a.text)));
  if (log.children.length > 400) log.firstChild.remove();
  if (scroll && atEnd) log.scrollTop = log.scrollHeight;
}

function questionForm(id, q) {
  let value = '';
  const input = q.choices
    ? choiceChips(q.choices, null, (v) => { value = v; })
    : h('input', { type: 'text', placeholder: q.default ? `Default: ${q.default}` : 'Your answer', oninput: (e) => { value = e.target.value; } });
  const send = async (useDefault) => {
    try {
      await api(`/api/projects/${id}/questions/${q.id}`, { method: 'POST', body: { answer: useDefault ? null : value || null } });
      toast('Answered');
    } catch (e) { toast(e.message, true); }
  };
  return h('div', { class: 'qa' }, h('div', { class: 'q' }, q.text), input,
    h('div', { class: 'actions', style: { marginTop: '10px' } }, h('button', { class: 'btn solid', onclick: () => send(false) }, 'Answer'),
      q.default ? h('button', { class: 'btn link', onclick: () => send(true) }, `Your call (${q.default})`) : null));
}

function choiceChips(choices, selected, onPick) {
  const wrap = h('div', { class: 'choices', role: 'radiogroup' });
  for (const c of choices) {
    const b = h('button', { type: 'button', class: `choice${c === selected ? ' on' : ''}`, role: 'radio', 'aria-checked': String(c === selected), onclick: () => {
      for (const x of wrap.children) { x.classList.remove('on'); x.setAttribute('aria-checked', 'false'); }
      b.classList.add('on'); b.setAttribute('aria-checked', 'true');
      onPick(c);
    } }, c);
    add(wrap, b);
  }
  return wrap;
}

// ---------- the review room ----------

async function reviewView(id, stepKey, versionParam) {
  const d = await api(`/api/projects/${id}`);
  const step = d.steps.find((s) => s.key === stepKey);
  if (!step) throw new Error(`no step ${stepKey}`);
  const versions = d.submissions.filter((s) => s.step === stepKey);
  if (!versions.length) throw new Error(`${step.name} hasn't been sent yet`);
  const sub = versionParam ? versions.find((v) => v.version === Number(versionParam)) : versions.at(-1);
  if (!sub) throw new Error('no such version');
  const editable = step.state === 'in_review' && step.version === sub.version && !sub.feedback;
  const prev = versions.find((v) => v.version === sub.version - 1);
  const draftKey = `mfx-draft-${id}-${stepKey}-${sub.version}`;
  const draft = (editable && store.get(draftKey)) || { notes: [], answers: {}, overall: '' };
  const save = () => { if (editable) store.set(draftKey, draft); };
  const media = sub.items.map((it, i) => ({ ...it, i })).filter((it) => it.kind !== 'text');
  const texts = sub.items.map((it, i) => ({ ...it, i })).filter((it) => it.kind === 'text');
  let sel = media[0]?.i ?? null;
  let pinMode = false;
  let rollMeta = null; // the MIDI's tempo and meter, once it's read
  let focusCh = null;  // the part picked on the roll: it stays picked when the roll redraws
  const editorStates = new Map(); // MIDI file → the note editor's copy, so a redraw keeps changes not yet in the song
  // Quick edits (src/edits.mjs): whether this version can be changed right here, and the edits made on it.
  let editInfo = media.some((m) => m.kind === 'midi') ? await api(`/api/projects/${id}/music/edits`).catch(() => null) : null;
  const canEdit = () => Boolean(editInfo?.can && editInfo.step === stepKey && editInfo.version === sub.version);
  // A note's edit follows the studio's record of it (kept or undone elsewhere, or in another tab).
  for (const n of draft.notes) delete n._busy;
  for (const n of draft.notes) if (n.edit) { const e = editInfo?.edits.find((x) => x.id === n.edit.id); if (e) n.edit.status = e.status; }

  const stageBox = h('div');
  const notesBox = h('div');
  const docBox = h('div');

  function whereText(n) {
    const it = n.item !== null && n.item !== undefined ? sub.items[n.item] : null;
    const bars = n.bars ? (n.bars[0] === n.bars[1] ? `bar ${n.bars[0]}` : `bars ${n.bars[0]}-${n.bars[1]}`) : null;
    return [it?.label, bars, n.part ? n.part : null, !bars && n.time_sec !== undefined ? `at ${clock(n.time_sec)}` : null, n.x !== undefined ? 'pinned spot' : null, n.paragraph !== undefined ? `paragraph ${n.paragraph + 1}` : null].filter(Boolean).join(' · ') || 'general';
  }

  function addNote(n) {
    if (!editable) return;
    draft.notes.push({ text: '', ...n });
    save();
    drawNotes(true);
    drawStage();
    drawDoc();
  }

  function drawStage({ playFrom = null } = {}) {
    if (sel === null) return fill(stageBox);
    const it = sub.items[sel];
    const url = fileUrl(id, it.file);
    const pins = draft.notes.map((n, k) => ({ ...n, k })).filter((n) => n.item === sel && n.x !== undefined);
    const shown = editable ? pins : (sub.feedback?.notes || []).filter((n) => n.item === sel && n.x !== undefined).map((n) => ({ ...n, k: n.n - 1 }));
    let stage;
    const marks = (filter = () => true) => shown.filter(filter).map((n) => h('span', { class: 'pinmark', style: { left: `${n.x * 100}%`, top: `${n.y * 100}%` } }, n.k + 1));
    const spot = (e, el) => { const r = el.getBoundingClientRect(); return { x: Math.round(((e.clientX - r.left) / r.width) * 1e4) / 1e4, y: Math.round(((e.clientY - r.top) / r.height) * 1e4) / 1e4 }; };
    const tools = h('div', { class: 'stage-tools' });
    if (it.kind === 'image') {
      const img = h('img', { src: url, alt: it.label });
      stage = h('div', { class: `stage${editable ? ' pinning' : ''}` }, img, marks());
      if (editable) stage.addEventListener('click', (e) => addNote({ item: sel, ...spot(e, img) }));
      if (editable) tools.append(h('span', { class: 'meta' }, 'Click the picture to pin a note to that spot.'));
    } else if (it.kind === 'midi') {
      // The MIDI as a piano roll: click a moment to note it (bar and beat, as seconds into the piece).
      stage = h('div', { class: 'roll-wrap' }, h('div', { class: 'meta' }, 'Reading the MIDI…'));
      const pid = `${id}/${it.file}`;
      fetch(`${url}?v=${Date.now()}`).then((r) => r.arrayBuffer()).then((buf) => {
        const midi = readMidi(new Uint8Array(buf));
        const bpb = midi.beatsPerBar || 4;
        rollMeta = { bpm: midi.bpm, bpb };
        const barOf = (sec) => Math.floor((sec * midi.bpm) / 60 / bpb) + 1;
        const list = editable ? draft.notes : sub.feedback?.notes || [];
        const noteEdits = new Set(list.map((n) => n.edit?.id).filter(Boolean));
        const bands = [
          ...(editInfo?.edits || []).filter((e) => e.kind !== 'notes' && e.step === stepKey && e.version === sub.version && e.status !== 'undone' && !noteEdits.has(e.id)).map((e) => ({ from: e.from, to: e.to, label: 'edited', kind: 'edited' })),
          ...list.map((n, k) => ({ n, k })).filter(({ n }) => n.item === sel && (n.bars || n.time_sec !== undefined)).map(({ n, k }) => {
            const [from, to] = n.bars || [barOf(n.time_sec), barOf(n.time_sec)];
            return { from, to, label: String(editable ? k + 1 : n.n), kind: n.edit && n.edit.status !== 'undone' ? 'edited' : 'note', index: editable ? k : undefined };
          }),
        ];
        const playInfo = { id: pid, title: `${d.project.title} · ${media.length > 1 ? it.label : `${step.name} v${sub.version}`}`, href: location.hash };
        // The arrangement can be changed by hand; a section's clip starts at its own bar 1, so it's only read.
        const whole = !/(^|\/)sections\//.test(it.source || it.file);
        const est = editorStates.get(it.file) || {};
        editorStates.set(it.file, est);
        const wb = midiWorkbench({ id, piece: midi, play: playInfo, info: editInfo, step: stepKey, version: sub.version, handEditable: canEdit() && whole, est, focus: focusCh, onFocus: (ch) => { focusCh = ch; }, bands,
          onNote: editable ? ({ part, channel, text, bars }) => addNote({ item: sel, part, channel, text, ...(bars ? { bars, time_sec: Math.round((((bars[0] - 1) * bpb * 60) / midi.bpm) * 100) / 100 } : {}) }) : null,
          onMark: editable ? ({ from, to, sec }) => addNote({ item: sel, bars: [from, to], time_sec: sec, channel: focusCh, part: midi.tracks.find((t) => t.channel === focusCh)?.name || null }) : null,
          onMoveBand: editable ? (k, from, to) => { const n = draft.notes[k]; if (n.edit && n.edit.status !== 'undone') return toast('That note already changed those bars: undo it first'); n.bars = [from, to]; n.time_sec = Math.round((((from - 1) * bpb * 60) / midi.bpm) * 100) / 100; save(); drawNotes(); drawStage(); } : null,
          after: async (what, bar) => { editInfo = await api(`/api/projects/${id}/music/edits`).catch(() => editInfo); if (what !== 'keep') editorStates.clear(); drawStage({ playFrom: what === 'keep' ? null : bar }); } });
        fill(stage, wb.el);
        // After an edit: hear the new bars at once, from a bar before them.
        if (playFrom) {
          player.load({ ...playInfo, piece: midi }, { force: true });
          player.seek(Math.max(0, ((playFrom - 2) * bpb * 60) / midi.bpm));
          player.play();
          wb.editor.goTo(playFrom);
        }
      }).catch((e) => fill(stage, h('div', { class: 'err' }, `Couldn't read this MIDI file: ${e.message}`)));
      if (editable) tools.append(h('button', { class: 'btn', onclick: () => {
        const s = player.state();
        if (s.id !== pid || !rollMeta) return toast('Play it first, then mark the bar you hear');
        const bar = Math.floor((s.time * rollMeta.bpm) / 60 / rollMeta.bpb) + 1;
        addNote({ item: sel, bars: [bar, bar], time_sec: Math.round((((bar - 1) * rollMeta.bpb * 60) / rollMeta.bpm) * 100) / 100 });
      } }, 'Mark the playhead\'s bar'),
        h('span', { class: 'meta' }, canEdit() ? 'or drag across bars on the whole piece. Pick an instrument first to ask Claude to change only that part.' : 'or drag across bars on the whole piece to mark them.'));
    } else if (it.kind === 'audio') {
      const audio = h('audio', { src: url, controls: true, style: { width: '100%' } });
      stage = h('div', { style: { padding: '12px 0' } }, audio);
      if (editable) tools.append(h('button', { class: 'btn', onclick: () => addNote({ item: sel, time_sec: Math.round(audio.currentTime * 100) / 100 }) }, 'Note at this moment'));
    } else {
      stage = h('div', { class: 'empty' }, h('a', { href: url, target: '_blank', rel: 'noopener' }, `Open ${it.label}`));
      if (editable) tools.append(h('button', { class: 'btn', onclick: () => addNote({ item: sel }) }, 'Add a note about this'));
    }
    fill(stageBox,
      media.length > 1 ? h('div', { class: 'thumbs' }, media.map((m) => h('button', { class: `thumb${m.i === sel ? ' on' : ''}`, onclick: () => { sel = m.i; pinMode = false; drawStage(); } },
        m.kind === 'image' ? h('img', { src: fileUrl(id, m.file), alt: '' }) : h('div', { class: 'thumb-glyph' }, m.kind === 'midi' ? '♫' : '♪'),
        h('span', null, m.label)))) : null,
      stage,
      h('div', { class: 'stage-label' }, h('span', null, it.label, it.section ? ` · ${it.section}` : ''), h('a', { href: fileUrl(id, it.file, true) }, 'Download')),
      tools);
  }

  function drawDoc() {
    fill(docBox, ...texts.map((it) => {
      const paras = it.text.split(/\n\s*\n/).filter((x) => x.trim());
      const notedParas = new Set((editable ? draft.notes : sub.feedback?.notes || []).filter((n) => n.item === it.i && n.paragraph !== undefined).map((n) => n.paragraph));
      return h('section', null, h('h2', null, it.label),
        h('div', { class: 'doc' }, paras.map((para, k) => h('div', { class: `para${notedParas.has(k) ? ' noted' : ''}` },
          h('span', { class: 'n' }, k + 1), para.trim(),
          editable ? h('button', { class: 'btn link add', onclick: () => addNote({ item: it.i, paragraph: k }) }, 'Comment') : null))));
    }));
  }

  function drawNotes(focusLast = false) {
    if (!editable) {
      const fb = sub.feedback;
      fill(notesBox, fb ? h('section', null, h('h2', null, fb.verdict === 'approve' ? 'You approved this' : 'You asked for changes'),
        fb.overall ? h('p', null, fb.overall) : null,
        h('ul', { class: 'notes' }, fb.notes.map((n) => h('li', null, h('span', { class: 'num' }, n.n), h('div', null, h('div', { class: 'where' }, whereText(n)), h('div', null, n.text)), h('span'))))) : h('section', null, h('p', { class: 'meta' }, 'A newer version replaced this one before you reviewed it.')));
      return;
    }
    const list = h('ul', { class: 'notes' }, draft.notes.map((n, k) => h('li', null,
      h('span', { class: 'num' }, k + 1),
      h('div', null, h('div', { class: 'where' }, whereText(n)),
        h('textarea', { placeholder: n.bars && canEdit() ? 'What should change here? ("a different drum beat")' : 'What should change (or what you love)?', value: n.text, oninput: (e) => { n.text = e.target.value; save(); } }),
        editControls(n)),
      h('button', { class: 'x', 'aria-label': `Remove note ${k + 1}`, onclick: async () => {
        if (n.edit?.status === 'tried' && !(await undo(n))) return; // a note's change goes with it
        draft.notes.splice(draft.notes.indexOf(n), 1); save(); drawNotes(); drawStage(); drawDoc();
      } }, '×'))));
    fill(notesBox, h('section', null, h('h2', null, 'Your notes', h('span', { class: 'count' }, draft.notes.length || '')),
      draft.notes.length ? list : h('p', { class: 'meta' }, media.length ? 'Note a moment on the MIDI or the audio, or comment on a paragraph. No notes needed to approve.' : 'Comment on a paragraph, or approve as it is.'),
      h('button', { class: 'btn link', onclick: () => addNote({}) }, 'Add a general note')));
    if (focusLast) list.querySelector('li:last-child textarea')?.focus();
  }

  // ---- quick edits: change the marked bars right here ----
  let busy = false;
  const barsWord = (n) => (n.bars[0] === n.bars[1] ? `bar ${n.bars[0]}` : `bars ${n.bars[0]}-${n.bars[1]}`);
  function editControls(n) {
    if (!canEdit() || !n.bars || sub.items[n.item]?.kind !== 'midi') return null;
    if (n._busy) return h('div', { class: 'edit-line' }, h('span', { class: 'pulse', 'aria-hidden': 'true' }), ` Changing ${barsWord(n)}${n.part ? ` (${n.part})` : ''}…`);
    const e = n.edit;
    if (!e || e.status === 'undone') {
      return h('div', { class: 'edit-line' }, h('button', { class: 'btn', disabled: busy, onclick: () => quickEdit(n) }, 'Change it now'),
        h('span', { class: 'meta' }, e ? ' undone · ' : ' ', 'or leave it for the session'));
    }
    return h('div', { class: 'edit-line done' },
      h('div', { class: 'facts' }, `Changed ${barsWord(n)}${e.part ? `, ${e.part}` : ''}: ${(e.facts || []).join(' · ')}`),
      e.said ? h('div', { class: 'meta' }, e.said) : null,
      e.warnings?.length ? h('div', { class: 'warn' }, `The check would flag: ${e.warnings.join(' · ')}`) : null,
      h('div', { class: 'actions' },
        e.status === 'kept' ? h('span', { class: 'chip ok' }, 'kept') : h('button', { class: 'btn', disabled: busy, onclick: () => keep(n) }, 'Keep'),
        h('button', { class: 'btn link', disabled: busy, onclick: () => undo(n) }, 'Undo'),
        h('button', { class: 'btn link', disabled: busy, onclick: () => quickEdit(n, e.id) }, 'Try another')));
  }
  async function quickEdit(n, retryOf = null) {
    if (!(n.text || '').trim()) { toast('Say what should change first'); return; }
    busy = true; n._busy = true; drawNotes();
    try {
      const r = await api(`/api/projects/${id}/music/edits`, { method: 'POST', body: { from: n.bars[0], to: n.bars[1], channel: n.channel ?? null, ask: n.text.trim(), retry_of: retryOf } });
      const e = r.edit;
      n.edit = { id: e.id, status: e.status, facts: e.facts, said: e.said, warnings: e.warnings, part: e.part };
      editInfo = await api(`/api/projects/${id}/music/edits`).catch(() => editInfo);
      save();
      toast(`Changed in ${r.seconds} s`);
      drawStage({ playFrom: n.bars[0] });
    } catch (err) { toast(err.message, true); } finally { busy = false; delete n._busy; drawNotes(); }
  }
  async function keep(n) {
    try { const r = await api(`/api/projects/${id}/music/edits/${n.edit.id}/keep`, { method: 'POST' }); n.edit.status = r.edit.status; save(); toast('Kept: the studio remembers this one'); } catch (err) { toast(err.message, true); }
    drawNotes();
  }
  async function undo(n) {
    busy = true; drawNotes();
    try {
      const r = await api(`/api/projects/${id}/music/edits/${n.edit.id}/undo`, { method: 'POST' });
      n.edit.status = r.edit.status;
      editInfo = await api(`/api/projects/${id}/music/edits`).catch(() => editInfo);
      save(); drawStage({ playFrom: n.bars[0] });
      return true;
    } catch (err) { toast(err.message, true); return false; } finally { busy = false; drawNotes(); }
  }

  const questions = sub.questions.length ? h('section', null, h('h2', null, 'Questions'), sub.questions.map((q) => {
    const answered = sub.feedback?.answers?.find((a) => a.id === q.id);
    if (!editable) return h('div', { class: 'qa' }, h('div', { class: 'q' }, q.text), h('div', { class: 'meta' }, answered ? `${answered.answer}${answered.used_default ? ' (default)' : ''}` : '-'));
    const set = (v) => { draft.answers[q.id] = v; save(); };
    return h('div', { class: 'qa' }, h('div', { class: 'q' }, q.text),
      q.choices ? choiceChips(q.choices, draft.answers[q.id] ?? null, set) : null,
      h('input', { type: 'text', style: { marginTop: q.choices ? '8px' : '0' }, placeholder: q.default !== null ? `Default: ${q.default}` : 'Needs an answer', value: q.choices && q.choices.includes(draft.answers[q.id]) ? '' : (draft.answers[q.id] || ''), oninput: (e) => set(e.target.value) }));
  })) : null;

  const rateBox = editable && step.delivers && d.project.flow ? h('section', null, h('h2', null, 'How did it land?', h('span', { class: 'count' }, 'optional')),
    ratingChips(draft.rating || null, (r) => { draft.rating = r; save(); }),
    h('p', { class: 'meta', style: { marginTop: '8px' } }, 'Sent with your approval. The studio picks your next flows with it.')) : null;

  const overall = editable ? h('section', null, h('h2', null, 'Anything else'), h('textarea', { placeholder: 'An overall comment (optional)', value: draft.overall, oninput: (e) => { draft.overall = e.target.value; save(); } })) : null;

  const err = h('div', { class: 'err' });
  const decide = async (verdict) => {
    err.textContent = '';
    // A note whose bars were already changed here goes to the session as done (so it isn't changed again).
    if (unsavedNotes && !confirmInline(null)) { err.textContent = `Your note changes in ${unsavedNotes} bar${unsavedNotes === 1 ? '' : 's'} aren't in the song yet: "Put it in the song" first, or click again to send without them.`; return; }
    const notes = draft.notes.map((n) => ({ item: n.item ?? null, x: n.x, y: n.y, time_sec: n.time_sec, paragraph: n.paragraph, bars: n.bars, part: n.part || undefined, channel: n.channel ?? undefined,
      text: n.edit && n.edit.status !== 'undone' && (n.text || '').trim() ? `Changed in the studio already (my edit: keep it): ${n.text.trim()}` : (n.text || '').trim() }));
    if (notes.some((n) => !n.text)) { err.textContent = 'Every note needs some words (or remove it).'; return; }
    if (verdict === 'changes' && !notes.length && !draft.overall.trim()) { err.textContent = 'Say what to change: leave a note or write an overall comment.'; return; }
    try {
      await api(`/api/projects/${id}/reviews/${stepKey}/${sub.version}`, { method: 'POST', body: { verdict, notes, overall: draft.overall, answers: draft.answers } });
      if (verdict === 'approve' && draft.rating && rateBox) await api(`/api/projects/${id}/rate`, { method: 'POST', body: { rating: draft.rating } }).catch(() => {});
      store.del(draftKey);
      toast(verdict === 'approve' ? `${step.name} approved` : 'Changes sent: the studio is on it');
      location.hash = `#/p/${id}`;
    } catch (e) { err.textContent = e.message; }
  };

  const prevNotes = prev?.feedback?.notes || [];
  const changed = sub.pin_changes.length ? h('section', null, h('h2', null, 'What changed for your notes'),
    h('ul', { class: 'rows changed' }, sub.pin_changes.map((c) => h('li', null,
      h('div', { class: 'yours' }, `Your note ${c.note}: ${prevNotes[c.note - 1]?.text || ''}`),
      h('div', null, c.change, ' ', h('span', { class: `chip ${c.status === 'done' ? 'ok' : 'warn'}` }, c.status.replace('_', ' '))))))) : null;

  const checks = sub.error_checks.length ? h('section', null, h('details', null,
    h('summary', null, `Studio checks: ${sub.error_checks.filter((c) => c.result === 'pass').length} passed${sub.error_checks.some((c) => c.result === 'fixed') ? `, ${sub.error_checks.filter((c) => c.result === 'fixed').length} fixed` : ''}`),
    h('ul', { class: 'checks-list' }, sub.error_checks.map((c) => h('li', null, h('span', null, step.checks.find((x) => x.id === c.id)?.title || c.id, c.note ? h('span', { class: 'meta' }, ` · ${c.note}`) : null),
      h('span', { class: `chip ${c.result === 'pass' ? 'ok' : 'warn'}` }, c.result)))))) : null;

  mount(
    h('a', { class: 'crumb', href: `#/p/${id}` }, `← ${d.project.title}`),
    h('div', { class: 'head' }, h('div', null, h('h1', null, `${step.name} v${sub.version}`),
      h('div', { class: 'sub versions' }, `Sent ${ago(sub.submitted_at)}`, versions.length > 1 ? ' · ' : '',
        versions.length > 1 ? versions.map((v, i) => [i ? ' ' : '', v.version === sub.version ? h('b', null, `v${v.version}`) : h('a', { href: `#/p/${id}/review/${stepKey}/${v.version}` }, `v${v.version}`)]) : null))),
    h('div', { class: 'review' },
      h('div', { class: 'stage-wrap' }, h('div', { class: 'note-from' }, sub.note), changed, stageBox, docBox),
      h('aside', { class: 'side' }, notesBox, questions, overall, rateBox, checks,
        editable ? h('div', { class: 'decide' }, err,
          h('button', { class: 'btn primary', onclick: () => decide('approve') }, 'Approve'),
          h('button', { class: 'btn', onclick: () => decide('changes') }, 'Ask for changes')) : null)),
  );
  drawStage();
  drawDoc();
  drawNotes();
  current.refresh = null; // never redraw under someone's half-written notes
}

// ---------- new piece ----------

// The flow question: the studio's pick (the default), a shape or a feel for it to pick from, or one by name.
function flowPicker(kind, cat, onChange) {
  const list = cat.flows.filter((f) => !f.error && f.for === kind);
  const buckets = [...new Set([...Object.keys(cat.buckets), ...list.map((f) => f.bucket)])].filter((b) => list.some((f) => f.bucket === b));
  const feels = [...new Set(list.flatMap((f) => f.feels))].sort();
  const MODES = { studio: 'Let the studio choose', shape: 'From a shape', feel: 'From a feel', pick: 'Pick one' };
  let mode = 'studio';
  const box = h('div');
  const help = h('div', { class: 'help' });
  const draw = () => {
    let below = null;
    if (mode === 'studio') help.textContent = 'It leans toward the flows you\'ve loved, away from the ones you just had, and now and then tries one you haven\'t heard.';
    if (mode === 'shape') {
      help.textContent = 'The studio picks a flow with this shape.';
      below = choiceChips(buckets.map(cap), null, (v) => { const b = v.toLowerCase(); onChange(`shape:${b}`); help.textContent = `${(cat.buckets[b] || cap(b)).replace(/^[^:]+: /, '')}: ${list.filter((f) => f.bucket === b).map((f) => f.name).join(', ')}.`; });
    }
    if (mode === 'feel') {
      help.textContent = 'The studio picks a flow with this feel.';
      below = choiceChips(feels.map(cap), null, (v) => { const f = v.toLowerCase(); onChange(`feel:${f}`); help.textContent = `Flows that feel ${f}: ${list.filter((x) => x.feels.includes(f)).map((x) => x.name).join(', ')}.`; });
    }
    if (mode === 'pick') {
      help.textContent = '';
      const rows = h('div', { role: 'radiogroup', 'aria-label': 'Flows' }, list.map((f) => h('div', { class: 'pick', role: 'radio', 'aria-checked': 'false', tabindex: '0', onclick: (e) => {
        for (const r of rows.children) { r.classList.remove('on'); r.setAttribute('aria-checked', 'false'); }
        e.currentTarget.classList.add('on'); e.currentTarget.setAttribute('aria-checked', 'true'); onChange(f.slug);
      } }, h('span', { class: 'title' }, f.name), h('span', { class: 'meta' }, [cap(f.bucket), ...f.feels].join(' · ')), h('div', { class: 'desc' }, f.summary))));
      below = rows;
    }
    fill(box, choiceChips(Object.values(MODES), MODES[mode], (v) => { mode = Object.keys(MODES).find((k) => MODES[k] === v); onChange(mode === 'studio' ? 'studio' : null); draw(); }),
      below ? h('div', { style: { marginTop: '10px' } }, below) : null, help);
  };
  draw();
  return box;
}

async function newView(slug) {
  const pipelines = (await api('/api/pipelines')).filter((p) => !p.error);
  if (!slug) {
    mount(h('div', { class: 'head' }, h('h1', null, 'New piece')),
      h('section', null, h('h2', null, 'What are we making?'), pipelines.map((p) => h('div', { class: 'pick', onclick: () => { location.hash = `#/new/${p.slug}`; } },
        h('a', { class: 'title', href: `#/new/${p.slug}` }, p.name), h('span', { class: 'chip' }, p.makes),
        h('div', { class: 'desc' }, p.description),
        h('div', { class: 'chain' }, p.steps.filter((s) => s.review !== 'internal').map((s) => s.name).join(' → '))))));
    return;
  }
  const p = pipelines.find((x) => x.slug === slug);
  if (!p) throw new Error(`no pipeline ${slug}`);
  const answers = {};
  const files = {};
  for (const q of p.intake) if (q.type === 'choice' && q.default) answers[q.id] = q.default;
  let title = '';
  let backend = null;
  let createdId = null;
  let flow = 'studio';
  const cat = p.flows ? await api('/api/flows').catch(() => null) : null;
  const err = h('div', { class: 'err' });
  const keysBox = h('div');

  const field = (label, control, { help, optional } = {}) => h('div', { class: 'field' }, h('label', null, label, optional ? h('div', { class: 'opt' }, 'optional') : null), h('div', null, control, help ? h('div', { class: 'help' }, help) : null));
  const fields = p.intake.map((q) => {
    let control;
    if (q.type === 'long') control = h('textarea', { oninput: (e) => { answers[q.id] = e.target.value; } });
    else if (q.type === 'choice') control = choiceChips(q.choices, q.default || null, (v) => { answers[q.id] = v; });
    else if (q.type === 'files') control = h('input', { type: 'file', multiple: true, onchange: (e) => { files[q.id] = [...e.target.files]; } });
    else control = h('input', { type: 'text', oninput: (e) => { answers[q.id] = e.target.value; } });
    return field(q.label || q.id, control, { help: q.help, optional: !q.required });
  });

  const startBtn = h('button', { class: 'btn solid', onclick: async () => {
    err.textContent = '';
    startBtn.disabled = true;
    try {
      if (!createdId) {
        if (cat && !flow) throw new Error('Pick a flow (or let the studio choose).');
        const { id } = await api('/api/projects', { method: 'POST', body: { pipeline: slug, title, answers, backend, flow: cat ? flow : 'none' } });
        for (const [fieldId, list] of Object.entries(files)) {
          for (const f of list) {
            startBtn.textContent = `Uploading ${f.name}…`;
            await api(`/api/projects/${id}/files?field=${encodeURIComponent(fieldId)}&name=${encodeURIComponent(f.name)}`, { method: 'POST', raw: f });
          }
        }
        createdId = id;
      }
      const id = createdId;
      try {
        await api(`/api/projects/${id}/start`, { method: 'POST' });
      } catch (e) {
        // Keys this project needs: ask for them right here, then Start again.
        if (!e.data?.needs_keys) { location.hash = `#/p/${id}`; throw e; }
        const { keys } = await api('/api/keys');
        const drawKeys = (list) => fill(keysBox, h('section', null, h('h2', null, 'Before it starts'), h('p', { class: 'meta' }, e.message),
          list.filter((k) => e.data.needs_keys.includes(k.id)).map((k) => keyRow(k, (r) => drawKeys(r.keys)))));
        drawKeys(keys);
        throw new Error('Add the key above, then Start.');
      }
      location.hash = `#/p/${id}`;
    } catch (e) {
      err.textContent = e.message;
      startBtn.disabled = false;
      startBtn.textContent = 'Start';
    }
  } }, 'Start');

  mount(
    h('a', { class: 'crumb', href: '#/new' }, '← Everything we make'),
    h('div', { class: 'head' }, h('div', null, h('h1', null, p.name), h('div', { class: 'sub' }, p.description))),
    // The demo makes no music: say so before anyone writes a brief for it.
    studio?.config.backend === 'demo' ? h('div', { class: 'needs' }, h('div', { class: 'what' }, 'This studio runs the demo: no Claude, no real music.'),
      h('div', { class: 'meta' }, 'Every piece comes out as the same 4-bar placeholder with 3 parts, to try the review room for free. ', h('a', { href: '#/settings' }, 'Choose Claude in Settings'), ' to make real music.')) : null,
    h('section', null,
      field('Title', h('input', { type: 'text', placeholder: p.name, oninput: (e) => { title = e.target.value; } })),
      cat ? field('Flow', flowPicker(p.flows, cat, (v) => { flow = v; }), { optional: true }) : null,
      fields,
      studio?.config.backend === 'demo' ? null
        : field('Made by', choiceChips([BACKEND_NAMES[studio?.config.backend] || 'Studio default', 'Demo (placeholder work, free)'], BACKEND_NAMES[studio?.config.backend] || 'Studio default', (v) => { backend = v.startsWith('Demo') ? 'demo' : null; }), { help: 'The demo walks every stage with placeholder work: handy to learn the review room.' })),
    keysBox,
    h('div', { class: 'form-end' }, err, startBtn),
  );
}

// ---------- flows ----------

async function flowsView() {
  const cat = await api('/api/flows');
  let by = store.get('flows-by') || 'shape';
  const body = h('div');
  const statLine = (st) => (st ? [st.used ? `used ${st.used}×` : null, st.love ? `loved ${st.love}×` : null, st.fine ? `fine ${st.fine}×` : null, st.no ? `not for you ${st.no}×` : null].filter(Boolean).join(' · ') : '');
  const row = (f) => {
    const more = h('div', { class: 'flow-md' });
    return h('details', { class: 'flow', ontoggle: async (e) => {
      if (!e.target.open || more.dataset.loaded) return;
      more.dataset.loaded = '1';
      try { const x = await api(`/api/flows/${f.slug}`); fill(more, ...x.flow_md.split(/\n\s*\n/).filter((t) => t.trim()).slice(1).map((t) => (/^#+ /.test(t) ? h('h3', null, t.replace(/^#+ /, '')) : h('p', null, t.replace(/\*\*/g, ''))))); } catch (err) { fill(more, h('p', { class: 'err' }, err.message)); }
    } },
    h('summary', null, h('span', { class: 'title' }, f.name), f.source === 'yours' ? h('span', { class: 'chip' }, 'yours') : null, h('span', { class: 'meta' }, f.feels.join(' · ')), statLine(f.stats) ? h('span', { class: 'meta stat' }, statLine(f.stats)) : null),
    h('p', null, f.summary),
    h('ul', { class: 'meta flow-facts' },
      f.techniques.required.length ? h('li', null, `Its techniques: ${f.techniques.required.join(', ')}${f.techniques.free ? `, plus ${f.techniques.free} the piece picks` : ''}`) : null,
      f.techniques.avoid.length ? h('li', null, `Avoids: ${f.techniques.avoid.join(', ')}`) : null,
      f.from ? h('li', null, `Made from "${f.from.title}"`) : null),
    more);
  };
  const draw = () => {
    const parts = [];
    for (const [kind, label] of Object.entries(cat.kinds)) {
      const list = cat.flows.filter((f) => !f.error && f.for === kind);
      if (!list.length) continue;
      const groups = by === 'feel'
        ? [...new Set(list.flatMap((f) => f.feels))].sort().map((x) => [cap(x), list.filter((f) => f.feels.includes(x))])
        : [...new Set([...Object.keys(cat.buckets), ...list.map((f) => f.bucket)])].map((b) => [cat.buckets[b] || cap(b), list.filter((f) => f.bucket === b)]).filter(([, fs]) => fs.length);
      parts.push(h('section', null, h('h2', null, `For ${label}`, h('span', { class: 'count' }, list.length)),
        groups.map(([g, fs]) => [h('h3', { class: 'group' }, g), fs.map(row)])));
    }
    const broken = cat.flows.filter((f) => f.error);
    if (broken.length) parts.push(h('section', null, h('h2', null, 'Flows that need fixing'), broken.map((f) => h('p', { class: 'err' }, `${f.slug}: ${f.error}`))));
    fill(body, ...parts);
  };
  mount(h('div', { class: 'head' }, h('div', null, h('h1', null, 'Flows'), h('div', { class: 'sub' }, 'The shape a piece takes. Pick one in a brief, or let the studio choose: it learns from how you rate what you get.'))),
    choiceChips(['By shape', 'By feel'], by === 'feel' ? 'By feel' : 'By shape', (v) => { by = v === 'By feel' ? 'feel' : 'shape'; store.set('flows-by', by); draw(); }),
    body);
  draw();
}

// ---------- pipelines ----------

async function pipelinesView() {
  const pipelines = await api('/api/pipelines');
  mount(h('div', { class: 'head' }, h('div', null, h('h1', null, 'Pipelines'), h('div', { class: 'sub' }, 'How each kind of piece is made. Add your own in the studio folder\'s pipelines/ (see docs/PIPELINES.md).'))),
    pipelines.map((p) => h('section', null,
      h('h2', null, p.name || p.slug, h('span', { class: 'count' }, p.source)),
      p.error ? h('p', { class: 'err' }, p.error) : [
        h('p', null, p.description),
        h('ul', { class: 'rows' }, p.steps.map((s) => h('li', { class: 'row' }, icon(s.review === 'internal' ? 'blocked' : 'ready'),
          h('div', { class: 'main' }, h('span', { class: 'title' }, s.name), h('div', { class: 'meta' }, s.describe || (s.after.length ? `after ${s.after.join(', ')}` : 'first'))),
          h('div', { class: 'end' }, h('span', { class: 'chip' }, s.review === 'internal' ? 'studio' : `you review ${s.review}`))))),
        h('details', null, h('summary', null, `Error checks (${p.checks.length})`), h('ul', { class: 'checks-list' }, p.checks.map((c) => h('li', null, h('span', null, h('b', null, c.title), h('span', { class: 'meta' }, ` · ${c.how}`)))))),
      ])));
}

// ---------- settings ----------

async function settingsView() {
  studio = await api('/api/studio');
  const checks = await api('/api/checks');
  const c = { ...studio.config };
  let apiKey;
  const msg = h('span', { class: 'okmsg' });
  const field = (label, control, help) => h('div', { class: 'field' }, h('label', null, label), h('div', null, control, help ? h('div', { class: 'help' }, help) : null));
  const toggle = (key, text) => h('label', { class: 'toggle' }, h('input', { type: 'checkbox', checked: c[key], onchange: (e) => { c[key] = e.target.checked; } }), text);

  const backendRows = Object.entries(studio.backends).map(([name, b]) => h('li', { class: 'row', style: { cursor: 'pointer' }, onclick: () => { c.backend = name; draw(); } },
    h('input', { type: 'radio', name: 'backend', checked: c.backend === name, 'aria-label': BACKEND_NAMES[name] }),
    h('div', { class: 'main' }, h('span', { class: 'title' }, { 'claude-code': 'Your Claude Code', 'anthropic-api': 'Claude API with your key', demo: 'Demo' }[name] || name),
      h('div', { class: 'meta' }, { 'claude-code': 'Runs `claude -p` with your own login or plan.', 'anthropic-api': 'Mortiflix Music runs the agent loop on the API; you pay per token.', demo: 'Placeholder work, no Claude: for trying the studio.' }[name])),
    h('div', { class: 'end' }, h('span', { class: `chip ${b.ok ? 'ok' : 'bad'}` }, b.ok ? 'ready' : 'not set up'))));

  const body = h('div');
  function draw() {
    for (const r of body.querySelectorAll('input[name=backend]')) r.checked = r.getAttribute('aria-label') === BACKEND_NAMES[c.backend];
    fill(extra, ...(c.backend === 'anthropic-api' ? [
      field('API key', h('input', { type: 'password', autocomplete: 'off', placeholder: studio.api_key_set ? 'Saved (type to replace)' : studio.api_key_env ? 'Using ANTHROPIC_API_KEY' : 'sk-ant-…', oninput: (e) => { apiKey = e.target.value; } }),
        'Stored only in the studio folder (secrets.json, readable by you alone). Never shown again, never sent anywhere but the Claude API.'),
      field('Model', h('input', { type: 'text', placeholder: 'claude-opus-5-5', value: c.model || '', oninput: (e) => { c.model = e.target.value; } })),
      field('Effort', choiceChips(['low', 'medium', 'high', 'xhigh', 'max'], c.effort, (v) => { c.effort = v; }), 'High suits most songs; higher is slower and costs more.'),
      field('Tools', h('div', null, toggle('webTools', 'Web search and fetch for research'), toggle('fallbacks', 'Refusal fallbacks (another model continues if one declines)'))),
    ] : c.backend === 'claude-code' ? [
      field('Model', h('input', { type: 'text', placeholder: 'Claude Code\'s default', value: c.model || '', oninput: (e) => { c.model = e.target.value; } })),
      field('Sandbox', toggle('sandbox', 'Run each session inside bubblewrap (Linux)'), 'The session then sees only its project folder, read-only system files and its Claude login.'),
    ] : []));
  }
  const extra = h('div');

  const save = h('button', { class: 'btn solid', onclick: async () => {
    try {
      const patch = { backend: c.backend, model: c.model || null, effort: c.effort, sandbox: c.sandbox, webTools: c.webTools, fallbacks: c.fallbacks, maxSessionMinutes: Number(c.maxSessionMinutes) };
      if (apiKey !== undefined && apiKey !== '') patch.api_key = apiKey;
      studio = await api('/api/config', { method: 'PUT', body: patch });
      msg.textContent = 'Saved';
      setTimeout(() => { msg.textContent = ''; }, 2000);
      loadStudio();
    } catch (e) { toast(e.message, true); }
  } }, 'Save');

  const decideCheck = (cid, approve) => async () => { try { await api(`/api/checks/${cid}`, { method: 'POST', body: { approve } }); settingsView(); } catch (e) { toast(e.message, true); } };

  add(body, await setupSection(), 
    h('section', null, h('h2', null, 'Who makes the music'), h('ul', { class: 'rows' }, backendRows), extra,
      field('Session limit', h('input', { type: 'number', min: 5, max: 1440, value: c.maxSessionMinutes, oninput: (e) => { c.maxSessionMinutes = e.target.value; }, style: { maxWidth: '120px' } }), 'Minutes before a session is stopped (the next one picks up from its handoff).'),
      h('div', { class: 'form-end' }, msg, save)),
    h('section', null, h('h2', null, 'The error checklist', h('span', { class: 'count' }, checks.proposed.length ? `${checks.proposed.length} proposed` : '')),
      h('p', { class: 'meta' }, 'When you point out a real mistake, the session proposes a check so it never reaches you again. Approved checks run on every future piece.'),
      checks.proposed.length ? h('ul', { class: 'rows' }, checks.proposed.map((k) => h('li', { class: 'row' }, icon('in_review'),
        h('div', { class: 'main' }, h('span', { class: 'title' }, k.title), h('div', { class: 'meta', style: { whiteSpace: 'normal' } }, k.how)),
        h('div', { class: 'end' }, h('button', { class: 'btn', onclick: decideCheck(k.id, true) }, 'Add'), h('button', { class: 'btn link', onclick: decideCheck(k.id, false) }, 'Dismiss'))))) : null,
      checks.active.length ? h('ul', { class: 'rows' }, checks.active.map((k) => h('li', { class: 'row' }, icon('done'), h('div', { class: 'main' }, h('span', { class: 'title' }, k.title)), h('div', { class: 'end' }, (k.applies_to || []).join(', '))))) : h('p', { class: 'meta' }, 'No studio checks yet (each pipeline has its own).')),
    await keysSection(),
  );
  mount(h('div', { class: 'head' }, h('h1', null, 'Settings')), body);
  draw();
}

// ---------- setup (Settings › Setup): every part, why it's needed, what it installs ----------

async function setupSection() {
  const wrap = h('section', { id: 'setup' });
  let info = await api('/api/setup');
  let poll = null;
  const field = (label, control, help) => h('div', { class: 'field' }, h('label', null, label), h('div', null, control, help ? h('div', { class: 'help' }, help) : null));
  const chip = (ok, yes, no, neutral = false) => h('span', { class: `chip ${ok ? 'ok' : neutral ? '' : 'bad'}` }, ok ? yes : no);
  const refresh = async () => { info = await api('/api/setup'); draw(); };

  // One tool: its status, what installing it means, an Install button and the live log while it runs.
  const UPDATABLE = ['strudel'];   // Chrome is reused when found
  const toolRow = (id, label = info.tools[id].name) => {
    const st = info.status[id];
    const t = info.tools[id];
    const job = info.jobs[id];
    const running = job?.state === 'running';
    const btn = h('button', { class: 'btn', disabled: running || Object.values(info.jobs).some((j) => j.state === 'running'), onclick: async () => {
      try { await api(`/api/setup/install/${id}`, { method: 'POST' }); await refresh(); startPoll(); } catch (e) { toast(e.message, true); }
    } }, running ? 'Installing…' : st.ok ? 'Update' : 'Install');
    const log = job ? h('pre', { class: 'setup-log' }, job.log.join('\n') || '…') : null;
    if (log) setTimeout(() => { log.scrollTop = log.scrollHeight; });
    return h('div', { class: 'tool-row' },
      h('div', { class: 'actions' }, h('b', null, label), chip(st.ok, st.detail, st.detail), !st.ok || UPDATABLE.includes(id) || running ? btn : null),
      h('div', { class: 'help' }, `${t.what}. Goes to ${t.where} (${t.size}).`),
      job?.state === 'failed' ? h('div', { class: 'err' }, job.error) : null,
      job?.problems ? h('div', { class: 'err' }, job.problems) : null,
      log);
  };
  const startPoll = () => {
    clearInterval(poll);
    poll = setInterval(async () => {
      if (!document.body.contains(wrap)) return clearInterval(poll);
      await refresh();
      if (!Object.values(info.jobs).some((j) => j.state === 'running')) { clearInterval(poll); loadStudio(); }
    }, 1200);
  };

  function draw() {
    const st = info.status;
    const part = (id) => info.parts.find((x) => x.id === id);
    const intro = (id) => [h('p', null, part(id).why), h('p', { class: 'meta' }, part(id).needs)];
    const dawNames = { ...info.daws, none: 'None' };
    fill(wrap,
      h('h2', null, 'Setup'),
      h('p', { class: 'meta' }, 'What the studio uses, why, and what it installs. Everything goes into the studio folder, never system-wide, and nothing installs until you press Install.'),
      field(part('claude').title, h('div', null, ...intro('claude'), h('p', { class: 'meta' }, 'Choose below, in "Who makes the music".'))),
      field(part('music').title, h('div', null, ...intro('music'), toolRow('strudel'), toolRow('chrome'))),
      field(part('daw').title, h('div', null, ...intro('daw'),
        choiceChips(Object.values(dawNames), st.daw.name || 'None', async (v) => {
          const daw = Object.keys(dawNames).find((k) => dawNames[k] === v);
          try { info = await api('/api/setup/daw', { method: 'PUT', body: { daw: daw === 'none' ? null : daw } }); draw(); } catch (e) { toast(e.message, true); }
        }))),
    );
  }
  draw();
  if (Object.values(info.jobs).some((j) => j.state === 'running')) startPoll();
  return wrap;
}

// ---------- keys (Settings › Keys, and New piece when one is missing) ----------

const KEY_SOURCES = { saved: 'saved in this studio', 'session.env': 'from session.env', environment: 'from your environment' };

// One key: where it comes from (never its value) and a hidden field to set it. The server checks it with a free call
// before saving it.
function keyRow(k, onChange) {
  const input = h('input', { type: 'password', autocomplete: 'off', 'aria-label': k.name, placeholder: k.source === 'saved' ? 'Saved (paste to replace)' : k.source ? `Using ${k.env} ${KEY_SOURCES[k.source]}` : 'Paste your key' });
  const note = h('div', { class: 'help' }, `For ${k.for} Get one at ${k.get}.`);
  const save = h('button', { class: 'btn', onclick: async () => {
    if (!input.value.trim()) return input.focus();
    save.disabled = true;
    save.textContent = 'Checking…';
    try {
      const r = await api(`/api/keys/${k.id}`, { method: 'PUT', body: { value: input.value } });
      input.value = '';
      toast(r.check?.ok ? `${k.name}: ${r.check.detail}` : `${k.name} saved${r.check ? `, but ${r.check.detail}` : ''}`);
      onChange(r);
    } catch (e) {
      fill(note, h('span', { class: 'err' }, e.message));
      save.disabled = false;
      save.textContent = 'Save';
    }
  } }, 'Save');
  const remove = k.source === 'saved' ? h('button', { class: 'btn link', onclick: async () => { try { onChange(await api(`/api/keys/${k.id}`, { method: 'DELETE' })); } catch (e) { toast(e.message, true); } } }, 'Remove') : null;
  const chip = h('span', { class: `chip ${k.source ? 'ok' : k.in_use ? 'bad' : ''}` }, k.source ? KEY_SOURCES[k.source] : k.in_use ? 'needed' : 'not set');
  return h('div', { class: 'field' }, h('label', null, k.name, h('div', null, chip)), h('div', null, h('div', { class: 'actions' }, input, save, remove), note));
}

async function keysSection() {
  const wrap = h('section', { id: 'keys' });
  const draw = ({ keys, other }) => {
    let name = '';
    let value = '';
    const addOther = async () => {
      try { draw(await api(`/api/keys/${encodeURIComponent(name.trim().toUpperCase())}`, { method: 'PUT', body: { value } })); } catch (e) { toast(e.message, true); }
    };
    fill(wrap, h('h2', null, 'Keys'),
      h('p', { class: 'meta' }, 'Mortiflix Music runs on your own accounts. Keys stay in the studio folder (secrets.json and session.env, readable by you alone) and are never shown again.'),
      keys.map((k) => keyRow(k, draw)),
      h('div', { class: 'field' }, h('label', null, 'Other keys', h('div', { class: 'opt' }, 'handed to every session')),
        h('div', null,
          other.length ? h('div', { class: 'actions' }, other.map((n) => h('span', { class: 'chip' }, n, h('button', { class: 'btn link', 'aria-label': `Remove ${n}`, onclick: async () => { try { draw(await api(`/api/keys/${n}`, { method: 'DELETE' })); } catch (e) { toast(e.message, true); } } }, '×')))) : null,
          h('div', { class: 'actions', style: { marginTop: other.length ? '10px' : '0' } },
            h('input', { type: 'text', placeholder: 'NAME, e.g. GEMINI_API_KEY', 'aria-label': 'Key name', oninput: (e) => { name = e.target.value; }, style: { maxWidth: '240px' } }),
            h('input', { type: 'password', autocomplete: 'off', placeholder: 'value', 'aria-label': 'Key value', oninput: (e) => { value = e.target.value; } }),
            h('button', { class: 'btn', onclick: addOther }, 'Add')),
          h('div', { class: 'help' }, 'For tools a pipeline uses that read a key from the environment.'))),
      h('p', { class: 'meta' }, `Studio folder: ${studio.root}`));
  };
  draw(await api('/api/keys'));
  return wrap;
}

// The MIDI to hear, see and change, for someone still learning: the instruments (each lights up while it plays), the
// whole piece (click to go there), and its notes to change by hand, with the changes made so far. Shared by the review
// room and the project page. `info` is GET /music/edits; `step`/`version` say which edits belong here ('pack' 0: the
// piece itself, while it waits for you). `after(what, bar)` redraws once an edit is in, kept or undone.
function midiWorkbench({ id, piece, play, info, step, version, handEditable, est, focus = null, onFocus = () => {}, bands = [], onMark = null, onMoveBand = null, onNote = null, after }) {
  const bpb = piece.beatsPerBar || piece.beats_per_bar || 4;
  const bpm = piece.bpm || 120;
  const tracks = (piece.tracks || []).filter((t) => t.notes?.length);
  const colors = colorsFor(tracks);
  const mine = (info?.edits || []).filter((e) => e.step === step && e.version === version && e.status !== 'undone');
  // Bars changed by hand, shaded on the whole piece like the bars Claude changed.
  bands = [...bands];
  for (const e of mine.filter((x) => x.kind === 'notes')) for (const [a, b] of barRuns(e.cells || [e.from])) bands.push({ from: a, to: b, label: 'by hand', kind: 'edited' });
  if (step === 'pack') for (const e of mine.filter((x) => x.kind !== 'notes')) bands.push({ from: e.from, to: e.to, label: 'edited', kind: 'edited' });
  let roll = null; let strip = null; let editor = null;
  const setFocus = (ch, from) => { focus = ch; onFocus(ch); if (from !== 'strip') strip?.setFocus(ch); roll?.setFocus?.(ch); if (from !== 'editor') editor?.setFocus(ch); };
  const loadIt = () => { if (player.state().id !== play.id) player.load({ ...play, piece: editor ? editor.piece() : piece }); };
  const pickedBars = () => { const sel = est.notes?.filter((n) => est.selected?.has(n.id)) || []; if (!sel.length) return null; const b = sel.map((n) => Math.floor(n.start / bpb) + 1); return [Math.min(...b), Math.max(...b)]; };
  const sections = piece.markers ? piece.markers.map((m) => ({ name: m.text, start_bar: Math.round(m.beat / bpb) + 1 })) : piece.sections || [];
  strip = instrumentStrip({ tracks, roles: info?.roles || [], colors, pid: play.id, bpb, load: loadIt, focus, onFocus: (ch) => setFocus(ch, 'strip'), onNote, selectionBars: pickedBars });
  editor = noteEditor({ piece: { ...piece, tracks }, colors, st: est, pid: play.id, play, editable: handEditable, focus, onFocus: (ch) => setFocus(ch, 'editor'), toast, sections,
    onChange: (n) => { unsavedNotes = n; roll?.setPiece?.(editor.piece()); },
    onSave: async ({ cells, notes, firstBar }) => {
      const r = await api(`/api/projects/${id}/music/notes-edit`, { method: 'POST', body: { cells, notes } });
      for (const k of Object.keys(est)) delete est[k];
      unsavedNotes = 0;
      toast(`In the ${step === 'pack' ? 'MIDI' : 'song'} (${r.seconds} s): ${r.edits.map((e) => `${e.part}: ${(e.facts || []).join(', ')}`).join(' · ')}`);
      await after('save', firstBar);
    } });
  roll = pianoRoll(piece, { bands, play, focusChannel: focus, colors, legend: false, current: () => editor.piece(), onPick: (bar) => editor.goTo(bar), onMark, onMoveBand });
  editor.addEventListener('view', (e) => roll.setView?.(e.detail.from, e.detail.to));
  if (est.notes) roll.setPiece?.(editor.piece());

  // The changes made by hand: what they did, measured, with Keep and Undo (a change across several instruments is one).
  let busy = false;
  const act = async (e, what) => {
    if (busy) return; busy = true;
    try {
      await api(`/api/projects/${id}/music/edits/${e.id}/${what}`, { method: 'POST' });
      if (what === 'undo') { for (const k of Object.keys(est)) delete est[k]; unsavedNotes = 0; toast('Undone: those bars are back as they were'); } else toast('Kept: the studio remembers this one');
      await after(what, e.from);
    } catch (err) { toast(err.message, true); } finally { busy = false; }
  };
  const hand = mine.filter((e) => e.kind === 'notes' || step === 'pack');
  const batches = [...new Set(hand.map((e) => e.batch || e.id))].map((b) => hand.filter((e) => (e.batch || e.id) === b));
  const list = handEditable && batches.length ? h('section', { class: 'hand-edits' }, h('h2', null, 'Your changes', h('span', { class: 'count' }, batches.length)),
    h('ul', { class: 'rows' }, batches.map((es) => h('li', null,
      h('div', { class: 'main' }, es.map((e) => h('div', null, h('b', null, e.part || `channel ${e.channel}`), ` · ${runsText(e.cells || barsFrom(e.from, e.to))}${e.kind === 'notes' ? '' : `, "${e.ask}"`}: `, h('span', { class: 'facts' }, (e.facts || []).join(' · ')))),
        es[0].warnings?.length ? h('div', { class: 'warn' }, `The check would flag: ${es[0].warnings.join(' · ')}`) : null),
      h('div', { class: 'end actions' },
        es.every((e) => e.status === 'kept') ? h('span', { class: 'chip ok' }, 'kept') : h('button', { class: 'btn', onclick: () => act(es[0], 'keep') }, 'Keep'),
        h('button', { class: 'btn link', onclick: () => act(es[0], 'undo') }, 'Undo'))))),
    h('p', { class: 'meta' }, step === 'pack' ? 'They\'re in the score and the MIDI pack now. Kept changes teach the studio what you like.' : 'Sending your review keeps any you haven\'t undone. Kept changes teach the studio what you like.')) : null;

  const el = h('div', { class: 'workbench' },
    h('h2', { class: 'stage-h' }, 'The instruments'), strip,
    h('h2', { class: 'stage-h' }, 'The whole piece'), roll,
    h('h2', { class: 'stage-h' }, handEditable ? 'The notes: change them here' : 'The notes'), editor,
    list);
  return { el, editor, roll, strip };
}
const barsFrom = (a, b) => Array.from({ length: b - a + 1 }, (_, i) => a + i);

// Bars as runs: [3, 4, 5, 9] → [[3, 5], [9, 9]], and in words: "bars 3-5, 9".
function barRuns(bars) {
  const out = [];
  for (const b of [...bars].sort((x, y) => x - y)) { const last = out.at(-1); if (last && b === last[1] + 1) last[1] = b; else out.push([b, b]); }
  return out;
}
const runsText = (bars) => `bar${bars.length === 1 ? '' : 's'} ${barRuns(bars).map(([a, b]) => (a === b ? a : `${a}-${b}`)).join(', ')}`;

// Note changes in the editor that aren't in the song yet: leaving the page asks first.
let unsavedNotes = 0;
window.addEventListener('beforeunload', (e) => { if (unsavedNotes) { e.preventDefault(); e.returnValue = ''; } });

// ---------- MIDI: a small reader and a piano roll ----------

// A Standard MIDI File → { bpm, beatsPerBar, tracks: [{ name, channel, drums, notes: [{ pitch, start, dur, velocity }] }],
// markers: [{ beat, text }] }. Times are in beats. Enough for what the studio writes and what DAWs export.
function readMidi(b) {
  const str = (o, n) => String.fromCharCode(...b.subarray(o, o + n));
  const u32 = (o) => ((b[o] << 24) | (b[o + 1] << 16) | (b[o + 2] << 8) | b[o + 3]) >>> 0;
  if (str(0, 4) !== 'MThd') throw new Error('not a MIDI file');
  const ntr = (b[10] << 8) | b[11]; const ppq = (b[12] << 8) | b[13];
  if (ppq & 0x8000) throw new Error('SMPTE timing is not supported');
  let pos = 8 + u32(4); let bpm = 120; let beatsPerBar = 4; const tracks = []; const markers = [];
  for (let t = 0; t < ntr && pos < b.length; t += 1) {
    const len = u32(pos + 4); let p = pos + 8; const end = p + len; pos = end;
    let tick = 0; let status = 0; const open = new Map(); const tr = { name: '', channel: null, notes: [] };
    const vlq = () => { let v = 0; let c; do { c = b[p++]; v = (v << 7) | (c & 0x7f); } while (c & 0x80 && p < end); return v; };
    while (p < end) {
      tick += vlq();
      let s = b[p];
      if (s & 0x80) { status = s; p += 1; } else s = status;
      if (s === 0xff) {
        const type = b[p++]; const l = vlq(); const d = b.subarray(p, p + l); p += l;
        const text = () => new TextDecoder().decode(d);
        if (type === 0x03 && !tr.name) tr.name = text();
        else if (type === 0x51) bpm = Math.round(60000000 / ((d[0] << 16) | (d[1] << 8) | d[2]) * 100) / 100;
        else if (type === 0x58) beatsPerBar = d[0] * (4 / 2 ** d[1]);
        else if (type === 0x06) markers.push({ beat: tick / ppq, text: text() });
      } else if (s === 0xf0 || s === 0xf7) p += vlq();
      else {
        const kind = s & 0xf0; const ch = s & 0x0f;
        const a = b[p++]; const v = kind === 0xc0 || kind === 0xd0 ? 0 : b[p++];
        if (kind === 0x90 && v > 0) { open.set(`${ch}:${a}`, { tick, v }); tr.channel ??= ch + 1; }
        else if (kind === 0x80 || (kind === 0x90 && v === 0)) {
          const o = open.get(`${ch}:${a}`);
          if (o) { tr.notes.push({ pitch: a, start: o.tick / ppq, dur: Math.max(1, tick - o.tick) / ppq, velocity: o.v / 127 }); open.delete(`${ch}:${a}`); }
        }
      }
    }
    if (tr.notes.length) tracks.push({ ...tr, drums: tr.channel === 10 });
  }
  return { bpm, beatsPerBar, tracks, markers };
}

// A piano roll for a piece: { bpm, beatsPerBar | beats_per_bar, tracks: [{ name, channel, notes }], markers? | sections? }.
// Quiet by default: every part in faint ink; pick a part to see it in full ink. `bands` shade bars: notes you left
// ({ from, to, label, kind: 'note' }) and bars you changed ({ kind: 'edited' }). With `onMark({ from, to, sec, channel })`
// a press-and-drag marks bars (snapped; the part you've picked comes along), and with `onMoveBand(index, from, to)` a
// note's band can be dragged elsewhere. With `play` ({ id, title, href }) it gets a play button and a playhead, and
// (when it doesn't mark) a click plays from there. With `colors` (channel → colour) each part is drawn in its colour,
// `legend: false` leaves the part chips out (the page has its own), and `onPick(bar)` hears a click that didn't drag.
// `current()` is the piece to play (an editor's copy, with changes not yet in the song). The box it returns can be
// told: setFocus(channel), setView(from, to) (the bars an editor shows), setPiece(piece) (redraw with other notes).
function pianoRoll(piece, { bands = [], onMark = null, onMoveBand = null, play = null, focusChannel = null, onFocus = null, colors = null, legend = true, onPick = null, current = null } = {}) {
  const bpb = piece.beatsPerBar || piece.beats_per_bar || 4;
  const bpm = piece.bpm || 120;
  let tracks = (piece.tracks || []).filter((t) => t.notes?.length);
  if (!tracks.length) return h('div', { class: 'meta' }, 'No notes yet.');
  const markers = piece.markers || (piece.sections || []).map((x) => ({ beat: (x.start_bar - 1) * bpb, text: x.name }));
  let all = tracks.flatMap((t) => t.notes);
  const endBeat = Math.max(...all.map((n) => n.start + n.dur), (piece.bars || 0) * bpb);
  const bars = Math.ceil(endBeat / bpb);
  // Pitched parts share the pitch rows; drums (channel 10, General MIDI) get a lane of their own underneath, one row per
  // drum sound, so a kick never sits on the bass's rows.
  const isDrums = (t) => t.drums || t.kind === 'drums' || t.channel === 10;
  let kit; let lo; let hi;
  const measure = () => {
    const pitched = tracks.filter((t) => !isDrums(t)).flatMap((t) => t.notes);
    kit = [...new Set(tracks.filter(isDrums).flatMap((t) => t.notes.map((n) => n.pitch)))].sort((a, b) => a - b);
    lo = pitched.length ? Math.min(...pitched.map((n) => n.pitch)) - 2 : 0;
    hi = pitched.length ? Math.max(...pitched.map((n) => n.pitch)) + 2 : -1;
  };
  measure();
  let focus = focusChannel === null ? null : tracks.find((t) => t.channel === focusChannel) || null;
  const canvas = h('canvas', { class: 'roll', role: 'img', 'aria-label': `Piano roll: ${tracks.length} parts, ${bars} bars at ${bpm} BPM` });
  const readout = h('span', { class: 'meta' }, `${bars} bars · ${bpm} BPM · ${bpb}/4`);
  const at = (beat) => `bar ${Math.floor(beat / bpb) + 1}, beat ${Math.floor(beat % bpb) + 1}`;
  const draw = () => {
    const css = getComputedStyle(document.documentElement);
    const color = (v) => css.getPropertyValue(v).trim();
    const rows = hi - lo + 1;
    const w = canvas.clientWidth || 800;
    const top = rows > 0 ? Math.max(120, Math.min(300, rows * 6)) : 0;
    const lane = kit.length ? Math.max(40, Math.min(120, kit.length * 9)) : 0;
    const gap = top && lane ? 8 : 0; const ht = top + gap + lane;
    const dpr = window.devicePixelRatio || 1;
    canvas.width = w * dpr; canvas.height = ht * dpr; canvas.style.height = `${ht}px`;
    const g = canvas.getContext('2d'); g.scale(dpr, dpr);
    const x = (beat) => (beat / (bars * bpb)) * w;
    const row = top / Math.max(1, rows); const drumRow = lane / Math.max(1, kit.length);
    const y = (t, pitch) => (isDrums(t) ? top + gap + kit.indexOf(pitch) * drumRow : top - (pitch - lo + 1) * row);
    const tall = (t) => Math.max(2, (isDrums(t) ? drumRow : row) - 1);
    g.clearRect(0, 0, w, ht);
    g.strokeStyle = color('--line'); g.lineWidth = 1;
    for (let bar = 0; bar <= bars; bar += 1) { g.beginPath(); g.moveTo(Math.round(x(bar * bpb)) + 0.5, 0); g.lineTo(Math.round(x(bar * bpb)) + 0.5, ht); g.stroke(); }
    if (gap) { g.beginPath(); g.moveTo(0, top + gap / 2 + 0.5); g.lineTo(w, top + gap / 2 + 0.5); g.stroke(); }
    g.fillStyle = color('--muted'); g.font = `11px ${color('--text') || 'sans-serif'}`;
    for (const m of markers) { const mx = x(m.beat); g.fillRect(mx, 0, 1, ht); g.fillText(m.text, mx + 4, 12); }
    for (const t of tracks) {
      const on = focus === null || focus === t;
      g.fillStyle = colors?.get(t.channel) || (focus === t ? color('--ink') : color('--body'));
      g.globalAlpha = on ? (focus ? 1 : 0.75) : 0.18;
      for (const n of t.notes) g.fillRect(x(n.start), y(t, n.pitch), Math.max(1.5, x(n.dur) - 1), tall(t));
    }
    g.globalAlpha = 1;
  };
  const secAt = (e) => { const r = canvas.getBoundingClientRect(); return Math.max(0, ((e.clientX - r.left) / r.width) * bars * bpb) * 60 / bpm; };
  const barAt = (e) => { const r = canvas.getBoundingClientRect(); return Math.min(bars, Math.max(1, Math.floor(((e.clientX - r.left) / r.width) * bars) + 1)); };
  const barSec = (bar) => Math.round((((bar - 1) * bpb * 60) / bpm) * 100) / 100;
  const loadMe = () => { if (player.state().id !== play.id) player.load({ ...play, piece: current ? current() : piece }); };
  // The bands: shaded bars with their number on top; a note's band can be picked up and moved.
  const bandLayer = h('div', { class: 'bands' });
  const stage = h('div', { class: 'roll-stage' }, canvas, bandLayer);
  const place = (el, from, to) => { el.style.left = `${((from - 1) / bars) * 100}%`; el.style.width = `${((to - from + 1) / bars) * 100}%`; };
  for (const b of bands) {
    const el = h('div', { class: `band ${b.kind}`, title: b.kind === 'edited' ? `Bars ${b.from}-${b.to}: changed by you` : `Your note ${b.label}` }, h('span', null, b.label));
    place(el, b.from, b.to);
    if (onMoveBand && b.kind !== 'edited' && b.index !== undefined) el.dataset.index = String(b.index);
    bandLayer.append(el);
  }
  if (onMark) {
    canvas.style.cursor = 'crosshair';
    const ghost = h('div', { class: 'band marking', hidden: true });
    bandLayer.append(ghost);
    let drag = null;
    stage.addEventListener('pointerdown', (e) => {
      if (e.button !== 0) return;
      const moving = e.target.closest('.band[data-index]');
      const bar = barAt(e);
      drag = moving ? { move: Number(moving.dataset.index), el: moving, grab: bar, from: bands.find((b) => b.index === Number(moving.dataset.index)).from, to: bands.find((b) => b.index === Number(moving.dataset.index)).to }
        : { start: bar, from: bar, to: bar, x0: e.clientX, dragged: !onPick };
      e.currentTarget.setPointerCapture(e.pointerId);
      if (!moving && !onPick) { ghost.hidden = false; place(ghost, bar, bar); }
      e.preventDefault();
    });
    stage.addEventListener('pointermove', (e) => {
      if (!drag) return;
      const bar = barAt(e);
      if (drag.move !== undefined) {
        const len = drag.to - drag.from; const from = Math.min(bars - len, Math.max(1, drag.from + bar - drag.grab));
        drag.nf = from; drag.nt = from + len; place(drag.el, from, from + len);
      } else {
        if (!drag.dragged && Math.abs(e.clientX - drag.x0) < 5) return;
        drag.dragged = true; ghost.hidden = false;
        drag.from = Math.min(drag.start, bar); drag.to = Math.max(drag.start, bar); place(ghost, drag.from, drag.to);
      }
    });
    stage.addEventListener('pointerup', () => {
      const d = drag; drag = null; ghost.hidden = true;
      if (!d) return;
      if (d.move !== undefined) { if (d.nf !== undefined && d.nf !== d.from) onMoveBand(d.move, d.nf, d.nt); return; }
      if (!d.dragged) { onPick(d.start); return; }
      onMark({ from: d.from, to: d.to, sec: barSec(d.from), channel: focus?.channel ?? null, part: focus ? focus.name : null });
    });
  } else if (onPick) {
    canvas.style.cursor = 'pointer';
    canvas.addEventListener('click', (e) => onPick(barAt(e)));
  } else if (play) {
    canvas.style.cursor = 'pointer';
    canvas.title = 'Click to play from here';
    canvas.addEventListener('click', (e) => { loadMe(); player.seek(secAt(e)); player.play(); });
  }
  canvas.addEventListener('mousemove', (e) => {
    const r = canvas.getBoundingClientRect();
    readout.textContent = at(((e.clientX - r.left) / r.width) * bars * bpb);
  });
  const legendEl = !legend ? null : h('div', { class: 'choices roll-parts', role: 'radiogroup', 'aria-label': 'Parts' },
    tracks.map((t) => h('button', { type: 'button', class: `choice${focus === t ? ' on' : ''}`, role: 'radio', 'aria-checked': String(focus === t), onclick: (e) => {
      focus = focus === t ? null : t;
      for (const b of legendEl.children) { b.classList.remove('on'); b.setAttribute('aria-checked', 'false'); }
      if (focus) { e.currentTarget.classList.add('on'); e.currentTarget.setAttribute('aria-checked', 'true'); }
      onFocus?.(focus?.channel ?? null);
      draw();
    } }, `${t.channel ? `${t.channel} · ` : ''}${t.name || 'part'}`)));
  let playBtn = null;
  if (play) {
    const head = h('div', { class: 'playhead', hidden: true });
    stage.append(head);
    playBtn = playToggle(play.id, play.title, loadMe);
    watch({ id: play.id, el: stage, update: (s) => {
      playheadAt(head, s, play.id);
      if (s.id === play.id && s.playing) readout.textContent = at((s.time * bpm) / 60);
    } });
  }
  const view = h('div', { class: 'band view', hidden: true });
  bandLayer.prepend(view);
  const count = h('span', { class: 'meta' }, `${tracks.length} parts · ${all.length} notes`);
  const box = h('div', { class: 'roll-box' }, h('div', { class: 'roll-head' }, playBtn, legendEl, legend || !onPick ? null : h('span', { class: 'meta' }, onMark ? 'Click to go there in the notes below; drag across bars to leave a note on them.' : 'Click to go there in the notes below.')), stage, h('div', { class: 'roll-foot' }, readout, count));
  box.setFocus = (ch) => { focus = ch === null ? null : tracks.find((t) => t.channel === ch) || null; draw(); };
  box.setView = (from, to) => { view.hidden = false; place(view, from, to); };
  box.setPiece = (p) => { tracks = (p.tracks || []).filter((t) => t.notes?.length); all = tracks.flatMap((t) => t.notes); if (focus) focus = tracks.find((t) => t.channel === focus.channel) || null; measure(); count.textContent = `${tracks.length} parts · ${all.length} notes`; draw(); };
  requestAnimationFrame(draw);
  new ResizeObserver(() => draw()).observe(canvas);
  return box;
}

// ---------- start ----------

playerBar();
await loadStudio();
connect();
render();
