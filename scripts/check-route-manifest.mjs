#!/usr/bin/env node
import { readFileSync } from 'node:fs';

const manifestPath = 'crates/gate-server/src/route_manifest.rs';
const manifest = readFileSync(manifestPath, 'utf8');

const routeMacroBlocks = [...manifest.matchAll(/route!\(\s*"([^"]+)"\s*,\s*"([^"]+)"\s*,\s*([A-Za-z0-9_]+)\s*,\s*([A-Z_]+)\s*\)/g)];
const routeStructBlocks = [...manifest.matchAll(/RouteMeta\s*\{([\s\S]*?)\},/g)].map((block) => block[1]);
const routes = routeMacroBlocks.length
  ? routeMacroBlocks.map((m) => ({ method: m[1], path: m[2], auth: m[3], mode: m[4], raw: m[0] }))
  : routeStructBlocks.map((text) => {
      const method = text.match(/method:\s*"([^"]+)"/)?.[1];
      const path = text.match(/path:\s*"([^"]+)"/)?.[1];
      const auth = text.match(/auth:\s*AuthClass::([A-Za-z0-9_]+)/)?.[1];
      const mode = text.match(/modes:\s*([A-Z_]+)/)?.[1];
      return { method, path, auth, mode, raw: text };
    });
const routeKeyList = routes.map((r) => `${r.method} ${r.path}`);
const routeKeys = new Set(routeKeyList);

function fail(message) {
  console.error(`route manifest failed: ${message}`);
  process.exit(1);
}

for (const route of routes) {
  if (!route.method || !route.path || !route.auth || !route.mode) {
    fail(`malformed RouteMeta block: ${route.raw}`);
  }
}

const dataPlane = new Set([
  '/v1/models',
  '/v1/chat/completions',
  '/v1/responses',
  '/v1/embeddings',
  '/v1/images/generations',
  '/v1/audio/speech',
  '/v1/audio/transcriptions',
]);
const gatewayRoutes = routes.filter((r) => r.mode === 'DATA_PLANE').map((r) => r.path);
const forbidden = gatewayRoutes.filter((route) => !dataPlane.has(route));
if (forbidden.length) fail(`gateway leak: ${forbidden.join(', ')}`);
for (const route of dataPlane) {
  if (!gatewayRoutes.includes(route)) fail(`missing data-plane route: ${route}`);
}

const httpVerbsByHandler = {
  get: ['GET'],
  post: ['POST'],
  put: ['PUT'],
  delete: ['DELETE'],
};

function methodsFromHandler(handler) {
  const methods = [];
  for (const [name, verbs] of Object.entries(httpVerbsByHandler)) {
    const re = new RegExp(`(^|[^A-Za-z0-9_])${name}\\s*\\(`);
    if (re.test(handler)) methods.push(...verbs);
  }
  return methods;
}

function routerPrefix(file) {
  if (file.endsWith('/health.rs')) return '';
  if (file.endsWith('/admin.rs') || file.endsWith('/admin/mod.rs') || file.endsWith('/request_logs.rs')) return '/v1/admin';
  return '/v1';
}

const routeFiles = [
  'crates/gate-server/src/routes/audio.rs',
  'crates/gate-server/src/routes/auth.rs',
  'crates/gate-server/src/routes/health.rs',
  'crates/gate-server/src/routes/api_keys.rs',
  'crates/gate-server/src/routes/model_aliases.rs',
  'crates/gate-server/src/routes/me.rs',
  'crates/gate-server/src/routes/embeddings.rs',
  'crates/gate-server/src/routes/usage.rs',
  'crates/gate-server/src/routes/billing.rs',
  'crates/gate-server/src/routes/chat.rs',
  'crates/gate-server/src/routes/settings.rs',
  'crates/gate-server/src/routes/sso.rs',
  'crates/gate-server/src/routes/admin/mod.rs',
  'crates/gate-server/src/routes/models.rs',
  'crates/gate-server/src/routes/images.rs',
  'crates/gate-server/src/routes/invitations.rs',
  'crates/gate-server/src/routes/quotas.rs',
  'crates/gate-server/src/routes/request_logs.rs',
  'crates/gate-server/src/routes/channels.rs',
  'crates/gate-server/src/routes/projects.rs',
  'crates/gate-server/src/routes/setup.rs',
];

const discovered = [];
for (const file of routeFiles) {
  const source = readFileSync(file, 'utf8');
  const prefix = routerPrefix(file);
  const routeRe = /\.route\(\s*"([^"]+)"\s*,\s*([^)]*(?:\([^)]*\))?[^)]*)\)/g;
  for (const match of source.matchAll(routeRe)) {
    const path = `${prefix}${match[1]}`;
    const handler = match[2];
    const methods = methodsFromHandler(handler);
    if (!methods.length) fail(`could not infer method for ${file}:${path}`);
    for (const method of methods) discovered.push({ method, path, file });
  }
}

const missing = discovered.filter((r) => !routeKeys.has(`${r.method} ${r.path}`));
if (missing.length) {
  for (const route of missing) console.error(`  - ${route.method} ${route.path} (${route.file})`);
  fail(`${missing.length} served route(s) missing from ${manifestPath}`);
}

const duplicates = [];
const seen = new Set();
for (const key of routeKeyList) {
  if (seen.has(key)) duplicates.push(key);
  seen.add(key);
}
if (duplicates.length) fail(`duplicate route entries: ${duplicates.join(', ')}`);

console.log(
  `route manifest ok: ${routes.length} manifest routes, ${discovered.length} served route handlers, ${gatewayRoutes.length} data-plane routes`,
);
