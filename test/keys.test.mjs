import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { randomBytes } from 'node:crypto';
import { request } from 'node:http';
import { tempStudio } from './helpers.mjs';
import { paths, readSessionEnv } from '../src/studio.mjs';
import * as keys from '../src/keys.mjs';
import { findPipeline } from '../src/pipelines.mjs';
import { createProject, startProject, loadProject } from '../src/projects.mjs';
import { Runner } from '../src/runner.mjs';
import { startServer } from '../src/web/server.mjs';

// Keys from the machine running the tests must not count as "set".
for (const k of ['ANTHROPIC_API_KEY', 'ANTHROPIC_AUTH_TOKEN', 'ANTHROPIC_PROFILE']) delete process.env[k];

const mode = (f) => statSync(f).mode & 0o777;

test('keys: which a project needs follows the backend', (t) => {
  const root = tempStudio(t);
  const song = findPipeline(root, 'song');
  assert.deepEqual(keys.keysFor(root, { backend: 'demo', pipeline: song }), []);
  assert.deepEqual(keys.keysFor(root, { backend: 'claude-code', pipeline: song }), []);
  assert.deepEqual(keys.keysFor(root, { backend: 'anthropic-api', pipeline: song }), ['anthropic']);
});

test('keys: saved mode 600, reported by where they come from, never by value', (t) => {
  const root = tempStudio(t);
  assert.equal(mode(root), 0o700);
  keys.saveKey(root, 'anthropic', '  sk-secret  ');
  assert.equal(mode(paths(root).secrets), 0o600);
  assert.equal(keys.keySource(root, 'anthropic'), 'saved');
  assert.ok(!JSON.stringify(keys.keyStatus(root)).includes('sk-secret'));
  assert.throws(() => keys.saveKey(root, 'anthropic', 'two words'), /spaces/);
  assert.throws(() => keys.saveKey(root, 'elevenlabs', 'x'), /unknown key/);
  keys.saveKey(root, 'anthropic', null);
  assert.equal(keys.keySource(root, 'anthropic'), null);
});

test('keys: other keys go to session.env (mode 600), with names checked', (t) => {
  const root = tempStudio(t);
  keys.setSessionKey(root, 'GEMINI_API_KEY', 'g-secret');
  keys.setSessionKey(root, 'OTHER_KEY', 'o-secret');
  assert.equal(mode(paths(root).sessionEnv), 0o600);
  assert.deepEqual(readSessionEnv(root), { GEMINI_API_KEY: 'g-secret', OTHER_KEY: 'o-secret' });
  assert.deepEqual(keys.sessionKeyNames(root), ['GEMINI_API_KEY', 'OTHER_KEY']);
  keys.setSessionKey(root, 'OTHER_KEY', '');
  assert.equal(readFileSync(paths(root).sessionEnv, 'utf8'), 'GEMINI_API_KEY=g-secret\n');
  assert.throws(() => keys.setSessionKey(root, 'lower', 'x'), /UPPER_CASE/);
  assert.throws(() => keys.setSessionKey(root, 'ANTHROPIC_API_KEY', 'x'), /its own place/);
  assert.throws(() => keys.setSessionKey(root, 'A_KEY', 'x\ny'), /one line/);
});

test('keys: checked with a free call, and a refused key is told apart from an unreachable API', async (t) => {
  const root = tempStudio(t);
  const reply = (status, body = {}) => async () => new Response(JSON.stringify(body), { status });
  assert.equal((await keys.verifyKey(root, 'anthropic', 'k', { fetch: reply(200, { data: [] }) })).ok, true);
  assert.equal((await keys.verifyKey(root, 'anthropic', 'k', { fetch: reply(401) })).ok, false);
  assert.equal((await keys.verifyKey(root, 'anthropic', 'k', { fetch: async () => { throw new Error('offline'); } })).ok, null);
  assert.equal((await keys.verifyKey(root, 'anthropic', 'k', { fetch: reply(529) })).ok, null);
});

test('the runner asks for a missing key instead of starting a session that would fail', async (t) => {
  const root = tempStudio(t);
  const p = createProject(root, { pipeline: 'song', title: 'Keyless', backend: 'anthropic-api', answers: { genre: 'house', topic: 'keys' } });
  startProject(root, p.id);
  const runner = new Runner(root);
  runner.acquire();
  t.after(() => runner.release());
  await runner.loop();
  const after = loadProject(root, p.id);
  assert.equal(after.state, 'paused');
  assert.match(after.needs_you.text, /Anthropic API key.*mortiflix-music keys/);
  assert.equal(after.usage.sessions, 0);
});

function call(socketPath, method, path, body) {
  return new Promise((ok, fail) => {
    const req = request({ socketPath, method, path, headers: { host: 'localhost', ...(method !== 'GET' ? { 'x-mortiflix': '1', 'content-type': 'application/json' } : {}) } }, (res) => {
      let data = '';
      res.on('data', (c) => { data += c; });
      res.on('end', () => ok({ status: res.statusCode, text: data, json: JSON.parse(data || '{}') }));
    });
    req.on('error', fail);
    req.end(body === undefined ? null : JSON.stringify(body));
  });
}

test('web: a project that needs a key says which before it starts; keys are set but never shown', async (t) => {
  const root = tempStudio(t, { backend: 'anthropic-api' });
  const socketPath = join(tmpdir(), `mfx-keys-${randomBytes(4).toString('hex')}.sock`);
  const srv = await startServer(root, { socketPath, quiet: true, runner: false });
  t.after(() => srv.close());
  const s = (...a) => call(socketPath, ...a);

  const { json: created } = await s('POST', '/api/projects', { pipeline: 'song', title: 'Needs a key', answers: { genre: 'house', topic: 'bikes' } });
  const refused = await s('POST', `/api/projects/${created.id}/start`);
  assert.equal(refused.status, 409);
  assert.deepEqual(refused.json.needs_keys, ['anthropic']);
  assert.equal(loadProject(root, created.id).state, 'draft');

  const other = await s('PUT', '/api/keys/GEMINI_API_KEY', { value: 'g-secret' });
  assert.deepEqual(other.json.other, ['GEMINI_API_KEY']);
  assert.ok(!other.text.includes('g-secret'));
  assert.equal((await s('PUT', '/api/keys/anthropic', { value: '' })).status, 400);

  keys.saveKey(root, 'anthropic', 'sk-secret'); // as a successful PUT would (that one checks with the Claude API first)
  const listed = await s('GET', '/api/keys');
  assert.equal(listed.json.keys.find((k) => k.id === 'anthropic').source, 'saved');
  assert.ok(!listed.text.includes('sk-secret'));
  assert.equal((await s('POST', `/api/projects/${created.id}/start`)).status, 200);
  assert.equal((await s('DELETE', '/api/keys/GEMINI_API_KEY')).json.other.length, 0);
});

test('usage: plan work is shown as what it would have cost, API work as spend', async () => {
  const { costText } = await import('../src/usage.mjs');
  assert.equal(costText({ cost_usd: 0 }), null);
  assert.equal(costText({ cost_usd: 12.9568, plan_usd: 12.9568 }), 'Would have cost about $12.96 at API prices. On your Claude plan, you didn\'t pay that.');
  assert.equal(costText({ cost_usd: 3.5 }), 'About $3.50 spent on the Claude API.');
  assert.match(costText({ cost_usd: 5, plan_usd: 2 }), /^About \$3\.00 spent on the Claude API, plus about \$2\.00 of work on your Claude plan/);
});
