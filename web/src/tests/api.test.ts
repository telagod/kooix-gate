import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';

let mockFetch: ReturnType<typeof vi.fn>;

describe('api module', () => {
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

	it('login sends POST with email/password and returns result', async () => {
		const body = {
			access_token: 'at',
			refresh_token: 'rt',
			expires_at: '2026-01-01T00:00:00Z',
			user: { id: '1', email: 'a@b.com', display_name: null }
		};
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 200,
			json: async () => body
		});

		const { login } = await loadApi();
		const result = await login('a@b.com', 'pass');

		expect(mockFetch).toHaveBeenCalledTimes(1);
		const [url, opts] = mockFetch.mock.calls[0];
		expect(url).toContain('/v1/auth/login');
		expect(opts.method).toBe('POST');
		expect(JSON.parse(opts.body)).toEqual({ email: 'a@b.com', password: 'pass' });
		expect(result.access_token).toBe('at');
	});

	it('throws ApiError on non-ok response', async () => {
		mockFetch.mockResolvedValueOnce({
			ok: false,
			status: 422,
			json: async () => ({ error: { code: 'validation', message: 'bad input' } })
		});

		const { listProjects, ApiError } = await loadApi();
		await expect(listProjects('org-1')).rejects.toThrow('bad input');
		try {
			await listProjects('org-1');
		} catch (e: any) {
			// second call is new import needed because we exhausted mock
		}
	});

	it('attaches Authorization header when token exists', async () => {
		localStorage.setItem('kooix_access_token', 'my-token');
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 200,
			json: async () => ({ subject: { kind: 'user' }, current_org: null, is_platform_admin: false, orgs: [] })
		});

		const { getMe } = await loadApi();
		await getMe();

		const [, opts] = mockFetch.mock.calls[0];
		const headers = new Headers(opts.headers);
		expect(headers.get('Authorization')).toBe('Bearer my-token');
	});

	it('auto-refreshes on 401 then retries', async () => {
		localStorage.setItem('kooix_access_token', 'old');
		localStorage.setItem('kooix_refresh_token', 'rt-valid');

		// First call: 401
		mockFetch.mockResolvedValueOnce({
			ok: false,
			status: 401,
			json: async () => ({ error: { code: 'expired', message: 'token expired' } })
		});
		// Refresh call: success
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 200,
			json: async () => ({ access_token: 'new-token', expires_at: '2026-12-01T00:00:00Z' })
		});
		// Retry call: success
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 200,
			json: async () => ({ subject: { kind: 'user' }, current_org: 'org-1', is_platform_admin: false, orgs: ['org-1'] })
		});

		const { getMe } = await loadApi();
		const result = await getMe();

		expect(mockFetch).toHaveBeenCalledTimes(3);
		expect(result.current_org).toBe('org-1');
		// Token should be updated in localStorage
		expect(localStorage.getItem('kooix_access_token')).toBe('new-token');
	});

	it('redirects to login on 401 when refresh fails', async () => {
		localStorage.setItem('kooix_access_token', 'old');
		localStorage.setItem('kooix_refresh_token', 'rt-bad');

		// First call: 401
		mockFetch.mockResolvedValueOnce({
			ok: false,
			status: 401,
			json: async () => ({})
		});
		// Refresh call: fails
		mockFetch.mockResolvedValueOnce({
			ok: false,
			status: 401,
			json: async () => ({})
		});

		const { getMe, ApiError } = await loadApi();
		await expect(getMe()).rejects.toThrow();
		expect(window.location.href).toBe('/login');
	});

	it('handles 204 No Content', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 204,
			json: async () => { throw new Error('no body'); }
		});

		const { revokeKey } = await loadApi();
		const result = await revokeKey('org', 'proj', 'key-id');
		expect(result).toBeUndefined();
	});

	it('getUsage passes range and group_by params', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 200,
			json: async () => ({
				range: '30d', group_by: 'model', from: '', to: '',
				total_cost_usd: 0, total_tokens_in: 0, total_tokens_out: 0, series: []
			})
		});

		const { getUsage } = await loadApi();
		await getUsage('org-1', '30d', 'model');

		const [url] = mockFetch.mock.calls[0];
		expect(url).toContain('range=30d');
		expect(url).toContain('group_by=model');
	});

	it('exportBillingCsv returns blob on success', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		const blob = new Blob(['csv,data'], { type: 'text/csv' });
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 200,
			blob: async () => blob
		});

		const { exportBillingCsv } = await loadApi();
		const result = await exportBillingCsv('org-1', '2026-01-01T00:00:00Z', '2026-02-01T00:00:00Z');
		expect(result).toBe(blob);
	});
});
