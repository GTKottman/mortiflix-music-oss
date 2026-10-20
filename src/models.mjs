// The model for each kind of AI call (owner, 2026-10-20: "hand off to Haiku so that we're not wasting valuable opus
// tokens"). config.models maps a role to a model (an alias the claude CLI knows: opus, sonnet, haiku, or a full id);
// a role left out runs on the session's default. Roles:
//   brief    the brief and its research          script   the script
//   write    style frames, the board's choice, the animatic (the code that becomes the video)
//   review   a look at a render (contact sheet + checks), pass or fixed files
//   codefix  the video's code after a render or type error (the error and the code in, fixed files out)
//   fix      a plan the studio refused, sent back with only the errors (the music plan's fix rounds)
//   plan     the music plan                       respell  voice lines that failed the word check
// A cheaper model that gives no usable answer is never the end: the same call runs again on the default model.
export const ROLES = ['brief', 'script', 'write', 'review', 'codefix', 'fix', 'plan', 'respell'];
// The studio's choice, tested 2026-10-20 on the Braille benchmark: Sonnet reviewed the animatic like Opus did (passed the
// approved one; caught two wrong numbers planted on screen, fixed them, passed the fix); Haiku fixed a refused music plan
// (both errors, nothing else changed) and re-spelled voice lines correctly. Writing, the script, the brief and the music
// plan stay on the default (Opus).
export const STUDIO_MODELS = { review: 'sonnet', codefix: 'sonnet', fix: 'haiku', respell: 'haiku' };

const add = (a = {}, b = {}) => ({ input_tokens: (a.input_tokens || 0) + (b.input_tokens || 0), output_tokens: (a.output_tokens || 0) + (b.output_tokens || 0) });

export async function runAs(run, role, args) {
  const model = (args.config?.models ?? STUDIO_MODELS)[role];
  if (!model) return run(args);
  const r = await run({ ...args, config: { ...args.config, model } });
  const wantsJson = (args.config?.claudeArgs || []).includes('--json-schema');
  if (r.ok && (!wantsJson || r.structured)) return { ...r, model };
  const again = await run(args);
  return { ...again, usage: add(r.usage, again.usage), model: `default (after ${model} gave no answer)` };
}
