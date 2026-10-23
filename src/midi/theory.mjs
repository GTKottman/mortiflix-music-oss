// Keys, modes and chord symbols for the MIDI studio's checks: every pitch is checked against the song's key and the
// chord sounding at that moment, so a gate never has to ask the owner whether something is in tune.
const NAMES = { C: 0, D: 2, E: 4, F: 5, G: 7, A: 9, B: 11 };
export const MODES = {
  major: [0, 2, 4, 5, 7, 9, 11], minor: [0, 2, 3, 5, 7, 8, 10], dorian: [0, 2, 3, 5, 7, 9, 10],
  mixolydian: [0, 2, 4, 5, 7, 9, 10], phrygian: [0, 1, 3, 5, 7, 8, 10], lydian: [0, 2, 4, 6, 7, 9, 11],
  'harmonic minor': [0, 2, 3, 5, 7, 8, 11],
};
// Strudel's scale names for the modes above.
export const STRUDEL_MODE = { major: 'major', minor: 'minor', dorian: 'dorian', mixolydian: 'mixolydian', phrygian: 'phrygian', lydian: 'lydian', 'harmonic minor': 'harmonic minor' };

// "F#" → 6, "Bb" → 10; null when it isn't a note name.
export function pitchClass(name) {
  const m = /^([A-Ga-g])([#b]?)/.exec(String(name).trim());
  if (!m) return null;
  return (NAMES[m[1].toUpperCase()] + (m[2] === '#' ? 1 : m[2] === 'b' ? -1 : 0) + 12) % 12;
}

export function scaleOf(tonic, mode) {
  const pc = pitchClass(tonic);
  const steps = MODES[mode];
  return pc === null || !steps ? null : steps.map((s) => (pc + s) % 12);
}

// The chord symbol's root and its tones as pitch classes ("Am7" → root 9, tones [9, 0, 4, 7]). Strudel's own voicing
// decides the actual notes; this is only for the checks.
const QUALITIES = [
  [/^(maj7|M7|\^7)/, [0, 4, 7, 11]], [/^(m7b5|ø)/, [0, 3, 6, 10]], [/^(dim7|o7)/, [0, 3, 6, 9]], [/^(dim|o)/, [0, 3, 6]],
  [/^(m9|-9)/, [0, 3, 7, 10, 2]], [/^(maj9|\^9)/, [0, 4, 7, 11, 2]], [/^(m11)/, [0, 3, 7, 10, 5]], [/^(m7|-7)/, [0, 3, 7, 10]],
  [/^(m6)/, [0, 3, 7, 9]], [/^(m13)/, [0, 3, 7, 10, 2, 9]], [/^(m|-)/, [0, 3, 7]], [/^(9sus4|9sus)/, [0, 5, 7, 10, 2]], [/^(13sus4|13sus)/, [0, 5, 7, 10, 2, 9]], [/^(13)/, [0, 4, 7, 10, 2, 9]], [/^(11)/, [0, 7, 10, 2, 5]], [/^(maj13|\^13)/, [0, 4, 7, 11, 2, 9]], [/^(9)/, [0, 4, 7, 10, 2]], [/^(7sus4|7sus)/, [0, 5, 7, 10]], [/^(7)/, [0, 4, 7, 10]],
  [/^(6)/, [0, 4, 7, 9]], [/^(sus4|sus)/, [0, 5, 7]], [/^(sus2)/, [0, 2, 7]], [/^(add9|add2)/, [0, 4, 7, 2]], [/^(aug|\+)/, [0, 4, 8]], [/^$/, [0, 4, 7]],
];
export function chordTones(symbol) {
  const m = /^([A-G][#b]?)(.*)$/.exec(String(symbol).trim());
  if (!m) return null;
  const root = pitchClass(m[1]);
  const rest = m[2].split('/')[0];
  const q = QUALITIES.find(([re]) => re.test(rest));
  if (!q) return null;
  return { root, tones: q[1].map((i) => (root + i) % 12) };
}

export const inScale = (key, scale) => scale.includes(((key % 12) + 12) % 12);

// The chord symbols of a progression string as Strudel reads it: "<Am7 F!2 C>" → ["Am7", "F", "F", "C"].
export function progressionChords(chords, barsPerChord = 1) {
  return chords.flatMap((c) => Array(barsPerChord).fill(c));
}
export const strudelProgression = (chords, barsPerChord = 1) => `<${chords.map((c) => (barsPerChord > 1 ? `${c}!${barsPerChord}` : c)).join(' ')}>`;
