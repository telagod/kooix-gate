import { describe, expect, it } from 'vitest';
import {
	buildPluginAuthManifest,
	capabilityList,
	defaultPluginAuthForm,
	defaultPluginAuthForPreset,
	PLUGIN_PRESET_OPTIONS,
	PLUGIN_AUTH_STRATEGY_OPTIONS,
	authFormFromManifest,
	buildPluginBuilderManifest,
	defaultPluginBuilderDraft,
	manifestPreset,
	pluginCapabilitiesForPreset,
	pluginPresetBaseUrlSuggestion,
	parsePluginManifest,
	pluginManifestFromPreset,
	selectedPluginMapping,
	suggestResponsePaths
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
		expect(values).toContain('vllm');
		expect(values).toContain('lm_studio');
		expect(values).toContain('ollama_openai');
		expect(values).toContain('localai');
		expect(values).toContain('xinference');
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
				capabilities: {
					chat: true,
					streaming: true,
					tools: true,
					embeddings: false,
					image: false,
					audio: false,
					vision: true,
					json_mode: true,
					batch: false
				},
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
				capabilities: {
					chat: true,
					streaming: true,
					tools: true,
					embeddings: false,
					image: false,
					audio: false,
					vision: true,
					json_mode: true,
					batch: false
				},
				auth: {
					strategy: 'api_key_header',
					header_name: 'x-api-key',
					secret_slot: 'anthropic_key'
				},
				preset: { provider: 'anthropic_messages' }
			}
		});
	});

	it('exposes capability defaults and base url suggestions for local OpenAI variants', () => {
		for (const preset of ['vllm', 'lm_studio', 'ollama_openai', 'localai', 'xinference']) {
			const caps = pluginCapabilitiesForPreset(preset);
			expect(capabilityList(caps)).toEqual(expect.arrayContaining(['chat', 'streaming', 'embeddings']));
			expect(pluginPresetBaseUrlSuggestion(preset)).toMatch(/^https?:\/\//);
		}
	});

	it('uses selected preset over stale manifest input', () => {
		const selected = selectedPluginMapping('azure_openai', '{"plugin":{"preset":{"provider":"openai_compatible"}}}');
		expect(manifestPreset(selected)).toBe('azure_openai');
	});

	it('parses custom manifest when no preset is selected', () => {
		const parsed = selectedPluginMapping('', '{"plugin":{"request":{"chat_path":"/x"}}}');
		expect(parsePluginManifest(JSON.stringify(parsed))).toEqual(parsed);
	});

	it('accepts advanced stream normalizer fields in custom manifests', () => {
		const parsed = selectedPluginMapping(
			'',
			JSON.stringify({
				plugin: {
					stream: {
						openai_compatible: false,
						event_path: 'payload',
						ignore_events: ['ping'],
						done_events: ['close'],
						done: ['EOF'],
						done_path: 'type',
						done_values: ['message_stop'],
						tool_calls_path: 'tool_calls',
						usage: { raw_path: 'usage' }
					}
				}
			})
		);
		const stream = (parsed.plugin as Record<string, unknown>).stream as Record<string, unknown>;
		expect(stream.done_path).toBe('type');
		expect(stream.tool_calls_path).toBe('tool_calls');
		expect(stream.ignore_events).toEqual(['ping']);
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

	it('builds manifest from wizard draft with response sample hints and probe', () => {
		const draft = defaultPluginBuilderDraft('');
		draft.auth = { ...draft.auth, strategy: 'api_key_header', header_name: 'X-Api-Key' };
		draft.request_path = '/private/chat';
		draft.response_sample = JSON.stringify({
			result: { text: 'mapped ok', finish: 'stop' },
			usage: { input: 2, output: 3 }
		});
		draft.probe_path = '/health/chat';
		draft.probe_model = 'tiny-model';
		draft.probe_success_status = '200,204';
		draft.probe_max_cost_micros = '100';

		const manifest = buildPluginBuilderManifest(draft);
		expect(manifest).toMatchObject({
			plugin: {
				version: 1,
				auth: { strategy: 'api_key_header', header_name: 'X-Api-Key' },
				request: { path: '/private/chat' },
				response: {
					openai_compatible: false,
					content_path: 'result.text',
					finish_reason_path: 'result.finish',
					usage: {
						prompt_tokens_path: 'usage.input',
						completion_tokens_path: 'usage.output'
					}
				},
				probe: {
					path: '/health/chat',
					model: 'tiny-model',
					success_status: [200, 204],
					max_cost_micros: 100
				}
			}
		});
	});

	it('suggests response paths from non-stream samples', () => {
		const suggestions = suggestResponsePaths(
			JSON.stringify({
				result: { text: 'hello' },
				usage: { input: 1, output: 2, total_tokens: 3 }
			})
		);
		expect(suggestions.map(s => s.path)).toContain('result.text');
		expect(suggestions.map(s => s.path)).toContain('usage.total_tokens');
	});

	it('lets builder path clicks override automatic response suggestions', () => {
		const draft = defaultPluginBuilderDraft('openai_compatible');
		draft.response_content_path = 'data.answer';
		draft.response_total_tokens_path = 'metrics.tokens';
		const manifest = buildPluginBuilderManifest(draft);
		expect(manifest).toMatchObject({
			plugin: {
				preset: { provider: 'openai_compatible' },
				response: {
					openai_compatible: false,
					content_path: 'data.answer',
					usage: { total_tokens_path: 'metrics.tokens' }
				}
			}
		});
	});
});
