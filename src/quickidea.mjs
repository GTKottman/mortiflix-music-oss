// The app studio's Idea stage (Design Docs/App Studio - Plan v1.md, v2): the chat asks what problem the app solves,
// who uses it, what it must do and what it must never do, then a one-page brief comes out. Two small calls with no
// tools, never a session (the app studio's job is to save tokens): one suggests an answer to each question from the
// owner's idea (the chat asks them; the owner takes the suggestion or types their own), one writes the brief from the
// answers. The studio writes brief.md and submits it; Next at the stage gate approves it.
import { runAs } from './models.mjs';
import { existsSync, readFileSync, writeFileSync, appendFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { paths } from './studio.mjs';
import { loadProject, projectPaths, projectPipeline } from './projects.mjs';
import { stepView, submit, submissions } from './gates.mjs';
import { chatHistory } from './svc/chat.mjs';

// The four questions, in the owner's plan's words. Their ids are the answers' keys in the brief prompt.
export const IDEA_QUESTIONS = [
  { id: 'problem', text: 'What problem does the app solve?' },
  { id: 'users', text: 'Who uses it?' },
  { id: 'must_do', text: 'What must it do?' },
  { id: 'must_never', text: 'What must it never do?' },
];
const IDEA_STEPS = ['questions', 'idea'];

// The questions or the brief step, when it's the work to do now; otherwise null.
export function quickIdeaStep(root, id) {
  const pipeline = projectPipeline(root, id);
  if (!IDEA_STEPS.every((k) => pipeline.steps.some((s) => s.key === k))) return null;
  const runnable = stepView(loadProject(root, id), pipeline).filter((s) => ['ready', 'working', 'changes'].includes(s.state));
  return runnable.length === 1 && IDEA_STEPS.includes(runnable[0].key) ? runnable[0] : null;
}

const SUGGEST_SCHEMA = {
  type: 'object',
  required: IDEA_QUESTIONS.map((q) => q.id),
  properties: Object.fromEntries(IDEA_QUESTIONS.map((q) => [q.id, { type: 'string', description: `a suggested answer to "${q.text}", one or two short plain sentences` }])),
};

const BRIEF_SCHEMA = {
  type: 'object',
  required: ['name', 'summary', 'problem', 'users', 'must_do', 'must_never', 'note'],
  properties: {
    name: { type: 'string', description: 'the app\'s name: the owner\'s, or a plain working name' },
    summary: { type: 'string', description: 'one sentence: what the app is and who it is for' },
    problem: { type: 'string', description: 'the problem it solves, two or three plain sentences' },
    users: { type: 'array', items: { type: 'string' }, description: 'who uses it: one line per kind of user' },
    must_do: { type: 'array', items: { type: 'string' }, description: 'what it must do: one short line each, the most important first' },
    must_never: { type: 'array', items: { type: 'string' }, description: 'what it must never do: one short line each' },
    note: { type: 'string', description: 'for the owner: one or two lines on what to check before pressing Next' },
    pin_changes: {
      type: 'array',
      items: { type: 'object', required: ['note', 'change', 'status'], properties: { note: { type: 'integer' }, change: { type: 'string' }, status: { type: 'string', enum: ['done', 'partly', 'not_done'] } } },
    },
  },
};

const RULES = 'Plain words a person with no software training understands; no jargon. A brief is the product owner\'s one page: no screens, no colours and no technology yet (later stages decide those). Never add what the owner didn\'t ask for, and never imitate a real brand that isn\'t the owner\'s own.';

function ownerWords(root, id) {
  const project = loadProject(root, id);
  const parts = [`## The owner's idea (the new-project form)\n${JSON.stringify(project.intake?.answers || {}, null, 1)}`];
  const chat = chatHistory(root, id, { limit: 500 }).filter((r) => ['user', 'action'].includes(r.role) && !r.edit).slice(-40);
  if (chat.length) parts.push(`## The owner's words in the chat (newest last; they win over the form)\n${chat.map((r) => `- ${r.text}`).join('\n')}`);
  return parts;
}

export function suggestPrompt(root, id) {
  const project = loadProject(root, id);
  return [
    `You help the owner of "${project.title}", an app they are planning in the Mortifex studio, answer four questions about it. Suggest an answer to each from what they said, in their voice. Answer with the JSON asked for, nothing else.`,
    ...ownerWords(root, id),
    `## The questions\n${IDEA_QUESTIONS.map((q) => `- ${q.id}: ${q.text}`).join('\n')}`,
    RULES,
  ].join('\n\n');
}

export function briefPrompt(root, id, view) {
  const project = loadProject(root, id);
  const subs = submissions(root, id);
  const asked = subs.filter((s) => s.step === 'questions').at(-1);
  const answers = (asked?.feedback?.answers || []).map((a) => `- ${a.text} → ${a.answer}${a.used_default ? ' (the suggestion, taken as it was)' : ''}`);
  const parts = [
    `You write the one-page brief for "${project.title}", an app the owner is planning in the Mortifex studio. Every later stage (features, screens, look, build) is planned from it. Answer with the JSON asked for, nothing else.`,
    ...ownerWords(root, id),
  ];
  if (answers.length) parts.push(`## The owner's answers\n${answers.join('\n')}`);
  const last = subs.filter((s) => s.step === 'idea').at(-1);
  if (view.state === 'changes' && last) {
    const text = existsSync(join(projectPaths(root, id).work, 'brief.md')) ? readFileSync(join(projectPaths(root, id).work, 'brief.md'), 'utf8') : last.note;
    parts.push(`## Version ${last.version} (the owner asked for changes)\n${text}\n\n## The owner's notes on it\n${last.feedback?.overall ? `Overall: ${last.feedback.overall}\n` : ''}${(last.feedback?.notes || []).map((n) => `${n.n}. ${n.text}`).join('\n')}\nAnswer every note in pin_changes (note number, what you changed, done/partly/not_done); an overall comment is note 1.`);
  }
  parts.push(RULES);
  return parts.join('\n\n');
}

const lines = (a) => (Array.isArray(a) ? a.map((x) => String(x).trim()).filter(Boolean) : []);

export function briefMarkdown(s, version) {
  const list = (a) => lines(a).map((x) => `- ${x}`).join('\n');
  return `# ${String(s.name).trim()} · Brief v${version}\n\n${String(s.summary).trim()}\n\n## The problem\n\n${String(s.problem).trim()}\n\n## Who uses it\n\n${list(s.users)}\n\n## What it must do\n\n${list(s.must_do)}\n\n## What it must never do\n\n${list(s.must_never)}\n`;
}

// The demo backend's answers (no AI, free): the tests and a first look at the flow.
const DEMO_SUGGEST = { problem: 'Demo: it keeps something people lose track of in one place.', users: 'Demo: the owner and people like them.', must_do: 'Demo: save, find and change what it keeps.', must_never: 'Demo: lose anything, or share it without asking.' };
const DEMO_BRIEF = { name: 'Demo app', summary: 'Demo: one place for what people lose track of.', problem: 'Demo: things get lost.', users: ['Demo: the owner'], must_do: ['Save it', 'Find it again'], must_never: ['Lose anything'], note: 'Demo brief.' };

async function call(run, runArgs, config, prompt, schema) {
  if (config.backend === 'demo') return { ok: true, structured: schema === SUGGEST_SCHEMA ? DEMO_SUGGEST : DEMO_BRIEF, usage: {} };
  if (config.backend !== 'claude-code') return { ok: false, error: 'the Idea stage runs on Claude Code: link it in Setup' };
  return runAs(run, 'brief', { ...runArgs, prompt, config: { ...config, claudeArgs: ['--tools', '', '--json-schema', JSON.stringify(schema)] } });
}

// Ask the questions (with suggestions) or write the brief, and submit it. Never falls back to a session: a call that
// fails is reported, and the runner pauses the project after two (the owner sees why and resumes).
export async function quickIdea({ root, id, view, run, runArgs, config, onActivity }) {
  const pp = projectPaths(root, id);
  const dir = join(paths(root).run, 'quick');
  mkdirSync(dir, { recursive: true });
  const args = { ...runArgs, workdir: dir };
  const version = view.version + 1;
  if (view.key === 'questions') {
    onActivity({ kind: 'info', text: 'Suggesting answers to the four questions.' });
    const r = await call(run, args, config, suggestPrompt(root, id), SUGGEST_SCHEMA);
    if (!r.ok || !r.structured) return { ...r, ok: false, error: r.error || 'no suggestions came back' };
    const questions = IDEA_QUESTIONS.map((q) => ({ id: q.id, text: q.text, default: String(r.structured[q.id] || '').trim().slice(0, 1000) || null }));
    submit(root, id, 'questions', { note: 'Four questions about the app, each with a suggested answer from your idea.', questions, items: [], error_checks: [], pin_changes: [] });
    appendFileSync(join(pp.work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · The four Idea questions asked in the chat (v${version}).\n`);
    return r;
  }
  onActivity({ kind: 'info', text: 'Writing the brief in one call.' });
  const r = await call(run, args, config, briefPrompt(root, id, view), BRIEF_SCHEMA);
  const s = r.ok ? r.structured : null;
  if (!s || !String(s.summary || '').trim() || !lines(s.must_do).length) return { ...r, ok: false, error: r.error || 'the brief call gave no brief' };
  writeFileSync(join(pp.work, 'brief.md'), briefMarkdown(s, version));
  submit(root, id, 'idea', { note: String(s.note || '').trim() || 'The brief.', questions: [], items: [{ path: 'brief.md', label: `Brief v${version}` }], error_checks: [], pin_changes: s.pin_changes || [] });
  appendFileSync(join(pp.work, 'JOURNAL.md'), `\n- ${new Date().toISOString()} · Brief v${version} written in one quick call and submitted.\n`);
  return r;
}

// What the Idea stage shows: the brief (when there is one) and where the stage is.
export function ideaView(root, id) {
  const steps = stepView(loadProject(root, id), projectPipeline(root, id));
  const state = (k) => steps.find((s) => s.key === k)?.state || null;
  const f = join(projectPaths(root, id).work, 'brief.md');
  const brief = existsSync(f) ? readFileSync(f, 'utf8') : null;
  return { questions: state('questions'), brief_state: state('idea'), version: steps.find((s) => s.key === 'idea')?.version || 0, brief };
}
