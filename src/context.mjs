// The context engine: what Claude is told before it touches the music, so an edit is made for this piece, this
// section and this owner instead of thrown at the bars.
//
//   editContext(root, id, …)     the brief for one quick edit: the piece, the section and what it's for, the chords,
//                                the part's code and notes there, the bars around it, what the other parts play, what
//                                must survive, the owner's notes on this project and their saved differences
//   saveDifference(root, diff)   a kept (or undone) edit, as a measured difference: the studio's memory of your taste
//   relevantDifferences(…)       the saved differences that bear on a part in a kind of section of a genre
//   differencesBrief(root, p)    the same, in words, for a session writing a new score (CLAUDE.md)
import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { REPO, paths, readJson, writeJson } from './studio.mjs';
import { projectPaths, loadProject } from './projects.mjs';
import * as gates from './gates.mjs';

const { blocksOf, notesText } = await import(join(REPO, 'pipelines', '_shared', 'skills', 'music', 'lib', 'edit.mjs'));

const readJ = (f) => { try { return JSON.parse(readFileSync(f, 'utf8')); } catch { return null; } };
const tasteFile = (root) => join(paths(root).root, 'taste.json');

// ---------- one edit's brief ----------

export const EDIT_SYSTEM = `You edit a few bars of one part of a Strudel score for the studio's owner, who marked the bars and said
what they want there. The studio splices your bars in by itself: everything outside them stays exactly as it is, so
write only the new bars. Make the change they asked for, fitted to this piece: its genre, its section, its chords, the
bars around it and the other parts. If they asked for "different", make it clearly different, not a nudge.

Answer with one JSON object and nothing else:
{"channel": <the MIDI channel you changed>, "bars": {"<bar number>": {"notes": "<one bar of mini-notation>", "velocity": "<a number 0-1, or a pattern of numbers>"}}, "said": "<one plain sentence: what you changed and why it fits>"}

One entry per marked bar. Each "notes" is exactly one bar (one cycle) of Strudel mini-notation: "[a b c d]" splits the
bar evenly, "," stacks layers ("[36 ~ 38 ~], [42*8]"), "~" is a rest, "x*4" repeats, nested [ ] subdivide. No "<>",
no functions, no quotes. Drums (channel 10) are General MIDI numbers: 36 kick, 38 snare, 37 rim, 39 clap, 42 closed
hat, 44 pedal hat, 46 open hat, 49 crash, 51 ride, 45/47/48/50 toms. Pitched parts use note names like a3, c#4, eb2,
in the part's own register, inside the chords and scale.`;

export function editContext(root, id, { from, to, channel = null, ask, manifest, code, located, attempts = [] }) {
  const pp = projectPaths(root, id);
  const project = loadProject(root, id);
  const w = (f) => join(pp.work, f);
  const bp = readJ(w('music/blueprint.json')) || {};
  const genre = readJ(w('music/genre.json'));
  const flow = readJ(w('flow/flow.json'));
  const bpb = manifest.beats_per_bar || 4;
  const sections = (bp.sections || []).filter((s) => s.start_bar <= to && s.end_bar >= from);
  const sectionNames = sections.map((s) => s.name);
  const role = (ch) => (bp.roles || []).find((r) => r.channel === ch);
  const track = (ch) => manifest.tracks.find((t) => t.channel === ch);
  const isDrums = (ch) => ch === 10 || track(ch)?.kind === 'drums';
  const out = [];
  const add = (title, body) => { if (body && String(body).trim()) out.push(`## ${title}\n${String(body).trim()}`); };

  add('What the owner asked', `Bars ${from === to ? from : `${from}-${to}`}${channel ? `, ${partName(channel)}` : ''}: "${ask}"${channel ? '' : '\nThey didn\'t pick a part: choose the one their words mean (a beat or groove is the drums, channel 10).'}`);
  add('The piece', [`${bp.title || project.title}: ${project.intake?.answers?.genre || genre?.name || ''}, ${bp.bpm || manifest.bpm} BPM, ${bp.meter || '4/4'}, ${bp.scale || ''}, ${bp.bars || '?'} bars.`,
    bp.dramatic_reading ? `Its story: ${bp.dramatic_reading}.` : null, bp.style ? `Style: ${bp.style}` : null,
    flow ? `Its flow: ${flow.name}: ${flow.summary}` : null].filter(Boolean).join('\n'));
  add('The section', sections.map((s) => `${s.name} (bars ${s.start_bar}-${s.end_bar}): ${s.function || ''}, intensity ${s.intensity}${s.material ? `, material ${s.material}` : ''}`).join('\n'));
  const chords = (bp.chart || []).filter((c) => c.bars?.[0] <= to && c.bars?.[1] >= from).map((c) => `bars ${c.bars[0]}-${c.bars[1]}: ${c.chords.map((x) => (Array.isArray(x) ? `[${x.join(' ')}]` : x)).join(' ')}`);
  add('The chords there', chords.join('\n'));
  const techs = (bp.techniques || []).filter((t) => (t.where === 'all' || [].concat(t.where).some((x) => sectionNames.includes(x))));
  add('Techniques planned there (keep them heard)', techs.map((t) => `- ${t.name}${t.part ? ` on ${partName(t.part)}` : ''}: ${t.how || ''}`).join('\n'));

  // The parts that play there, the chosen one (or all of them) in full.
  const parts = located.filter((l) => l.channel !== null);
  // No part picked: words about the beat mean the drums, so they get the detail (a shorter brief answers faster);
  // otherwise every part playing there does.
  const DRUM_WORDS = /beat|drum|groove|kick|snare|hat|fill|percussion|rhythm section/i;
  const guess = channel ?? (DRUM_WORDS.test(ask) && parts.some((l) => l.channel === 10) ? 10 : null);
  const focus = guess ? parts.filter((l) => l.channel === guess) : parts;
  for (const l of (focus.length ? focus : guess ? [{ channel: guess, blocks: [], tokens: [] }] : [])) {
    const t = track(l.channel);
    const blocks = blocksOf(code).filter((b) => b.channel === l.channel).map((b) => `${b.label}: ${b.expr}`);
    const lines = [...new Set(l.tokens.map((x) => `line ${x.line}: ${x.marked}`))].slice(0, 12);
    add(`${partName(l.channel)}${role(l.channel)?.job ? `: ${role(l.channel).job}` : ''}`, [
      blocks.length ? `Its code:\n${blocks.join('\n').slice(0, 2500)}` : null,
      lines.length ? `What plays in the marked bars comes from these tokens («marked»):\n${lines.join('\n')}` : null,
      t ? `Now, in the marked bars:\n${notesText(t.notes, { drums: isDrums(l.channel), from, to, beatsPerBar: bpb })}` : 'It is silent in the marked bars.',
      t && from > 1 ? `Before them:\n${notesText(t.notes, { drums: isDrums(l.channel), from: Math.max(1, from - 2), to: from - 1, beatsPerBar: bpb })}` : null,
      t && to < (bp.bars || to) ? `After them:\n${notesText(t.notes, { drums: isDrums(l.channel), from: to + 1, to: Math.min(bp.bars || to + 2, to + 2), beatsPerBar: bpb })}` : null,
    ].filter(Boolean).join('\n\n'));
  }
  const others = manifest.tracks.filter((t) => !focus.some((l) => l.channel === t.channel));
  add('The other parts in those bars', others.map((t) => `${partName(t.channel)}:\n${notesText(t.notes, { drums: isDrums(t.channel), from, to, beatsPerBar: bpb })}`).join('\n'));

  if (genre) {
    const drumsAsked = guess === 10;
    add(`The genre (${genre.name || 'from its research'})`, drumsAsked
      ? [genre.beat?.description, ...(genre.beat?.grids || []).map((g) => `${g.name} (${g.steps} steps):\n${Object.entries(g.parts).map(([k, v]) => `  ${k.padEnd(6)} ${v}`).join('\n')}`),
        genre.beat?.variations?.length ? `Variations: ${genre.beat.variations.join('; ')}` : null,
        ...(genre.unforgettable || []).filter((e) => e.area === 'beat').map((e) => `Signature: ${e.element}: ${e.how}`)].filter(Boolean).join('\n')
      : [genre.feel ? `Feel: ${genre.feel}` : null, genre.hook?.traits?.length ? `Hooks: ${genre.hook.traits.join('; ')}` : null,
        ...(genre.unforgettable || []).filter((e) => e.area !== 'beat').slice(0, 6).map((e) => `Signature (${e.area}): ${e.element}: ${e.how}`)].filter(Boolean).join('\n'));
  }

  // The owner: what they said on this project, and what they've kept or undone before.
  const said = gates.submissions(root, id).flatMap((s) => [s.feedback?.overall, ...(s.feedback?.notes || []).map((n) => n.text)]).filter(Boolean).slice(-8);
  add('What the owner said on this piece so far', said.map((x) => `- ${x}`).join('\n'));
  const kind = guess ? (isDrums(guess) ? 'drums' : 'pitched') : null;
  const diffs = relevantDifferences(root, { genre: project.intake?.answers?.genre || genre?.name, kind, part: channel ? partName(channel) : null, section: sections[0] });
  add('What the owner kept before (do more of this)', diffs.kept.map(diffLine).join('\n'));
  add('What the owner undid before (avoid this)', diffs.undone.map(diffLine).join('\n'));
  const taste = existsSync(join(root, 'TASTE.md')) ? readFileSync(join(root, 'TASTE.md'), 'utf8').trim() : '';
  add('The owner\'s taste (their words)', taste.slice(0, 1500));
  if (attempts.length) add('Already tried for these bars (they want something else)', attempts.map((a, i) => `${i + 1}. ${a.said || ''}\n${Object.entries(a.bars).map(([b, v]) => `  bar ${b}: ${v.notes}`).join('\n')}`).join('\n'));
  add('What must survive', [
    'Everything outside the marked bars stays as it is: the studio guarantees that.',
    channel === 10 || kind === 'drums' ? 'The groove still has to connect to the bars before and after (a fill leads into the next bar; a new beat keeps the tempo feel).' : 'Stay inside the chords and the scale (the harmony lock is checked).',
    techs.length ? 'The techniques planned for this section stay audible.' : null].filter(Boolean).join('\n'));
  return { system: EDIT_SYSTEM, prompt: out.join('\n\n'), sections, kind };

  function partName(ch) { return `${track(ch)?.name || role(ch)?.name || 'part'} (channel ${ch})`; }
}

const diffLine = (d) => `- ${[d.genre, d.part, d.section?.function || d.section?.name, d.section?.intensity !== undefined ? `intensity ${d.section.intensity}` : null].filter(Boolean).join(', ')}: asked "${d.ask}" → ${d.facts.join('; ')}${d.said ? ` (${d.said})` : ''}`;

// ---------- saved differences ----------

export function readDifferences(root) {
  const t = existsSync(tasteFile(root)) ? readJson(tasteFile(root)) : {};
  return Array.isArray(t.differences) ? t.differences : [];
}

export function saveDifference(root, diff) {
  const t = existsSync(tasteFile(root)) ? readJson(tasteFile(root)) : { ratings: {} };
  const list = Array.isArray(t.differences) ? t.differences.filter((d) => d.id !== diff.id) : [];
  list.push(diff);
  t.differences = list.slice(-400);
  writeJson(tasteFile(root), t);
  return diff;
}

// The differences that bear on a part: the same kind of part (drums or pitched) always, then the same genre, part,
// section job and energy count for more; newer ones win ties.
export function relevantDifferences(root, { genre = null, kind = null, part = null, section = null } = {}, limit = 6) {
  const g = String(genre || '').toLowerCase();
  const words = (s) => new Set(String(s || '').toLowerCase().split(/[^a-z0-9]+/).filter((x) => x.length > 2));
  const gw = words(g);
  const score = (d) => {
    if (kind && d.kind !== kind) return -1;
    let s = 0;
    const dg = words(d.genre);
    if (g && [...gw].some((x) => dg.has(x))) s += 3;
    if (part && d.part && words(part).size && [...words(part)].some((x) => words(d.part).has(x))) s += 2;
    if (section && d.section && (d.section.function === section.function || d.section.name === section.name)) s += 2;
    if (section?.intensity !== undefined && d.section?.intensity !== undefined && Math.abs(section.intensity - d.section.intensity) <= 2) s += 1;
    return s;
  };
  const ranked = readDifferences(root).map((d) => ({ d, s: score(d) })).filter((x) => x.s >= 0).sort((a, b) => b.s - a.s || String(b.d.at).localeCompare(String(a.d.at)));
  return { kept: ranked.filter((x) => x.d.status === 'kept').slice(0, limit).map((x) => x.d), undone: ranked.filter((x) => x.d.status === 'undone').slice(0, Math.ceil(limit / 2)).map((x) => x.d) };
}

// For a session writing a score: the differences the owner kept and undid in this genre, as lines for CLAUDE.md.
export function differencesBrief(root, project) {
  const genre = project.intake?.answers?.genre || null;
  const drums = relevantDifferences(root, { genre, kind: 'drums' }, 5);
  const pitched = relevantDifferences(root, { genre, kind: 'pitched' }, 5);
  const kept = [...drums.kept, ...pitched.kept]; const undone = [...drums.undone, ...pitched.undone];
  if (!kept.length && !undone.length) return null;
  return [
    'Edits the owner made in the studio and kept, measured: write the score so they wouldn\'t need making again.',
    ...kept.map(diffLine),
    ...(undone.length ? ['Edits they tried and undid: don\'t write these.', ...undone.map(diffLine)] : []),
  ].join('\n');
}
