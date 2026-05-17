export interface PluginPresetOption {
	value: string;
	label: string;
}

export const PLUGIN_PRESET_OPTIONS: PluginPresetOption[] = [
	{ value: '', label: '自定义 manifest' },
	{ value: 'openai_compatible', label: 'OpenAI-compatible' },
	{ value: 'anthropic_messages', label: 'Anthropic Messages' },
	{ value: 'azure_openai', label: 'Azure OpenAI' },
	{ value: 'gemini', label: 'Google Gemini' },
	{ value: 'deepseek', label: 'DeepSeek' },
	{ value: 'mistral', label: 'Mistral' },
	{ value: 'cohere_chat', label: 'Cohere Chat' },
	{ value: 'ollama', label: 'Ollama' },
	{ value: 'groq', label: 'Groq' },
	{ value: 'together', label: 'Together AI' },
	{ value: 'openrouter', label: 'OpenRouter' },
	{ value: 'moonshot', label: 'Moonshot' },
	{ value: 'zhipu', label: '智谱 GLM' },
	{ value: 'qwen', label: '通义千问' },
	{ value: 'yi', label: '零一万物' },
	{ value: 'bedrock_converse', label: 'AWS Bedrock Converse' }
];

export function pluginManifestFromPreset(provider: string): Record<string, unknown> {
	return provider ? { plugin: { preset: { provider } } } : {};
}

export function parsePluginManifest(input: string): Record<string, unknown> {
	if (!input.trim()) return {};
	const parsed = JSON.parse(input);
	if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
		throw new Error('Plugin manifest 必须是 JSON object');
	}
	return parsed as Record<string, unknown>;
}

export function manifestPreset(mapping: Record<string, unknown> | undefined): string {
	const plugin = mapping?.plugin;
	if (!plugin || typeof plugin !== 'object' || Array.isArray(plugin)) return '';
	const preset = (plugin as Record<string, unknown>).preset;
	if (!preset || typeof preset !== 'object' || Array.isArray(preset)) return '';
	const provider = (preset as Record<string, unknown>).provider;
	return typeof provider === 'string' ? provider : '';
}

export function selectedPluginMapping(preset: string, input: string): Record<string, unknown> {
	if (!preset) return parsePluginManifest(input);
	if (input.trim()) {
		const parsed = parsePluginManifest(input);
		if (manifestPreset(parsed) === preset) return parsed;
	}
	return pluginManifestFromPreset(preset);
}
