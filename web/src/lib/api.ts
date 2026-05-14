import { getAccessToken, getRefreshToken, saveTokens, clearTokens } from '$lib/auth.js';

const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:8000';

export class ApiError extends Error {
	constructor(
		public status: number,
		public code: string,
		message: string
	) {
		super(message);
	}
}

let refreshPromise: Promise<string | null> | null = null;

async function tryRefresh(): Promise<string | null> {
	const rt = getRefreshToken();
	if (!rt) return null;
	try {
		const resp = await fetch(`${BASE_URL}/v1/auth/refresh`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json' },
			body: JSON.stringify({ refresh_token: rt })
		});
		if (!resp.ok) return null;
		const data = await resp.json();
		saveTokens(data.access_token, rt);
		return data.access_token;
	} catch {
		return null;
	}
}

async function apiFetch<T>(
	path: string,
	options: RequestInit & { skipAuth?: boolean } = {}
): Promise<T> {
	const { skipAuth, ...init } = options;
	const headers = new Headers(init.headers);

	if (!headers.has('Content-Type') && init.body) {
		headers.set('Content-Type', 'application/json');
	}

	if (!skipAuth) {
		const token = getAccessToken();
		if (token) {
			headers.set('Authorization', `Bearer ${token}`);
		}
	}

	let resp = await fetch(`${BASE_URL}${path}`, { ...init, headers });

	if (resp.status === 401 && !skipAuth) {
		if (!refreshPromise) {
			refreshPromise = tryRefresh().finally(() => {
				refreshPromise = null;
			});
		}
		const newToken = await refreshPromise;
		if (newToken) {
			headers.set('Authorization', `Bearer ${newToken}`);
			resp = await fetch(`${BASE_URL}${path}`, { ...init, headers });
		}
	}

	if (resp.status === 401) {
		if (typeof window !== 'undefined' && !window.location.pathname.startsWith('/login')) {
			clearTokens();
			window.location.href = '/login';
		}
		const body = await resp.json().catch(() => ({}));
		throw new ApiError(401, body?.error?.code ?? 'unauthorized', body?.error?.message ?? 'Unauthorized');
	}

	if (!resp.ok) {
		const body = await resp.json().catch(() => ({}));
		throw new ApiError(resp.status, body?.error?.code ?? 'error', body?.error?.message ?? resp.statusText);
	}

	if (resp.status === 204) return undefined as T;
	return resp.json();
}

// ── System Status ─────────────────────────────────

export interface SystemStatus {
	initialized: boolean;
	version: string;
}

export async function getSystemStatus(): Promise<SystemStatus> {
	const resp = await fetch(`${BASE_URL}/health/status`);
	if (!resp.ok) {
		throw new ApiError(resp.status, 'error', 'Failed to check system status');
	}
	return resp.json();
}

export interface SetupRequest {
	email: string;
	password: string;
	org_name?: string;
	org_slug?: string;
	project_name?: string;
	project_slug?: string;
}

export interface SetupResponse {
	user_id: string;
	email: string;
	org_id: string;
	org_name: string;
	project_id: string;
	project_name: string;
}

export async function postSetup(data: SetupRequest): Promise<SetupResponse> {
	return apiFetch<SetupResponse>('/v1/setup', {
		method: 'POST',
		body: JSON.stringify(data),
		skipAuth: true
	});
}

// ── Auth ──────────────────────────────────────────

export interface LoginResult {
	access_token: string;
	refresh_token: string;
	expires_at: string;
	user: { id: string; email: string; display_name: string | null };
}

export interface SsoStartResponse {
	authorize_url: string;
	state: string;
}

export async function login(email: string, password: string): Promise<LoginResult> {
	return apiFetch<LoginResult>('/v1/auth/login', {
		method: 'POST',
		body: JSON.stringify({ email, password }),
		skipAuth: true
	});
}

export async function refreshTokenApi(refreshTk: string): Promise<{ access_token: string; expires_at: string }> {
	return apiFetch('/v1/auth/refresh', {
		method: 'POST',
		body: JSON.stringify({ refresh_token: refreshTk }),
		skipAuth: true
	});
}

export async function logout(): Promise<void> {
	await apiFetch('/v1/auth/logout', { method: 'POST' }).catch(() => {});
}

export async function ssoStart(slug: string, redirectTo?: string): Promise<SsoStartResponse> {
	const params = redirectTo ? `?redirect_to=${encodeURIComponent(redirectTo)}` : '';
	return apiFetch<SsoStartResponse>(`/v1/auth/sso/${slug}/start${params}`, {
		skipAuth: true
	});
}

// ── Me / Orgs ─────────────────────────────────────

export interface MeResult {
	subject: { kind: string; user_id?: string };
	current_org: string | null;
	is_platform_admin: boolean;
	orgs: string[];
}

export async function getMe(orgId?: string): Promise<MeResult> {
	const headers: Record<string, string> = {};
	if (orgId) headers['X-Kooix-Org'] = orgId;
	return apiFetch<MeResult>('/v1/me', { headers });
}

// ── Projects ──────────────────────────────────────

export interface Project {
	id: string;
	org_id: string;
	name: string;
	slug: string;
	status: string;
}

export async function listProjects(orgId: string): Promise<Project[]> {
	return apiFetch<Project[]>(`/v1/orgs/${orgId}/projects`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function createProject(orgId: string, name: string, slug: string): Promise<Project> {
	return apiFetch<Project>(`/v1/orgs/${orgId}/projects`, {
		method: 'POST',
		body: JSON.stringify({ name, slug }),
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── Usage ─────────────────────────────────────────

export interface UsagePoint {
	key: string;
	cost_usd: number;
	tokens_in: number;
	tokens_out: number;
}

export interface UsageResponse {
	range: string;
	group_by: string;
	from: string;
	to: string;
	total_cost_usd: number;
	total_tokens_in: number;
	total_tokens_out: number;
	series: UsagePoint[];
}

export async function getUsage(
	orgId: string | null,
	range: '7d' | '30d' = '7d',
	groupBy: 'day' | 'model' | 'channel' = 'day'
): Promise<UsageResponse> {
	const params = new URLSearchParams({ range, group_by: groupBy });
	const headers: Record<string, string> = {};
	if (orgId) headers['X-Kooix-Org'] = orgId;
	return apiFetch<UsageResponse>(`/v1/usage?${params}`, { headers });
}

// ── Channels (Org-scoped read-only) ───────────────

export async function listChannels(orgId: string): Promise<Channel[]> {
	return apiFetch<Channel[]>(`/v1/orgs/${orgId}/channels`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── Admin Channels ────────────────────────────────

export interface Channel {
	id: string;
	code: string;
	name: string;
	provider_type: string;
	base_url: string;
	status: string;
	health: string;
	supported_models: string[];
	rpm_limit: number | null;
	tpm_limit: number | null;
	timeout_ms: number;
	max_retries: number;
	tags: string[];
	model_mapping: Record<string, string>;
	balance: number | null;
	balance_updated_at: string | null;
	last_error: string | null;
	last_error_at: string | null;
	created_at: string;
	updated_at: string;
}

export interface PaginatedChannels {
	data: Channel[];
	total: number;
	page: number;
	page_size: number;
}

export interface ChannelListParams {
	search?: string;
	provider?: string;
	status?: string;
	health?: string;
	tag?: string;
	page?: number;
	page_size?: number;
	sort_by?: string;
	sort_dir?: string;
}

export interface CreateChannelRequest {
	code: string;
	provider_type: string;
	base_url: string;
	name?: string;
	enabled?: boolean;
	supported_models?: string[];
	rpm_limit?: number | null;
	tpm_limit?: number | null;
	timeout_ms?: number;
	max_retries?: number;
	tags?: string[];
	model_mapping?: Record<string, string>;
}

export interface UpdateChannelRequest {
	name?: string;
	base_url?: string;
	enabled?: boolean;
	supported_models?: string[];
	rpm_limit?: number | null;
	tpm_limit?: number | null;
	timeout_ms?: number;
	max_retries?: number;
	tags?: string[];
	model_mapping?: Record<string, string>;
}

export async function listAdminChannels(params: ChannelListParams = {}): Promise<PaginatedChannels> {
	const qs = new URLSearchParams();
	if (params.search) qs.set('search', params.search);
	if (params.provider) qs.set('provider', params.provider);
	if (params.status) qs.set('status', params.status);
	if (params.health) qs.set('health', params.health);
	if (params.tag) qs.set('tag', params.tag);
	if (params.page) qs.set('page', String(params.page));
	if (params.page_size) qs.set('page_size', String(params.page_size));
	if (params.sort_by) qs.set('sort_by', params.sort_by);
	if (params.sort_dir) qs.set('sort_dir', params.sort_dir);
	const q = qs.toString();
	return apiFetch<PaginatedChannels>(`/v1/admin/channels${q ? '?' + q : ''}`);
}

export async function createChannel(data: CreateChannelRequest): Promise<Channel> {
	return apiFetch<Channel>('/v1/admin/channels', {
		method: 'POST',
		body: JSON.stringify(data)
	});
}

export async function updateChannel(id: string, data: UpdateChannelRequest): Promise<Channel> {
	return apiFetch<Channel>(`/v1/admin/channels/${id}`, {
		method: 'PUT',
		body: JSON.stringify(data)
	});
}

export async function deleteChannel(id: string): Promise<void> {
	return apiFetch(`/v1/admin/channels/${id}`, { method: 'DELETE' });
}

export async function batchEnableChannels(ids: string[]): Promise<{ affected: number }> {
	return apiFetch('/v1/admin/channels/batch-enable', {
		method: 'POST',
		body: JSON.stringify({ ids })
	});
}

export async function batchDisableChannels(ids: string[]): Promise<{ affected: number }> {
	return apiFetch('/v1/admin/channels/batch-disable', {
		method: 'POST',
		body: JSON.stringify({ ids })
	});
}

export async function batchDeleteChannels(ids: string[]): Promise<{ affected: number }> {
	return apiFetch('/v1/admin/channels/batch-delete', {
		method: 'POST',
		body: JSON.stringify({ ids })
	});
}

// Channel probe (model discovery)
export interface ProbeResponse {
	channel_id: string;
	provider_type: string;
	models: string[];
}

export async function probeChannel(channelId: string): Promise<ProbeResponse> {
	return apiFetch<ProbeResponse>(`/v1/admin/channels/${channelId}/probe`, { method: 'POST' });
}

// Channel test
export interface TestResponse {
	success: boolean;
	model: string;
	response_time_ms: number;
	message: string | null;
	error: string | null;
}

export async function testChannel(channelId: string, model?: string): Promise<TestResponse> {
	const params = model ? `?model=${encodeURIComponent(model)}` : '';
	return apiFetch<TestResponse>(`/v1/admin/channels/${channelId}/test${params}`);
}

// Channel balance
export interface BalanceResponse {
	channel_id: string;
	provider_type: string;
	supported: boolean;
	balance_usd: number | null;
	used_usd: number | null;
	message: string | null;
}

export async function getChannelBalance(channelId: string): Promise<BalanceResponse> {
	return apiFetch<BalanceResponse>(`/v1/admin/channels/${channelId}/balance`);
}

// ── Admin Channel Keys ────────────────────────────

export interface ChannelKeySummary {
	id: string;
	channel_id: string;
	label: string | null;
	fingerprint: string;
	weight: number;
	health: string;
	total_requests: number;
	total_errors: number;
	consecutive_errors: number;
	last_error_code: number | null;
	last_error_at: string | null;
	cooldown_until: string | null;
	created_at: string;
}

export async function listChannelKeys(channelId: string): Promise<ChannelKeySummary[]> {
	return apiFetch<ChannelKeySummary[]>(`/v1/admin/channels/${channelId}/keys`);
}

export async function createChannelKey(
	channelId: string,
	secret: string,
	alias?: string
): Promise<ChannelKeySummary> {
	return apiFetch<ChannelKeySummary>(`/v1/admin/channels/${channelId}/keys`, {
		method: 'POST',
		body: JSON.stringify({ secret, alias })
	});
}

export async function rotateChannelKey(
	channelId: string,
	secret: string,
	alias?: string
): Promise<ChannelKeySummary> {
	return apiFetch<ChannelKeySummary>(`/v1/admin/channels/${channelId}/keys/rotate`, {
		method: 'POST',
		body: JSON.stringify({ secret, alias })
	});
}

export async function revokeChannelKey(channelId: string, keyId: string): Promise<void> {
	return apiFetch(`/v1/admin/channels/${channelId}/keys/${keyId}`, { method: 'DELETE' });
}

// ── Admin Audit Logs ──────────────────────────────

export interface AuditLog {
	id: string;
	ts: string;
	actor_kind: string;
	actor_id: string | null;
	action: string;
	resource_kind: string;
	resource_id: string | null;
	org_id: string | null;
	outcome: string;
	after: Record<string, unknown> | null;
}

export async function listAuditLogs(
	orgId: string,
	limit = 50,
	offset = 0
): Promise<AuditLog[]> {
	const params = new URLSearchParams({
		org_id: orgId,
		limit: String(limit),
		offset: String(offset)
	});
	return apiFetch<AuditLog[]>(`/v1/admin/audit-logs?${params}`);
}

// ── API Keys ──────────────────────────────────────

export interface ApiKey {
	id: string;
	name: string;
	prefix: string;
	last4: string;
	allowed_models: string[];
	created_at: string | null;
	last_used_at: string | null;
	revoked: boolean;
}

export interface CreateKeyResponse {
	id: string;
	name: string;
	plaintext: string;
	prefix: string;
	last4: string;
}

export async function listKeys(orgId: string, projectId: string): Promise<ApiKey[]> {
	return apiFetch<ApiKey[]>(`/v1/orgs/${orgId}/projects/${projectId}/api-keys`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function createKey(orgId: string, projectId: string, name: string): Promise<CreateKeyResponse> {
	return apiFetch<CreateKeyResponse>(`/v1/orgs/${orgId}/projects/${projectId}/api-keys`, {
		method: 'POST',
		body: JSON.stringify({ name }),
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function revokeKey(orgId: string, projectId: string, keyId: string): Promise<void> {
	return apiFetch(`/v1/orgs/${orgId}/projects/${projectId}/api-keys/${keyId}`, {
		method: 'DELETE',
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── Quotas ────────────────────────────────────────

export interface Quota {
	id: string;
	scope_kind: string;
	scope_id: string;
	dimension: string;
	model_filter: string | null;
	limit_value: string;
	window_seconds: number | null;
	enabled: boolean;
}

export interface UpsertQuotaRequest {
	scope_kind: string;
	scope_id: string;
	dimension: string;
	model_filter?: string | null;
	limit_value: number;
	window_seconds?: number | null;
}

export async function listQuotas(orgId: string): Promise<Quota[]> {
	return apiFetch<Quota[]>(`/v1/orgs/${orgId}/quotas`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function upsertQuota(orgId: string, data: UpsertQuotaRequest): Promise<Quota> {
	return apiFetch<Quota>(`/v1/orgs/${orgId}/quotas`, {
		method: 'POST',
		body: JSON.stringify(data),
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function deleteQuota(orgId: string, quotaId: string): Promise<void> {
	return apiFetch(`/v1/orgs/${orgId}/quotas/${quotaId}`, {
		method: 'DELETE',
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── Billing ───────────────────────────────────────

export interface MonthlyBill {
	org_id: string;
	month: string;
	total_cost_usd: string;
	total_tokens_in: number;
	total_tokens_out: number;
	total_requests: number;
	breakdown_by_project: { project_id: string; cost_usd: string; requests: number }[];
	breakdown_by_model: {
		model: string;
		cost_usd: string;
		tokens_in: number;
		tokens_out: number;
		requests: number;
	}[];
}

export interface QuotaAlert {
	quota_id: string;
	dimension: string;
	scope_kind: string;
	scope_id: string;
	limit_value: number;
	current_value: number;
	percent: number;
	level: 'approaching' | 'exceeded';
}

export async function getMonthlyBill(orgId: string, month: string): Promise<MonthlyBill> {
	return apiFetch<MonthlyBill>(`/v1/orgs/${orgId}/billing/${month}`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function exportBillingCsv(orgId: string, from: string, to: string): Promise<Blob> {
	const token = getAccessToken();
	const params = new URLSearchParams({ from, to });
	const resp = await fetch(`${BASE_URL}/v1/orgs/${orgId}/billing/export?${params}`, {
		headers: {
			'X-Kooix-Org': orgId,
			...(token ? { Authorization: `Bearer ${token}` } : {})
		}
	});
	if (!resp.ok) {
		const body = await resp.json().catch(() => ({}));
		throw new ApiError(resp.status, body?.error?.code ?? 'error', body?.error?.message ?? 'Export failed');
	}
	return resp.blob();
}

export async function getQuotaAlerts(orgId: string): Promise<QuotaAlert[]> {
	return apiFetch<QuotaAlert[]>(`/v1/orgs/${orgId}/quota-alerts`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── Admin Orgs ────────────────────────────────────

export interface OrgDetail {
	id: string;
	name: string;
	slug: string;
	owner_user_id: string;
	status: string;
	billing_email: string | null;
	created_at: string;
	updated_at: string;
}

export async function listAllOrgs(): Promise<OrgDetail[]> {
	return apiFetch<OrgDetail[]>('/v1/admin/orgs');
}

export async function createOrg(name: string, slug: string): Promise<OrgDetail> {
	return apiFetch<OrgDetail>('/v1/admin/orgs', {
		method: 'POST',
		body: JSON.stringify({ name, slug })
	});
}

export async function updateOrg(id: string, data: { name?: string; billing_email?: string }): Promise<OrgDetail> {
	return apiFetch<OrgDetail>(`/v1/admin/orgs/${id}`, {
		method: 'PUT',
		body: JSON.stringify(data)
	});
}

// ── Admin Users ───────────────────────────────────

export interface UserDetail {
	id: string;
	email: string;
	display_name: string | null;
	status: string;
	mfa_enabled: boolean;
	last_login_at: string | null;
	created_at: string;
}

export async function listUsers(limit = 50, offset = 0): Promise<UserDetail[]> {
	const params = new URLSearchParams({ limit: String(limit), offset: String(offset) });
	return apiFetch<UserDetail[]>(`/v1/admin/users?${params}`);
}

export async function updateUserStatus(id: string, status: string): Promise<UserDetail> {
	return apiFetch<UserDetail>(`/v1/admin/users/${id}/status`, {
		method: 'PUT',
		body: JSON.stringify({ status })
	});
}

// ── Project Detail ────────────────────────────────

export async function getProject(orgId: string, projectId: string): Promise<Project> {
	return apiFetch<Project>(`/v1/orgs/${orgId}/projects/${projectId}`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function updateProject(orgId: string, projectId: string, data: { name?: string; status?: string }): Promise<Project> {
	return apiFetch<Project>(`/v1/orgs/${orgId}/projects/${projectId}`, {
		method: 'PUT',
		body: JSON.stringify(data),
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── Settings ──────────────────────────────────────

export async function changePassword(currentPassword: string, newPassword: string): Promise<{ ok: boolean }> {
	return apiFetch<{ ok: boolean }>('/v1/me/password', {
		method: 'PUT',
		body: JSON.stringify({ current_password: currentPassword, new_password: newPassword })
	});
}

// ── Admin Channel Groups ──────────────────────────

export interface ChannelGroup {
	id: string;
	name: string;
	strategy: string;
	enabled: boolean;
	description?: string;
	fallback_group_id?: string | null;
	channel_count?: number;
	updated_at?: string;
	created_at: string;
}

export interface GroupBinding {
	channel_id: string;
	channel_code: string;
	channel_name: string;
	provider_type: string;
	priority: number;
	weight: number;
	model_filter?: string[];
	enabled?: boolean;
	channel_status?: string;
	channel_health?: string;
}

export interface GroupDetail {
	group: ChannelGroup;
	bindings: GroupBinding[];
	project_ids: string[];
}

export async function listGroups(): Promise<ChannelGroup[]> {
	return apiFetch<ChannelGroup[]>('/v1/admin/groups');
}

export async function createGroup(name: string, strategy: string, description?: string, fallback_group_id?: string | null): Promise<ChannelGroup> {
	return apiFetch<ChannelGroup>('/v1/admin/groups', {
		method: 'POST',
		body: JSON.stringify({ name, strategy, description, fallback_group_id })
	});
}

export async function updateGroup(id: string, data: {
	name?: string;
	strategy?: string;
	enabled?: boolean;
	description?: string;
	fallback_group_id?: string | null;
}): Promise<ChannelGroup> {
	return apiFetch<ChannelGroup>(`/v1/admin/groups/${id}`, {
		method: 'PUT',
		body: JSON.stringify(data)
	});
}

export async function deleteGroup(id: string): Promise<void> {
	return apiFetch(`/v1/admin/groups/${id}`, { method: 'DELETE' });
}

export async function getGroupDetail(groupId: string): Promise<GroupDetail> {
	return apiFetch<GroupDetail>(`/v1/admin/groups/${groupId}/detail`);
}

export async function listGroupBindings(groupId: string): Promise<GroupBinding[]> {
	return apiFetch<GroupBinding[]>(`/v1/admin/groups/${groupId}/bindings`);
}

export async function addGroupBinding(groupId: string, channelId: string, priority?: number, weight?: number): Promise<void> {
	return apiFetch(`/v1/admin/groups/${groupId}/bindings`, {
		method: 'POST',
		body: JSON.stringify({ channel_id: channelId, priority, weight })
	});
}

export async function updateGroupBinding(groupId: string, channelId: string, data: {
	priority?: number;
	weight?: number;
	model_filter?: string[];
	enabled?: boolean;
}): Promise<void> {
	return apiFetch(`/v1/admin/groups/${groupId}/bindings/${channelId}`, {
		method: 'PUT',
		body: JSON.stringify(data)
	});
}

export async function removeGroupBinding(groupId: string, channelId: string): Promise<void> {
	return apiFetch(`/v1/admin/groups/${groupId}/bindings/${channelId}`, { method: 'DELETE' });
}

export async function setProjectDefaultGroup(projectId: string, groupId: string): Promise<void> {
	return apiFetch(`/v1/admin/projects/${projectId}/default-group`, {
		method: 'PUT',
		body: JSON.stringify({ group_id: groupId })
	});
}

// ── Model Aliases ─────────────────────────────────

export interface ModelAlias {
	id: string;
	alias: string;
	target_model: string;
	enabled: boolean;
	created_at: string;
}

export async function listModelAliases(orgId: string, projectId: string): Promise<ModelAlias[]> {
	return apiFetch<ModelAlias[]>(`/v1/orgs/${orgId}/projects/${projectId}/model-aliases`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function upsertModelAlias(orgId: string, projectId: string, alias: string, targetModel: string): Promise<void> {
	return apiFetch(`/v1/orgs/${orgId}/projects/${projectId}/model-aliases`, {
		method: 'POST',
		body: JSON.stringify({ alias, target_model: targetModel }),
		headers: { 'X-Kooix-Org': orgId }
	});
}

export async function deleteModelAlias(orgId: string, projectId: string, alias: string): Promise<void> {
	return apiFetch(`/v1/orgs/${orgId}/projects/${projectId}/model-aliases/${encodeURIComponent(alias)}`, {
		method: 'DELETE',
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── Chat Playground ───────────────────────────────

export interface ModelInfo {
	id: string;
	object: string;
	created: number;
	owned_by: string;
}

export async function listModels(): Promise<ModelInfo[]> {
	const resp = await apiFetch<{ data: ModelInfo[] }>('/v1/models');
	return resp.data;
}

export function chatCompletionStream(
	orgId: string,
	model: string,
	messages: { role: string; content: string }[],
	onChunk: (text: string) => void,
	onDone: () => void,
	onError: (err: string) => void
): AbortController {
	const ctrl = new AbortController();
	const token = getAccessToken();
	fetch(`${BASE_URL}/v1/chat/completions`, {
		method: 'POST',
		headers: {
			'Content-Type': 'application/json',
			...(token ? { Authorization: `Bearer ${token}` } : {}),
			...(orgId ? { 'X-Kooix-Org': orgId } : {})
		},
		body: JSON.stringify({ model, messages, stream: true }),
		signal: ctrl.signal
	})
		.then(async (resp) => {
			if (!resp.ok) {
				const body = await resp.json().catch(() => ({}));
				onError(body?.error?.message ?? resp.statusText);
				return;
			}
			const reader = resp.body?.getReader();
			if (!reader) { onError('no reader'); return; }
			const decoder = new TextDecoder();
			let buf = '';
			while (true) {
				const { done, value } = await reader.read();
				if (done) break;
				buf += decoder.decode(value, { stream: true });
				const lines = buf.split('\n');
				buf = lines.pop() ?? '';
				for (const line of lines) {
					if (!line.startsWith('data: ')) continue;
					const payload = line.slice(6).trim();
					if (payload === '[DONE]') { onDone(); return; }
					try {
						const json = JSON.parse(payload);
						const delta = json.choices?.[0]?.delta?.content;
						if (delta) onChunk(delta);
					} catch (e) {
						console.warn('SSE parse error:', e);
						onError(`SSE parse error: ${e}`);
					}
				}
			}
			onDone();
		})
		.catch((err) => {
			if (err.name !== 'AbortError') onError(String(err));
		});
	return ctrl;
}

// ── Request Logs (Admin) ─────────────────────────

export interface RequestRecord {
	ts: string;
	request_id: string;
	org_id: string;
	project_id: string;
	api_key_id: string;
	user_id: string | null;
	channel_id: string;
	channel_key_id: string | null;
	group_id: string | null;
	model_requested: string;
	model_actual: string;
	stream: boolean;
	tokens_in: number;
	tokens_out: number;
	tokens_cached: number;
	cost_usd: number;
	latency_ms: number | null;
	ttfb_ms: number | null;
	status: number;
	error_code: string | null;
	retries: number;
	client_ip: string | null;
	metadata: Record<string, unknown> | null;
}

export interface RequestPage {
	data: RequestRecord[];
	next_cursor: string | null;
	has_more: boolean;
}

export interface RequestListParams {
	org_id?: string;
	project_id?: string;
	channel_id?: string;
	api_key_id?: string;
	model?: string;
	status_min?: number;
	status_max?: number;
	error_only?: boolean;
	from?: string;
	to?: string;
	search?: string;
	cursor?: string;
	limit?: number;
}

export async function listRequests(params: RequestListParams = {}): Promise<RequestPage> {
	const qs = new URLSearchParams();
	if (params.org_id) qs.set('org_id', params.org_id);
	if (params.project_id) qs.set('project_id', params.project_id);
	if (params.channel_id) qs.set('channel_id', params.channel_id);
	if (params.api_key_id) qs.set('api_key_id', params.api_key_id);
	if (params.model) qs.set('model', params.model);
	if (params.status_min != null) qs.set('status_min', String(params.status_min));
	if (params.status_max != null) qs.set('status_max', String(params.status_max));
	if (params.error_only) qs.set('error_only', 'true');
	if (params.from) qs.set('from', params.from);
	if (params.to) qs.set('to', params.to);
	if (params.search) qs.set('search', params.search);
	if (params.cursor) qs.set('cursor', params.cursor);
	if (params.limit) qs.set('limit', String(params.limit));
	const q = qs.toString();
	return apiFetch<RequestPage>(`/v1/admin/requests${q ? '?' + q : ''}`);
}

export async function getRequest(requestId: string): Promise<RequestRecord> {
	return apiFetch<RequestRecord>(`/v1/admin/requests/${requestId}`);
}

// ── Dashboard Stats (Admin) ──────────────────────

export interface ModelRank {
	model: string;
	requests: number;
	cost_usd: number;
}

export interface HourlyBucket {
	hour: string;
	requests: number;
	errors: number;
	cost_usd: number;
}

export interface DashboardStatsResponse {
	total_requests: number;
	total_errors: number;
	error_rate: number;
	p50_latency_ms: number | null;
	p95_latency_ms: number | null;
	total_cost_usd: number;
	total_tokens: number;
	top_models: ModelRank[];
	hourly_trend: HourlyBucket[];
	recent_errors: RequestRecord[];
}

export async function getDashboardStats(orgId?: string, hours = 24): Promise<DashboardStatsResponse> {
	const qs = new URLSearchParams({ hours: String(hours) });
	if (orgId) qs.set('org_id', orgId);
	return apiFetch<DashboardStatsResponse>(`/v1/admin/dashboard-stats?${qs}`);
}
