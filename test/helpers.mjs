import { mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { ensureStudio, saveConfig } from '../src/studio.mjs';

// A throwaway studio using the demo backend (no Claude, no cost), cleaned up after the test.
export function tempStudio(t, config = {}) {
  const root = mkdtempSync(join(tmpdir(), 'mfx-test-'));
  ensureStudio(root);
  saveConfig(root, { backend: 'demo', demoDelayMs: 0, ...config });
  t.after(() => rmSync(root, { recursive: true, force: true }));
  return root;
}

// A started song whose genre step is done, so its blueprint (a reviewed document) is ready: the gates' first stop.
export async function songAtBlueprint(root, { title = 'Night drive' } = {}) {
  const { createProject, startProject, projectPaths } = await import('../src/projects.mjs');
  const gates = await import('../src/gates.mjs');
  const { writeFileSync, mkdirSync } = await import('node:fs');
  const p = createProject(root, { pipeline: 'song', title, answers: { genre: 'trip hop', topic: 'a night drive' } });
  startProject(root, p.id);
  gates.stepDone(root, p.id, 'genre', gates.stepChecks(root, p.id, 'genre').map((c) => ({ id: c.id, result: 'pass' })));
  const work = projectPaths(root, p.id).work;
  mkdirSync(join(work, 'music'), { recursive: true });
  writeFileSync(join(work, 'music', 'BLUEPRINT.md'), '# Night drive\n\nHazy, 90 BPM, A minor.\n\nThe beat arrives at bar 9.\n');
  return { id: p.id, work };
}
