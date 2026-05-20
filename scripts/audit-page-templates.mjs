#!/usr/bin/env node
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const routesRoot = path.join(repoRoot, 'web', 'src', 'routes');

const args = new Set(process.argv.slice(2));
const json = args.has('--json');
const failOnGaps = args.has('--fail-on-gaps');

const TEMPLATE_NAMES = [
  'PageShell',
  'AuthFrame',
  'SectionCard',
  'StatePanel',
  'ModalFrame',
  'DataToolbar',
  'FilterPanel',
  'DataTable'
];

const SHELL_OVERRIDES = new Map([
  ['/', 'custom-public'],
  ['/login', 'AuthFrame'],
  ['/setup', 'AuthFrame'],
  ['/invite/accept', 'AuthFrame'],
  ['/playground', 'custom-canvas']
]);

const TOOLBAR_ROUTES = new Set([
  '/admin/audit',
  '/admin/pricing',
  '/admin/requests',
  '/admin/sso',
  '/admin/users',
  '/channels',
  '/channels/[channelId]',
  '/orgs/[orgId]/billing',
  '/orgs/[orgId]/quotas',
  '/usage',
  '/usage/requests'
]);

const STATE_OPTIONAL_ROUTES = new Set(['/', '/playground']);

function walk(dir) {
  const entries = fs.readdirSync(dir, { withFileTypes: true });
  return entries.flatMap((entry) => {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) return walk(full);
    return entry.isFile() && entry.name === '+page.svelte' ? [full] : [];
  });
}

function routeFromFile(file) {
  const rel = path.relative(routesRoot, file).replaceAll(path.sep, '/');
  const route = rel.replace(/\/?\+page\.svelte$/, '');
  return route ? `/${route}` : '/';
}

function expectedShell(route) {
  return SHELL_OVERRIDES.get(route) ?? 'PageShell';
}

function hasAny(source, patterns) {
  return patterns.some((pattern) =>
    typeof pattern === 'string' ? source.includes(pattern) : pattern.test(source)
  );
}

function audit(file) {
  const source = fs.readFileSync(file, 'utf8');
  const route = routeFromFile(file);
  const templates = Object.fromEntries(TEMPLATE_NAMES.map((name) => [name, hasTemplate(source, name)]));
  const nativeTable = /<table\b/.test(source);
  const shell = templates.PageShell
    ? 'PageShell'
    : templates.AuthFrame
      ? 'AuthFrame'
      : /<h1\b/.test(source)
        ? 'custom-h1'
        : 'missing';
  const wants = expectedShell(route);
  const expectsToolbar = TOOLBAR_ROUTES.has(route);
  const expectsState = !STATE_OPTIONAL_ROUTES.has(route);

  const controls = {
    toolbar: templates.DataToolbar || hasAny(source, ['FilterPills', 'toolbarRow', /搜索|查询|过滤|筛选|selected[A-Z]/]),
    filter: templates.FilterPanel || hasAny(source, ['FilterPills', /filter[A-Z_]|Filter|筛选|搜索/]),
    table: nativeTable ? (templates.DataTable ? 'DataTable' : 'native-table') : 'n/a',
    loading: templates.StatePanel || hasAny(source, ['Skeleton', 'animate-pulse', '加载中', /\bloading\b/i]),
    error: templates.StatePanel || hasAny(source, ['<Alert', '加载失败', '失败', /\berror\b/i]),
    empty: templates.StatePanel || hasAny(source, ['暂无', '无匹配', '还没有', '没有关联', /length\s*===\s*0/, /isEmpty=/])
  };

  const gaps = [];
  if (wants === 'PageShell' && !templates.PageShell) gaps.push('header:missing PageShell');
  if (wants === 'AuthFrame' && !templates.AuthFrame) gaps.push('header:missing AuthFrame');
  if (nativeTable && !templates.DataTable) gaps.push('table:missing DataTable');
  if (expectsState && !controls.loading) gaps.push('state:loading not explicit');
  if (expectsState && !controls.error) gaps.push('state:error not explicit');
  if (nativeTable && !controls.empty) gaps.push('state:empty not explicit');
  if (expectsToolbar && !templates.DataToolbar) {
    gaps.push('toolbar:missing DataToolbar');
  }
  if (hasAny(source, [/advanced/i, '高级筛选']) && !templates.FilterPanel) {
    gaps.push('filter:missing FilterPanel');
  }

  return {
    route,
    file: path.relative(repoRoot, file),
    expected_shell: wants,
    expected_toolbar: expectsToolbar,
    expected_state: expectsState,
    shell,
    templates: TEMPLATE_NAMES.filter((name) => templates[name]),
    has_native_table: nativeTable,
    controls,
    gaps
  };
}

function hasTemplate(source, name) {
  return new RegExp(`<${name}\\b|from ['"][^'"]*${name}\\.svelte['"]`).test(source);
}

const rows = walk(routesRoot).sort().map(audit);
const pagesWithGaps = rows.filter((row) => row.gaps.length > 0);

if (json) {
  console.log(JSON.stringify({ total: rows.length, pages_with_gaps: pagesWithGaps.length, rows }, null, 2));
} else {
  console.log(`# Frontend template audit`);
  console.log('');
  console.log(`Pages: ${rows.length}`);
  console.log(`Pages with gaps: ${pagesWithGaps.length}`);
  console.log('');
  console.log('| Route | Shell | Templates | Table | States | Gaps |');
  console.log('| --- | --- | --- | --- | --- | --- |');
  for (const row of rows) {
    const states = [
      row.controls.loading ? 'loading' : '',
      row.controls.error ? 'error' : '',
      row.controls.empty ? 'empty' : ''
    ].filter(Boolean).join(' / ') || 'none';
    const templates = row.templates.length ? row.templates.join(', ') : '-';
    const gaps = row.gaps.length ? row.gaps.join('<br>') : '—';
    console.log(`| \`${row.route}\` | ${row.shell} / expected ${row.expected_shell} | ${templates} | ${row.controls.table} | ${states} | ${gaps} |`);
  }
}

if (failOnGaps && pagesWithGaps.length > 0) {
  process.exitCode = 1;
}
