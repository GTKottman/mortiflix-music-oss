// The Mortiflix service: the AI side of Mortiflix FilmCraft. The desktop app (Rust) runs
// `node bin/mortiflix-service.mjs` and talks JSON lines over stdin/stdout, no port.
// Protocol: docs/mortiflix/protocol.md in the mortiflix-filmcraft repo.
//   request  {id, type, params}
//   reply    {id, ok: true, result} | {id, ok: false, error: {code, message}}
//   event    {event, data}
// Parts extend `handlers` (one entry per message type) and say so in protocol.md.
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { REPO, studioRoot } from './studio.mjs';
import { createProject, loadProject, listProjects } from './projects.mjs';
import { canvasView, nodeAction } from './canvas.mjs';

export const PROTOCOL = 1;
export const STAGES = ['script', 'look', 'shots', 'voiceMusic', 'filmcraft', 'render'];

const version = () => {
  try { return JSON.parse(readFileSync(join(REPO, 'package.json'), 'utf8')).version; } catch { return '0.0.0'; }
};

class ProtocolError extends Error {
  constructor(code, message) { super(message); this.code = code; }
}

export function createService({ root = studioRoot(), send }) {
  const state = {
    project: null,
    stage: 0,
    stages: STAGES.map(() => ({ kind: 'notStarted' })),
    line: '',
  };
  const emit = (event, data) => send({ event, data });
  const status = () => emit('status', { line: state.line, stage: state.stage, project: state.project?.id ?? null, stages: state.stages });
  const needProject = (params) => {
    const id = params?.project || state.project?.id;
    if (!id) throw new ProtocolError('no_project', 'open or make a project first');
    return id;
  };

  const handlers = {
    hello: (p) => {
      const result = { service: 'mortiflix-service', version: version(), protocol: PROTOCOL, studio: root, client: String(p?.client || '') };
      setImmediate(status);
      return result;
    },
    'state.get': () => ({ project: state.project, stage: state.stage, stages: state.stages, line: state.line }),
    'project.list': () => listProjects(root).map((p) => ({ id: p.id, title: p.title, state: p.state, pipeline: p.pipeline })),
    'project.new': (p) => {
      const project = createProject(root, { pipeline: p?.pipeline || 'explainer', title: p?.title, answers: p?.answers || {} });
      state.project = project;
      state.stage = 0;
      state.stages = STAGES.map(() => ({ kind: 'notStarted' }));
      status();
      return { project };
    },
    'project.open': (p) => {
      if (!p?.id) throw new ProtocolError('bad_request', 'project.open needs an id');
      state.project = loadProject(root, String(p.id));
      status();
      return { project: state.project };
    },
    'stage.set': (p) => {
      const stage = Number.isInteger(p?.stage) ? p.stage : STAGES.indexOf(p?.key);
      if (stage < 0 || stage >= STAGES.length) throw new ProtocolError('bad_request', 'stage must be 0-5 or a stage key');
      if (p?.done === true && Number.isInteger(p?.from) && p.from >= 0 && p.from < STAGES.length) state.stages[p.from] = { kind: 'done' };
      state.stage = stage;
      status();
      return { stage, key: STAGES[stage] };
    },
    // Chat (doc 37 fills this in): the service is the source of truth, so the user's own message comes
    // back as a chat.message too.
    'chat.send': (p) => {
      const text = String(p?.text ?? '').trim().slice(0, 20_000);
      if (!text) throw new ProtocolError('bad_request', 'say something');
      const at = new Date().toISOString();
      emit('chat.message', { role: 'user', text, at });
      emit('chat.message', { role: 'system', text: 'The AI chat is not connected yet.', at });
      return { queued: true };
    },
    'node.list': (p) => {
      const view = canvasView(root, needProject(p));
      return { nodes: Object.entries(view.nodes).map(([id, n]) => ({ ...n, id })), open_edits: view.open_edits };
    },
    // OK / Fix / Cut it (and clear) on one node: the canvas's own nodeAction.
    'node.edit': (p) => {
      const action = String(p?.action || '').toLowerCase();
      if (!['ok', 'fix', 'cut', 'clear'].includes(action)) throw new ProtocolError('bad_request', 'action must be ok, fix, cut or clear');
      if (!p?.node) throw new ProtocolError('bad_request', 'node.edit needs a node');
      return nodeAction(root, needProject(p), String(p.node), { action, text: p.text, bars: p.bars ?? null, part: p.part ?? null });
    },
  };

  async function handle(msg) {
    if (!msg || typeof msg !== 'object' || Array.isArray(msg)) return send({ id: null, ok: false, error: { code: 'bad_request', message: 'a request is a JSON object' } });
    const { id = null, type, params = {} } = msg;
    const h = typeof type === 'string' && Object.hasOwn(handlers, type) ? handlers[type] : null;
    if (!h) return send({ id, ok: false, error: { code: 'unknown_type', message: `unknown type ${JSON.stringify(type)}` } });
    try {
      const result = await h(params ?? {});
      send({ id, ok: true, result: result ?? null });
    } catch (e) {
      send({ id, ok: false, error: { code: e.code && typeof e.code === 'string' && !e.code.startsWith('E') ? e.code : 'failed', message: String(e.message || e).slice(0, 2000) } });
    }
  }

  return { handle, handlers, state, status };
}

// Run on stdin/stdout. Nothing but protocol lines goes to stdout: console.log goes to stderr.
export function runStdio({ input = process.stdin, output = process.stdout, root } = {}) {
  console.log = console.error;
  const send = (obj) => output.write(JSON.stringify(obj) + '\n');
  const svc = createService({ root, send });
  let buf = '';
  let chain = Promise.resolve();
  input.setEncoding('utf8');
  input.on('data', (chunk) => {
    buf += chunk;
    if (buf.length > 16 * 1024 * 1024 && !buf.includes('\n')) { buf = ''; console.error('mortiflix-service: dropped a line over 16 MB'); return; }
    let nl;
    while ((nl = buf.indexOf('\n')) >= 0) {
      const line = buf.slice(0, nl).trim();
      buf = buf.slice(nl + 1);
      if (!line) continue;
      let msg;
      try { msg = JSON.parse(line); } catch { send({ id: null, ok: false, error: { code: 'bad_json', message: 'not JSON' } }); continue; }
      chain = chain.then(() => svc.handle(msg));
    }
  });
  // The app closed our stdin (it quit or restarts us): finish what is running and go.
  input.on('end', () => chain.finally(() => process.exit(0)));
  return svc;
}
