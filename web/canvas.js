// The canvas: a project as a FilmCraft-style workspace. A 38 px header with Import / Edit / Export, then panels on
// black gutters, each part of the work in its own corner, and the board along the bottom where FilmCraft has its
// timeline. The AI fills it as it works; the owner looks, plays and steers from here.
//
// app.js passes its helpers in (`ui`), so this module has no imports of its own and no copy of them.

export function canvasView(ui, id, mode = 'edit') {
  const { h, fill, api, fileUrl, clock, store } = ui;
  const keepKey = `mfx-canvas-keep-${id}`;
  let d = null;
  let graph = null;
  let pieces = [];
  let sel = 0;
  let keep = store.get(keepKey) ?? true;
  let advance = null;

  const monitor = h('div', { class: 'cv-body cv-monitor' });
  const board = h('div', { class: 'cv-body cv-board' });
  const doing = h('div', { class: 'cv-body cv-doing' });
  const head = h('header', { class: 'cv-head' });
  const root = h('div', { class: 'canvas' });

  const panel = (name, body, extra = null, cls = '') => h('section', { class: `cv-panel ${cls}` },
    h('div', { class: 'cv-tabs' }, h('span', { class: 'cv-tab on' }, name), extra), body);

  // The shots, in film order, each in its beat's colour: from the project graph (src/canvas.mjs). A shot plays its own
  // seconds of the animatic; a planned shot (before the animatic) shows its style frame.
  function shotPieces() {
    const n = graph?.nodes || {};
    return (graph?.order?.shots || []).map((sid) => {
      const s = n[sid];
      const v = s.versions?.find((x) => x.v === s.current);
      const look = s.look ? n[s.look] : null;
      const lookFile = look?.versions?.find((x) => x.v === look.current)?.file || null;
      const beat = s.beat ? n[s.beat] : null;
      return { kind: v?.file ? 'shot' : lookFile ? 'image' : 'planned', id: sid, file: v?.file || lookFile, from: v?.from_sec ?? null, to: v?.to_sec ?? null,
        still: lookFile, label: `${s.label} · ${s.title || ''}`.trim(), beatLabel: beat?.label || null, beatTitle: beat?.title || null, colour: s.colour,
        stepName: v?.file ? 'Animatic' : 'Planned shot', version: s.current || 1, planned: !v?.file, step: null };
    });
  }

  // Every picture, video and sound the project has so far, in pipeline order (latest version of each step).
  function collect() {
    const shots = shotPieces();
    if (shots.length) return shots;
    const out = [];
    for (const s of d.steps) {
      const sub = d.submissions.filter((x) => x.step === s.key).at(-1);
      if (!sub) continue;
      sub.items.forEach((it, i) => {
        if (it.kind === 'text' && !it.text) return;
        out.push({ ...it, step: s.key, stepName: s.name, version: sub.version, i });
      });
    }
    return out;
  }

  function drawHead() {
    const tab = (key, label, href, live = true) => h('a', { class: `cv-mode${mode === key ? ' on' : ''}${live ? '' : ' off'}`, href: live ? href : null }, label);
    const finalSub = d.submissions.some((s) => d.steps.find((x) => x.key === s.step)?.review === 'video' && s.step === d.steps.at(-1)?.key);
    const turn = d.steps.find((s) => s.state === 'in_review');
    fill(head,
      h('a', { class: 'cv-brand', href: '#/' }, h('img', { src: 'img/mark.png', alt: '' }), h('span', null, 'Mortiflix')),
      h('span', { class: 'cv-title' }, d.title),
      h('nav', { class: 'cv-modes' },
        tab('import', 'Import', `#/p/${id}/canvas/import`),
        tab('edit', 'Edit', `#/p/${id}/canvas`),
        tab('export', 'Export', `#/p/${id}/canvas/export`, finalSub)),
      h('div', { class: 'cv-status' },
        d.status ? h('span', { class: 'cv-live' }, d.running ? h('span', { class: 'pulse', 'aria-hidden': 'true' }) : null, h('span', null, d.status)) : null,
        turn ? h('a', { class: 'btn primary', href: `#/p/${id}/review/${turn.key}` }, `Your turn: ${turn.name}`) : null,
        h('a', { class: 'cv-out', href: `#/p/${id}` }, 'Project page')));
  }

  function media(it, { autoplay = false } = {}) {
    const url = fileUrl(id, it.file || '');
    if (it.kind === 'shot') {
      // Only this shot's seconds of the video: start at its first frame, stop (or go on) at its last.
      const v = h('video', { src: `${url}#t=${it.from},${it.to}`, controls: true, preload: 'auto', playsinline: true });
      v.addEventListener('loadedmetadata', () => { v.currentTime = it.from; if (autoplay) v.play().catch(() => {}); });
      v.addEventListener('timeupdate', () => { if (v.currentTime >= it.to - 0.02) { v.pause(); if (keep && !v.dataset.done) { v.dataset.done = '1'; go(sel + 1, true); } } });
      return v;
    }
    if (it.kind === 'planned') return h('div', { class: 'cv-empty' }, h('div', { class: 'big' }, 'Planned'), h('p', null, 'This shot is made with the animatic.'));
    if (it.kind === 'image') return h('img', { src: url, alt: it.label });
    if (it.kind === 'video') {
      const v = h('video', { src: url, controls: true, preload: 'metadata', autoplay, playsinline: true });
      v.addEventListener('ended', () => { if (keep) go(sel + 1, true); });
      return v;
    }
    if (it.kind === 'audio') {
      const a = h('audio', { src: url, controls: true, autoplay });
      a.addEventListener('ended', () => { if (keep) go(sel + 1, true); });
      return a;
    }
    return h('pre', { class: 'cv-text' }, it.text || '');
  }

  function drawMonitor(autoplay = false) {
    clearTimeout(advance);
    const it = pieces[sel];
    if (!it) {
      return fill(monitor, h('div', { class: 'cv-empty' }, h('div', { class: 'big' }, 'Nothing to show yet'),
        h('p', null, d.status || 'The first frames appear here as soon as they are made.')));
    }
    // A still holds for 3 s when "Keep playing" is on, then the next piece plays.
    if (autoplay && keep && it.kind === 'image') advance = setTimeout(() => go(sel + 1, true), 3000);
    fill(monitor,
      h('div', { class: 'cv-stage' }, media(it, { autoplay })),
      h('div', { class: 'cv-caption' },
        it.beatLabel ? h('span', { class: 'cv-beat', style: { borderLeft: `3px solid ${it.colour?.chip || 'transparent'}` } }, it.beatLabel) : null,
        h('span', { class: 'cv-label' }, it.label),
        h('span', { class: 'cv-meta' }, `${it.stepName} · v${it.version}${it.from !== null && it.from !== undefined ? ` · ${clock(it.from)}–${clock(it.to)}` : ''} · ${sel + 1} of ${pieces.length}`),
        it.step ? h('a', { href: `#/p/${id}/review/${it.step}/${it.version}` }, 'Open in the review room') : null),
      it.beatTitle ? h('div', { class: 'cv-says' }, it.beatTitle) : null);
  }

  function drawBoard() {
    if (!pieces.length) return fill(board, h('div', { class: 'cv-empty' }, 'Pieces line up here, in order, as they are made.'));
    fill(board, h('ol', { class: 'cv-strip' }, pieces.map((it, k) => h('li', null,
      h('button', { class: `cv-card${k === sel ? ' on' : ''}`, onclick: () => go(k, true), title: it.label, style: it.colour ? { background: it.colour.hex } : null },
        it.beatLabel ? h('span', { class: 'cv-ribbon', style: { background: it.colour?.chip } }, it.beatLabel) : null,
        h('span', { class: `cv-thumb kind-${it.kind}` },
          it.still ? h('img', { src: fileUrl(id, it.still), alt: '', loading: 'lazy' })
          : it.kind === 'image' ? h('img', { src: fileUrl(id, it.file), alt: '', loading: 'lazy' })
            : it.kind === 'video' ? h('video', { src: `${fileUrl(id, it.file)}#t=0.5`, preload: 'metadata', muted: true })
              : h('span', { class: 'cv-kind' }, it.kind === 'audio' ? 'Sound' : 'Text')),
        h('span', { class: 'cv-name' }, it.label),
        h('span', { class: 'cv-step' }, it.stepName))))));
    board.querySelector('.cv-card.on')?.scrollIntoView({ block: 'nearest', inline: 'nearest' });
  }

  function drawDoing() {
    const log = h('div', { class: 'log', id: `log-${id}` });
    for (const a of d.activity || []) ui.appendLog(log, a, false);
    fill(doing,
      h('div', { class: 'cv-now' }, d.running ? h('span', { class: 'pulse', 'aria-hidden': 'true' }) : null, h('span', null, d.status || 'Waiting')),
      h('ol', { class: 'cv-steps' }, d.steps.map((s) => h('li', { class: `st-${s.state}` }, ui.icon(s.state), h('span', null, s.name)))),
      log);
    log.scrollTop = log.scrollHeight;
  }

  function go(k, autoplay = false) {
    if (k < 0 || k >= pieces.length) return;
    sel = k;
    drawMonitor(autoplay);
    drawBoard();
  }

  async function load() {
    const raw = await api(`/api/projects/${id}`);
    const p = raw.project;
    const runner = ui.studio()?.runner;
    graph = await api(`/api/projects/${id}/canvas`).catch(() => null);
    d = { ...raw, title: p.title, status: p.status || (p.state === 'waiting' ? 'Waiting for you' : p.state === 'done' ? 'Delivered' : null),
      running: runner?.project === id, intake: p.intake, pipelineName: raw.pipeline?.name, intakeQs: raw.pipeline?.intake || [] };
    const keyOf = (x) => x.id || `${x.step}/${x.version}/${x.i}`;
    const before = pieces[sel] ? keyOf(pieces[sel]) : null;
    pieces = collect();
    const at = before ? pieces.findIndex((x) => keyOf(x) === before) : -1;
    if (at >= 0) sel = at; else sel = Math.min(sel, Math.max(0, pieces.length - 1));
  }

  function drawImport() {
    const brief = Object.entries(d.intake?.answers || {}).filter(([, v]) => String(v).trim());
    return h('div', { class: 'cv-grid cv-grid-one' },
      panel('Idea', h('div', { class: 'cv-body' },
        h('dl', { class: 'cv-brief' }, h('dt', null, 'Name'), h('dd', null, d.title), h('dt', null, 'Preset'), h('dd', null, d.pipelineName),
          brief.map(([k, v]) => [h('dt', null, d.intakeQs.find((q) => q.id === k)?.label || k), h('dd', null, String(v))])))));
  }

  function draw() {
    drawHead();
    if (mode === 'import') return fill(root, head, drawImport());
    const keepBox = h('label', { class: 'cv-keep' }, h('input', { type: 'checkbox', checked: keep, onchange: (e) => { keep = e.target.checked; store.set(keepKey, keep); } }), 'Keep playing into the next piece');
    fill(root, head, h('div', { class: 'cv-grid' },
      panel('Monitor', monitor, null, 'cv-p-monitor'),
      panel('What the AI is doing', doing, null, 'cv-p-doing'),
      panel('Board', board, keepBox, 'cv-p-board')));
    drawMonitor();
    drawBoard();
    drawDoing();
  }

  const onKey = (e) => {
    if (e.target.closest('input, textarea, select')) return;
    if (e.key === 'ArrowRight') go(sel + 1, true);
    else if (e.key === 'ArrowLeft') go(sel - 1, true);
  };

  return {
    async show() {
      await load();
      document.body.classList.add('in-canvas');
      document.addEventListener('keydown', onKey);
      draw();
      return root;
    },
    async refresh() {
      const playing = monitor.querySelector('video, audio');
      const busy = playing && !playing.paused;
      await load();
      drawHead();
      if (mode === 'import') return draw();
      drawBoard();
      drawDoing();
      if (!busy) drawMonitor();
    },
    leave() {
      clearTimeout(advance);
      document.removeEventListener('keydown', onKey);
      document.body.classList.remove('in-canvas');
    },
  };
}
