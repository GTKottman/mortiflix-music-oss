// The tests were written for Mortiflix Studio's default (video): this repo's default is Mortiflix Music, so they run with
// MFX_PRODUCT=video unless set; the music and app studio tests set their own product before importing the service.
import { spawnSync } from 'node:child_process';

const r = spawnSync(process.execPath, ['--test', ...(process.argv.slice(2).length ? process.argv.slice(2) : ['test/'])], {
  stdio: 'inherit',
  env: { ...process.env, MFX_PRODUCT: process.env.MFX_PRODUCT || 'video' },
});
process.exit(r.status ?? 1);
