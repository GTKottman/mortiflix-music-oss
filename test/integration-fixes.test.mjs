// Fixes found while merging the Mortiflix FilmCraft parts: a missing BPM stays missing, ComfyUI counts as installed
// only when it is the studio's own, handoff.plan names the .fcproj, asset nodes keep their place, shot versions get a
// PNG still.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { existsSync, mkdtempSync, rmSync, mkdirSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { spawnSync } from 'node:child_process';
import { createServer } from 'node:http';
import { cleanTheme } from '../src/svc/theme.mjs';
import { makeHandoffDemo } from '../src/demo-handoff.mjs';
import { handoffPlan, graphView, moveNodes, stillsSettled } from '../src/svc/graph.mjs';
import { setupStatus } from '../src/setup.mjs';
import { saveConfig } from '../src/studio.mjs';
import { update } from '../src/projects.mjs';

const hasFfmpeg = spawnSync('ffmpeg', ['-version']).status === 0;

test('a music theme without a BPM keeps it missing (null, empty and false are not 30)', () => {
  const t = cleanTheme({ tempo: { feel: 'slow', bpm_min: null, bpm_max: '', bpm_fixed: 0 } });
  assert.equal(t.tempo.bpm_min, null);
  assert.equal(t.tempo.bpm_max, null);
  assert.equal(t.tempo.bpm_fixed, undefined);
  assert.equal(cleanTheme({ tempo: { bpm_min: false } }).tempo.bpm_min, null);
  assert.equal(cleanTheme({ tempo: { bpm_min: 10, bpm_max: '96' } }).tempo.bpm_min, 30);
  assert.equal(cleanTheme({ tempo: { bpm_min: 10, bpm_max: '96' } }).tempo.bpm_max, 96);
});

test('ComfyUI is installed only when it is the studio\'s own: another server on the port is not counted', async (t) => {
  const root = mkdtempSync(join(tmpdir(), 'mfx-comfy-'));
  const server = createServer((req, res) => { res.setHeader('content-type', 'application/json'); res.end(req.url.startsWith('/system_stats') ? '{"system":{"comfyui_version":"0.3"},"devices":[]}' : '{}'); });
  await new Promise((ok) => server.listen(0, '127.0.0.1', ok));
  t.after(() => { server.close(); rmSync(root, { recursive: true, force: true }); });
  const url = `http://127.0.0.1:${server.address().port}`;
  saveConfig(root, { voice: { qwen: { url } } });
  const before = (await setupStatus(root)).comfyui;
  assert.equal(before.ok, false, JSON.stringify(before));
  assert.equal(before.installed, false);
  assert.match(before.detail, /not installed/);
  mkdirSync(join(root, 'tools', 'ComfyUI'), { recursive: true });
  writeFileSync(join(root, 'tools', 'ComfyUI', 'main.py'), '');
  const after = (await setupStatus(root)).comfyui;
  assert.equal(after.installed, true, JSON.stringify(after));
});

test('handoff.plan names the .fcproj beside the project record; asset nodes keep their place', (t) => {
  const root = mkdtempSync(join(tmpdir(), 'mfx-fix-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const p = makeHandoffDemo(root);
  const plan = handoffPlan(root, p.id);
  assert.match(plan.fcproj, /[/\\]state[/\\][a-z0-9-]+[/\\]filmcraft[/\\][^/\\]+\.fcproj$/);
  // an asset id is known to node.move; graph.get sends its pos back
  update(root, p.id, (project) => { project.assets = [...(project.assets || []), { id: 'a01', name: 'logo.png', kind: 'logo', file: 'assets/logo.png', use: 'fits', where: null, note: '', versions: [], used_in: [] }]; });
  const r = moveNodes(root, p.id, { a01: [120, -40] });
  assert.equal(r.moved, 1);
  const a = graphView(root, p.id).assets.find((x) => x.id === 'a01');
  assert.ok(a, 'the asset is in graph.get');
  assert.deepEqual(a.pos, [120, -40]);
});

test('every shot version gets a PNG still (its first frame)', { skip: !hasFfmpeg && 'needs ffmpeg' }, async (t) => {
  const root = mkdtempSync(join(tmpdir(), 'mfx-still-'));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  const p = makeHandoffDemo(root);
  graphView(root, p.id); // queues the stills
  await stillsSettled();
  const shots = graphView(root, p.id).nodes.filter((n) => n.kind === 'shot');
  const versions = shots.flatMap((n) => n.versions.filter((v) => v.path));
  assert.ok(versions.length > 0);
  for (const v of versions) {
    assert.ok(v.still, `no still for ${v.path}`);
    assert.ok(existsSync(v.still));
    assert.match(v.still, /\.png$/);
  }
});
