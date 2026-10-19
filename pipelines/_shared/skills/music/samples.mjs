#!/usr/bin/env node
// The studio's own copy of the real instruments the music engine plays (VCSL, the Versilian Community Sample Library,
// CC0). Strudel can load VCSL from GitHub, but an offline render starts before a large sample has downloaded, so the
// notes come out silent (moon test, 2026-10-19): the engine serves this local copy instead (lib/headless.mjs, /samples/).
//
//   node samples.mjs fetch <dir> [instrument ...]   download (once) into <dir>/vcsl/ and write <dir>/vcsl.json
//   node samples.mjs list                           the instruments the studio uses
//
// Files are stored as 16-bit 44.1 kHz WAV. vcsl.json keeps Strudel's sample-map format with _base "/samples/vcsl/".
import { existsSync, mkdirSync, writeFileSync, renameSync, rmSync } from 'node:fs';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';

const MAP = 'https://raw.githubusercontent.com/felixroos/dough-samples/main/vcsl.json';
export const INSTRUMENTS = ['steinway', 'harp', 'vibraphone', 'vibraphone_bowed', 'kalimba', 'marimba', 'glockenspiel', 'handbells',
  'tubularbells', 'recorder_alto_sus', 'bassdrum1', 'clap', 'shaker_small', 'tom2_mallet', 'sus_cymbal'];
const LICENCE = 'VCSL, the Versilian Community Sample Library (CC0, public domain): https://github.com/sgossner/VCSL';

const [cmd, dir, ...names] = process.argv.slice(2);
if (cmd === 'list') { console.log(INSTRUMENTS.join(' ')); process.exit(0); }
if (cmd !== 'fetch' || !dir) { console.error('usage: samples.mjs fetch <dir> [instrument ...] | list'); process.exit(1); }

const want = names.length ? names : INSTRUMENTS;
const map = await (await fetch(MAP)).json();
const base = map._base;
const out = { _base: '/samples/vcsl/' };
const jobs = [];
for (const name of want) {
  const entry = map[name];
  if (!entry) { console.error(`✖ VCSL has no "${name}"`); process.exitCode = 1; continue; }
  const local = (rel, i) => `${name}/${String(i).padStart(3, '0')}.wav`;
  if (Array.isArray(entry)) out[name] = entry.map((rel, i) => { jobs.push([base + rel, local(rel, i)]); return local(rel, i); });
  else out[name] = Object.fromEntries(Object.entries(entry).map(([note, v], i) => { const rel = Array.isArray(v) ? v[0] : v; jobs.push([base + rel, local(rel, i)]); return [note, local(rel, i)]; }));
}
const root = join(dir, 'vcsl');
let done = 0; let got = 0; const missing = new Set();
const one = async ([url, rel]) => {
  const dest = join(root, rel);
  if (existsSync(dest)) { done++; return; }
  mkdirSync(dirname(dest), { recursive: true });
  const r = await fetch(url.replace(/ /g, '%20'));
  if (!r.ok) { missing.add(rel); console.log(`! ${r.status}: ${url} (left out: the map points at a file VCSL doesn't have)`); return; }
  const tmp = `${dest}.src`;
  writeFileSync(tmp, Buffer.from(await r.arrayBuffer()));
  const c = spawnSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-i', tmp, '-c:a', 'pcm_s16le', '-ar', '44100', `${dest}.part.wav`]);
  rmSync(tmp, { force: true });
  if (c.status !== 0) throw new Error(`ffmpeg: ${String(c.stderr).slice(-200)}`);
  renameSync(`${dest}.part.wav`, dest);
  done++; got++;
};
const queue = [...jobs];
await Promise.all(Array.from({ length: 6 }, async () => { while (queue.length) await one(queue.shift()); }));
for (const [k, v] of Object.entries(out)) {
  if (k === '_base') continue;
  if (Array.isArray(v)) out[k] = v.filter((rel) => !missing.has(rel));
  else out[k] = Object.fromEntries(Object.entries(v).filter(([, rel]) => !missing.has(rel)));
  if (!Object.keys(out[k]).length) { delete out[k]; console.log(`! ${k}: no files, left out`); }
}
mkdirSync(dir, { recursive: true });
writeFileSync(join(dir, 'vcsl.json'), `${JSON.stringify(out, null, 1)}\n`);
writeFileSync(join(dir, 'SOURCES.md'), `# Samples\n\n${LICENCE}\n\nInstruments: ${Object.keys(out).filter((k) => k !== '_base').join(', ')}\n`);
console.log(`✔ ${done} files (${got} downloaded now) for ${Object.keys(out).length - 1} instruments in ${root}; map ${join(dir, 'vcsl.json')}`);
