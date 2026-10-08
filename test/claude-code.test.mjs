// Mortiflix Music from Claude Code (/mortiflix-music): the CLI without a terminal (JSON views, respond, answer), the owner-only
// guard, the plugin's files, and sessions that don't inherit the conversation that started them.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync, writeFileSync, chmodSync, mkdtempSync, rmSync } from 'node:fs';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { tempStudio } from './helpers.mjs';
import { HELP, run } from '../src/cli.mjs';
import { REPO } from '../src/studio.mjs';
import * as claudeCode from '../src/backends/claude-code.mjs';

// The CLI in this process (spawning one per call would make these tests take seconds), output captured.
async function cli(root, args, env = {}) {
  const out = [];
  const err = [];
  const { log, error } = console;
  const token = process.env.MFX_TOKEN;
  console.log = (...a) => out.push(a.join(' '));
  console.error = (...a) => err.push(a.join(' '));
  if (env.MFX_TOKEN) process.env.MFX_TOKEN = env.MFX_TOKEN; else delete process.env.MFX_TOKEN;
  process.exitCode = undefined;
  try {
    await run(['--studio', root, ...args]);
  } finally {
    Object.assign(console, { log, error });
    if (token === undefined) delete process.env.MFX_TOKEN; else process.env.MFX_TOKEN = token;
  }
  const status = process.exitCode ?? 0;
  process.exitCode = undefined;
  const stdout = out.join('\n');
  return { status, stdout, stderr: err.join('\n'), json: () => JSON.parse(stdout) };
}

const json = async (root, args) => (await cli(root, args)).json();
const status = async (root, args, env) => (await cli(root, args, env)).status;

// A studio with one MIDI pack waiting on its first review (the blueprint).
async function waitingStudio(t) {
  const root = tempStudio(t);
  const riff = join(root, 'riff.mid');
  writeFileSync(riff, 'MThd');
  assert.equal(await status(root, ['new', 'midi-pack', '--title', 'Acme', '--set', 'genre=deep house', '--set', 'topic=sunrise over a harbour', '--file', `material=${riff}`, '--yes']), 0);
  assert.equal(await status(root, ['run']), 0);
  return root;
}

test('a whole project without a terminal: new, run, review --json, changes with notes, approve, delivered', async (t) => {
  const studioRoot = await waitingStudio(t);
  const intake = (await json(studioRoot, ['pipelines', '--json'])).find((p) => p.slug === 'midi-pack').intake;
  assert.ok(intake.some((q) => q.id === 'material' && q.type === 'files'));

  const studio = await json(studioRoot, ['list', '--json']);
  assert.equal(studio.runner, null);
  assert.deepEqual(studio.projects.map((p) => [p.state, p.in_review]), [['waiting', ['blueprint']]]);

  const [waiting] = await json(studioRoot, ['review', '--json']);
  const step = waiting.reviews[0];
  assert.equal(step.version, 1);
  assert.equal(step.items[0].kind, 'text');
  assert.ok(step.items[0].paragraphs.length >= 2);
  assert.equal(step.web, `#/p/${waiting.id}/review/blueprint`);

  const sent = await cli(studioRoot, ['respond', 'acme', 'blueprint', '--changes', '--note', '0¶1: darker chords', '--note', '0@40s: the hook drops out here', '--note', 'slower overall', '--overall', 'close']);
  assert.equal(sent.status, 0, sent.stderr);
  const fb = JSON.parse(readFileSync(join(studioRoot, 'state', waiting.id, 'reviews', 'blueprint', 'v1', 'feedback.json'), 'utf8'));
  assert.deepEqual(fb.notes.map((n) => [n.item, n.paragraph, n.time_sec, n.text]), [[0, 1, undefined, 'darker chords'], [0, undefined, 40, 'the hook drops out here'], [null, undefined, undefined, 'slower overall']]);
  assert.equal(fb.overall, 'close');

  await cli(studioRoot, ['run']);
  const v2 = (await json(studioRoot, ['review', 'acme', '--json']))[0].reviews[0];
  assert.equal(v2.version, 2);
  assert.equal(v2.pin_changes.length, 3);

  assert.equal(await status(studioRoot, ['respond', 'acme', 'blueprint', '--approve']), 0);
  await cli(studioRoot, ['run']);
  const final = (await json(studioRoot, ['review', 'acme', '--json']))[0].reviews[0];
  assert.deepEqual([final.step, final.review, final.items[0].kind], ['final', 'midi', 'midi']);
  assert.equal(await status(studioRoot, ['respond', 'acme', 'final', '--approve']), 0);
  const done = await json(studioRoot, ['status', 'acme', '--json']);
  assert.equal(done.state, 'delivered');
  assert.ok(done.deliverables[0].path.endsWith('.mid'));
});

test('respond refuses unclear verdicts, stale versions and unknown steps, and nothing changes', async (t) => {
  const root = await waitingStudio(t);
  for (const args of [[], ['--approve', '--changes'], ['--changes'], ['--approve', '--version', '2']]) {
    assert.equal(await status(root, ['respond', 'acme', 'blueprint', ...args]), 1, args.join(' '));
  }
  assert.equal(await status(root, ['respond', 'acme', 'nope', '--approve']), 1);
  assert.deepEqual((await json(root, ['list', '--json'])).projects[0].in_review, ['blueprint']);
});

test('inside a studio session the owner\'s commands are refused', async (t) => {
  const root = await waitingStudio(t);
  const session = { MFX_TOKEN: 'a-session-token' };
  for (const args of [['respond', 'acme', 'blueprint', '--approve'], ['answer', 'acme', 'q1', 'yes'], ['review', 'acme'],
    ['reopen', 'acme', 'blueprint', 'again'], ['pause', 'acme'], ['resume', 'acme'], ['cancel', 'acme'], ['checks', 'approve', 'c1']]) {
    const r = await cli(root, args, session);
    assert.equal(r.status, 1, args.join(' '));
    assert.match(r.stderr, /only the owner does that/, args.join(' '));
  }
  // Reading is fine.
  assert.equal(await status(root, ['review', 'acme', '--json'], session), 0);
  assert.equal((await json(root, ['list', '--json'])).projects[0].state, 'waiting');
});

test('setup choices without a terminal', async (t) => {
  const root = tempStudio(t);
  const config = () => JSON.parse(readFileSync(join(root, 'config.json'), 'utf8'));
  assert.equal(await status(root, ['setup', 'daw', 'ableton']), 0);
  assert.equal(config().daw, 'ableton');
  assert.equal((await json(root, ['setup', 'status', '--json'])).tools.daw.name, 'Ableton Live');
  assert.equal(await status(root, ['setup', 'daw', 'none']), 0);
  assert.equal(config().daw, undefined);
  assert.equal(await status(root, ['setup', 'daw', 'jazz']), 1);
  assert.match((await cli(root, ['setup', 'install', 'nope'])).stderr, /unknown tool "nope"/);
  assert.match((await cli(root, ['setup', 'install', 'blender'])).stderr, /unknown tool "blender" \(strudel, chrome\)/);
  assert.ok((await json(root, ['keys', '--json'])).keys.every((k) => !('value' in k)));
});

test('the MIDI pack from a terminal: every file, a clip per section, in one zip', async (t) => {
  const root = await waitingStudio(t);
  const { id } = (await json(root, ['list', '--json'])).projects[0];
  assert.match((await cli(root, ['midi', 'acme'])).stderr, /no MIDI yet/);
  const midi = join(root, 'projects', id, 'out', 'music', 'midi');
  const { mkdirSync } = await import('node:fs');
  mkdirSync(join(midi, 'sections', '01-dawn'), { recursive: true });
  writeFileSync(join(midi, 'acme-all.mid'), 'MThd');
  writeFileSync(join(midi, 'sections', '01-dawn', '01-bass.mid'), 'MThd');
  writeFileSync(join(midi, 'CUE-SHEET.md'), '# Acme');
  const out = join(root, 'pack.zip');
  const r = await cli(root, ['midi', 'acme', '--out', out]);
  assert.equal(r.status, 0, r.stderr);
  const { unzip } = await import('../src/zip.mjs');
  assert.deepEqual(unzip(readFileSync(out)).map((f) => f.name).sort(), ['midi/CUE-SHEET.md', 'midi/acme-all.mid', 'midi/sections/01-dawn/01-bass.mid']);
});

test('the plugin: valid manifests, owner-only skill, and every command it names exists', () => {
  const market = JSON.parse(readFileSync(join(REPO, '.claude-plugin', 'marketplace.json'), 'utf8'));
  assert.equal(market.plugins[0].source, './plugin');
  const plugin = JSON.parse(readFileSync(join(REPO, 'plugin', '.claude-plugin', 'plugin.json'), 'utf8'));
  assert.equal(plugin.name, 'mortiflix-music');
  assert.equal(plugin.version, JSON.parse(readFileSync(join(REPO, 'package.json'), 'utf8')).version);

  const skill = readFileSync(join(REPO, 'plugin', 'skills', 'mortiflix-music', 'SKILL.md'), 'utf8');
  const front = skill.match(/^---\n([\s\S]*?)\n---\n/)[1];
  assert.match(front, /^name: mortiflix-music$/m);
  // Typed by the owner only: a studio session (also Claude Code) must never pick it up by itself.
  assert.match(front, /^disable-model-invocation: true$/m);
  // Pre-allowed commands only read.
  assert.doesNotMatch(front.match(/^allowed-tools: (.*)$/m)[1], /respond|answer|\brun\b|\bnew\b|serve|install|approve/);

  const known = new Set([...HELP.matchAll(/mortiflix-music ([a-z|]+)/g)].flatMap((m) => m[1].split('|')));
  const text = skill + readFileSync(join(REPO, 'plugin', 'skills', 'mortiflix-music', 'reference.md'), 'utf8');
  assert.doesNotMatch(text, /`mortiflix [a-z]/, 'every command is spelled mortiflix-music');
  for (const [, cmd] of text.matchAll(/`mortiflix-music ([a-z]+)/g)) assert.ok(known.has(cmd), `the skill names "mortiflix-music ${cmd}", which the CLI doesn't have`);
});

test('a claude-code session doesn\'t inherit the Claude Code conversation that started the run', async (t) => {
  const dir = mkdtempSync(join(tmpdir(), 'mfx-cc-'));
  t.after(() => rmSync(dir, { recursive: true, force: true }));
  const fake = join(dir, 'claude');
  writeFileSync(fake, `#!/bin/sh\nenv > "${join(dir, 'env.txt')}"\necho '{"type":"result","result":"ok","usage":{"input_tokens":1,"output_tokens":1}}'\n`);
  chmodSync(fake, 0o755);
  const saved = {};
  for (const k of ['CLAUDECODE', 'CLAUDE_CODE_SESSION_ID', 'CLAUDE_CODE_MESSAGING_SOCKET']) { saved[k] = process.env[k]; process.env[k] = 'parent'; }
  t.after(() => { for (const [k, v] of Object.entries(saved)) if (v === undefined) delete process.env[k]; else process.env[k] = v; });
  const r = await claudeCode.run({ root: dir, workdir: dir, prompt: 'go', env: { MFX_SOCKET: 's', MFX_TOKEN: 't' }, transcript: join(dir, 't.jsonl'),
    onActivity: () => {}, config: { claudeBin: fake, claudeArgs: [] } });
  assert.equal(r.ok, true);
  const env = readFileSync(join(dir, 'env.txt'), 'utf8');
  for (const k of claudeCode.PARENT_SESSION_VARS) assert.doesNotMatch(env, new RegExp(`^${k}=`, 'm'));
  assert.match(env, /^MFX_TOKEN=t$/m);
});
