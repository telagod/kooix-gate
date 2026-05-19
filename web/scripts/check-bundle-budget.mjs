import { readdir, stat } from 'node:fs/promises';
import { join } from 'node:path';

const root = new URL('../build', import.meta.url).pathname;
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

await walk(root);
if (failures.length) {
  for (const item of failures) {
    console.error(`bundle budget exceeded: ${item.path} ${item.size} > ${maxEntryBytes}`);
  }
  process.exit(1);
}
console.log(`bundle budget ok: every js/css asset <= ${maxEntryBytes} bytes`);
