import { describe, expect, it } from 'vitest';
import {
	PLUGIN_PRESET_OPTIONS,
	manifestPreset,
	parsePluginManifest,
	pluginManifestFromPreset,
	selectedPluginMapping
} from '$lib/plugin-presets';

describe('plugin provider presets', () => {
	it('contains mainstream provider presets', () => {
		const values = PLUGIN_PRESET_OPTIONS.map(o => o.value);
		expect(values).toContain('openai_compatible');
		expect(values).toContain('anthropic_messages');
		expect(values).toContain('azure_openai');
		expect(values).toContain('gemini');
		expect(values).toContain('deepseek');
		expect(values).toContain('mistral');
		expect(values).toContain('cohere_chat');
		expect(values).toContain('ollama');
		expect(values).toContain('bedrock_converse');
	});

	it('builds and detects preset manifest', () => {
		const manifest = pluginManifestFromPreset('anthropic_messages');
		expect(manifest).toEqual({
			plugin: {
				version: 1,
				capabilities: { chat: true, streaming: true },
				auth: { strategy: 'bearer', secret_slot: 'primary' },
				preset: { provider: 'anthropic_messages' }
			}
		});
		expect(manifestPreset(manifest)).toBe('anthropic_messages');
	});

	it('uses selected preset over stale manifest input', () => {
		const selected = selectedPluginMapping('azure_openai', '{"plugin":{"preset":{"provider":"openai_compatible"}}}');
		expect(manifestPreset(selected)).toBe('azure_openai');
	});

	it('parses custom manifest when no preset is selected', () => {
		const parsed = selectedPluginMapping('', '{"plugin":{"request":{"chat_path":"/x"}}}');
		expect(parsePluginManifest(JSON.stringify(parsed))).toEqual(parsed);
	});
});
