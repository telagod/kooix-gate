import { readFile, readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';

const root = new URL('../build', import.meta.url).pathname;
const manifestPath = new URL('../.svelte-kit/output/client/.vite/manifest.json', import.meta.url)
  .pathname;
const generatedNodesRoot = new URL('../.svelte-kit/generated/client-optimized/nodes/', import.meta.url)
  .pathname;
const maxEntryBytes = Number(process.env.KOOIX_WEB_BUNDLE_MAX_BYTES ?? 750_000);
let failures = [];

async function walk(dir) {
  let entries;
  try {
    entries = await readdir(dir, { withFileTypes: true });
  } catch (error) {
    if (error?.code === 'ENOENT') {
      throw new Error('build output missing; run npm run build before bundle:budget');
    }
    throw error;
  }
  for (const entry of entries) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      await walk(path);
      continue;
    }
    if (!/\.(js|css)$/.test(entry.name)) continue;
    const info = await stat(path);
    if (info.size > maxEntryBytes) failures.push({ path, size: info.size });
  }
}

async function readJson(path, message) {
  try {
    return JSON.parse(await readFile(path, 'utf8'));
  } catch (error) {
    if (error?.code === 'ENOENT') {
      throw new Error(message);
    }
    throw error;
  }
}

async function findRouteNode(routeSource) {
  const entries = await readdir(generatedNodesRoot, { withFileTypes: true });
  for (const entry of entries) {
    if (!entry.isFile() || !entry.name.endsWith('.js')) continue;
    const path = join(generatedNodesRoot, entry.name);
    const source = await readFile(path, 'utf8');
    if (source.includes(routeSource)) {
      return `.svelte-kit/generated/client-optimized/nodes/${entry.name}`;
    }
  }
  return null;
}

function collectStaticChunkFiles(manifest, manifestKey, seen = new Set()) {
  if (!manifestKey || seen.has(manifestKey)) return seen;
  seen.add(manifestKey);
  const entry = manifest[manifestKey];
  if (!entry) return seen;
  if (entry.file) seen.add(entry.file);
  for (const imported of entry.imports ?? []) {
    collectStaticChunkFiles(manifest, imported, seen);
  }
  return seen;
}

function assertManifest(condition, message) {
  if (!condition) failures.push({ path: message, size: 0, manifest: true });
}

async function verifySplitBudget() {
  const manifest = await readJson(
    manifestPath,
    'client manifest missing; run npm run build before bundle:budget'
  );
  const appEntry = manifest['.svelte-kit/generated/client-optimized/app.js'];
  const playgroundNode = await findRouteNode('src/routes/playground/+page.svelte');
  const flowEditorKey = Object.keys(manifest).find((key) =>
    key.includes('src/lib/components/playground/FlowEditor.svelte')
  );
  const markdownKey = Object.entries(manifest).find(
    ([key, entry]) =>
      key.includes('src/lib/components/ui/MarkdownRenderer.svelte') ||
      entry?.name === 'MarkdownRenderer'
  )?.[0];
  const markdownEntry = manifest[markdownKey];
  const flowEntry = manifest[flowEditorKey];

  assertManifest(
    appEntry?.dynamicImports?.length > 10,
    'route-level splitting missing: SvelteKit app entry should dynamic-import route nodes'
  );
  assertManifest(playgroundNode, 'route-level splitting missing: playground route node not found');
  assertManifest(flowEditorKey, 'flow editor lazy load missing: FlowEditor chunk not found');
  assertManifest(
    manifest[playgroundNode]?.dynamicImports?.includes(flowEditorKey),
    'flow editor lazy load missing: playground route must dynamic-import FlowEditor'
  );
  assertManifest(
    !collectStaticChunkFiles(manifest, playgroundNode).has(flowEntry?.file),
    'flow editor lazy load missing: FlowEditor chunk is still statically imported by playground route'
  );
  assertManifest(markdownKey, 'markdown highlighter lazy load missing: MarkdownRenderer chunk not found');
  assertManifest(
    markdownEntry?.dynamicImports?.some(
      (key) => key.includes('highlight.js') && /[/\\]core\.js$/.test(key)
    ),
    'markdown highlighter lazy load missing: highlight.js core must be a dynamic import'
  );
  assertManifest(
    markdownEntry?.dynamicImports?.some((key) => key.includes('marked/lib/marked.esm.js')),
    'markdown highlighter lazy load missing: marked must be a dynamic import'
  );
  assertManifest(
    !markdownEntry?.imports?.some((key) => key.includes('highlight.js') || key.includes('marked')),
    'markdown highlighter lazy load missing: MarkdownRenderer statically imports markdown parser'
  );
}

await walk(root);
await verifySplitBudget();
if (failures.length) {
  for (const item of failures) {
    if (item.manifest) {
      console.error(`bundle split budget failed: ${item.path}`);
    } else {
      console.error(`bundle budget exceeded: ${item.path} ${item.size} > ${maxEntryBytes}`);
    }
  }
  process.exit(1);
}
console.log(
  `bundle budget ok: every js/css asset <= ${maxEntryBytes} bytes; route, flow editor and markdown lazy split verified`
);
