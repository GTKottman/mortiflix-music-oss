// One short question to Claude, for the studio's own quick work (an edit the owner asks for in the review room):
// no tools, no session, one answer. It goes through the studio's backend, so it runs on the owner's own login or key.
// The demo backend answers with `demo(prompt)` instead (no Claude, no cost).
import { spawn } from 'node:child_process';
import Anthropic from '@anthropic-ai/sdk';
import { loadConfig } from './studio.mjs';
import { credentials } from './backends/anthropic-api.mjs';
import { PARENT_SESSION_VARS } from './backends/claude-code.mjs';

// Quick work wants speed over depth: Sonnet answers an edit in a few seconds. `quickModel` in config.json overrides it.
const QUICK = { 'claude-code': 'sonnet', 'anthropic-api': 'claude-sonnet-5-5' };

export async function ask(root, { system, prompt, demo = null, backend = null, timeoutMs = 90_000 }) {
  const config = loadConfig(root);
  const which = backend || config.backend;
  if (which === 'demo') {
    if (!demo) throw new Error('the demo backend can\'t answer this');
    return demo(prompt);
  }
  const model = config.quickModel || QUICK[which];
  if (which === 'anthropic-api') {
    const { key } = credentials(root);
    const client = new Anthropic({ ...(key ? { apiKey: key } : {}), maxRetries: 2, timeout: timeoutMs, ...(config._fetch ? { fetch: config._fetch, apiKey: 'test' } : {}) });
    const msg = await client.messages.create({ model, max_tokens: 4000, system, messages: [{ role: 'user', content: prompt }] });
    return msg.content.filter((c) => c.type === 'text').map((c) => c.text).join('');
  }
  if (which === 'claude-code') return claudeOnce(config, { system, prompt, model, timeoutMs });
  throw new Error(`unknown backend ${which}`);
}

function claudeOnce(config, { system, prompt, model, timeoutMs }) {
  const env = { ...process.env };
  for (const k of PARENT_SESSION_VARS) delete env[k];
  const args = ['-p', '--model', model, '--tools', '', '--no-session-persistence', '--output-format', 'text', '--append-system-prompt', system];
  return new Promise((ok, fail) => {
    const p = spawn(config.claudeBin || 'claude', args, { env, stdio: ['pipe', 'pipe', 'pipe'] });
    let out = ''; let err = '';
    const timer = setTimeout(() => { p.kill('SIGTERM'); fail(new Error(`Claude didn't answer within ${Math.round(timeoutMs / 1000)} s`)); }, timeoutMs);
    p.stdout.on('data', (d) => { out += d; });
    p.stderr.on('data', (d) => { err += d; });
    p.on('error', (e) => { clearTimeout(timer); fail(e); });
    p.on('close', (code) => { clearTimeout(timer); if (code === 0) ok(out); else fail(new Error(`claude exited ${code}: ${err.trim().slice(0, 300) || out.trim().slice(0, 300)}`)); });
    p.stdin.end(prompt);
  });
}

// The first JSON object in a model's answer (it may wrap it in a code fence or a sentence).
export function jsonFrom(text) {
  const fenced = /```(?:json)?\s*([\s\S]*?)```/.exec(text);
  const body = fenced ? fenced[1] : text;
  const a = body.indexOf('{'); const b = body.lastIndexOf('}');
  if (a < 0 || b <= a) throw new Error('the answer had no JSON in it');
  return JSON.parse(body.slice(a, b + 1));
}
