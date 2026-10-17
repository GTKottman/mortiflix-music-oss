// The Mortiflix Studio service (part 08): every handler the app's parts use, driven in-process with the demo
// backend (no Claude, no cost) and the studio's runner stepped by hand so each step is checked.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { writeFileSync, existsSync, mkdirSync, chmodSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { tempStudio } from './helpers.mjs';
import { createService } from '../src/service.mjs';
import { Runner } from '../src/runner.mjs';
import { loadProject } from '../src/projects.mjs';
import { saveConfig } from '../src/studio.mjs';
import { videosDir } from '../src/svc/export.mjs';

const hasFfmpeg = spawnSync('ffmpeg', ['-version']).status === 0;
const sleep = (ms) => new Promise((ok) => setTimeout(ok, ms));

function harness(t, root) {
  const out = [];
  const svc = createService({ root, send: (m) => out.push(m), runner: false, tickMs: 0 });
  t.after(() => svc.close());
  let n = 1;
  const call = async (type, params = {}) => {
    const id = n++;
    await svc.handle({ id, type, params });
    const r = out.find((m) => m.id === id);
    if (!r.ok) { const e = new Error(`${type}: ${r.error.code}: ${r.error.message}`); e.code = r.error.code; throw e; }
    return r.result;
  };
  const events = (name) => out.filter((m) => m.event === name).map((m) => m.data);
  // One tick of the service (chat from events, policies, node changes, statuses).
  const tick = async () => { svc.poke(0); await sleep(30); };
  return { svc, call, events, out, tick };
}

async function until(fn, { ms = 15_000, step = 25 } = {}) {
  const t0 = Date.now();
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() - t0 > ms) throw new Error('timed out');
    await sleep(step);
  }
}

test('service core: presets list, save with steps off, look, voice and theme, then delete', async (t) => {
  const root = tempStudio(t);
  const { call } = harness(t, root);
  const list = await call('presets.list');
  const ex = list.built_in.find((p) => p.slug === 'explainer');
  assert.ok(ex);
  assert.equal(ex.idea_field, 'topic');
  assert.ok(ex.stages.includes('shots'));
  assert.equal(ex.steps.find((s) => s.key === 'final').stage, 'render');
  const pic = join(root, 'ref.png');
  writeFileSync(pic, 'png');
  const saved = await call('presets.save', { from: 'explainer', name: 'My explainer', defaults: { length: '30 s' }, steps: { transitions: { on: false } },
    look: { words: 'Warm paper', frames: 4, pictures: [pic] }, voice: { name: 'Ryan', delivery: 'calm' }, music_theme: { mood: ['calm'], genre: 'ambient' } });
  assert.equal(saved.slug, 'my-explainer');
  assert.equal(saved.preset.steps.find((s) => s.key === 'transitions').off, true);
  assert.equal(saved.preset.length, '30 s');
  assert.deepEqual(saved.preset.look.pictures, ['look/ref.png']);
  assert.equal(saved.preset.music_theme.genre, 'ambient');
  await assert.rejects(call('presets.save', { from: 'explainer', name: 'Nope', steps: { script: { on: false } } }), /always runs/);
  await assert.rejects(call('presets.save', { from: 'explainer', name: 'My explainer' }), /already have a preset/);
  assert.equal((await call('presets.list')).yours.length, 1);
  await assert.rejects(call('presets.delete', { slug: 'explainer' }), /built in/);
  await call('presets.delete', { slug: 'my-explainer' });
  assert.equal((await call('presets.list')).yours.length, 0);
});

test('service core: new project from preset + idea + assets + theme, the whole flow on the demo backend', { skip: !hasFfmpeg && 'needs ffmpeg' }, async (t) => {
  const root = tempStudio(t);
  const { call, events, tick, out } = harness(t, root);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());
  const run = async () => { await runner.loop(); await tick(); };

  // Missing idea: refused before anything is made.
  await assert.rejects(call('project.new', { preset: 'explainer', title: 'X' }), /needed/);
  const logo = join(root, 'logo.svg');
  writeFileSync(logo, '<svg/>');
  const made = await call('project.new', { preset: 'explainer', title: 'Bikes', idea: 'Why bike lanes pay off', backend: 'demo',
    assets: [{ path: logo, use: 'must', where: ['b04'], note: 'Last shot, bottom right' }], music: { theme: { mood: ['calm'], genre: 'piano' } } });
  const id = made.project.id;
  assert.equal(made.assets_pending, 1);
  await until(() => loadProject(root, id).state === 'queued');
  const added = events('assets.added')[0].asset;
  assert.equal(added.id, 'a01');
  assert.ok(existsSync(added.path));
  assert.ok(events('assets.progress').length >= 1);
  assert.equal((await call('music.theme.get')).theme.picked_by, 'you');

  // The brief's questions all have defaults: they never block (the policy approves them and says so in the chat).
  await run();
  await run();
  await run();
  const chat = await call('chat.history');
  assert.equal(chat[0].role, 'user');
  assert.match(chat.map((r) => r.text).join('\n'), /Going with: /);
  assert.ok(chat.some((r) => r.card?.kind === 'script' && r.card.beats.length === 4));
  assert.ok(chat.some((r) => r.card?.kind === 'looks'));
  assert.ok(chat.some((r) => r.role === 'action' && /Added logo\.svg/.test(r.text)));

  const g = await call('graph.get');
  assert.deepEqual(g.order.film, ['s01', 's02', 's03', 's04']);
  assert.deepEqual(g.beats.map((b) => b.no), [1, 2, 3, 4]);
  assert.ok(g.nodes.find((n) => n.id === 'b01-1')?.versions[0].path, 'demo voice lines');
  assert.ok(g.edges.some((e) => e.kind === 'next' && e.from === 's01'));
  assert.ok(g.edges.some((e) => e.kind === 'says' && e.from === 'b02-1' && e.to === 's02'));
  assert.ok(g.edges.some((e) => e.kind === 'under' && e.from === 'm01'));
  assert.ok(g.nodes.find((n) => n.id === 's01').default_pos);
  assert.ok(events('node.changed').length >= 1);

  const st = await call('stage.status');
  assert.equal(st.length, 6);
  assert.equal(st[0].key, 'script');
  assert.equal(st[0].kind, 'needsYou', 'four beats wait for an OK');
  assert.equal(st[2].total, 4, '4 shots: the 3 transitions never count as shots');
  assert.ok(events('status').at(-1).stages);

  // Preview plan: every shot plays its own piece of the animatic, voice at its time, the score.
  const pv = await call('preview.plan');
  assert.equal(pv.shots.length, 4);
  assert.equal(pv.shots[1].media.kind, 'video');
  assert.deepEqual([pv.shots[1].start_sec, pv.shots[1].dur_sec], [2, 2]);
  assert.equal(pv.voice.length, 4);
  assert.ok(pv.music?.path);
  assert.deepEqual((await call('preview.plan', { scope: 'beat', node: 'b03' })).range, [4, 6]);

  // OK, Fix (a session remakes only that shot), the chat lines, node.changed for it.
  await call('node.edit', { node: 's01', action: 'ok' });
  const fix = await call('node.edit', { node: 's02', action: 'fix', text: 'bluer' });
  assert.equal(fix.edit.node, 's02');
  await run();
  const s02 = (await call('graph.get')).nodes.find((n) => n.id === 's02');
  assert.equal(s02.current, 2, JSON.stringify(s02.versions.map(({ v, file, from_sec, to_sec, sha }) => ({ v, file, from_sec, to_sec, sha: sha.slice(0, 30) }))) + JSON.stringify(s02.edits));
  assert.ok(events('node.changed').some((e) => e.nodes.some((n) => n.id === 's02' && n.current === 2)));
  const h2 = await call('chat.history');
  assert.ok(h2.some((r) => r.role === 'action' && r.text === 'OK on S01'));
  assert.ok(h2.some((r) => r.role === 'ai' && /S02 is remade \(E001\)/.test(r.text)));

  // Typing in a node context is a Fix there; on an OK'd node it asks first; in the whole video the AI answers.
  const typed = await call('chat.send', { text: 'slower please', context: 's03' });
  assert.equal(typed.move.kind, 'fix');
  const asked = await call('chat.send', { text: 'redo it', context: 's01' });
  assert.equal(asked.move, null);
  assert.match((await call('chat.history')).at(-1).text, /S01 is OK'd\. Remake it anyway\?/);
  await call('chat.send', { text: 'redo it', context: 's01', confirm: true });
  await call('chat.send', { text: 'Make the whole thing warmer', context: 'video' });
  await until(async () => (await call('chat.history')).some((r) => r.role === 'ai' && /Demo: noted/.test(r.text)));
  assert.deepEqual(events('chat.typing').map((e) => e.on).slice(0, 2), [true, false]);
  await run();

  // Free placement, reorder = rewire, pick an older version, insert a new shot.
  assert.equal((await call('node.move', { positions: { s01: [10, 20], s02: [400.25, 0], bogus: [1, 2], s03: 'x' } })).moved, 2);
  assert.deepEqual((await call('graph.get')).nodes.find((n) => n.id === 's02').pos, [400.3, 0]);
  assert.deepEqual((await call('node.reorder', { order: ['s02', 's01'] })).film, ['s02', 's01', 's03', 's04']);
  assert.equal((await call('node.pick', { node: 's02', v: 1 })).plays, 1);
  assert.equal((await call('preview.plan')).shots[0].version, 1);
  const ins = await call('node.insert', { after: 's01', text: 'a close-up of a bell' });
  assert.equal(ins.node, 's05');
  await run();
  const g2 = await call('graph.get');
  assert.deepEqual(g2.order.film, ['s02', 's01', 's05', 's03', 's04']);
  assert.equal(g2.nodes.find((n) => n.id === 's05').current, 1);
  assert.deepEqual((await call('node.find', { query: 'bell' })).map((n) => n.id), []);
  assert.ok((await call('node.find', { query: 's05' })).length === 1);

  // Voice & music: levels, nudges, cues, theme samples (demo: quiet placeholders).
  const start = (await call('voicemusic.view')).voice.find((l) => l.node === 'b02-1').start_sec;
  const mix = await call('mix.set', { node: 'b02-1', gain_db: -3, nudge_sec: 0.1 });
  assert.equal(mix.mix.gain_db, -3);
  assert.equal(mix.mix.start_sec, Math.round((start + 0.1) * 1000) / 1000, '+0.1 s is exactly 0.1 s later');
  assert.equal((await call('mix.set', { node: 'b02-1', nudge_sec: -0.1 })).mix.start_sec, start, 'and -0.1 s is back where it was');
  await call('mix.set', { node: 'b02-1', nudge_sec: 0.1 });
  const vm = await call('voicemusic.view');
  assert.equal(vm.voice.find((l) => l.node === 'b02-1').timing_locked, true);
  assert.equal(vm.cues.length, 4);
  assert.equal(vm.cues[0].scored, true);
  const cue = await call('music.cue.set', { cue: 'm02', feel: 'more pulse', energy: 4 });
  assert.equal(cue.edit.node, 'm02');
  const samples = await call('music.theme.samples');
  assert.equal(samples.samples.length, 3);
  assert.ok(existsSync(samples.samples[0].wav));
  const used = await call('music.theme.use', { sample: 't2' });
  assert.equal(used.theme.using, 't2');
  // the music is written already: the theme pick remakes it in that theme (run 2: it changed nothing)
  assert.equal(used.edit?.node, 'music');
  await call('voice.line.take', { line: 'b03-1' });
  await call('chat.send', { text: 'Keep the logo small', context: 'video' });
  await until(async () => (await call('chat.history')).filter((r) => r.role === 'ai' && /Demo: noted/.test(r.text)).length === 2);
  await run();

  // Assets: change, use (a Fix on that shot), replace (old kept), list.
  await call('assets.update', { asset: 'a01', note: 'Top right instead' });
  const use = await call('assets.use', { asset: 'a01', node: 's04', how: 'logo corner' });
  assert.equal(use.edit.node, 's04');
  const logo2 = join(root, 'logo2.svg');
  writeFileSync(logo2, '<svg>2</svg>');
  await call('assets.replace', { asset: 'a01', path: logo2 });
  await until(() => events('assets.added').some((e) => e.replaced));
  const al = await call('assets.list');
  assert.equal(al.yours[0].version, 2);
  assert.equal(al.yours[0].versions.length, 2);
  assert.equal(al.yours[0].used_in[0].node, 's04');
  const brief = readFileSync(join(root, 'projects', id, 'CLAUDE.md'), 'utf8');
  assert.match(brief, /## Your assets/);
  assert.match(brief, /What the owner said in the chat[\s\S]*Keep the logo small/);
  assert.doesNotMatch(brief, /Make the whole thing warmer/, 'an older message was in an earlier brief');
  await run();

  // Next on each stage keeps it as it is; leaving Voice & music by Get the video hands the end to the app.
  for (const from of [0, 1, 2]) await call('stage.set', { stage: from + 1, from, done: true });
  const left = await call('stage.set', { stage: 5, from: 3, done: true });
  assert.deepEqual(left.handover.skipped, ['build', 'final']);
  const st2 = await call('stage.status');
  assert.ok(st2.slice(0, 3).every((s) => s.kind === 'done' || s.kind === 'making'), JSON.stringify(st2.map((s) => s.kind)));
  // The demo can't remake a music cue without Strudel: that failed Fix still needs the owner after Next.
  assert.equal(st2[3].kind, 'needsYou');
  assert.equal(st2[3].need, 1);
  assert.equal(loadProject(root, id).mfx.stage, 5);
  assert.ok((await call('stage.remakes', { stage: 'look' })).remade.some((r) => r.step === 'animatic'));

  // Into Mortiflix Studio: the plan, the map back, a new version to swap in.
  const plan = await call('handoff.plan');
  assert.equal(plan.clips.filter((c) => c.kind === 'shot').length, 5);
  assert.ok(plan.clips.some((c) => c.kind === 'transition'));
  assert.ok(plan.clips.some((c) => c.kind === 'line' && c.track === 'A1'));
  assert.ok(plan.clips.some((c) => c.kind === 'music'));
  assert.equal(plan.markers.filter((m) => m.kind === 'chapter').length, 4);
  assert.ok(plan.captions.length >= 4);
  assert.equal(plan.your_assets[0].id, 'a01');
  assert.ok(plan.older_versions.length > 0);
  assert.deepEqual([plan.sequence.width, plan.sequence.height], [1920, 1080]);
  await call('handoff.done', { map: { clips: plan.clips.filter((c) => c.track).map((c, i) => ({ node: c.node, version: c.version, clip: i + 1 })) } });
  assert.deepEqual((await call('handoff.swaps')).swaps, []);
  await call('node.pick', { node: 's02', v: null });
  const sw = (await call('handoff.swaps')).swaps;
  assert.equal(sw[0].node, 's02');
  await call('handoff.swapped', { node: 's02', version: sw[0].version });
  assert.deepEqual((await call('handoff.swaps')).swaps, []);

  // One render queue: the app's export takes a turn, reports progress, finishes; the file is delivered.
  const job = await call('render.add', { label: 'YouTube 1080p' });
  assert.equal(job.state, 'running');
  assert.equal(job.external, true);
  assert.equal((await call('render.progress', { id: job.id, done: 50, total: 100 })).progress.percent, 50);
  const target = await call('export.target', { ext: 'mp4' });
  assert.match(target.file, /Bikes\.mp4$/);
  const video = join(root, 'final.mp4');
  writeFileSync(video, 'not really a video');
  assert.equal((await call('render.finish', { id: job.id, ok: true, file: video })).state, 'done');
  assert.ok(events('render.job').some((j) => j.id === job.id && j.state === 'done'));
  const del = await call('export.done', { file: video, preset: 'YouTube 1080p' });
  assert.equal(del.project_state, 'delivered');
  assert.ok(existsSync(join(root, 'state', id, del.delivered)));
  await tick();
  assert.ok(events('project.changed').some((e) => e.state === 'delivered'));
  await assert.rejects(call('publish.send', { to: 'youtube' }), /confirm/);
  const second = await call('render.add', { label: 'two' });
  const third = await call('render.add', { label: 'three' });
  assert.equal(third.state, 'waiting');
  assert.equal((await call('render.cancel', { id: third.id })).cancelled, true);
  await call('render.finish', { id: second.id, ok: false, error: 'stopped' });
  assert.ok(out.every((m) => !('id' in m) || m.ok || m.error.code !== 'unknown_type'));
});

test('service core: setup status, keys, backend, and the Claude Code chat with a stand-in claude', async (t) => {
  const root = tempStudio(t);
  const bin = join(root, 'fake-claude');
  // A stand-in Claude Code: --version, auth status, and -p answering a move as JSON (no network, no cost).
  writeFileSync(bin, `#!/bin/sh
case "$1" in
  --version) echo "9.9.9 (Claude Code)";;
  auth) echo '{"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "pro", "email": "x@y"}';;
  -p) case "$2" in *linked*) echo '{"type":"result","is_error":false,"result":"Mortiflix is linked."}';;
      *) echo '{"type":"result","is_error":false,"result":"Sure. {\\"reply\\": \\"Making S01 bluer.\\", \\"move\\": {\\"kind\\": \\"fix\\", \\"node\\": \\"s01\\", \\"text\\": \\"bluer\\"}}"}';; esac;;
esac
`);
  chmodSync(bin, 0o755);
  saveConfig(root, { claudeBin: bin });
  const { call } = harness(t, root);
  const s = await call('setup.status');
  assert.equal(s.claude.found, true);
  assert.equal(s.claude.logged_in, true);
  assert.equal(s.claude.subscription, 'pro');
  assert.equal(s.claude.email, undefined, 'the account email is never passed on');
  assert.deepEqual(s.claude.login_command, [bin, 'auth', 'login']);
  assert.equal(s.first_run, true);
  assert.equal(s.tools.node.ok, true);
  assert.equal((await call('setup.claude.test')).reply, 'Mortiflix is linked.');
  const k = await call('keys.set', { id: 'gemini', value: 'g-123' });
  assert.equal(k.source, 'session.env');
  assert.equal(JSON.stringify(await call('keys.list')).includes('g-123'), false, 'key values are never sent');
  await assert.rejects(call('keys.set', { id: 'elevenlabs', value: 'has space' }), /one line/);
  await call('keys.remove', { id: 'gemini' });
  assert.equal((await call('keys.list')).find((x) => x.id === 'gemini').set, false);
  assert.equal((await call('setup.backend', { backend: 'claude-code' })).backend, 'claude-code');
  await assert.rejects(call('setup.backend', { backend: 'gpt' }), /backend/);
  assert.equal((await call('setup.done')).done, true);
  assert.equal((await call('setup.status')).first_run, false);

  // The chat's quick reply through the stand-in: it picks a move, which becomes a Fix (once s01 exists).
  const p = await call('project.new', { preset: 'explainer', title: 'Chat', idea: 'x', start: false });
  const { syncCanvas } = await import('../src/canvas.mjs');
  const work = join(root, 'projects', p.project.id);
  mkdirSync(join(work, 'script'), { recursive: true });
  writeFileSync(join(work, 'script', 'beats.json'), JSON.stringify({ beats: [{ id: 'b01', words: 'One.' }] }));
  syncCanvas(root, p.project.id);
  await call('chat.send', { text: 'the first shot should be bluer', context: 'video' });
  const h = await until(async () => { const x = await call('chat.history'); return x.some((r) => r.role === 'system' && /Sent as Fix on S01/.test(r.text)) ? x : null; });
  assert.ok(h.some((r) => r.role === 'ai' && r.text === 'Making S01 bluer.'));
});

test('service core: bad input is refused with a code, never a crash', async (t) => {
  const root = tempStudio(t);
  const { call } = harness(t, root);
  for (const [type, params, code] of [
    ['graph.get', {}, 'no_project'],
    ['stage.set', { stage: 9 }, 'bad_request'],
    ['node.move', { project: 'zz', positions: {} }, 'failed'],
    ['assets.use', { project: 'x', asset: '../etc', node: 's01' }, 'bad_request'],
    ['chat.context', { context: '../../x' }, 'bad_request'],
    ['music.cue.set', { project: 'x', cue: 'zz' }, 'bad_request'],
    ['render.finish', { id: 999, ok: true }, 'failed'],
    ['presets.get', { slug: 'nope' }, 'failed'],
  ]) {
    await assert.rejects(call(type, params), (e) => e.code === code, `${type} → ${code}`);
  }
  assert.ok(videosDir({ platform: 'win32', env: { USERPROFILE: 'C:\\Users\\a' } }).endsWith('Videos'));
  assert.ok(videosDir({ platform: 'darwin', home: '/Users/a' }).endsWith('Movies'));
});

test('service core: the chat can talk about a cut without a transition, a stage, or a piece in any case', async (t) => {
  const root = tempStudio(t);
  const { call } = harness(t, root);
  await call('project.new', { preset: 'explainer', title: 'Contexts', idea: 'x', start: false, backend: 'demo' });
  // The node area's cut pills (no transition picked yet) are selections too: owner's first run refused them.
  assert.equal((await call('chat.context', { context: 'cut:s02:s03' })).context, 'cut:s02:s03');
  assert.equal((await call('chat.context', { context: 'stage:look' })).context, 'stage:look');
  assert.equal((await call('chat.context', { context: 'S03' })).context, 's03');
  assert.equal((await call('chat.context', { context: 'video' })).context, 'video');
  const r = await call('chat.send', { text: 'make this cut softer', context: 'cut:s02:s03' });
  assert.equal(r.queued, true);
  await until(async () => (await call('chat.history')).some((x) => x.role === 'ai' && /Demo/.test(x.text)));
  assert.ok((await call('chat.history')).some((x) => x.role === 'user' && x.context === 'cut:s02:s03'));
  await assert.rejects(call('chat.context', { context: 'cut:s02' }), (e) => e.code === 'bad_request');
});

test('service core: voice.set picks the narration engine (Settings › Mortiflix › Narration)', async (t) => {
  const root = tempStudio(t);
  const { call } = harness(t, root);
  const v = await call('voice.set', { engine: 'qwen' });
  assert.equal(v.engine, 'qwen');
  assert.equal((await call('voice.list')).engine, 'qwen');
  assert.equal((await call('voice.set', { engine: 'none' })).engine, 'none');
  await assert.rejects(call('voice.set', { engine: 'loud' }), /engine must be/);
});

test('service core: three themes asked of a real backend start a session, and Done on the theme page keeps the request', async (t) => {
  const root = tempStudio(t);
  const { call } = harness(t, root);
  const { project } = await call('project.new', { preset: 'explainer', title: 'Bees', idea: 'how bees make honey', start: false });
  saveConfig(root, { backend: 'claude-code' });
  const { update } = await import('../src/projects.mjs');
  update(root, project.id, (p) => { p.backend = 'claude-code'; p.state = 'waiting'; return p; });
  const r = await call('music.theme.samples', {});
  assert.equal(r.made, false);
  // run 2: the project waited for the owner, so no session came to make them
  assert.equal(loadProject(root, project.id).state, 'queued');
  // and Done on the theme page right after dropped the request
  await call('music.theme.set', { theme: { mood: ['Warm'], genre: 'light acoustic pop' } });
  assert.ok((await call('music.theme.get')).theme.samples_wanted, 'still asked for');
});
