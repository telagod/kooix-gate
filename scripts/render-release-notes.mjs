#!/usr/bin/env node
import { readFileSync } from 'node:fs';

const version = (process.env.GITHUB_REF_NAME || process.argv[2] || 'v0.0.0').replace(/^refs\/tags\//, '');
const semver = version.replace(/^v/, '');
const changelog = readFileSync('CHANGELOG.md', 'utf8');

function sectionForVersion(text, versionName) {
  const lines = text.split(/\r?\n/);
  const heading = `## [${versionName}]`;
  const fallbackHeading = '## [Unreleased]';
  const start = findHeading(lines, heading);
  const fallbackStart = findHeading(lines, fallbackHeading);
  const selectedStart = start >= 0 ? start : fallbackStart;
  if (selectedStart < 0) return '- No changelog section found for this tag.';

  const selectedEnd = findNextVersionHeading(lines, selectedStart + 1);
  const body = lines
    .slice(selectedStart + 1, selectedEnd < 0 ? lines.length : selectedEnd)
    .join('\n')
    .trim();
  return body || '- No changelog section found for this tag.';
}

function findHeading(lines, prefix) {
  return lines.findIndex((line) => line.startsWith(prefix));
}

function findNextVersionHeading(lines, from) {
  for (let i = from; i < lines.length; i += 1) {
    if (lines[i].startsWith('## [')) return i;
  }
  return -1;
}

const dockerImage = `ghcr.io/telagod/kooix-gate:${version}`;
const notes = `# Kooix Gate ${version}

## Changelog

${sectionForVersion(changelog, semver)}

## Docker image tag

- \`${dockerImage}\`
- \`ghcr.io/telagod/kooix-gate:latest\` is updated only for stable release tags.

## Migration notes

- Run \`kgctl migrate --dry-run\` before deploying this tag.
- Run \`kgctl migrate\` during the maintenance window, then \`kgctl doctor\`.
- If a release includes storage changes, keep a PostgreSQL backup/snapshot until post-smoke passes.

## Known limitations

- SQL migrations are forward-only; rollback normally means previous image + DB backup restore or a hotfix migration.
- WASM Plugin ABI remains vNext design material unless explicitly listed as runtime-enabled in changelog.
- High-throughput usage retention still requires operator policy for PostgreSQL partition / Timescale deployment.

## Post-release smoke

\`\`\`bash
kgctl doctor
kgctl smoke \\
  --base-url "$KOOIX_PUBLIC_URL" \\
  --email "$KOOIX_SMOKE_EMAIL" \\
  --password "$KOOIX_SMOKE_PASSWORD" \\
  --upstream-base-url "$KOOIX_SMOKE_UPSTREAM_BASE_URL" \\
  --upstream-api-key "$KOOIX_SMOKE_UPSTREAM_API_KEY" \\
  --model "${process.env.KOOIX_SMOKE_MODEL || 'gpt-4o-mini'}"
\`\`\`
`;

process.stdout.write(`${notes}\n`);
