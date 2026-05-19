import { describe, expect, it } from 'vitest';
import {
	buildPluginAuthManifest,
	defaultPluginAuthForm,
	defaultPluginAuthForPreset,
	PLUGIN_PRESET_OPTIONS,
	PLUGIN_AUTH_STRATEGY_OPTIONS,
	authFormFromManifest,
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

	it('exposes runtime auth strategies for channel forms', () => {
		const values = PLUGIN_AUTH_STRATEGY_OPTIONS.map(o => o.value);
		expect(values).toContain('bearer');
		expect(values).toContain('api_key_header');
		expect(values).toContain('hmac');
		expect(values).toContain('aws_sigv4');
		expect(values).toContain('oauth_client_credentials');
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

	it('applies auth form to preset manifest', () => {
		const auth = { ...defaultPluginAuthForPreset('anthropic_messages'), secret_slot: 'anthropic_key' };
		const manifest = pluginManifestFromPreset('anthropic_messages', auth);
		expect(manifest).toEqual({
			plugin: {
				version: 1,
				capabilities: { chat: true, streaming: true },
				auth: {
					strategy: 'api_key_header',
					header_name: 'x-api-key',
					secret_slot: 'anthropic_key'
				},
				preset: { provider: 'anthropic_messages' }
			}
		});
	});

	it('uses selected preset over stale manifest input', () => {
		const selected = selectedPluginMapping('azure_openai', '{"plugin":{"preset":{"provider":"openai_compatible"}}}');
		expect(manifestPreset(selected)).toBe('azure_openai');
	});

	it('parses custom manifest when no preset is selected', () => {
		const parsed = selectedPluginMapping('', '{"plugin":{"request":{"chat_path":"/x"}}}');
		expect(parsePluginManifest(JSON.stringify(parsed))).toEqual(parsed);
	});

	it('builds oauth client credentials auth and validates token url', () => {
		const form = {
			...defaultPluginAuthForm('oauth_client_credentials'),
			oauth_token_url: 'https://idp.example.com/oauth/token',
			oauth_scope: 'chat:write',
			oauth_audience: 'https://api.example.com'
		};
		expect(buildPluginAuthManifest(form)).toEqual({
			strategy: 'oauth_client_credentials',
			oauth: {
				token_url: 'https://idp.example.com/oauth/token',
				client_id_slot: 'client_id',
				client_secret_slot: 'client_secret',
				scope: 'chat:write',
				audience: 'https://api.example.com',
				expiry_skew_seconds: 60
			}
		});

			expect(() =>
				buildPluginAuthManifest({ ...form, oauth_token_url: 'http://idp.example.com/oauth/token' })
			).toThrow('HTTPS');
			expect(() =>
				buildPluginAuthManifest({ ...form, oauth_token_url: 'http://localhost.evil.example/oauth/token' })
			).toThrow('HTTPS');
		});

	it('round-trips auth form from manifest', () => {
		const form = authFormFromManifest({
			plugin: {
				auth: {
					strategy: 'hmac',
					secret_slot: 'signing',
					hmac: {
						signature_header: 'X-Sig',
						signed_payload: '{{method}}\\n{{path}}',
						signature_encoding: 'base64'
					}
				}
			}
		});
		expect(form.strategy).toBe('hmac');
		expect(form.secret_slot).toBe('signing');
		expect(form.hmac_signature_header).toBe('X-Sig');
		expect(form.hmac_signature_encoding).toBe('base64');
	});
});
