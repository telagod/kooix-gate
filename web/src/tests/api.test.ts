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
			json: async () => ({ access_token: 'new-token', refresh_token: 'rt-rotated', expires_at: '2026-12-01T00:00:00Z' })
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
		// Tokens should be rotated in localStorage
		expect(localStorage.getItem('kooix_access_token')).toBe('new-token');
		expect(localStorage.getItem('kooix_refresh_token')).toBe('rt-rotated');
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

	it('listAuditLogs passes pagination and sort params', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		mockFetch.mockResolvedValueOnce({
			ok: true,
			status: 200,
			json: async () => []
		});

		const { listAuditLogs } = await loadApi();
		await listAuditLogs('019e2c1b-a7d1-7162-8422-07e4b24f5f98', {
			limit: 100,
			offset: 200,
			sort_by: 'action',
			sort_dir: 'asc'
		});

		const [url] = mockFetch.mock.calls[0];
		expect(url).toContain('/v1/admin/audit-logs');
		expect(url).toContain('org_id=019e2c1b-a7d1-7162-8422-07e4b24f5f98');
		expect(url).toContain('limit=100');
		expect(url).toContain('offset=200');
		expect(url).toContain('sort_by=action');
		expect(url).toContain('sort_dir=asc');
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

	it('admin user APIs create, update status, and reset password', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		mockFetch
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ id: 'usr_019e2c1ba7d17162842207e4b24f5f98', email: 'new@example.com', display_name: null, status: 'active', mfa_enabled: false, last_login_at: null, created_at: '2026-01-01T00:00:00Z' })
			})
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ id: 'usr_019e2c1ba7d17162842207e4b24f5f98', email: 'new@example.com', display_name: null, status: 'suspended', mfa_enabled: false, last_login_at: null, created_at: '2026-01-01T00:00:00Z' })
			})
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ id: 'usr_019e2c1ba7d17162842207e4b24f5f98', email: 'new@example.com', display_name: null, status: 'suspended', mfa_enabled: false, last_login_at: null, created_at: '2026-01-01T00:00:00Z' })
			});

		const { createUser, updateUserStatus, resetUserPassword } = await loadApi();
		await createUser({ email: 'new@example.com', password: 'strong-password-123', status: 'active' });
		await updateUserStatus('usr_019e2c1ba7d17162842207e4b24f5f98', 'suspended');
		await resetUserPassword('usr_019e2c1ba7d17162842207e4b24f5f98', 'new-password-456');

		expect(mockFetch).toHaveBeenCalledTimes(3);
		let [url, opts] = mockFetch.mock.calls[0];
		expect(url).toContain('/v1/admin/users');
		expect(opts.method).toBe('POST');
		expect(JSON.parse(opts.body)).toEqual({ email: 'new@example.com', password: 'strong-password-123', status: 'active' });

		[url, opts] = mockFetch.mock.calls[1];
		expect(url).toContain('/v1/admin/users/019e2c1b-a7d1-7162-8422-07e4b24f5f98/status');
		expect(opts.method).toBe('PUT');
		expect(JSON.parse(opts.body)).toEqual({ status: 'suspended' });

		[url, opts] = mockFetch.mock.calls[2];
		expect(url).toContain('/v1/admin/users/019e2c1b-a7d1-7162-8422-07e4b24f5f98/password');
		expect(opts.method).toBe('PUT');
		expect(JSON.parse(opts.body)).toEqual({ password: 'new-password-456' });
	});

	it('admin user session APIs use raw typed id paths', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		const userId = 'usr_019e2c1ba7d17162842207e4b24f5f98';
		const sessionId = '019e2c1b-a7d1-7162-8422-07e4b24f5f99';
		mockFetch
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ([
					{
						id: sessionId,
						user_id: userId,
						user_agent: 'Vitest',
						ip: '127.0.0.1',
						created_at: '2026-01-01T00:00:00Z',
						last_used_at: '2026-01-01T00:00:00Z',
						expires_at: '2026-01-02T00:00:00Z',
						current: false
					}
				])
			})
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ revoked: 1 })
			})
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ revoked: 2 })
			});

		const { listUserSessions, revokeUserSession, revokeUserSessions } = await loadApi();
		await listUserSessions(userId);
		await revokeUserSession(userId, sessionId);
		await revokeUserSessions(userId);

		expect(mockFetch).toHaveBeenCalledTimes(3);
		expect(mockFetch.mock.calls[0][0]).toContain('/v1/admin/users/019e2c1b-a7d1-7162-8422-07e4b24f5f98/sessions');
		expect(mockFetch.mock.calls[1][0]).toContain('/v1/admin/users/019e2c1b-a7d1-7162-8422-07e4b24f5f98/sessions/019e2c1b-a7d1-7162-8422-07e4b24f5f99');
		expect(mockFetch.mock.calls[1][1].method).toBe('DELETE');
		expect(mockFetch.mock.calls[2][0]).toContain('/v1/admin/users/019e2c1b-a7d1-7162-8422-07e4b24f5f98/sessions');
		expect(mockFetch.mock.calls[2][1].method).toBe('DELETE');
	});

	it('invitation APIs use raw typed id paths and public accept endpoints', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		const orgId = 'org_019e2c1ba7d17162842207e4b24f5f98';
		const projectId = 'proj_019e2c1ba7d17162842207e4b24f5f99';
		const invitationId = '019e2c1b-a7d1-7162-8422-07e4b24f5f97';
		const invitation = {
			id: invitationId,
			scope_kind: 'project',
			scope_id: projectId,
			email: 'dev@example.com',
			role: 'developer',
			invited_by: 'usr_019e2c1ba7d17162842207e4b24f5f96',
			expires_at: '2026-01-02T00:00:00Z',
			accepted_at: null,
			accepted_by: null,
			revoked_at: null,
			created_at: '2026-01-01T00:00:00Z',
			status: 'pending'
		};
		mockFetch
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ ...invitation, token: 'kg_inv_token', accept_url: null }) })
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => [invitation] })
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ ...invitation, status: 'revoked', revoked_at: '2026-01-01T01:00:00Z' }) })
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ ...invitation, id: invitationId }) })
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ user_id: 'usr_1', email: 'dev@example.com', scope_kind: 'project', scope_id: projectId, role: 'developer', accepted_at: '2026-01-01T00:00:00Z' }) });

		const {
			createProjectInvitation,
			listProjectInvitations,
			revokeProjectInvitation,
			previewInvitation,
			acceptInvitation
		} = await loadApi();
		await createProjectInvitation(orgId, projectId, { email: 'dev@example.com', role: 'developer', ttl_hours: 24 });
		await listProjectInvitations(orgId, projectId, true);
		await revokeProjectInvitation(orgId, projectId, invitationId);
		await previewInvitation('kg_inv_token');
		await acceptInvitation({ token: 'kg_inv_token', email: 'dev@example.com', password: 'strong-password-123' });

		expect(mockFetch).toHaveBeenCalledTimes(5);
		expect(mockFetch.mock.calls[0][0]).toContain('/v1/admin/orgs/019e2c1b-a7d1-7162-8422-07e4b24f5f98/projects/019e2c1b-a7d1-7162-8422-07e4b24f5f99/invitations');
		expect(mockFetch.mock.calls[0][1].method).toBe('POST');
		expect(JSON.parse(mockFetch.mock.calls[0][1].body)).toEqual({ email: 'dev@example.com', role: 'developer', ttl_hours: 24 });
		expect(mockFetch.mock.calls[1][0]).toContain('include_inactive=true');
		expect(mockFetch.mock.calls[2][0]).toContain(`/invitations/${invitationId}`);
		expect(mockFetch.mock.calls[2][1].method).toBe('DELETE');
		expect(mockFetch.mock.calls[3][0]).toContain('/v1/invitations/preview');
		expect(mockFetch.mock.calls[3][1].headers.has('Authorization')).toBe(false);
		expect(mockFetch.mock.calls[4][0]).toContain('/v1/invitations/accept');
		expect(mockFetch.mock.calls[4][1].headers.has('Authorization')).toBe(false);
	});

	it('admin channel drain APIs use raw typed id paths', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		const channel = {
			id: 'ch_019e2c1ba7d17162842207e4b24f5f98',
			code: 'openai-prod',
			name: 'OpenAI Prod',
			provider_type: 'openai',
			base_url: 'https://api.openai.com/v1',
			status: 'draining',
			health: 'healthy',
			supported_models: [],
			rpm_limit: null,
			tpm_limit: null,
			timeout_ms: 60000,
			max_retries: 2,
			tags: [],
			capabilities: {
				chat: true,
				streaming: true,
				tools: true,
				embeddings: true,
				image: false,
				audio: false,
				vision: false,
				json_mode: true,
				batch: false
			},
			model_mapping: {},
			balance: null,
			balance_updated_at: null,
			last_error: null,
			last_error_at: null,
			created_at: '2026-01-01T00:00:00Z',
			updated_at: '2026-01-01T00:00:00Z'
		};
		mockFetch
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ channel, inflight: 2, safe_to_disable: false })
			})
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ channel, inflight: 0, safe_to_disable: true })
			})
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({ channel: { ...channel, status: 'disabled' }, inflight: 0, safe_to_disable: true })
			});

		const { drainChannel, getChannelDrainStatus, disableChannelWhenIdle } = await loadApi();
		await drainChannel(channel.id);
		await getChannelDrainStatus(channel.id);
		await disableChannelWhenIdle(channel.id);

		expect(mockFetch).toHaveBeenCalledTimes(3);
		expect(mockFetch.mock.calls[0][0]).toContain('/v1/admin/channels/019e2c1b-a7d1-7162-8422-07e4b24f5f98/drain');
		expect(mockFetch.mock.calls[0][1].method).toBe('POST');
		expect(mockFetch.mock.calls[1][0]).toContain('/v1/admin/channels/019e2c1b-a7d1-7162-8422-07e4b24f5f98/drain-status');
		expect(mockFetch.mock.calls[2][0]).toContain('/v1/admin/channels/019e2c1b-a7d1-7162-8422-07e4b24f5f98/disable-when-idle');
		expect(mockFetch.mock.calls[2][1].method).toBe('POST');
	});

	it('channel create wizard helpers can write key, bind group, and probe with raw typed ids', async () => {
		localStorage.setItem('kooix_access_token', 'tok');
		const channel = {
			id: 'ch_019e2c1ba7d17162842207e4b24f5f98',
			code: 'plugin-private',
			name: 'Plugin Private',
			provider_type: 'plugin',
			base_url: 'https://upstream.example.com',
			status: 'active',
			health: 'unknown',
			supported_models: [],
			rpm_limit: null,
			tpm_limit: null,
			timeout_ms: 60000,
			max_retries: 2,
			tags: [],
			capabilities: {
				chat: true,
				streaming: true,
				tools: false,
				embeddings: false,
				image: false,
				audio: false,
				vision: false,
				json_mode: false,
				batch: false
			},
			model_mapping: { plugin: { version: 1 } },
			balance: null,
			balance_updated_at: null,
			last_error: null,
			last_error_at: null,
			created_at: '2026-01-01T00:00:00Z',
			updated_at: '2026-01-01T00:00:00Z'
		};
		mockFetch
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => channel })
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({
					id: 'chk_019e2c1ba7d17162842207e4b24f5f97',
					channel_id: channel.id,
					label: 'primary',
					fingerprint: 'sha256:abc',
					weight: 1,
					health: 'healthy',
					total_requests: 0,
					total_errors: 0,
					consecutive_errors: 0,
					last_error_code: null,
					last_error_at: null,
					cooldown_until: null,
					created_at: '2026-01-01T00:00:00Z'
				})
			})
			.mockResolvedValueOnce({ ok: true, status: 200, json: async () => ({ ok: true }) })
			.mockResolvedValueOnce({
				ok: true,
				status: 200,
				json: async () => ({
					channel_id: channel.id,
					provider_type: 'plugin',
					models: ['private-model'],
					probe_model: 'private-model',
					max_cost_micros: 100
				})
			});

		const { createChannel, createChannelKey, addGroupBinding, probeChannel } = await loadApi();
		const created = await createChannel({
			code: 'plugin-private',
			provider_type: 'plugin',
			base_url: 'https://upstream.example.com',
			model_mapping: { plugin: { version: 1 } }
		});
		await createChannelKey(created.id, 'secret-value', 'primary');
		await addGroupBinding('grp_019e2c1ba7d17162842207e4b24f5f99', created.id, 1, 1);
		await probeChannel(created.id);

		expect(mockFetch).toHaveBeenCalledTimes(4);
		expect(mockFetch.mock.calls[0][0]).toContain('/v1/admin/channels');
		expect(mockFetch.mock.calls[0][1].method).toBe('POST');
		expect(mockFetch.mock.calls[1][0]).toContain('/v1/admin/channels/019e2c1b-a7d1-7162-8422-07e4b24f5f98/keys');
		expect(JSON.parse(mockFetch.mock.calls[1][1].body)).toEqual({ secret: 'secret-value', alias: 'primary' });
		expect(mockFetch.mock.calls[2][0]).toContain('/v1/admin/groups/019e2c1b-a7d1-7162-8422-07e4b24f5f99/bindings');
		expect(JSON.parse(mockFetch.mock.calls[2][1].body)).toEqual({
			channel_id: '019e2c1b-a7d1-7162-8422-07e4b24f5f98',
			priority: 1,
			weight: 1,
			canary_percent_bps: undefined
		});
		expect(mockFetch.mock.calls[3][0]).toContain('/v1/admin/channels/019e2c1b-a7d1-7162-8422-07e4b24f5f98/probe');
		expect(mockFetch.mock.calls[3][1].method).toBe('POST');
	});
});
