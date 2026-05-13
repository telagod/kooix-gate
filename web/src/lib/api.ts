// API 基础 URL，走 Vite 环境变量；本地默认 localhost:3000
const BASE_URL = import.meta.env.VITE_API_BASE_URL ?? 'http://localhost:3000';

class ApiError extends Error {
	constructor(
		public status: number,
		public code: string,
		message: string
	) {
		super(message);
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
		const token = getToken();
		if (token) {
			headers.set('Authorization', `Bearer ${token}`);
		}
	}

	const resp = await fetch(`${BASE_URL}${path}`, { ...init, headers });

	if (resp.status === 401) {
		// 清 token，跳登录（避免无限跳转：只在非登录页跳）
		if (typeof window !== 'undefined' && !window.location.pathname.startsWith('/login')) {
			clearToken();
			window.location.href = '/login';
		}
		const body = await resp.json().catch(() => ({}));
		throw new ApiError(401, body?.error?.code ?? 'unauthorized', body?.error?.message ?? 'Unauthorized');
	}

	if (!resp.ok) {
		const body = await resp.json().catch(() => ({}));
		throw new ApiError(resp.status, body?.error?.code ?? 'error', body?.error?.message ?? resp.statusText);
	}

	// 204 No Content
	if (resp.status === 204) return undefined as T;

	return resp.json();
}

// ── token storage ──────────────────────────────────

const TOKEN_KEY = 'kooix_access_token';
const REFRESH_KEY = 'kooix_refresh_token';

function getToken(): string | null {
	if (typeof localStorage === 'undefined') return null;
	return localStorage.getItem(TOKEN_KEY);
}

function clearToken(): void {
	if (typeof localStorage === 'undefined') return;
	localStorage.removeItem(TOKEN_KEY);
	localStorage.removeItem(REFRESH_KEY);
}

// ── API calls ──────────────────────────────────────

export interface LoginResult {
	access_token: string;
	refresh_token: string;
	expires_at: string;
	user: { id: string; email: string; display_name: string | null };
}

export interface MeResult {
	subject: { kind: string; user_id?: string };
	current_org: string | null;
	is_platform_admin: boolean;
	orgs: string[];
}

export interface Project {
	id: string;
	org_id: string;
	name: string;
	slug: string;
	status: string;
}

export async function login(email: string, password: string): Promise<LoginResult> {
	return apiFetch<LoginResult>('/v1/auth/login', {
		method: 'POST',
		body: JSON.stringify({ email, password }),
		skipAuth: true
	});
}

export async function refreshToken(refreshTk: string): Promise<{ access_token: string; expires_at: string }> {
	return apiFetch('/v1/auth/refresh', {
		method: 'POST',
		body: JSON.stringify({ refresh_token: refreshTk }),
		skipAuth: true
	});
}

export async function logout(): Promise<void> {
	await apiFetch('/v1/auth/logout', { method: 'POST' }).catch(() => {});
}

export async function getMe(orgId?: string): Promise<MeResult> {
	const headers: Record<string, string> = {};
	if (orgId) headers['X-Kooix-Org'] = orgId;
	return apiFetch<MeResult>('/v1/me', { headers });
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

// ── Usage ───────────────────────────────────────────

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

// ── Channels (only listing) ─────────────────────────

export interface Channel {
	id: string;
	code: string;
	name: string;
	provider_type: string;
	status: string;
	health: string;
	updated_at: string;
}

export async function listChannels(orgId: string): Promise<Channel[]> {
	return apiFetch<Channel[]>(`/v1/orgs/${orgId}/channels`, {
		headers: { 'X-Kooix-Org': orgId }
	});
}

// ── SSO ─────────────────────────────────────────────

export interface SsoStartResponse {
	authorize_url: string;
	state: string;
}

export async function ssoStart(slug: string, redirectTo?: string): Promise<SsoStartResponse> {
	const params = redirectTo ? `?redirect_to=${encodeURIComponent(redirectTo)}` : '';
	return apiFetch<SsoStartResponse>(`/v1/auth/sso/${slug}/start${params}`, {
		skipAuth: true
	});
}

export { getToken, clearToken, ApiError };
