// The web server over a Unix socket (no network port is opened by the tests).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { request } from 'node:http';
import { join } from 'node:path';
import { tmpdir, hostname } from 'node:os';
import { randomBytes } from 'node:crypto';
import { tempStudio } from './helpers.mjs';
import { startServer, ownName } from '../src/web/server.mjs';

function call(socketPath, method, path, { body, raw, headers = {} } = {}) {
  return new Promise((ok, fail) => {
    const data = raw ?? (body !== undefined ? JSON.stringify(body) : null);
    const req = request({ socketPath, method, path, headers: { host: 'localhost', ...(method !== 'GET' ? { 'x-mortiflix': '1' } : {}), ...headers } }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => {
        const buf = Buffer.concat(chunks);
        let json = null;
        try { json = JSON.parse(buf.toString()); } catch { /* not json */ }
        ok({ status: res.statusCode, headers: res.headers, json, buf });
      });
    });
    req.on('error', fail);
    req.end(data);
  });
}

const until = async (fn, ms = 5000) => {
  const end = Date.now() + ms;
  for (;;) {
    const v = await fn();
    if (v) return v;
    if (Date.now() > end) throw new Error('timed out');
    await new Promise((r) => setTimeout(r, 50));
  }
};

test('web: create, upload, start, review, media and the guards', async (t) => {
  const root = tempStudio(t);
  const socketPath = join(tmpdir(), `mfx-web-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true });
  t.after(() => srv.close());
  const s = (...a) => call(socketPath, ...a);

  const studio = await s('GET', '/api/studio');
  assert.equal(studio.json.config.backend, 'demo');
  assert.equal(studio.json.api_key_set, false);

  // Writes need the X-Mortiflix header (CSRF).
  const csrf = await call(socketPath, 'POST', '/api/projects', { body: {}, headers: { 'x-mortiflix': '' } });
  assert.equal(csrf.status, 403);

  const missing = await s('POST', '/api/projects', { body: { pipeline: 'midi-pack', title: 'No topic', answers: { genre: 'house' } } });
  assert.equal((await s('POST', `/api/projects/${missing.json.id}/start`)).status, 400); // "What's it about?" is required
  const created = await s('POST', '/api/projects', { body: { pipeline: 'midi-pack', title: 'Web song', answers: { genre: 'house', topic: 'sunrise' } } });
  const id = created.json.id;
  const up = await s('POST', `/api/projects/${id}/files?field=material&name=../../evil.mid`, { raw: 'MThd' });
  assert.equal(up.json.path, 'input/material/evil.mid');
  assert.equal((await s('POST', `/api/projects/${id}/start`)).status, 200);

  // The runner inside the server picks it up (demo backend) and stops at the first gate: the blueprint.
  const detail = await until(async () => { const d = (await s('GET', `/api/projects/${id}`)).json; return d.project.state === 'waiting' ? d : null; });
  assert.equal(detail.submissions[0].step, 'blueprint');

  // Review through the API: changes on a paragraph, then the next version arrives answering it.
  const bad = await s('POST', `/api/projects/${id}/reviews/blueprint/1`, { body: { verdict: 'changes' } });
  assert.equal(bad.status, 400);
  await s('POST', `/api/projects/${id}/reviews/blueprint/1`, { body: { verdict: 'changes', notes: [{ item: 0, paragraph: 1, text: 'more space' }] } });
  const v2 = await until(async () => { const d = (await s('GET', `/api/projects/${id}`)).json; return d.submissions.find((x) => x.version === 2); });
  assert.equal(v2.pin_changes[0].note, 1);
  await s('POST', `/api/projects/${id}/reviews/blueprint/2`, { body: { verdict: 'approve' } });
  const final = await until(async () => { const d = (await s('GET', `/api/projects/${id}`)).json; return d.submissions.find((x) => x.step === 'final'); });
  assert.equal(final.items[0].kind, 'midi');

  // Media: served from the record, with range support, a sandboxing CSP, and no way out of the folder.
  const file = final.items[0].file;
  const media = await s('GET', `/files/${id}/${file}`, { headers: { range: 'bytes=0-9' } });
  assert.equal(media.status, 206);
  assert.equal(media.buf.length, 10);
  assert.equal(media.buf.toString('latin1', 0, 4), 'MThd');
  assert.match(media.headers['content-security-policy'], /sandbox/);
  assert.equal(media.headers['content-type'], 'audio/midi');
  const dl = await s('GET', `/files/${id}/${file}?download`);
  assert.match(dl.headers['content-disposition'], /attachment; filename="Web song - demo-all.mid"/);
  assert.equal((await s('GET', `/files/${id}/reviews/../project.json`)).status, 404);
  assert.equal((await s('GET', `/files/${id}/reviews/..%2Fproject.json`)).status, 404);
  assert.equal((await s('GET', '/../package.json')).status, 404);

  // A moment on the piano roll is a time note, like on audio.
  await s('POST', `/api/projects/${id}/reviews/final/1`, { body: { verdict: 'changes', notes: [{ item: 0, time_sec: 2.67, text: 'the bass enters too early' }] } });
  const f2 = await until(async () => { const d = (await s('GET', `/api/projects/${id}`)).json; return d.submissions.find((x) => x.step === 'final' && x.version === 2); });
  assert.equal(f2.pin_changes.length, 1);

  // Settings: the API key is stored but never returned.
  const saved = await s('PUT', '/api/config', { body: { backend: 'demo', effort: 'xhigh', api_key: 'sk-ant-test-123' } });
  assert.equal(saved.json.api_key_set, true);
  assert.ok(!JSON.stringify(saved.json).includes('sk-ant-test-123'));
  assert.equal((await s('PUT', '/api/config', { body: { effort: 'turbo' } })).status, 400);

  const app = await s('GET', '/');
  assert.match(app.buf.toString(), /<title>Mortiflix Music<\/title>/);
  assert.match(app.headers['content-security-policy'], /default-src 'self'/);
});

test('web: the MIDI panel hands over the MIDI pack and its notes and takes the owner\'s master (resuming a project waiting for it)', async (t) => {
  const { mkdirSync, writeFileSync, existsSync } = await import('node:fs');
  const { spawnSync } = await import('node:child_process');
  const { createProject, startProject, projectPaths } = await import('../src/projects.mjs');
  const gates = await import('../src/gates.mjs');
  const root = tempStudio(t);
  const socketPath = join(tmpdir(), `mfx-web-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true, runner: false });
  t.after(() => srv.close());
  const s = (...a) => call(socketPath, ...a);
  const { id } = createProject(root, { pipeline: 'song', title: 'Cue', answers: { genre: 'house', topic: 'x', finish: 'Me, from the MIDI in my own DAW' } });
  startProject(root, id);
  assert.equal((await s('GET', `/api/projects/${id}/music`)).json.expect, null);
  assert.equal((await s('GET', `/api/projects/${id}/music/midi.zip`)).status, 400);
  assert.equal((await s('GET', `/api/projects/${id}/music/notes`)).status, 400);
  const listed = async () => (await s('GET', '/api/projects')).json.find((x) => x.id === id);
  assert.deepEqual((await listed()).music, { notes: false, master: false, bpm: null, bars: null, seconds: null });
  assert.equal((await listed()).genre, 'house');

  const w = projectPaths(root, id).work;
  mkdirSync(join(w, 'music'), { recursive: true }); mkdirSync(join(w, 'out/music/midi'), { recursive: true });
  writeFileSync(join(w, 'music/blueprint.json'), JSON.stringify({ title: 'Cue', bpm: 120, meter: '4/4', bars: 2, sections: [{ name: 'Only', start_bar: 1, end_bar: 2 }] }));
  writeFileSync(join(w, 'music/notes.json'), JSON.stringify({ bpm: 120, beats_per_bar: 4, bars: 2, tracks: [{ channel: 1, name: 'Bass', notes: [{ pitch: 40, start: 0, dur: 1, velocity: 0.8 }] }] }));
  writeFileSync(join(w, 'out/music/midi/01-bass.mid'), 'MThd');
  const { saveDaw } = await import('../src/setup.mjs');
  saveDaw(root, 'logic');
  const st = (await s('GET', `/api/projects/${id}/music`)).json;
  assert.equal(st.finish, 'own'); assert.deepEqual(st.midi, ['01-bass.mid']); assert.equal(st.expect.seconds, 4); assert.equal(st.daw, 'Logic Pro');
  // The library plays it and shows its tempo and length.
  assert.deepEqual((await listed()).music, { notes: true, master: false, bpm: 120, bars: 2, seconds: 4 });
  const notes = (await s('GET', `/api/projects/${id}/music/notes`)).json;
  assert.equal(notes.tracks[0].name, 'Bass');
  assert.deepEqual(notes.sections.map((x) => x.name), ['Only']);
  const zip = await s('GET', `/api/projects/${id}/music/midi.zip`);
  assert.equal(zip.status, 200); assert.equal(zip.headers['content-type'], 'application/zip'); assert.match(zip.headers['content-disposition'], /Cue - MIDI\.zip/);

  gates.needsYou(root, id, 'Import your master');
  const wav = spawnSync('ffmpeg', ['-loglevel', 'error', '-f', 'lavfi', '-i', 'sine=frequency=220:duration=4.5', '-f', 'wav', '-'], { maxBuffer: 1 << 24 }).stdout;
  const short = await s('POST', `/api/projects/${id}/music/master?name=take.wav`, { raw: wav.subarray(0, 44 + 48000) });
  assert.equal(short.status, 422); assert.match(short.json.error, /score runs 4.00 s/);
  const ok = await s('POST', `/api/projects/${id}/music/master?name=take.wav`, { raw: wav });
  assert.equal(ok.status, 200, JSON.stringify(ok.json)); assert.equal(ok.json.resumed, true);
  assert.ok(existsSync(join(w, 'music/own-master/master.wav')));
  assert.equal((await listed()).music.master, true); // the library plays your master instead of the sketch
  const audio = await s('GET', `/api/projects/${id}/music/master`);
  assert.equal(audio.headers['content-type'], 'audio/wav');
  assert.equal((await s('POST', `/api/projects/${id}/music/master`, { raw: wav, headers: { 'x-mortiflix': '' } })).status, 403);
});

test('web: flows, a brief\'s flow, rating a piece and saving it as a flow', async (t) => {
  const { mkdirSync, writeFileSync } = await import('node:fs');
  const { projectPaths } = await import('../src/projects.mjs');
  const root = tempStudio(t);
  const socketPath = join(tmpdir(), `mfx-web-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true, runner: false });
  t.after(() => srv.close());
  const s = (...a) => call(socketPath, ...a);
  const cat = (await s('GET', '/api/flows')).json;
  assert.equal(cat.flows.length, 18);
  assert.ok(cat.buckets.builds && cat.feels.includes('hypnotic') && cat.ratings.love);
  assert.ok(!('dir' in cat.flows[0]), 'no paths on disk in the API');
  assert.match((await s('GET', '/api/flows/rondo')).json.flow_md, /^# Rondo/);
  assert.equal((await s('GET', '/api/flows/nope')).status, 400);

  const pipes = (await s('GET', '/api/pipelines')).json;
  assert.equal(pipes.find((p) => p.slug === 'loop-kit').flows, 'kit');
  const made = await s('POST', '/api/projects', { body: { pipeline: 'song', title: 'Harbour', answers: { genre: 'g', topic: 't' }, flow: 'feel:melancholic' } });
  const id = made.json.id;
  const p = (await s('GET', `/api/projects/${id}`)).json.project;
  assert.ok(p.flow.feels.includes('melancholic'));
  assert.equal(p.flow.picked, 'studio');
  assert.equal((await s('POST', '/api/projects', { body: { pipeline: 'song', title: 'x', flow: 'energy-ladder' } })).status, 400);

  assert.equal((await s('POST', `/api/projects/${id}/rate`, { body: { rating: 'meh' } })).status, 400);
  assert.equal((await s('POST', `/api/projects/${id}/rate`, { body: { rating: 'love' } })).json.rating, 'love');
  assert.equal((await s('GET', `/api/projects/${id}`)).json.project.rating, 'love');
  assert.equal((await s('GET', '/api/flows')).json.flows.find((f) => f.slug === p.flow.slug).stats.love, 1);

  assert.equal((await s('POST', `/api/projects/${id}/flow`, { body: { name: 'Harbour' } })).status, 400, 'no blueprint yet');
  const w = projectPaths(root, id).work;
  mkdirSync(join(w, 'music'), { recursive: true });
  writeFileSync(join(w, 'music/blueprint.json'), JSON.stringify({ bars: 16, sections: [{ name: 'Low', start_bar: 1, end_bar: 8, function: 'x', intensity: 2 }, { name: 'High', start_bar: 9, end_bar: 16, function: 'y', intensity: 8 }] }));
  const saved = await s('POST', `/api/projects/${id}/flow`, { body: { name: 'Harbour shape' } });
  assert.equal(saved.json.slug, 'harbour-shape');
  assert.equal(saved.json.source, 'yours');
  assert.ok(!('dir' in saved.json));
  assert.equal((await s('GET', '/api/flows')).json.flows.length, 19);
});

test('web: on --network, only this machine\'s own names pass the Host check (DNS rebinding)', () => {
  const me = hostname().toLowerCase().split('.')[0];
  for (const ok of ['192.168.1.20', '100.64.0.7', 'fe80::1', 'localhost', me, `${me}.local`, `${me}.tail1234.ts.net`]) assert.equal(ownName(ok), true, ok);
  for (const bad of ['evil.example.com', `${me}.evil.com`, `${me}x.local`, 'rebind.1.2.3.4.nip.io', '']) assert.equal(ownName(bad), false, bad);
});
