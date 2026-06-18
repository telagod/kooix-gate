import { readFile, readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';

const root = new URL('../build', import.meta.url).pathname;
const manifestPath = new URL('../.svelte-kit/output/client/.vite/manifest.json', import.meta.url)
  .pathname;
// 0.4.18 收紧门禁：channels 页拆分后 156KB；全局 chunk 最大 ~204KB（含 svelte runtime）。
// 阈值：220 KB（留 ~10% margin），可用 KOOIX_WEB_BUNDLE_MAX_BYTES 临时覆盖。
// 0.5.0+ 计划：channels 页 ChannelTable + drawer 全部拆出后阈值收到 180KB。
const maxEntryBytes = Number(process.env.KOOIX_WEB_BUNDLE_MAX_BYTES ?? 220_000);
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

function assertManifest(condition, message) {
  if (!condition) failures.push({ path: message, size: 0, manifest: true });
}

async function verifySplitBudget() {
  const manifest = await readJson(
    manifestPath,
    'client manifest missing; run npm run build before bundle:budget'
  );
  const appEntry = manifest['.svelte-kit/generated/client-optimized/app.js'];

  assertManifest(
    appEntry?.dynamicImports?.length > 10,
    'route-level splitting missing: SvelteKit app entry should dynamic-import route nodes'
  );
  // K1 (v0.5.0): Playground 删除带走孤儿三件套（FlowEditor + MarkdownRenderer
  // + highlight.js + marked）。flow editor / markdown highlighter lazy-load
  // 检查随之撤除；保留 route-level splitting 主门禁。
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
  `bundle budget ok: every js/css asset <= ${maxEntryBytes} bytes; route-level splitting verified`
);
