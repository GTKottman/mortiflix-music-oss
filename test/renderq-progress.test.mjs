// The live view's render bar: progress read from the render's own output.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { RenderQueue } from '../src/renderq.mjs';

test('render queue: Remotion-style "Rendered N/M" lines become percent and time left while it runs', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'mfx-rq-')); t.after(() => rmSync(dir, { recursive: true, force: true }));
  const q = new RenderQueue({ logDir: dir });
  const job = q.add({ projectId: 'p1', sessionId: 's', label: 'Animatic', argv: ['sh', '-c', 'echo "Rendered 25/100, time remaining: 3s"; sleep 0.6; echo "Encoded 100/100"'], cwd: dir, env: process.env });
  await new Promise((ok) => setTimeout(ok, 300));
  const mid = q.activeFor('p1')[0];
  assert.equal(mid.state, 'running');
  assert.deepEqual([mid.progress.phase, mid.progress.done, mid.progress.total, mid.progress.percent], ['rendering', 25, 100, 25]);
  assert.ok(Number.isInteger(mid.progress.seconds_left));
  const end = await q.wait(job.id, 'p1', 5);
  assert.equal(end.state, 'done');
  assert.equal(end.progress, undefined, 'no progress once it is done');
});

test('render queue: a render keeps its temp files on disk beside the logs (not a RAM /tmp) and they go when it ends', async (t) => {
  const { existsSync, readFileSync } = await import('node:fs');
  const dir = mkdtempSync(join(tmpdir(), 'mfx-rq-')); t.after(() => rmSync(dir, { recursive: true, force: true }));
  const q = new RenderQueue({ logDir: dir });
  const job = q.add({ projectId: 'p1', sessionId: 's', label: 'Temp', argv: ['sh', '-c', 'echo "tmp=$TMPDIR"; touch "$TMPDIR/frame.png"'], cwd: dir, env: process.env });
  const end = await q.wait(job.id, 'p1', 5);
  assert.equal(end.state, 'done');
  assert.match(readFileSync(join(dir, `render-${job.id}.log`), 'utf8'), new RegExp(`tmp=${join(dir, 'tmp', String(job.id))}`));
  assert.equal(existsSync(join(dir, 'tmp', String(job.id))), false);
});

test('render queue: render-stop ends a running render and its children by its process group, and a waiting one never starts', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'mfx-rq-')); t.after(() => rmSync(dir, { recursive: true, force: true }));
  const q = new RenderQueue({ logDir: dir });
  const a = q.add({ projectId: 'p1', sessionId: 's', label: 'Long', argv: ['sh', '-c', 'sleep 30 & echo "child $!"; wait'], cwd: dir, env: process.env });
  const b = q.add({ projectId: 'p1', sessionId: 's', label: 'Next', argv: ['sh', '-c', 'echo never'], cwd: dir, env: process.env });
  await new Promise((ok) => setTimeout(ok, 300));
  const { readFileSync } = await import('node:fs');
  const child = Number(readFileSync(join(dir, `render-${a.id}.log`), 'utf8').match(/child (\d+)/)[1]);
  assert.equal(q.stop(b.id, 'p1').state, 'failed', 'the waiting one is dropped');
  q.stop(a.id, 'p1', 500);
  const end = await q.wait(a.id, 'p1', 5);
  assert.equal(end.state, 'failed');
  await new Promise((ok) => setTimeout(ok, 200));
  assert.throws(() => process.kill(child, 0), /ESRCH/, 'the render\'s own child is gone too');
  assert.throws(() => q.stop(a.id, 'other'), /no render/);
});
