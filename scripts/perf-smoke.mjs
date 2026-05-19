#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync } from 'node:fs';

const testPath = 'crates/gate-server/tests/perf_smoke.rs';
if (!existsSync(testPath)) {
  console.error(`perf smoke missing: ${testPath}`);
  process.exit(1);
}
if (process.argv.includes('--check-script-only')) {
  console.log(`perf smoke script ok: ${testPath}`);
  process.exit(0);
}

const run = spawnSync('cargo', ['test', '-p', 'gate-server', '--test', 'perf_smoke', '--', '--nocapture'], {
  stdio: 'inherit',
  env: { ...process.env, KOOIX_PERF_SMOKE: '1' },
});
process.exit(run.status ?? 1);
