// Rendering in chunks (the same audio as one pass, in time that grows with the length) and no Chrome left behind when
// node is stopped. Real Strudel and Chrome only (MFX_TEST_STRUDEL=<a studio's tools/strudel>).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { existsSync, mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { tempStudio } from './helpers.mjs';
import * as setup from '../src/setup.mjs';
import { HeadlessStrudel } from '../pipelines/_shared/skills/music/lib/headless.mjs';
import { readWav } from '../pipelines/_shared/skills/music/lib/wav.mjs';

const skip = !process.env.MFX_TEST_STRUDEL && 'set MFX_TEST_STRUDEL to run';
const chromeFor = (t) => setup.setupStatusSync(tempStudio(t)).chrome;
const alive = (pid) => { try { process.kill(pid, 0); return true; } catch { return false; } };

test('render: chunks add up to one pass, held notes and tails included', { skip }, async (t) => {
  const chrome = chromeFor(t); if (!chrome.ok) return t.skip('no Chrome');
  const dir = mkdtempSync(join(tmpdir(), 'mfx-render-')); t.after(() => rmSync(dir, { recursive: true, force: true }));
  // Pitched sounds only (noise is random per render and never nulls). The pad holds 3 bars, across chunk edges; the
  // pluck rings out with a release and a delay tail.
  const code = `setcpm(120 / 4)
pad: note("<[c3,e3,g3]@3 [a2,c3,e3]>").s("triangle").gain(.25).release(.5).midichan(2)
pluck: note("c5 e5 g5 b5").s("sine").gain(.3).decay(.2).sustain(0).release(.3).delay(.4).delaytime(.25).midichan(3)`;
  const s = await new HeadlessStrudel({ strudel: process.env.MFX_TEST_STRUDEL, chrome: chrome.path }).start();
  t.after(() => s.close());
  await s.render(code, 9, join(dir, 'one.wav'), { chunk: 100 });
  await s.render(code, 9, join(dir, 'chunks.wav'), { chunk: 2 });
  const a = readWav(join(dir, 'one.wav')); const b = readWav(join(dir, 'chunks.wav'));
  assert.equal(b.channels[0].length, a.channels[0].length, 'same length');
  let sig = 0; let diff = 0;
  for (let c = 0; c < 2; c++) for (let i = 0; i < a.channels[c].length; i++) { sig += a.channels[c][i] ** 2; diff += (a.channels[c][i] - b.channels[c][i]) ** 2; }
  assert.ok(sig > 0, 'the one-pass render has sound');
  const nullDb = 10 * Math.log10(diff / sig);
  assert.ok(nullDb < -30, `chunks differ from one pass by ${nullDb.toFixed(1)} dB`);
});

test('render: a stopped render leaves no Chrome and no profile behind', { skip }, async (t) => {
  const chrome = chromeFor(t); if (!chrome.ok) return t.skip('no Chrome');
  const lib = new URL('../pipelines/_shared/skills/music/lib/headless.mjs', import.meta.url).href;
  const child = spawn(process.execPath, ['--input-type=module', '-e', `
    const { HeadlessStrudel } = await import(${JSON.stringify(lib)});
    const s = await new HeadlessStrudel({ strudel: ${JSON.stringify(process.env.MFX_TEST_STRUDEL)}, chrome: ${JSON.stringify(chrome.path)} }).start();
    console.log(JSON.stringify({ pid: s.chrome.pid, profile: s.profile }));
    setInterval(() => {}, 1000);`], { stdio: ['ignore', 'pipe', 'inherit'] });
  const started = await new Promise((ok, fail) => {
    let out = '';
    child.stdout.on('data', (d) => { out += d; if (out.includes('\n')) ok(JSON.parse(out)); });
    child.on('exit', (code) => fail(new Error(`exited ${code} before starting Chrome`)));
  });
  assert.ok(alive(started.pid) && existsSync(started.profile));
  child.kill('SIGTERM');
  await new Promise((ok) => child.on('exit', ok));
  for (let i = 0; i < 20 && alive(started.pid); i++) await new Promise((r) => setTimeout(r, 100));
  assert.ok(!alive(started.pid), 'Chrome was killed');
  assert.ok(!existsSync(started.profile), 'its profile was removed');
});

// The engine shares every song's chunks (the mix and each stem) among several workers and adds each song up as its
// chunks come back: the same audio as one worker, and each stem holds only its own channel.
test('render: the mix and its stems, chunked across workers, match one worker', { skip }, async (t) => {
  const chrome = chromeFor(t); if (!chrome.ok) return t.skip('no Chrome');
  const { spawnSync } = await import('node:child_process');
  const { mkdirSync, writeFileSync } = await import('node:fs');
  const { REPO } = await import('../src/studio.mjs');
  const dir = mkdtempSync(join(tmpdir(), 'mfx-par-')); t.after(() => rmSync(dir, { recursive: true, force: true }));
  mkdirSync(join(dir, 'music'));
  writeFileSync(join(dir, 'music', 'score.strudel.js'), `setcpm(120 / 4)
pad: note("<[c3,e3,g3]@3 [a2,c3,e3]>").s("triangle").gain(.25).release(.5).midichan(2)
pluck: note("c5 e5 g5 b5").s("sine").gain(.3).decay(.2).sustain(0).release(.3).midichan(3)
`);
  writeFileSync(join(dir, 'music', 'blueprint.json'), JSON.stringify({ title: 'Par', bpm: 120, meter: '4/4', bars: 8, scale: 'C major',
    roles: [{ channel: 2, name: 'Pad', harmony: 'chords' }, { channel: 3, name: 'Pluck', harmony: 'melody' }], sections: [{ name: 'All', start_bar: 1, end_bar: 8, intensity: 5, function: 'the whole test piece' }] }));
  writeFileSync(join(dir, 'music', 'notes.json'), JSON.stringify({ bpm: 120, beats_per_bar: 4, tracks: [{ channel: 2, name: 'Pad', notes: [] }, { channel: 3, name: 'Pluck', notes: [] }] }));
  const run = (workers, out) => spawnSync(process.execPath, [join(REPO, 'pipelines/_shared/skills/music/strudel.mjs'), 'render', '--stems', '--out', out], {
    cwd: dir, encoding: 'utf8', env: { ...process.env, MFX_STRUDEL: process.env.MFX_TEST_STRUDEL, MFX_CHROME: chrome.path, MFX_RENDER_WORKERS: String(workers), MFX_RENDER_CHUNK: '2' } });
  const one = run(1, 'one/mix.wav'); assert.equal(one.status, 0, one.stdout + one.stderr);
  const three = run(3, 'three/mix.wav'); assert.equal(three.status, 0, three.stdout + three.stderr);
  const nullDb = (fa, fb) => {
    const a = readWav(join(dir, fa)); const b = readWav(join(dir, fb)); let sig = 0; let diff = 0;
    for (let c = 0; c < 2; c++) for (let i = 0; i < a.channels[c].length; i++) { sig += a.channels[c][i] ** 2; diff += (a.channels[c][i] - (b.channels[c][i] || 0)) ** 2; }
    return { sig, db: 10 * Math.log10(diff / sig) };
  };
  for (const f of ['mix.wav', 'stems/02-pad.wav', 'stems/03-pluck.wav']) {
    const r = nullDb(`one/${f}`, `three/${f}`);
    assert.ok(r.sig > 0 && r.db < -30, `${f}: 3 workers differ from 1 by ${r.db.toFixed(1)} dB`);
  }
  // Each stem is its channel alone, and together they are the mix.
  const mix = readWav(join(dir, 'three/mix.wav')); const pad = readWav(join(dir, 'three/stems/02-pad.wav')); const pluck = readWav(join(dir, 'three/stems/03-pluck.wav'));
  let sig = 0; let diff = 0; let padOnly = 0;
  for (let c = 0; c < 2; c++) for (let i = 0; i < mix.channels[c].length; i++) {
    sig += mix.channels[c][i] ** 2; diff += (mix.channels[c][i] - pad.channels[c][i] - pluck.channels[c][i]) ** 2; padOnly += (mix.channels[c][i] - pad.channels[c][i]) ** 2;
  }
  assert.ok(10 * Math.log10(diff / sig) < -30, 'the stems add up to the mix');
  assert.ok(padOnly / sig > 1e-4, 'the pad stem lacks the pluck');
});
