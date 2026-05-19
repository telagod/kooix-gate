#!/usr/bin/env node
import { spawnSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';

const checker = '/home/telagod/.codex/skills/checking-code-quality/scripts/quality_checker.js';
const waiver = 'docs/waivers/quality/2026-05-19-large-files.md';
const paths = process.argv.slice(2);
const targets = paths.length
  ? paths
  : ['crates/gate-server/src', 'crates/gate-storage/src', 'crates/gate-providers/src', 'web/src'];

if (!existsSync(checker)) {
  console.warn(`quality checker not found, skipping: ${checker}`);
  process.exit(0);
}

const waiverText = existsSync(waiver) ? readFileSync(waiver, 'utf8') : '';
let hardFailures = [];
let warnings = [];

for (const target of targets) {
  const run = spawnSync('node', [checker, target, '--json'], { encoding: 'utf8' });
  if (run.status !== 0) {
    process.stdout.write(run.stdout ?? '');
    process.stderr.write(run.stderr ?? '');
    process.exit(run.status ?? 1);
  }
  const report = JSON.parse(run.stdout);
  for (const issue of report.issues ?? []) {
    if (issue.severity === 'warning') {
      const rel = issue.file_path?.replace(`${process.cwd()}/`, '') ?? '';
      if (waiverText.includes(rel)) {
        warnings.push(`waived: ${rel} — ${issue.message}`);
      } else {
        hardFailures.push(`${rel} — ${issue.message}`);
      }
    }
  }
}

for (const line of warnings) console.log(line);
if (hardFailures.length) {
  console.error('quality gate failed:');
  for (const line of hardFailures) console.error(`  - ${line}`);
  process.exit(1);
}
console.log('quality gate ok');
