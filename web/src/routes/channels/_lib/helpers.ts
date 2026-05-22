// channels/_lib/helpers.ts
// 0.4.2 T1：从 +page.svelte 抽出的纯函数 + provider/status/health 选项常量。
// 这些值与 state 无关，可被 _components/* 子组件共用。

import { CAPABILITY_LABELS, capabilityList, providerCapabilities } from '$lib/plugin-presets';
import type { PluginAuthForm, ProviderCapabilities, ProviderCapabilityKey } from '$lib/plugin-presets';
import type { ProviderOption } from '$lib/components/ui/ProviderSelect.svelte';

export const PROVIDER_OPTIONS: ProviderOption[] = [
	{ value: 'openai', label: 'OpenAI', description: 'GPT-4o / o1 / o3' },
	{ value: 'anthropic', label: 'Anthropic', description: 'Claude 4 / Sonnet / Haiku' },
	{ value: 'azure', label: 'Azure OpenAI', description: 'Azure 托管 GPT 部署' },
	{ value: 'bedrock', label: 'AWS Bedrock', description: 'Claude / Titan / Llama' },
	// 0.3.0 起 gemini / deepseek / mistral / ollama / cohere 走 plugin preset。
	// 老 channel 由 migration 20260522000001 自动迁移；新 channel 通过 plugin builder 接入。
	{
		value: 'plugin',
		label: 'HTTP Plugin',
		description:
			'自定义私有协议 / SSE 整流 / Gemini / DeepSeek / Mistral / Ollama / Cohere / Groq / Together / OpenRouter / Moonshot / 智谱 / 通义 / 零一 等 18+ preset',
	},
];

export const FILTER_PROVIDER_OPTIONS: ProviderOption[] = [
	{ value: '', label: '全部 Provider', description: '不过滤' },
	...PROVIDER_OPTIONS,
];

export const STATUS_OPTIONS = [
	{ value: '', label: '全部状态' },
	{ value: 'active', label: 'Active' },
	{ value: 'draining', label: 'Draining' },
	{ value: 'disabled', label: 'Disabled' },
];

export const HEALTH_OPTIONS = [
	{ value: '', label: '全部健康度' },
	{ value: 'healthy', label: 'Healthy' },
	{ value: 'degraded', label: 'Degraded' },
	{ value: 'unhealthy', label: 'Unhealthy' },
];

export const PLUGIN_PROVIDER_TYPES = ['plugin', 'custom', 'http', 'http_plugin'] as const;

export function isPluginProvider(providerType: string | undefined): boolean {
	return PLUGIN_PROVIDER_TYPES.includes((providerType ?? '') as (typeof PLUGIN_PROVIDER_TYPES)[number]);
}

export function capabilityFallback(
	providerType: string,
	caps: ProviderCapabilities | undefined,
): ProviderCapabilities {
	if (caps) return caps;
	return providerCapabilities(providerType);
}

export function capabilityTitle(caps: ProviderCapabilities | undefined): string {
	const active = capabilityList(caps);
	return active.length > 0
		? active.map((key) => CAPABILITY_LABELS[key]).join(', ')
		: 'No capability declared';
}

export function capabilityChipClass(key: ProviderCapabilityKey): string {
	if (key === 'image' || key === 'audio' || key === 'batch') {
		return 'bg-amber-50 text-amber-700 ring-amber-600/20 dark:bg-amber-500/10 dark:text-amber-400 dark:ring-amber-400/20';
	}
	return 'bg-zinc-100 text-zinc-700 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700';
}

export function pluginAuthSlotSummary(form: PluginAuthForm): string {
	switch (form.strategy) {
		case 'bearer':
		case 'api_key_header':
		case 'api_key_query':
		case 'hmac':
			return form.secret_slot.trim() || 'primary';
		case 'basic':
			return `${form.username_slot.trim() || 'username'} / ${form.password_slot.trim() || 'primary'}`;
		case 'aws_sigv4':
			return [
				form.aws_access_key_slot.trim() || 'primary',
				form.aws_secret_key_slot.trim() || 'aws_secret_key',
				form.aws_session_token_slot.trim(),
			]
				.filter(Boolean)
				.join(' / ');
		case 'oauth_client_credentials':
			return `${form.oauth_client_id_slot.trim() || 'client_id'} / ${form.oauth_client_secret_slot.trim() || 'client_secret'}`;
		case 'custom_headers':
			return '按 Headers JSON 内的 {{slot}} 引用';
		case 'none':
			return '无认证 slot';
		default:
			return 'primary';
	}
}

export function fmtLimit(v: number | null | undefined): string {
	if (v == null) return '—';
	return v.toLocaleString();
}

export function fmtDate(s: string | null | undefined): string {
	if (!s) return '—';
	try {
		return new Date(s).toLocaleDateString('zh-CN', {
			month: '2-digit',
			day: '2-digit',
			hour: '2-digit',
			minute: '2-digit',
		});
	} catch {
		return s;
	}
}

export function healthBadgeCls(health: string): string {
	if (health === 'healthy') return 'bg-green-50 dark:bg-green-500/10 text-green-700 dark:text-green-400 ring-1 ring-green-600/10 dark:ring-green-400/20';
	if (health === 'degraded') return 'bg-amber-50 dark:bg-amber-500/10 text-amber-700 dark:text-amber-400 ring-1 ring-amber-600/10 dark:ring-amber-400/20';
	if (health === 'unhealthy') return 'bg-red-50 dark:bg-red-500/10 text-red-700 dark:text-red-400 ring-1 ring-red-600/10 dark:ring-red-400/20';
	return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 ring-1 ring-zinc-200 dark:ring-zinc-700';
}

export function statusBadgeCls(status: string): string {
	if (status === 'active') return 'bg-green-50 dark:bg-green-500/10 text-green-700 dark:text-green-400 ring-1 ring-green-600/10 dark:ring-green-400/20';
	if (status === 'draining') return 'bg-amber-50 dark:bg-amber-500/10 text-amber-700 dark:text-amber-400 ring-1 ring-amber-600/10 dark:ring-amber-400/20';
	if (status === 'disabled') return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 ring-1 ring-zinc-200 dark:ring-zinc-700';
	return 'bg-red-50 dark:bg-red-500/10 text-red-700 dark:text-red-400 ring-1 ring-red-600/10 dark:ring-red-400/20';
}

export function healthDot(health: string): string {
	if (health === 'healthy') return 'bg-green-500';
	if (health === 'degraded') return 'bg-amber-500';
	if (health === 'unhealthy') return 'bg-red-500';
	return 'bg-zinc-400';
}
