// Mortiflix Music studio (web). No framework: a tiny DOM builder, hash routes, and live updates over SSE.
// Everything the studio or a session wrote is inserted as text, never as HTML.

const $app = document.getElementById('app');
const $status = document.getElementById('top-status');
let studio = null;
let projectsCache = [];
let current = { name: null, refresh: null };
let leaveView = null; // a view that holds something open releases it here

// ---------- helpers ----------

function h(tag, attrs, ...kids) {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs || {})) {
    if (v === null || v === undefined || v === false) continue;
    if (k === 'class') el.className = v;
    else if (k.startsWith('on')) el.addEventListener(k.slice(2), v);
    else if (k === 'style') Object.assign(el.style, v);
    else if (k === 'value') el.value = v;
    else if (k === 'checked') el.checked = Boolean(v);
    else el.setAttribute(k, v === true ? '' : v);
  }
  for (const kid of kids.flat(Infinity)) {
    if (kid === null || kid === undefined || kid === false) continue;
    el.append(kid instanceof Node ? kid : document.createTextNode(String(kid)));
  }
  return el;
}

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
  current = { name: view.name, refresh: null, args: path.match(re)?.slice(1) || [] };
  // The review room is wider; the header widens with it so both keep one left edge.
  document.body.classList.toggle('wide', view === reviewView);
  try {
    await view(...current.args);
  } catch (e) {
    fill($app, h('div', { class: 'empty' }, h('div', { class: 'big' }, 'Something went wrong'), e.message));
  }
}

// replaceChildren() would print null as "null": skipped sections are dropped here.
function fill(el, ...kids) {
  el.replaceChildren(...kids.flat(Infinity).filter((k) => k !== null && k !== undefined && k !== false));
}

// append() would print null as "null" too.
function add(el, ...kids) {
  el.append(...kids.flat(Infinity).filter((k) => k !== null && k !== undefined && k !== false));
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

// ---------- the studio (home) ----------

async function studioView() {
  const projects = projectsCache = await api('/api/projects');
  const turn = projects.filter((p) => p.state === 'waiting' || p.state === 'paused');
  const prod = projects.filter((p) => p.state === 'queued');
  const done = projects.filter((p) => p.state === 'delivered');
  const other = projects.filter((p) => p.state === 'draft' || p.state === 'cancelled');

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
  const prodRow = (p) => h('li', { class: 'row' },
    p.working ? h('span', { class: 'ico' }, h('span', { class: 'pulse' })) : icon('ready'),
    h('div', { class: 'main' }, h('a', { class: 'title', href: `#/p/${p.id}` }, p.title),
      h('div', { class: 'meta' }, p.working ? (p.status?.text || `Working on ${p.current?.name || 'it'}`) : 'Waiting for its turn')),
    h('div', { class: 'end' }, h('span', { class: 'chip' }, p.pipeline.name), `${p.done}/${p.total}`));
  const doneRow = (p) => h('li', { class: 'row' }, icon('done'),
    h('div', { class: 'main' }, h('a', { class: 'title', href: `#/p/${p.id}` }, p.title)),
    h('div', { class: 'end' }, h('span', { class: 'chip' }, p.pipeline.name), ago(p.delivered_at)));
  const otherRow = (p) => h('li', { class: 'row' }, icon('blocked'),
    h('div', { class: 'main' }, h('a', { class: 'title', href: `#/p/${p.id}` }, p.title)),
    h('div', { class: 'end' }, p.state));

  const section = (title, list, row) => (list.length ? h('section', null, h('h2', null, title, h('span', { class: 'count' }, list.length)), h('ul', { class: 'rows' }, list.map(row))) : null);

  mount(
    h('div', { class: 'head' }, h('h1', null, 'Studio'), h('div', { class: 'actions' }, h('a', { class: 'btn solid', href: '#/new' }, 'New piece'))),
    projects.length ? null : h('section', null, h('div', { class: 'empty' },
      h('div', { class: 'big' }, 'Nothing in production yet.'),
      h('p', null, 'Pick what to make, write the brief, and approve each stage as it comes in.'),
      h('div', { class: 'actions' }, h('a', { class: 'btn solid', href: '#/new' }, 'Make a song'),
        h('button', { class: 'btn', onclick: startDemo }, 'Try the demo'), h('span', { class: 'meta' }, 'a song with placeholder work, free')))),
    section('Your turn', turn, (p, i) => turnRow(p, i === 0)),
    section('In production', prod, prodRow),
    section('Delivered', done, doneRow),
    other.length ? h('section', null, h('details', null, h('summary', null, `Drafts and cancelled (${other.length})`), h('ul', { class: 'rows' }, other.map(otherRow)))) : null,
  );
  current.refresh = () => studioView();
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
function musicPanel(id, ms) {
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
  const facts = own ? `${own.source}: ${own.measured.seconds} s, ${own.measured.lufs} LUFS, true peak ${own.measured.true_peak} dBTP` : e ? `${e.bars} bars at ${e.bpm} BPM (${e.meter}), ${e.seconds} s${ms.daw ? ` · import steps for ${ms.daw}` : ''}` : '';
  const roll = h('div', { class: 'roll-wrap' });
  api(`/api/projects/${id}/music/notes`).then((n) => fill(roll, pianoRoll(n))).catch(() => roll.remove());
  return h('section', null, h('h2', null, 'MIDI'),
    h('div', { class: waiting ? 'turn-line' : 'state-line' },
      h('div', null,
        h('div', { class: 'what' }, waiting ? 'Finish it in your DAW' : own ? 'Your master is the music' : 'The MIDI is ready'),
        h('div', { class: 'why' }, waiting ? 'Every channel is its own MIDI file, with a clip per section and the tempo and sections as markers. Give each channel its sound, mix, export from bar 1, then import it here. ' : '', facts)),
      h('div', { class: 'actions' },
        h('a', { class: 'btn', href: `/api/projects/${id}/music/midi.zip` }, 'Download MIDI'),
        ms.masters || own ? [h('button', { class: waiting ? 'btn primary' : 'btn', onclick: () => input.click() }, own ? 'Replace master' : 'Import your master'), input] : null)),
    roll,
    own ? h('audio', { src: `/api/projects/${id}/music/master?v=${encodeURIComponent(own.imported_at)}`, controls: true, preload: 'none', style: { width: '100%', marginTop: '8px' } }) : null,
    own?.warnings?.length ? h('ul', { class: 'meta' }, own.warnings.map((w) => h('li', null, w))) : null);
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
  const musicBlock = ms?.midi?.length ? musicPanel(id, ms) : null;

  const deliverables = p.deliverables.length ? h('section', { class: 'deliver' }, h('h2', null, 'Delivered'),
    p.deliverables.map((f) => h('div', null,
      f.kind === 'audio' ? h('audio', { src: fileUrl(id, f.file), controls: true }) : null,
      h('div', { class: 'row' }, icon('done'), h('div', { class: 'main' }, h('span', { class: 'title' }, f.label)), h('div', { class: 'end' }, h('a', { href: fileUrl(id, f.file, true) }, 'Download')))))) : null;

  const questions = openQs.length ? h('section', null, h('h2', null, 'Questions'), openQs.map((q) => questionForm(id, q))) : null;

  // How it landed: a rating teaches the studio your taste; a piece you love can become a flow of your own.
  const tasteBlock = p.flow && (p.state === 'delivered' || p.rating) ? tasteSection(id, p, Boolean(ms?.expect)) : null;

  // The MIDI, once the score passes its check; and, when the brief chose to finish it in the owner's own DAW, the
  // place to import that master.
  const subsByStep = (key) => d.submissions.filter((s) => s.step === key);
  const steps = h('section', null, h('h2', null, 'Steps', h('span', { class: 'count' }, `${d.steps.filter((s) => ['approved', 'done', 'skipped'].includes(s.state)).length}/${d.steps.length}`)),
    h('ul', { class: 'rows' }, d.steps.map((s) => h('li', { class: 'row' }, icon(s.state),
      h('div', { class: 'main' }, h('span', { class: 'title' }, s.name),
        h('div', { class: 'meta' }, s.review === 'internal' ? 'made in the studio' : `you review ${s.review === 'questions' ? 'questions' : s.review === 'midi' ? 'the MIDI' : `the ${s.review}`}`,
          subsByStep(s.key).length ? ' · ' : '', subsByStep(s.key).map((x, i) => [i ? ' ' : '', h('a', { href: `#/p/${id}/review/${s.key}/${x.version}` }, `v${x.version}`)]))),
      h('div', { class: 'end' }, s.state === 'in_review' ? h('a', { href: `#/p/${id}/review/${s.key}` }, 'Review') : h('span', null, STATE_WORDS[s.state]))))));

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
    h('a', { class: 'crumb', href: '#/' }, '← Studio'),
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

  const stageBox = h('div');
  const notesBox = h('div');
  const docBox = h('div');

  function whereText(n) {
    const it = n.item !== null && n.item !== undefined ? sub.items[n.item] : null;
    return [it?.label, n.time_sec !== undefined ? `at ${clock(n.time_sec)}` : null, n.x !== undefined ? 'pinned spot' : null, n.paragraph !== undefined ? `paragraph ${n.paragraph + 1}` : null].filter(Boolean).join(' · ') || 'general';
  }

  function addNote(n) {
    if (!editable) return;
    draft.notes.push({ text: '', ...n });
    save();
    drawNotes(true);
    drawStage();
    drawDoc();
  }

  function drawStage() {
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
      fetch(url).then((r) => r.arrayBuffer()).then((buf) => {
        const midi = readMidi(new Uint8Array(buf));
        const marks = (editable ? draft.notes : sub.feedback?.notes || []).filter((n) => n.item === sel && n.time_sec !== undefined).map((n) => n.time_sec);
        fill(stage, pianoRoll(midi, { marks, onPick: editable ? (t) => addNote({ item: sel, time_sec: t }) : null }));
      }).catch((e) => fill(stage, h('div', { class: 'err' }, `Couldn't read this MIDI file: ${e.message}`)));
      if (editable) tools.append(h('span', { class: 'meta' }, 'Click the roll to note a moment.'));
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
        h('textarea', { placeholder: 'What should change (or what you love)?', value: n.text, oninput: (e) => { n.text = e.target.value; save(); } })),
      h('button', { class: 'x', 'aria-label': `Remove note ${k + 1}`, onclick: () => { draft.notes.splice(k, 1); save(); drawNotes(); drawStage(); drawDoc(); } }, '×'))));
    fill(notesBox, h('section', null, h('h2', null, 'Your notes', h('span', { class: 'count' }, draft.notes.length || '')),
      draft.notes.length ? list : h('p', { class: 'meta' }, media.length ? 'Note a moment on the MIDI or the audio, or comment on a paragraph. No notes needed to approve.' : 'Comment on a paragraph, or approve as it is.'),
      h('button', { class: 'btn link', onclick: () => addNote({}) }, 'Add a general note')));
    if (focusLast) list.querySelector('li:last-child textarea')?.focus();
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
    const notes = draft.notes.map((n) => ({ item: n.item ?? null, x: n.x, y: n.y, time_sec: n.time_sec, paragraph: n.paragraph, text: (n.text || '').trim() }));
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
// Quiet by default: every part in faint ink; pick a part to see it in full ink. `marks` are noted moments (seconds),
// and `onPick(seconds)` makes a click note that moment.
function pianoRoll(piece, { marks = [], onPick = null } = {}) {
  const bpb = piece.beatsPerBar || piece.beats_per_bar || 4;
  const bpm = piece.bpm || 120;
  const tracks = (piece.tracks || []).filter((t) => t.notes?.length);
  if (!tracks.length) return h('div', { class: 'meta' }, 'No notes yet.');
  const markers = piece.markers || (piece.sections || []).map((x) => ({ beat: (x.start_bar - 1) * bpb, text: x.name }));
  const all = tracks.flatMap((t) => t.notes);
  const endBeat = Math.max(...all.map((n) => n.start + n.dur), (piece.bars || 0) * bpb);
  const bars = Math.ceil(endBeat / bpb);
  // Pitched parts share the pitch rows; drums (channel 10, General MIDI) get a lane of their own underneath, one row per
  // drum sound, so a kick never sits on the bass's rows.
  const isDrums = (t) => t.drums || t.kind === 'drums' || t.channel === 10;
  const pitched = tracks.filter((t) => !isDrums(t)).flatMap((t) => t.notes);
  const kit = [...new Set(tracks.filter(isDrums).flatMap((t) => t.notes.map((n) => n.pitch)))].sort((a, b) => a - b);
  const lo = pitched.length ? Math.min(...pitched.map((n) => n.pitch)) - 2 : 0;
  const hi = pitched.length ? Math.max(...pitched.map((n) => n.pitch)) + 2 : -1;
  let focus = null;
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
      g.fillStyle = focus === t ? color('--ink') : color('--body');
      g.globalAlpha = on ? (focus ? 1 : 0.75) : 0.18;
      for (const n of t.notes) g.fillRect(x(n.start), y(t, n.pitch), Math.max(1.5, x(n.dur) - 1), tall(t));
    }
    g.globalAlpha = 1; g.fillStyle = color('--pin');
    for (const sec of marks) g.fillRect(x((sec * bpm) / 60), 0, 2, ht);
  };
  if (onPick) {
    canvas.style.cursor = 'crosshair';
    canvas.addEventListener('click', (e) => {
      const r = canvas.getBoundingClientRect();
      const beat = Math.max(0, ((e.clientX - r.left) / r.width) * bars * bpb);
      onPick(Math.round(((beat * 60) / bpm) * 100) / 100);
    });
  }
  canvas.addEventListener('mousemove', (e) => {
    const r = canvas.getBoundingClientRect();
    readout.textContent = at(((e.clientX - r.left) / r.width) * bars * bpb);
  });
  const legend = h('div', { class: 'choices roll-parts', role: 'radiogroup', 'aria-label': 'Parts' },
    tracks.map((t) => h('button', { type: 'button', class: 'choice', role: 'radio', 'aria-checked': 'false', onclick: (e) => {
      focus = focus === t ? null : t;
      for (const b of legend.children) { b.classList.remove('on'); b.setAttribute('aria-checked', 'false'); }
      if (focus) { e.currentTarget.classList.add('on'); e.currentTarget.setAttribute('aria-checked', 'true'); }
      draw();
    } }, `${t.channel ? `${t.channel} · ` : ''}${t.name || 'part'}`)));
  const box = h('div', { class: 'roll-box' }, legend, canvas, h('div', { class: 'roll-foot' }, readout, h('span', { class: 'meta' }, `${tracks.length} parts · ${all.length} notes`)));
  requestAnimationFrame(draw);
  new ResizeObserver(() => draw()).observe(canvas);
  return box;
}

// ---------- start ----------

await loadStudio();
connect();
render();
