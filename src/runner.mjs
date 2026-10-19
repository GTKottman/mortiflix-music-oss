// The runner: picks the next project that has work, starts one session for it, and repeats. Sessions end at gates,
// so a project waiting on you costs nothing. One session at a time, first in first out.
import { EventEmitter } from 'node:events';
import { mkdirSync, rmSync, writeFileSync, readFileSync, existsSync, appendFileSync, readdirSync, statSync } from 'node:fs';
import { join } from 'node:path';
import { REPO, paths, loadConfig, readSessionEnv } from './studio.mjs';
import { sessionVoiceEnv } from './voice/index.mjs';
import { sessionToolsEnv, ensureBrowser } from './setup.mjs';
import { listProjects, loadProject, projectPaths, projectPipeline, update, event, readEvents, now } from './projects.mjs';
import { settle, needsYou } from './gates.mjs';
import { projectMissingKeys, missingKeysText } from './keys.mjs';
import { prepareWorkdir, writeTorch, sessionReason } from './torch.mjs';
import { loadTheme } from './svc/theme.mjs';
import { openBridge } from './bridge.mjs';
import { quickScriptStep, quickScript, quickBriefStep, quickBrief } from './quickscript.mjs';
import { quickVoiceStep, quickVoice } from './quickvoice.mjs';
import { quickFramesStep, quickFrames } from './quickframes.mjs';
import { quickTransitionsStep, quickTransitions } from './quicktransitions.mjs';
import { quickAnimaticStep, quickAnimatic } from './quickanimatic.mjs';
import { RenderQueue } from './renderq.mjs';
import * as claudeCode from './backends/claude-code.mjs';
import * as anthropicApi from './backends/anthropic-api.mjs';
import * as demo from './backends/demo.mjs';

export const BACKENDS = { 'claude-code': claudeCode, 'anthropic-api': anthropicApi, demo };
const PROGRESS = ['SUBMITTED', 'STEP_DONE', 'ASKED', 'NEEDS_YOU', 'HANDOFF'];
const GATE_PROGRESS = ['SUBMITTED', 'STEP_DONE', 'ASKED', 'NEEDS_YOU'];
const MAX_SESSIONS_WITHOUT_GATE = 8;

// A session that ended because Claude Code isn't signed in ("OAuth session expired", "401", "Please run /login").
export const loginTrouble = (error) => /oauth|authenticat|\b401\b|\/login|not logged in|logged out|signed out|unauthori[sz]ed|invalid api key|credentials? (?:expired|invalid)|session expired|token expired/i.test(String(error || ''));

// What every session opened first, one tool call each (ls, cat checklist, JOURNAL, feedback, PIPELINE.md, GATES.md,
// SKILLS.md): handed over in the prompt instead, so the session starts working on its first step, not on step six
// with all of them re-read on every step after. Long journals and feedback keep their newest part.
const START_FILES = [
  ['CLAUDE.md', 'CLAUDE.md', 0],
  ['SKILLS.md', 'SKILLS.md', 0],
  ['pipeline/PIPELINE.md', 'pipeline/PIPELINE.md', 0],
  ['.mortiflix/GATES.md', '.mortiflix/GATES.md', 0],
  ['checklist.md', 'checklist.md', 0],
  ['JOURNAL.md', 'JOURNAL.md (newest part)', 12_000],
];
const FEEDBACK_MAX = 20_000;

export function startPrompt(work, reason, { claudeMd = true } = {}) {
  const parts = [];
  const add = (label, text, max) => {
    const body = max && text.length > max ? `…\n${text.slice(-max)}` : text;
    parts.push(`<file path="${label}">\n${body.trim()}\n</file>`);
  };
  for (const [file, label, max] of START_FILES) {
    if (file === 'CLAUDE.md' && !claudeMd) continue;
    const f = join(work, file);
    if (existsSync(f)) add(label, readFileSync(f, 'utf8'), max);
  }
  const fb = join(work, 'feedback');
  if (existsSync(fb)) {
    let left = FEEDBACK_MAX;
    const files = readdirSync(fb).filter((n) => statSync(join(fb, n)).isFile())
      .sort((a, b) => statSync(join(fb, b)).mtimeMs - statSync(join(fb, a)).mtimeMs);
    for (const n of files) {
      const text = readFileSync(join(fb, n), 'utf8');
      if (text.length > left) { parts.push(`(older feedback files not included: open feedback/ if you need them)`); break; }
      left -= text.length;
      add(`feedback/${n}`, text, 0);
    }
  }
  return [
    `These are this project's start files as they are right now: ${claudeMd ? 'CLAUDE.md (follow it), ' : ''}SKILLS.md, the pipeline, the gate protocol, the checklist, the journal and the feedback. Don't open them again: start on the work. Open any other file as usual.`,
    ...parts,
    `Resume this project from the checklist, the journal and the feedback above. Why this session started: ${reason}`,
  ].join('\n\n');
}

export class Runner extends EventEmitter {
  constructor(root, { log = () => {} } = {}) {
    super();
    this.root = root;
    this.log = log;
    this.current = null;
    this.activity = new Map(); // project id -> recent activity lines
    this.renders = new RenderQueue({ logDir: join(paths(root).run, 'renders'), onChange: () => this.emit('change') });
  }

  // Only one runner per studio (the web server and `mortiflix run` would otherwise start two sessions at once).
  acquire() {
    const lock = join(paths(this.root).run, 'runner.pid');
    mkdirSync(paths(this.root).run, { recursive: true });
    if (existsSync(lock)) {
      const pid = Number(readFileSync(lock, 'utf8'));
      if (pid && pid !== process.pid && alive(pid)) throw new Error(`another Mortiflix runner (pid ${pid}) is already running this studio`);
    }
    writeFileSync(lock, String(process.pid));
    this.lockFile = lock;
    // Sessions that were running when the last runner died are over: say so in their log.
    for (const p of listProjects(this.root)) {
      if (p.session?.running) {
        update(this.root, p.id, (x) => { x.session = null; });
        event(this.root, p.id, { event: 'SESSION_ENDED', details: 'interrupted (the studio was stopped)' });
      }
    }
  }

  release() {
    if (this.lockFile && existsSync(this.lockFile) && readFileSync(this.lockFile, 'utf8') === String(process.pid)) rmSync(this.lockFile);
  }

  next() {
    return listProjects(this.root)
      .filter((p) => p.state === 'queued')
      .sort((a, b) => String(a.queued_at).localeCompare(String(b.queued_at)))[0] || null;
  }

  // Run sessions until nothing is runnable. With `watch`, keep looking for new work until stopped.
  async loop({ watch = false, pollMs = 1500 } = {}) {
    this.stopping = false;
    while (!this.stopping) {
      const p = this.next();
      if (p) await this.runSession(p.id);
      else if (!watch) break;
      else await new Promise((ok) => { this.poke = ok; setTimeout(ok, pollMs); });
      this.poke = null;
    }
  }

  wake() { this.poke?.(); }

  async stop() {
    this.stopping = true;
    this.current?.abort.abort();
    this.wake();
    while (this.current) await new Promise((ok) => setTimeout(ok, 100));
  }

  // Stop the session working on one project (pause / cancel from the UI).
  stopProject(id) {
    if (this.current?.projectId === id) this.current.abort.abort();
  }

  addActivity(projectId, item) {
    const row = { t: now(), ...item, text: String(item.text || '').slice(0, 2000) };
    const list = this.activity.get(projectId) || [];
    list.push(row);
    if (list.length > 300) list.splice(0, list.length - 300);
    this.activity.set(projectId, list);
    if (this.current?.activityFile) appendFileSync(this.current.activityFile, JSON.stringify(row) + '\n');
    this.emit('activity', { project: projectId, ...row });
  }

  async runSession(projectId) {
    const root = this.root;
    // A key the next steps need is missing: ask for it now, rather than let a session fail halfway through.
    const missing = projectMissingKeys(root, projectId, { upcoming: true });
    if (missing.length) {
      needsYou(root, projectId, missingKeysText(missing));
      this.log(`‖ ${projectId}: needs a key (${missing.join(', ')})`);
      this.emit('change');
      return;
    }
    const config = loadConfig(root);
    // A project can pin its own backend (the demo project does); otherwise the studio's setting.
    const backendName = loadProject(root, projectId).backend || config.backend;
    config.backend = backendName;
    const backend = BACKENDS[backendName];
    if (!backend) throw new Error(`unknown backend "${backendName}" (claude-code, anthropic-api or demo)`);
    const pp = projectPaths(root, projectId);
    const sessionId = new Date().toISOString().replace(/[:.]/g, '-');
    const transcript = join(pp.sessions, `${sessionId}.jsonl`);
    const abort = new AbortController();
    this.current = { projectId, sessionId, abort, activityFile: join(pp.sessions, `${sessionId}.activity.jsonl`) };
    const startedAt = Date.now();
    const eventsBefore = readEvents(root, projectId, { limit: 100000 }).length;

    update(root, projectId, (p) => { p.session = { running: true, id: sessionId, backend: config.backend, started_at: now() }; });
    const reason = sessionReason(root, projectId);
    event(root, projectId, { event: 'SESSION_STARTED', details: `${config.backend}: ${reason}` });
    this.emit('change');
    this.log(`▶ ${projectId}: ${reason}`);

    let bridge;
    let result;
    const timer = setTimeout(() => { this.addActivity(projectId, { kind: 'info', text: `Session time limit (${config.maxSessionMinutes} min) reached: stopping.` }); abort.abort(); }, config.maxSessionMinutes * 60_000);
    try {
      prepareWorkdir(root, projectId);
      mkdirSync(join(paths(root).run, 'shared'), { recursive: true });
      writeTorch(root, projectId, { backend: config.backend, reason });
      // Keys and settings for the session: session.env, the studio's voice (its key only if it narrates with it), and the
      // tools setup installed (Strudel, Chrome, the studio's Blender) with the owner's music and asset choices.
      const sessionEnv = { ...readSessionEnv(root), ...sessionVoiceEnv(root), ...sessionToolsEnv(root), MFX_HOME: REPO };
      // The studio browser: up before the session (started if it can be), and this session's own browser-harness name.
      if (sessionEnv.BU_CDP_URL) {
        const b = await ensureBrowser(root);
        if (b.started) this.addActivity(projectId, { kind: 'info', text: `Started the studio browser (${b.detail}).` });
        else if (!b.ok) this.addActivity(projectId, { kind: 'info', text: `The studio browser isn't answering (${b.detail}): the session works without it.` });
        sessionEnv.BU_NAME = `mfx-${projectId.slice(-4)}-${String(sessionId).replace(/[^a-z0-9]/gi, '').slice(-8)}`;
      }
      bridge = await openBridge({ root, projectId, sessionId, workdir: pp.work, renders: this.renders, sessionEnv });
      this.addActivity(projectId, { kind: 'info', text: `Session started (${config.backend}). ${reason}` });
      const runArgs = {
        root,
        projectId,
        workdir: pp.work,
        prompt: startPrompt(pp.work, reason, { claudeMd: config.backend !== 'anthropic-api' }),
        env: { ...sessionEnv, ...bridge.env, MFX_SHARED: join(paths(root).run, 'shared') },
        transcript,
        onActivity: (a) => this.addActivity(projectId, a),
        signal: abort.signal,
        config,
      };
      // The script alone is next: one quick call writes it (src/quickscript.mjs), a session only if that can't.
      // The brief alone is next: the same, one call (quickBrief).
      const quick = backendName === 'claude-code';
      const scriptView = quick ? quickScriptStep(root, projectId, config) : null;
      const briefView = quick && !scriptView ? quickBriefStep(root, projectId, config) : null;
      // The voice is the studio's own work (quickvoice.mjs): no session, an AI call only to re-spell a failing line.
      const voiceView = quick && !scriptView && !briefView ? quickVoiceStep(root, projectId, config) : null;
      // The style frames, driven by the studio: fresh small calls, the studio renders and shows one contact sheet.
      const framesView = quick && !scriptView && !briefView && !voiceView ? quickFramesStep(root, projectId, config) : null;
      // The transition board, the same way: the catalog and one sheet in, the studio renders and checks the board.
      const cutsView = quick && !scriptView && !briefView && !voiceView && !framesView ? quickTransitionsStep(root, projectId, config) : null;
      // The animatic, the same way: one call writes the video, the studio renders and runs the QC, fresh reviews.
      const animaticView = quick && !scriptView && !briefView && !voiceView && !framesView && !cutsView ? quickAnimaticStep(root, projectId, config) : null;
      const q = { root, id: projectId, run: backend.run, runArgs, config, onActivity: runArgs.onActivity, fallback: () => backend.run(runArgs) };
      result = scriptView ? await quickScript({ ...q, view: scriptView })
        : briefView ? await quickBrief({ ...q, view: briefView })
          : voiceView ? await quickVoice({ ...q, view: voiceView })
            : framesView ? await quickFrames({ ...q, view: framesView, renders: this.renders, sessionId })
              : cutsView ? await quickTransitions({ ...q, view: cutsView, renders: this.renders, sessionId })
                : animaticView ? await quickAnimatic({ ...q, view: animaticView, renders: this.renders, sessionId })
          : await backend.run(runArgs);
    } catch (e) {
      result = { ok: false, error: e.message };
    } finally {
      clearTimeout(timer);
      this.renders.stopSession(sessionId);
      await bridge?.close();
    }

    const minutes = ((Date.now() - startedAt) / 60000).toFixed(1);
    const newEvents = readEvents(root, projectId, { limit: 100000 }).slice(eventsBefore).filter((e) => e.actor === 'studio');
    const progressed = newEvents.some((e) => PROGRESS.includes(e.event));
    const gated = newEvents.some((e) => GATE_PROGRESS.includes(e.event));
    const pipeline = projectPipeline(root, projectId);
    let pausedFor = null;
    let themesMade = false;
    update(root, projectId, (p) => {
      p.session = null;
      // the themes asked for were this session's to make (made or not, one session only: never a loop); made, they
      // go in the chat as a themes card
      if (p.mfx?.samples_wanted) {
        delete p.mfx.samples_wanted;
        themesMade = true;
      }
      p.usage.sessions += 1;
      p.usage.input_tokens += result?.usage?.input_tokens || 0;
      p.usage.output_tokens += result?.usage?.output_tokens || 0;
      if (result?.cost_usd) {
        p.usage.cost_usd = Math.round(((p.usage.cost_usd || 0) + result.cost_usd) * 10000) / 10000;
        // On a Claude plan the reported cost is list price, not a charge: keep that part apart so it's never shown as spend.
        if (result.billing === 'plan') p.usage.plan_usd = Math.round(((p.usage.plan_usd || 0) + result.cost_usd) * 10000) / 10000;
      }
      p.no_progress = progressed ? 0 : (p.no_progress || 0) + 1;
      p.without_gate = gated ? 0 : (p.without_gate || 0) + 1;
      if (p.status?.key === '_render') p.status = null;
      settle(p, pipeline);
      if (p.state === 'queued' && !abort.signal.aborted) {
        if (p.no_progress >= 2) pausedFor = `The last two sessions stopped without moving the project forward${result?.error ? ` (${result.error})` : ''}. Check the activity log, then resume.`;
        else if (p.without_gate >= MAX_SESSIONS_WITHOUT_GATE) pausedFor = `${MAX_SESSIONS_WITHOUT_GATE} sessions in a row ended without submitting or finishing a step. Check the journal, then resume.`;
        // `cause: login`: Claude Code's login expired or failed (the app offers Link Claude Code, then resumes)
        if (pausedFor) { p.state = 'paused'; p.needs_you = { text: pausedFor, at: now(), ...(loginTrouble(result?.error) ? { cause: 'login' } : {}) }; }
      }
    });
    if (themesMade) {
      const t = loadTheme(root, projectId);
      if (t?.samples?.length) event(root, projectId, { event: 'MUSIC_THEME', actor: 'studio', details: `${t.samples.length} themes to hear: ${t.samples.map((x) => x.name).filter(Boolean).join(', ').slice(0, 160)}` });
    }
    event(root, projectId, { event: 'SESSION_ENDED', details: `${result?.ok ? 'ok' : `error: ${result?.error}`} · ${minutes} min` });
    if (pausedFor) event(root, projectId, { event: 'NEEDS_YOU', details: pausedFor });
    this.addActivity(projectId, { kind: result?.ok ? 'info' : 'error', text: result?.ok ? `Session ended (${minutes} min).` : `Session ended: ${result?.error}` });
    this.log(`■ ${projectId}: ${result?.ok ? 'ok' : result?.error} (${minutes} min) → ${loadProject(root, projectId).state}`);
    this.current = null;
    this.emit('change');
    return result;
  }
}

function alive(pid) {
  try { process.kill(pid, 0); return true; } catch (e) { return e.code === 'EPERM'; }
}
