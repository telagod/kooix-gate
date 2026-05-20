import { describe, expect, it } from 'vitest';
import { readFileSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const routesDir = resolve(dirname(fileURLToPath(import.meta.url)), '../routes');
const route = (path: string) => readFileSync(resolve(routesDir, path), 'utf8');

describe('UI copy conventions', () => {
	it('keeps P2.1 wizard and status copy Chinese-first while preserving product terms', () => {
		const pricing = route('admin/pricing/+page.svelte');
		const quotas = route('orgs/[orgId]/quotas/+page.svelte');

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
		const audit = route('admin/audit/+page.svelte');
		const requests = route('admin/requests/+page.svelte');
		const incidents = route('admin/incidents/+page.svelte');

		expect(audit).toContain('Desc 降序');
		expect(audit).toContain('After 变更后');
		expect(requests).toContain('Request ID 请求 ID');
		expect(requests).toContain('Metadata 元数据');
		expect(incidents).toContain('Top failing channels 失败渠道');
		expect(incidents).toContain('Upstream errors 上游错误');
		expect(incidents).not.toContain('>Top failing<');
	});
});
