// One heavy render at a time, studio-wide. Remotion (and anything else that drives Chrome or the GPU) is bound by
// memory: two at once is how a long render gets killed. Sessions queue with `mfx render` and poll with
// `mfx render-wait`, so no tool call ever blocks longer than its timeout.
import { spawn } from 'node:child_process';
import { createWriteStream, mkdirSync, readFileSync, existsSync } from 'node:fs';
import { join } from 'node:path';

export class RenderQueue {
  constructor({ logDir, onChange = () => {} }) {
    this.logDir = logDir;
    this.onChange = onChange;
    this.jobs = new Map();
    this.order = [];
    this.next = 1;
  }

  add({ projectId, sessionId, label, argv, cwd, env }) {
    if (!Array.isArray(argv) || !argv.length) throw new Error('nothing to render: mfx render --label "..." -- <command>');
    const id = this.next++;
    mkdirSync(this.logDir, { recursive: true });
    const job = { id, projectId, sessionId, label: String(label || argv.join(' ')).slice(0, 120), argv, cwd, env, state: 'waiting', log: join(this.logDir, `render-${id}.log`), waiters: [] };
    this.jobs.set(id, job);
    this.order.push(id);
    this.pump();
    return this.view(job);
  }

  ahead(job) {
    return this.order.filter((i) => i !== job.id && this.jobs.get(i).state === 'running').length
      + this.order.slice(0, this.order.indexOf(job.id)).filter((i) => this.jobs.get(i).state === 'waiting').length;
  }

  view(job) {
    const tail = existsSync(job.log) ? readFileSync(job.log, 'utf8').slice(-3000) : '';
    return {
      id: job.id,
      label: job.label,
      state: job.state,
      ...(job.state === 'waiting' ? { renders_ahead: this.ahead(job) } : {}),
      ...(job.exitCode !== undefined ? { exit_code: job.exitCode } : {}),
      ...(['done', 'failed'].includes(job.state) ? { output_tail: tail } : {}),
      ...(job.state === 'running' && job.progress ? { progress: { ...job.progress,
        ...(job.progress.total ? { percent: Math.round((job.progress.done / job.progress.total) * 100) } : {}),
        // Time left from the rate so far.
        ...(job.progress.total && job.progress.done ? { seconds_left: Math.round(((Date.now() - Date.parse(job.startedAt)) / job.progress.done) * (job.progress.total - job.progress.done) / 1000) } : {}) } } : {}),
      started_at: job.startedAt || null,
      ended_at: job.endedAt || null,
    };
  }

  get(id, projectId) {
    const job = this.jobs.get(Number(id));
    if (!job || job.projectId !== projectId) throw new Error(`no render ${id} in this project`);
    return job;
  }

  // Resolves when the render finishes or after `seconds`, whichever comes first.
  wait(id, projectId, seconds = 100) {
    const job = this.get(id, projectId);
    if (['done', 'failed'].includes(job.state)) return Promise.resolve(this.view(job));
    return new Promise((ok) => {
      const t = setTimeout(() => { job.waiters = job.waiters.filter((w) => w !== done); ok(this.view(job)); }, Math.min(590, Math.max(1, seconds)) * 1000);
      const done = () => { clearTimeout(t); ok(this.view(job)); };
      job.waiters.push(done);
    });
  }

  pump() {
    if ([...this.jobs.values()].some((j) => j.state === 'running')) return;
    const id = this.order.find((i) => this.jobs.get(i).state === 'waiting');
    if (!id) return;
    const job = this.jobs.get(id);
    job.state = 'running';
    job.startedAt = new Date().toISOString();
    const out = createWriteStream(job.log);
    out.write(`$ ${job.argv.join(' ')}\n`);
    const p = spawn(job.argv[0], job.argv.slice(1), { cwd: job.cwd, env: job.env, stdio: ['ignore', 'pipe', 'pipe'], detached: true });
    job.proc = p;
    p.stdout.pipe(out, { end: false });
    p.stderr.pipe(out, { end: false });
    // How far it is, read from the render's own output (Remotion's "Rendered 120/450", "Encoded …"; ffmpeg's
    // "time=00:00:03.12"), for the owner's live view. At most one update a second.
    const watch = (d) => {
      const s = String(d);
      const m = [...s.matchAll(/(Rendered|Encoded|Rendering frames|Stitching)[^\d]{0,12}(\d+)\s*\/\s*(\d+)/g)].pop();
      const t = [...s.matchAll(/time=(\d+):(\d+):(\d+(?:\.\d+)?)/g)].pop();
      if (m) job.progress = { phase: /Encod|Stitch/.test(m[1]) ? 'encoding' : 'rendering', done: Number(m[2]), total: Number(m[3]) };
      else if (t) job.progress = { phase: 'encoding', seconds: Number(t[1]) * 3600 + Number(t[2]) * 60 + Number(t[3]) };
      else return;
      if (Date.now() - (job.shownAt || 0) > 1000) { job.shownAt = Date.now(); this.onChange(job); }
    };
    p.stdout.on('data', watch);
    p.stderr.on('data', watch);
    const finish = (code, err) => {
      if (job.state !== 'running') return;
      job.exitCode = code;
      job.state = code === 0 ? 'done' : 'failed';
      job.endedAt = new Date().toISOString();
      job.proc = null;
      if (err) out.write(`\n${err.message}\n`);
      out.end(() => {
        for (const w of job.waiters.splice(0)) w();
        this.onChange(job);
        this.pump();
      });
    };
    p.on('error', (e) => finish(127, e));
    p.on('close', (code, signal) => finish(code ?? (signal ? 128 : 1)));
    this.onChange(job);
  }

  // A session ended: its renders stop (a session can't leave work running behind it).
  stopSession(sessionId) {
    for (const job of this.jobs.values()) {
      if (job.sessionId !== sessionId) continue;
      if (job.state === 'waiting') { job.state = 'failed'; job.exitCode = -1; for (const w of job.waiters.splice(0)) w(); }
      if (job.state === 'running' && job.proc) { try { process.kill(-job.proc.pid, 'SIGTERM'); } catch { /* already gone */ } }
    }
    this.pump();
  }

  activeFor(projectId) {
    return [...this.jobs.values()].filter((j) => j.projectId === projectId && ['waiting', 'running'].includes(j.state)).map((j) => this.view(j));
  }
}
