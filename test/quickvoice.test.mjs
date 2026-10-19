// The voice without a session: lines from the script, and the word check that treats numbers as words.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { sentences, linesFromScript } from '../src/quickvoice.mjs';
import { REPO } from '../src/studio.mjs';

test('sentences split at full stops, never after a title or inside a number', () => {
  assert.deepEqual(sentences('Some nights the moon is round. It takes 29.5 days. Mr. Smith said so. Dr. Who? Yes.'),
    ['Some nights the moon is round.', 'It takes 29.5 days.', 'Mr. Smith said so.', 'Dr. Who? Yes.']);
  assert.deepEqual(sentences('  '), []);
});

test('voice lines come from the script: an edited beat keeps its lines, others are split into sentences', () => {
  const view = { beats: [
    { id: 'b01', lines: [{ id: 'b01-1', words: 'One idea. Two ideas.' }] },
    { id: 'b02', lines: [{ id: 'b02-1', words: 'Mine.' }, { id: 'b02-3', words: 'Also mine.' }] },
  ] };
  const l = linesFromScript(view);
  assert.deepEqual(l.map((x) => [x.id, x.script]), [['b01-1', 'One idea.'], ['b01-2', 'Two ideas.'], ['b02-1', 'Mine.'], ['b02-3', 'Also mine.']]);
  assert.equal(l[0].gap_after < l[1].gap_after, true, 'a longer pause between scenes');
  assert.equal(l.at(-1).gap_after, 0);
});

test('the voice check hears "29" and "twenty-nine" as the same words', () => {
  const src = readFileSync(join(REPO, 'pipelines', '_shared', 'skills', 'voiceover', 'vo.mjs'), 'utf8');
  const block = src.match(/const ONES[\s\S]*?const words = [^\n]*\n/)[0];
  // eslint-disable-next-line no-new-func
  const words = new Function(`${block}; return words;`)();
  assert.deepEqual(words('about twenty-nine and a half days'), words('about 29 and a half days'));
  assert.deepEqual(words('in 1979'), words('in nineteen seventy-nine'));
  assert.deepEqual(words('3.5 percent of 1,200'), words('three point five percent of one thousand two hundred'));
});
