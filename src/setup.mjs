// Setup: the applications the studio uses, why each one is needed, whether it's here, and how to install it.
// Everything installs into the studio folder (<studio>/tools/), never system-wide, and nothing installs without being
// asked. The choices (your DAW) live in config.json.
//
//   strudel   Strudel (AGPL, npm): it plays the score, so the engine can check it, capture its MIDI and render it
//   chrome    a headless Chrome that Strudel runs in (an existing Chrome, Chromium or Edge is reused)
import { spawn } from 'node:child_process';
import { existsSync, mkdirSync, readdirSync, writeFileSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { platform } from 'node:os';
import { loadConfig, saveConfig, UserError } from './studio.mjs';
import * as plat from './platform.mjs';

export const STRUDEL_VERSION = '1.3.0';
const OS = platform();
export const REPO_ROOT = plat.REPO_ROOT;
const EXE = OS === 'win32' ? '.exe' : '';

export const toolsDir = (root) => join(root, 'tools');

export const PARTS = [
  {
    id: 'claude',
    title: 'Claude',
    why: 'Claude does the work: it researches the genre, plans the song, writes every part as notes and checks them. Mortiflix Music is the studio around it.',
    needs: 'Your Claude Code login (your plan pays), or an Anthropic API key (you pay per token).',
  },
  {
    id: 'music',
    title: 'The music engine',
    why: 'Every piece is written in Strudel, one MIDI channel per part. The engine plays it to check it (the harmony, the story\'s curve), captures it as MIDI, and renders it to audio so you can listen.',
    needs: 'Strudel (about 20 MB) and a headless Chrome to run it in (your Chrome, Chromium or Edge is reused if you have one).',
    tools: ['strudel', 'chrome'],
  },
  {
    id: 'daw',
    title: 'Your DAW',
    why: 'The MIDI is made to be finished in a DAW. Name yours and every MIDI pack comes with the steps to bring it in there: the tempo, bar 1, one track per channel, the markers.',
    needs: 'Nothing to install. Optional: without one, the steps are written for any DAW.',
  },
];

// The DAWs a MIDI pack writes import steps for (anything else gets the general steps).
export const DAWS = {
  ableton: 'Ableton Live', fl: 'FL Studio', logic: 'Logic Pro', reaper: 'REAPER', bitwig: 'Bitwig Studio',
  cubase: 'Cubase', studioone: 'Studio One', garageband: 'GarageBand', protools: 'Pro Tools', other: 'Another DAW',
};

// ---- running things ----

const which = (bin) => plat.which(bin);

// A command whose output streams to `log`, line by line. Rejects on a non-zero exit.
export function run(cmd, args, { cwd, env, log = () => {} } = {}) {
  return new Promise((ok, fail) => {
    log(`$ ${[cmd, ...args].join(' ')}`);
    const c = plat.command(cmd, args);
    const p = spawn(c.file, c.args, { ...c.options, cwd, env: { ...process.env, ...env }, stdio: ['ignore', 'pipe', 'pipe'] });
    let tail = '';
    const onData = (d) => {
      const s = String(d);
      tail = (tail + s).slice(-2000);
      for (const line of s.split(/\r?\n|\r/)) if (line.trim()) log(`  ${line.trimEnd().slice(0, 300)}`);
    };
    p.stdout.on('data', onData);
    p.stderr.on('data', onData);
    p.on('error', (e) => fail(new Error(`${cmd}: ${e.message}`)));
    p.on('close', (code) => (code === 0 ? ok(tail) : fail(new Error(`${cmd} exited with ${code}: ${tail.trim().split('\n').slice(-3).join(' / ')}`))));
  });
}

// ---- detecting each tool ----

function strudelStatus(root) {
  const dir = join(toolsDir(root), 'strudel');
  const pkg = join(dir, 'node_modules', '@strudel', 'web', 'package.json');
  if (!existsSync(pkg)) return { ok: false, detail: 'not installed' };
  return { ok: true, path: dir, version: JSON.parse(readFileSync(pkg, 'utf8')).version, detail: `Strudel ${JSON.parse(readFileSync(pkg, 'utf8')).version}` };
}

function chromeCandidates(root) {
  const out = [];
  const headless = join(toolsDir(root), 'browsers', 'chrome-headless-shell');
  if (existsSync(headless)) {
    for (const ver of readdirSync(headless)) {
      for (const d of readdirSync(join(headless, ver))) out.push(join(headless, ver, d, `chrome-headless-shell${EXE}`));
    }
  }
  if (OS === 'darwin') out.push('/Applications/Google Chrome.app/Contents/MacOS/Google Chrome', '/Applications/Chromium.app/Contents/MacOS/Chromium');
  else if (OS === 'win32') {
    const bases = [process.env.PROGRAMFILES, process.env['PROGRAMFILES(X86)'], process.env.LOCALAPPDATA].filter(Boolean);
    for (const base of bases) out.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'));
    // Edge is Chromium and comes with Windows 10 and 11: it renders Strudel the same way, with nothing to download.
    for (const base of bases) out.push(join(base, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
  } else for (const b of ['google-chrome-stable', 'google-chrome', 'chromium', 'chromium-browser']) { const p = which(b); if (p) out.push(p); }
  return out;
}

function chromeStatus(root) {
  const saved = loadConfig(root).tools?.chrome;
  const found = [saved, ...chromeCandidates(root)].filter(Boolean).find((p) => existsSync(p));
  return found ? { ok: true, path: found, detail: found } : { ok: false, detail: 'no Chrome or Chromium found' };
}

// Everything at once, for the walkthrough, doctor, the web page and the session brief.
export async function setupStatus(root) {
  return setupStatusSync(root);
}

export function setupStatusSync(root) {
  const config = loadConfig(root);
  return { strudel: strudelStatus(root), chrome: chromeStatus(root), music: musicConfig(config), daw: dawConfig(config) };
}

// ---- the choices ----

// The engine is always Strudel and every piece comes with its MIDI: there is no "music off" in a music studio.
export function musicConfig() {
  return { engine: 'strudel', midi: true };
}

export function dawConfig(config) {
  const id = DAWS[config.daw] ? config.daw : null;
  return { id, name: id ? DAWS[id] : null };
}

export function saveDaw(root, id) {
  const v = id === null || id === '' || id === 'none' ? null : String(id).toLowerCase();
  if (v !== null && !DAWS[v]) throw new UserError(`which DAW? One of: ${Object.keys(DAWS).join(', ')} (or none)`);
  saveConfig(root, { daw: v ?? undefined });
  return dawConfig(loadConfig(root));
}

function saveToolPath(root, key, path) {
  const tools = { ...(loadConfig(root).tools || {}) };
  if (path) tools[key] = path; else delete tools[key];
  saveConfig(root, { tools });
}

// ---- installing ----

async function installStrudel(root, log) {
  const dir = join(toolsDir(root), 'strudel');
  mkdirSync(dir, { recursive: true });
  writeFileSync(join(dir, 'package.json'), JSON.stringify({ name: 'mortiflix-strudel', private: true, license: 'AGPL-3.0-or-later', dependencies: { '@strudel/web': STRUDEL_VERSION } }, null, 2) + '\n');
  await run('npm', ['install', '--no-audit', '--no-fund', '--loglevel=error'], { cwd: dir, log });
  const s = strudelStatus(root);
  if (!s.ok) throw new Error('npm finished but @strudel/web is missing');
  return s;
}

async function installChrome(root, log) {
  const have = chromeStatus(root);
  if (have.ok) { log(`Using ${have.path}`); saveToolPath(root, 'chrome', have.path); return have; }
  const dir = join(toolsDir(root), 'browsers');
  await run('npx', ['--yes', '@puppeteer/browsers', 'install', 'chrome-headless-shell@stable', '--path', dir], { log });
  const s = chromeStatus(root);
  if (!s.ok) throw new Error('the headless Chrome download finished but no executable was found');
  saveToolPath(root, 'chrome', s.path);
  return s;
}

const INSTALLERS = { strudel: installStrudel, chrome: installChrome };
export const TOOLS = Object.keys(INSTALLERS);

export const TOOL_INFO = {
  strudel: { name: 'Strudel', what: `@strudel/web ${STRUDEL_VERSION} from npm (AGPL-3.0)`, where: 'tools/strudel', size: '~20 MB' },
  chrome: { name: 'Headless Chrome', what: 'Chrome for Testing\'s headless shell, from Google (only if you have no Chrome, Chromium or Edge)', where: 'tools/browsers', size: '~100 MB' },
};

export async function installTool(root, id, { log = () => {} } = {}) {
  const fn = INSTALLERS[id];
  if (!fn) throw new UserError(`unknown tool "${id}" (${TOOLS.join(', ')})`);
  mkdirSync(toolsDir(root), { recursive: true });
  return fn(root, log);
}

// ---- what a session gets ----

export function sessionToolsEnv(root) {
  const config = loadConfig(root);
  const env = { MFX_MUSIC: JSON.stringify(musicConfig(config)) };
  const daw = dawConfig(config);
  if (daw.id) env.MFX_DAW = daw.id;
  const s = strudelStatus(root);
  if (s.ok) env.MFX_STRUDEL = s.path;
  const c = chromeStatus(root);
  if (c.ok) env.MFX_CHROME = c.path;
  return env;
}
