import { describe, expect, it } from 'vitest';
import { readFileSync, readdirSync, statSync, existsSync } from 'node:fs';
import { dirname, resolve, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const routesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../routes');
const route = (path: string) => readFileSync(resolve(routesDir, path), 'utf8');

// 把页面 + 同目录 _components/* 当作一个文案单元读
// 抽组件后不再误判文案丢失（0.4.x 起 wizard 都搬到 _components）
function routeBundle(pagePath: string): string {
	const pageFile = resolve(routesDir, pagePath);
	const pageDir = dirname(pageFile);
	const componentsDir = join(pageDir, '_components');
	const parts: string[] = [readFileSync(pageFile, 'utf8')];
	if (existsSync(componentsDir) && statSync(componentsDir).isDirectory()) {
		for (const entry of readdirSync(componentsDir)) {
			if (entry.endsWith('.svelte')) {
				parts.push(readFileSync(join(componentsDir, entry), 'utf8'));
			}
		}
	}
	return parts.join('\n');
}

describe('UI copy conventions', () => {
	it('keeps P2.1 wizard and status copy Chinese-first while preserving product terms', () => {
		const pricing = routeBundle('admin/pricing/+page.svelte');
		const quotas = routeBundle('orgs/[orgId]/quotas/+page.svelte');

		expect(pricing).toContain('Pricing wizard 向导');
		expect(pricing).toContain('Usage cost 模拟');
		expect(pricing).toContain('Prompt tokens 输入');
		expect(pricing).not.toContain('>estimated cost<');
		expect(pricing).not.toContain('>Usage cost simulation<');

		expect(quotas).toContain('Quota wizard 向导');
		expect(quotas).toContain('Scope 作用域');
		expect(quotas).toContain('Budget window 周期');
		expect(quotas).toContain('would deny 拦截');
		expect(quotas).not.toContain('>Requests to save<');
		expect(quotas).not.toContain('>Local preview<');
	});

	it('keeps admin telemetry labels readable in Chinese-first UI', () => {
		const audit = routeBundle('admin/audit/+page.svelte');
		const requests = routeBundle('admin/requests/+page.svelte');
		const incidents = routeBundle('admin/incidents/+page.svelte');

		expect(audit).toContain('Desc 降序');
		expect(audit).toContain('After 变更后');
		expect(requests).toContain('Request ID 请求 ID');
		expect(requests).toContain('Metadata 元数据');
		expect(incidents).toContain('Top failing channels 失败渠道');
		expect(incidents).toContain('Upstream errors 上游错误');
		expect(incidents).not.toContain('>Top failing<');
	});
});
