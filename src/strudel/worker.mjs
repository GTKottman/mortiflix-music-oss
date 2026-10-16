// The Strudel evaluator process (doc 28 §2.1). The studio starts it with Node's permission model (read access to the
// Strudel install and this folder only: no writes, no child processes, no network modules loaded) and talks to it over
// stdin/stdout, one JSON line each way. It runs a pattern's text and answers with its notes in ticks.
//   in:  { id, code, bars, tpb }
//   out: { id, ok: true, notes: [{ t, len, key, vel, loc: [[start, end], …] }], problems: [...] } | { id, ok: false, error, line }
import './hooks.mjs';
import { createInterface } from 'node:readline';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

const dir = process.argv[2];
const MAX_NOTES = 20_000;
const mod = (name) => import(pathToFileURL(join(dir, 'node_modules', '@strudel', name, 'dist', 'index.mjs')).href);

// Strudel prints a banner and browser warnings on load; stdout is our channel, so they go to stderr.
const out = process.stdout.write.bind(process.stdout);
console.log = console.info = console.warn = (...a) => process.stderr.write(a.join(' ') + '\n');

// A pattern's stray async error (a refused network call, say) must not take the evaluator down.
process.on('uncaughtException', (e) => process.stderr.write(`stray error: ${e.message}\n`));
process.on('unhandledRejection', (e) => process.stderr.write(`stray rejection: ${e?.message || e}\n`));

const core = await mod('core');
const mini = await mod('mini');
const tonal = await mod('tonal');
const { transpiler } = await mod('transpiler');
const noop = () => {};
await core.evalScope(core, mini, tonal, { samples: noop, soundfont: noop, registerSound: noop, aliasBank: noop, initAudioOnFirstClick: noop });

// The helpers a pattern's text may use (doc 28 §3.4). Bars are 1-based and inclusive, like the owner reads them.
globalThis.mfx = {
  bars: (pat, from, to, repl) => core.stack(
    pat.filterWhen((t) => t < from - 1 || t >= to),
    repl.late(from - 1).filterWhen((t) => t >= from - 1 && t < to)),
};

const frac = (f) => ({ s: Number(f.s ?? 1), n: Number(f.n), d: Number(f.d) });
const r3 = (v) => Math.round(v * 1000) / 1000;

function notesOf(pattern, bars, tpb) {
  const problems = [];
  const notes = [];
  const where = (b) => { const bar = Math.floor(b) + 1; return `bar ${bar}, beat ${Math.round(((b - bar + 1) * 4 + 1) * 100) / 100}`; };
  for (const h of pattern.queryArc(0, bars)) {
    if (!h.hasOnset()) continue;
    if (notes.length >= MAX_NOTES) { problems.push(`more than ${MAX_NOTES} notes: the rest are left out`); break; }
    const v = h.value !== null && typeof h.value === 'object' ? h.value : { note: h.value };
    const b = frac(h.whole.begin.mul(tpb));
    const span = frac(h.whole.end.sub(h.whole.begin).mul(tpb));
    const at = h.whole.begin.valueOf();
    let t = b.s * b.n / b.d;
    if (b.d !== 1) { problems.push(`off the 960 grid at ${where(at)}`); t = Math.round(t); }
    const clip = Number(v.clip ?? v.legato ?? 1);
    let key = null;
    try { key = core.valueToMidi(v); } catch { key = null; }
    if (key === null || key === undefined || Number.isNaN(Number(key))) { problems.push(`no pitch at ${where(at)}`); continue; }
    notes.push({
      t, len: Math.max(1, Math.round(span.n / span.d * clip)), key: Math.round(Number(key)),
      vel: r3(Math.min(1, Math.max(0, Number(v.velocity ?? 0.8)))),
      loc: (h.context?.locations || []).map((l) => [l.start, l.end]),
    });
  }
  notes.sort((x, y) => x.t - y.t || x.key - y.key || x.len - y.len);
  return { notes, problems };
}

createInterface({ input: process.stdin }).on('line', async (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { return; }
  try {
    const { pattern } = await core.evaluate(String(msg.code), transpiler);
    if (!pattern || typeof pattern.queryArc !== 'function') throw new Error('the text makes no pattern');
    out(JSON.stringify({ id: msg.id, ok: true, ...notesOf(pattern, msg.bars, msg.tpb) }) + '\n');
  } catch (e) {
    const lineNo = e.loc?.line ?? (String(e.stack).match(/<anonymous>:(\d+):/)?.[1] ? Number(RegExp.$1) : null);
    out(JSON.stringify({ id: msg.id, ok: false, error: String(e.message || e).split('\n')[0], line: lineNo }) + '\n');
  }
});
out(JSON.stringify({ ready: true }) + '\n');
