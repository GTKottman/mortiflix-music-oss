// The engine supervisor (doc 16 E2) against a fake engine: requests and replies, events, a heartbeat that kills a
// silent engine's PID and starts it again (restoring the song), giving up after 3 crashes, and a clean quit.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, readFileSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { createEngineSupervisor } from '../src/daw-engine.mjs';

const FAKE = join(import.meta.dirname, 'fixtures', 'fake-engine.mjs');
const until = async (fn, ms = 5000) => { const t0 = Date.now(); while (!fn()) { if (Date.now() - t0 > ms) throw new Error('timed out'); await new Promise((r) => setTimeout(r, 20)); } };

test('requests get their replies, events arrive, errors come back as errors, quit is clean', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'mfx-eng-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const e = createEngineSupervisor({ bin: process.execPath, args: [FAKE], logFile: join(dir, 'engine.log') });
  const events = [];
  e.on('event', (m) => events.push(m.ev));
  const hello = await e.start();
  assert.equal(hello.name, 'fake');
  assert.equal(e.state, 'running');
  assert.deepEqual(await e.call('song.load', { path: '/x/song.json', atSample: 96000 }), { loaded: '/x/song.json', at: 96000 });
  await assert.rejects(e.call('no.such'), /unknown command no.such/);
  await e.call('engine.tone', { hz: 1000 });
  await until(() => events.includes('alive') && events.includes('log'));
  assert.match(readFileSync(join(dir, 'engine.log'), 'utf8'), /fake engine started[\s\S]*tone 1000/);
  const pid = e.pid;
  await e.stop();
  assert.equal(e.state, 'stopped');
  assert.throws(() => process.kill(pid, 0), 'the engine process is gone');
});

test('a silent engine: its PID is killed after the heartbeat, it starts again and the song is loaded again', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'mfx-eng-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const restored = [];
  const e = createEngineSupervisor({ bin: process.execPath, args: [FAKE], env: { ...process.env, FAKE_MODE: 'silent', FAKE_COUNT: join(dir, 'n') },
    heartbeatMs: 300, checkMs: 50, backoffMs: [50], restore: async () => restored.push('song.load') });
  t.after(() => e.stop({ graceMs: 200 }));
  await e.start();
  const first = e.pid;
  await until(() => e.pid && e.pid !== first && e.state === 'running', 5000);
  assert.throws(() => process.kill(first, 0), 'the first engine was killed by its PID');
  await until(() => restored.length >= 1);
  assert.ok(readFileSync(join(dir, 'n'), 'utf8').length >= 2, 'started twice');
});

test('three crashes inside a minute: it stays stopped, with its last log lines', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'mfx-eng-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const states = [];
  const e = createEngineSupervisor({ bin: process.execPath, args: [FAKE], env: { ...process.env, FAKE_MODE: 'crash', FAKE_COUNT: join(dir, 'n') }, backoffMs: [30, 30, 30] });
  e.on('state', (s) => states.push(s.state));
  await e.start().catch(() => {});
  await until(() => e.state === 'failed', 8000);
  assert.equal(readFileSync(join(dir, 'n'), 'utf8').length, 3);
  assert.ok(e.log.some((l) => /fake engine crashing/.test(l)));
  assert.ok(states.includes('restarting'));
  assert.equal(e.pid, null);
});

// The real engine, when it has been built on this machine (doc 16 E1 + E2).
const { existsSync } = await import('node:fs');
const { homedir } = await import('node:os');
const { spawnSync } = await import('node:child_process');
const REAL = [process.env.MFX_DAW_ENGINE, join(homedir(), '.cache/mortiflix/engine-build/mortiflix-engine_artefacts/Release/mortiflix-engine')].find((f) => f && existsSync(f));

test('the real engine: --version, --selftest renders 1 s of a 0.5 sine into memory, --serve speaks the protocol', { skip: !REAL && 'the engine is not built on this machine' }, async (t) => {
  const v = JSON.parse(spawnSync(REAL, ['--version'], { encoding: 'utf8' }).stdout.trim());
  assert.equal(v.name, 'mortiflix-engine');
  assert.match(v.tracktion_engine, /^[0-9a-f]{40}$/);
  const st = spawnSync(REAL, ['--selftest'], { encoding: 'utf8', timeout: 60000 });
  assert.equal(st.status, 0, st.stdout + st.stderr);
  const r = JSON.parse(st.stdout.trim());
  assert.ok(Math.abs(r.peak - 0.5) < 0.002 && Math.abs(r.average - Math.SQRT1_2 / 2) < 0.002 && r.seconds === 1, JSON.stringify(r));
  assert.equal(st.stdout.trim().split('\n').length, 1, 'stdout carries only the JSON line');
  const e = createEngineSupervisor({ bin: REAL, args: ['--serve'] });
  t.after(() => e.stop());
  const hello = await e.start();
  assert.equal(hello.rate, 48000);
  await assert.rejects(e.call('no.such'), /unknown command no.such/);
  let alive = 0;
  e.on('alive', () => alive++);
  await until(() => alive >= 1, 3000);
  const pid = e.pid;
  await e.stop();
  assert.throws(() => process.kill(pid, 0));
});
