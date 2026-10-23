// Which studio this service runs (owner, 2026-10-21: "one code, two apps"). `video` is Mortiflix Studio (motion design
// videos). `apps` is the app-making studio: the same engine walks a full application through the industry's planning
// stages, and the AI builds it last. `midi` (owner, 2026-10-23) is the MIDI studio: the same engine walks a song
// through its stages and ends in MIDI files. Set by the app with MFX_PRODUCT=apps or midi; anything else is video.
export const PRODUCT = ['apps', 'midi'].includes(process.env.MFX_PRODUCT) ? process.env.MFX_PRODUCT : 'video';
export const APPS = PRODUCT === 'apps';
export const MIDI = PRODUCT === 'midi';
// The studios that walk their own stages (apps, midi) rather than the video's.
export const STAGED = APPS || MIDI;

export const VIDEO_STAGES = ['script', 'voice', 'styleFrames', 'transitions', 'animatic', 'music', 'edit', 'render'];
export const VIDEO_STAGE_NAMES = ['Script', 'Voice', 'Style frames', 'Transitions', 'Animatic', 'Music', 'Edit', 'Render'];
// Design Docs/App Studio - Plan v1.md: the ten stages in industry order.
export const APP_STAGES = ['idea', 'requirements', 'screens', 'look', 'mockups', 'prototype', 'techPlan', 'tasks', 'build', 'ship'];
export const APP_STAGE_NAMES = ['Idea', 'Requirements', 'Screens', 'Look', 'Mockups', 'Prototype', 'Tech plan', 'Tasks', 'Build', 'Ship'];

// The app steps the studio can do so far (one stage per version); the others wait, so nothing runs a session for them.
export const APP_READY = ['questions', 'idea'];

// Design Docs/MIDI Studio - Plan v1.md: the nine stages of a song, ending in MIDI files.
export const MIDI_STAGES = ['idea', 'genre', 'songMap', 'chords', 'shapes', 'melody', 'arrange', 'feel', 'export'];
export const MIDI_STAGE_NAMES = ['Idea', 'Genre', 'Song map', 'Chords', 'Shapes', 'Melody', 'Arrange', 'Feel', 'Export'];

// The MIDI steps the studio can do so far: none in v1 (one stage per version).
export const MIDI_READY = [];

// The steps that may run in this studio's projects; the video studio runs them all.
export const READY = APPS ? APP_READY : MIDI ? MIDI_READY : null;

// What a pipeline makes in this studio: the app studio lists only "app" pipelines, the MIDI studio only "midi" ones,
// Mortiflix Studio neither.
export const forProduct = (makes) => (APPS ? makes === 'app' : MIDI ? makes === 'midi' : makes !== 'app' && makes !== 'midi');
