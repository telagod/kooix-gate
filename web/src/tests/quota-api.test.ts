import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';

let mockFetch: ReturnType<typeof vi.fn>;

describe('quota api helpers', () => {
	beforeEach(() => {
		localStorage.clear();
		mockFetch = vi.fn();
		vi.stubGlobal('fetch', mockFetch);
		vi.stubGlobal('window', { location: { pathname: '/orgs', href: '' } });
	});

	afterEach(() => {
		vi.unstubAllGlobals();
		vi.resetModules();
	});

	async function loadApi() {
		return await import('$lib/api');
	}

	it('uses raw typed ids in paths and explain params', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		const orgId = 'org_019e2c1ba7d17162842207e4b24f5f98';
		const quota = {
			id: '019e2c1b-a7d1-7162-8422-07e4b24f5f97',
			scope_kind: 'org',
			scope_id: '019e2c1b-a7d1-7162-8422-07e4b24f5f98',
			dimension: 'rpm',
			model_filter: 'gpt-4o*',
			limit_value: '60',
			window_seconds: 60,
			mode: 'enforce',
			enabled: true
		};
		mockFetch
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => [quota] })
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => quota })
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ org_id: orgId, rules: [] }) })
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ org_id: orgId, rows: [] }) })
			.mockResolvedValueOnce({ ok: true, status: 204, json: async () => ({}) });

		const { listQuotas, upsertQuota, explainQuota, reconcileQuotas, deleteQuota } = await loadApi();
		await listQuotas(orgId);
		await upsertQuota(orgId, {
			scope_kind: 'org',
			scope_id: orgId,
			dimension: 'rpm',
			model_filter: 'gpt-4o*',
			limit_value: '60',
			window_seconds: 60,
			mode: 'enforce'
		});
		await explainQuota(orgId, {
			scope_kind: 'org',
			scope_id: orgId,
			dimension: 'rpm',
			model: 'gpt-4o-mini',
			estimated_tokens: 1000,
			estimated_cost_micros: 10000
		});
		await reconcileQuotas(orgId);
		await deleteQuota(orgId, 'quo_019e2c1ba7d17162842207e4b24f5f97');

		expect(mockFetch.mock.calls[0][0]).toContain('/v1/orgs/019e2c1b-a7d1-7162-8422-07e4b24f5f98/quotas');
		expect(new Headers(mockFetch.mock.calls[0][1].headers).get('X-Kooix-Org')).toBe(orgId);
		expect(mockFetch.mock.calls[1][0]).toContain('/v1/orgs/019e2c1b-a7d1-7162-8422-07e4b24f5f98/quotas');
		expect(mockFetch.mock.calls[1][1].method).toBe('POST');
		expect(JSON.parse(mockFetch.mock.calls[1][1].body).scope_id).toBe(orgId);
		expect(mockFetch.mock.calls[2][0]).toContain('/quotas/explain?');
		expect(mockFetch.mock.calls[2][0]).toContain('scope_id=019e2c1b-a7d1-7162-8422-07e4b24f5f98');
		expect(mockFetch.mock.calls[2][0]).toContain('estimated_tokens=1000');
		expect(mockFetch.mock.calls[3][0]).toContain('/quotas/reconcile');
		expect(mockFetch.mock.calls[4][0]).toContain('/quotas/019e2c1b-a7d1-7162-8422-07e4b24f5f97');
		expect(mockFetch.mock.calls[4][1].method).toBe('DELETE');
	});
});
