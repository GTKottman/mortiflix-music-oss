// Which studio this service runs (owner, 2026-10-21: "one code, two apps"). `video` is Mortiflix Studio (motion design
// videos). `apps` is the app-making studio: the same engine walks a full application through the industry's planning
// stages, and the AI builds it last. Set by the app with MFX_PRODUCT=apps; anything else is video.
export const PRODUCT = process.env.MFX_PRODUCT === 'apps' ? 'apps' : 'video';
export const APPS = PRODUCT === 'apps';

export const VIDEO_STAGES = ['script', 'voice', 'styleFrames', 'transitions', 'animatic', 'music', 'edit', 'render'];
export const VIDEO_STAGE_NAMES = ['Script', 'Voice', 'Style frames', 'Transitions', 'Animatic', 'Music', 'Edit', 'Render'];
// Design Docs/App Studio - Plan v1.md: the ten stages in industry order.
export const APP_STAGES = ['idea', 'requirements', 'screens', 'look', 'mockups', 'prototype', 'techPlan', 'tasks', 'build', 'ship'];
export const APP_STAGE_NAMES = ['Idea', 'Requirements', 'Screens', 'Look', 'Mockups', 'Prototype', 'Tech plan', 'Tasks', 'Build', 'Ship'];

// What a pipeline makes in this studio: the app studio lists only "app" pipelines, Mortiflix Studio never does.
export const forProduct = (makes) => (APPS ? makes === 'app' : makes !== 'app');
