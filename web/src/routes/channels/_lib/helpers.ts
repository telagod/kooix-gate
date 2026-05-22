// channels/_lib/helpers.ts
// 0.4.2 T1：从 +page.svelte 抽出的纯函数 + provider/status/health 选项常量。
// 这些值与 state 无关，可被 _components/* 子组件共用。

import { CAPABILITY_LABELS, capabilityList, providerCapabilities } from '$lib/plugin-presets';
import type { ProviderCapabilities, ProviderCapabilityKey } from '$lib/plugin-presets';
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
