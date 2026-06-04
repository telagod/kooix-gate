export interface PluginPresetOption {
	value: string;
	label: string;
}

export interface ProviderCapabilities {
	chat: boolean;
	streaming: boolean;
	tools: boolean;
	embeddings: boolean;
	image: boolean;
	audio: boolean;
	vision: boolean;
	json_mode: boolean;
	batch: boolean;
}

export type ProviderCapabilityKey = keyof ProviderCapabilities;

export const CAPABILITY_LABELS: Record<ProviderCapabilityKey, string> = {
	chat: 'Chat',
	streaming: 'Stream',
	tools: 'Tools',
	embeddings: 'Embed',
	image: 'Image',
	audio: 'Audio',
	vision: 'Vision',
	json_mode: 'JSON',
	batch: 'Batch'
};

const NO_CAPABILITIES: ProviderCapabilities = {
	chat: false,
	streaming: false,
	tools: false,
	embeddings: false,
	image: false,
	audio: false,
	vision: false,
	json_mode: false,
	batch: false
};

const CHAT_STREAM: ProviderCapabilities = { ...NO_CAPABILITIES, chat: true, streaming: true };
const OPENAI_COMPAT_CORE: ProviderCapabilities = {
	...CHAT_STREAM,
	tools: true,
	embeddings: true,
	vision: true,
	json_mode: true
};
const OPENAI_FULL: ProviderCapabilities = { ...OPENAI_COMPAT_CORE, image: true, audio: true };
const VERTEX_OPENAI_BASE_URL =
	'https://aiplatform.googleapis.com/v1/projects/<project>/locations/<location>/endpoints/openapi';

export const PROVIDER_CAPABILITIES: Record<string, ProviderCapabilities> = {
	openai: OPENAI_FULL,
	azure: OPENAI_COMPAT_CORE,
	vertex: OPENAI_COMPAT_CORE,
	anthropic: { ...CHAT_STREAM, tools: true, vision: true, json_mode: true },
	gemini: OPENAI_COMPAT_CORE,
	deepseek: { ...CHAT_STREAM, json_mode: true },
	ollama: { ...CHAT_STREAM, embeddings: true, tools: true, vision: true, json_mode: true },
	mistral: OPENAI_COMPAT_CORE,
	cohere: { ...CHAT_STREAM, tools: true, embeddings: true, json_mode: true },
	bedrock: CHAT_STREAM,
	groq: OPENAI_COMPAT_CORE,
	together: OPENAI_COMPAT_CORE,
	openrouter: OPENAI_COMPAT_CORE,
	moonshot: OPENAI_COMPAT_CORE,
	zhipu: OPENAI_COMPAT_CORE,
	qwen: OPENAI_COMPAT_CORE,
	yi: OPENAI_COMPAT_CORE,
	plugin: CHAT_STREAM,
	custom: CHAT_STREAM,
	http: CHAT_STREAM,
	http_plugin: CHAT_STREAM
};

export const PLUGIN_PRESET_CAPABILITIES: Record<string, ProviderCapabilities> = {
	openai: OPENAI_COMPAT_CORE,
	openai_compatible: OPENAI_COMPAT_CORE,
	vllm: OPENAI_COMPAT_CORE,
	lm_studio: OPENAI_COMPAT_CORE,
	ollama_openai: OPENAI_COMPAT_CORE,
	localai: OPENAI_COMPAT_CORE,
	xinference: OPENAI_COMPAT_CORE,
	anthropic_messages: { ...CHAT_STREAM, tools: true, vision: true, json_mode: true },
	azure_openai: OPENAI_COMPAT_CORE,
	vertex_openai: OPENAI_COMPAT_CORE,
	gemini: OPENAI_COMPAT_CORE,
	deepseek: { ...CHAT_STREAM, json_mode: true },
	mistral: OPENAI_COMPAT_CORE,
	cohere_chat: { ...CHAT_STREAM, tools: true, embeddings: true, json_mode: true },
	ollama: { ...CHAT_STREAM, embeddings: true, tools: true, vision: true, json_mode: true },
	groq: OPENAI_COMPAT_CORE,
	together: OPENAI_COMPAT_CORE,
	openrouter: OPENAI_COMPAT_CORE,
	moonshot: OPENAI_COMPAT_CORE,
	zhipu: OPENAI_COMPAT_CORE,
	qwen: OPENAI_COMPAT_CORE,
	yi: OPENAI_COMPAT_CORE,
	bedrock_converse: CHAT_STREAM,
	fireworks: OPENAI_COMPAT_CORE,
	perplexity: CHAT_STREAM,
	cerebras: OPENAI_COMPAT_CORE,
	sambanova: OPENAI_COMPAT_CORE,
	hyperbolic: OPENAI_COMPAT_CORE,
	cloudflare_ai: OPENAI_COMPAT_CORE,
	jina: { ...CHAT_STREAM, embeddings: true },
	baichuan: OPENAI_COMPAT_CORE,
	minimax: OPENAI_COMPAT_CORE,
	stepfun: OPENAI_COMPAT_CORE,
	siliconflow: OPENAI_COMPAT_CORE,
	tgi: CHAT_STREAM,
	jan: CHAT_STREAM,
	llamafile: CHAT_STREAM,
	gpt4all: CHAT_STREAM,
	tabby_api: CHAT_STREAM,
	doubao: OPENAI_COMPAT_CORE
};

export const PROVIDER_BASE_URL_SUGGESTIONS: Record<string, string> = {
	openai: 'https://api.openai.com/v1',
	anthropic: 'https://api.anthropic.com',
	gemini: 'https://generativelanguage.googleapis.com',
	azure: 'https://<resource>.openai.azure.com',
	vertex: VERTEX_OPENAI_BASE_URL,
	bedrock: 'https://bedrock-runtime.<region>.amazonaws.com',
	deepseek: 'https://api.deepseek.com/v1',
	ollama: 'http://localhost:11434/v1',
	mistral: 'https://api.mistral.ai/v1',
	cohere: 'https://api.cohere.com/v2',
	groq: 'https://api.groq.com/openai/v1',
	together: 'https://api.together.xyz/v1',
	openrouter: 'https://openrouter.ai/api/v1',
	moonshot: 'https://api.moonshot.cn/v1',
	zhipu: 'https://open.bigmodel.cn/api/paas/v4',
	qwen: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
	yi: 'https://api.lingyiwanwu.com/v1',
	plugin: 'https://api.example.com/v1'
};

export const PLUGIN_PRESET_BASE_URL_SUGGESTIONS: Record<string, string> = {
	openai: 'https://api.openai.com/v1',
	openai_compatible: 'https://api.openai.com/v1',
	vllm: 'http://localhost:8000/v1',
	lm_studio: 'http://localhost:1234/v1',
	ollama: 'http://localhost:11434/v1',
	ollama_openai: 'http://localhost:11434/v1',
	localai: 'http://localhost:8080/v1',
	xinference: 'http://localhost:9997/v1',
	deepseek: 'https://api.deepseek.com/v1',
	mistral: 'https://api.mistral.ai/v1',
	gemini: 'https://generativelanguage.googleapis.com',
	azure_openai: 'https://<resource>.openai.azure.com',
	vertex_openai: VERTEX_OPENAI_BASE_URL,
	anthropic_messages: 'https://api.anthropic.com',
	bedrock_converse: 'https://bedrock-runtime.<region>.amazonaws.com',
	cohere_chat: 'https://api.cohere.com/v2',
	groq: 'https://api.groq.com/openai/v1',
	together: 'https://api.together.xyz/v1',
	openrouter: 'https://openrouter.ai/api/v1',
	moonshot: 'https://api.moonshot.cn/v1',
	zhipu: 'https://open.bigmodel.cn/api/paas/v4',
	qwen: 'https://dashscope.aliyuncs.com/compatible-mode/v1',
	yi: 'https://api.lingyiwanwu.com/v1',
	fireworks: 'https://api.fireworks.ai/inference/v1',
	perplexity: 'https://api.perplexity.ai',
	cerebras: 'https://api.cerebras.ai/v1',
	sambanova: 'https://api.sambanova.ai/v1',
	hyperbolic: 'https://api.hyperbolic.xyz/v1',
	cloudflare_ai: 'https://api.cloudflare.com/client/v4/accounts/<account_id>/ai/v1',
	jina: 'https://api.jina.ai/v1',
	baichuan: 'https://api.baichuan-ai.com/v1',
	minimax: 'https://api.minimax.chat/v1',
	stepfun: 'https://api.stepfun.com/v1',
	siliconflow: 'https://api.siliconflow.cn/v1',
	tgi: 'http://localhost:8080/v1',
	jan: 'http://localhost:1337/v1',
	llamafile: 'http://localhost:8080/v1',
	gpt4all: 'http://localhost:4891/v1',
	tabby_api: 'http://localhost:5000/v1',
	doubao: 'https://ark.cn-beijing.volces.com/api/v3'
};

export type PluginAuthStrategy =
	| 'bearer'
	| 'api_key_header'
	| 'api_key_query'
	| 'basic'
	| 'custom_headers'
	| 'hmac'
	| 'aws_sigv4'
	| 'oauth_client_credentials'
	| 'none';

export interface PluginAuthStrategyOption {
	value: PluginAuthStrategy;
	label: string;
	description: string;
}

export interface PluginAuthForm {
	strategy: PluginAuthStrategy;
	secret_slot: string;
	header_name: string;
	query_name: string;
	username_slot: string;
	password_slot: string;
	custom_headers: string;
	hmac_signature_header: string;
	hmac_timestamp_header: string;
	hmac_nonce_header: string;
	hmac_signed_payload: string;
	hmac_signature_encoding: 'hex' | 'base64';
	aws_service: string;
	aws_region: string;
	aws_access_key_slot: string;
	aws_secret_key_slot: string;
	aws_session_token_slot: string;
	oauth_token_url: string;
	oauth_client_id_slot: string;
	oauth_client_secret_slot: string;
	oauth_scope: string;
	oauth_audience: string;
	oauth_expiry_skew_seconds: number;
}

export interface PluginBuilderDraft {
	preset: string;
	auth: PluginAuthForm;
	request_path: string;
	request_body: string;
	response_sample: string;
	response_content_path: string;
	response_finish_reason_path: string;
	response_prompt_tokens_path: string;
	response_completion_tokens_path: string;
	response_total_tokens_path: string;
	raw_sse: string;
	probe_path: string;
	probe_model: string;
	probe_body: string;
	probe_success_status: string;
	probe_max_cost_micros: string;
	target_group_id: string;
}

export interface PluginResponsePathSuggestion {
	label: string;
	path: string;
	value: string;
}

export const PLUGIN_PRESET_OPTIONS: PluginPresetOption[] = [
	{ value: '', label: '自定义 manifest' },
	{ value: 'openai_compatible', label: 'OpenAI-compatible' },
	{ value: 'vllm', label: 'vLLM' },
	{ value: 'lm_studio', label: 'LM Studio' },
	{ value: 'ollama_openai', label: 'Ollama OpenAI endpoint' },
	{ value: 'localai', label: 'LocalAI' },
	{ value: 'xinference', label: 'Xinference' },
	{ value: 'anthropic_messages', label: 'Anthropic Messages' },
	{ value: 'azure_openai', label: 'Azure OpenAI' },
	{ value: 'vertex_openai', label: 'Google Vertex AI OpenAI' },
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
	{ value: 'bedrock_converse', label: 'AWS Bedrock Converse' },
	{ value: 'fireworks', label: 'Fireworks AI' },
	{ value: 'perplexity', label: 'Perplexity' },
	{ value: 'cerebras', label: 'Cerebras' },
	{ value: 'sambanova', label: 'SambaNova' },
	{ value: 'hyperbolic', label: 'Hyperbolic' },
	{ value: 'cloudflare_ai', label: 'Cloudflare AI' },
	{ value: 'jina', label: 'Jina AI' },
	{ value: 'baichuan', label: '百川 Baichuan' },
	{ value: 'minimax', label: 'MiniMax' },
	{ value: 'stepfun', label: '阶跃星辰 Stepfun' },
	{ value: 'siliconflow', label: '硅基流动 SiliconFlow' },
	{ value: 'doubao', label: '豆包 Doubao' },
	{ value: 'tgi', label: 'Text Generation Inference' },
	{ value: 'jan', label: 'Jan' },
	{ value: 'llamafile', label: 'Llamafile' },
	{ value: 'gpt4all', label: 'GPT4All' },
	{ value: 'tabby_api', label: 'TabbyAPI' }
];

export const PLUGIN_AUTH_STRATEGY_OPTIONS: PluginAuthStrategyOption[] = [
	{ value: 'bearer', label: 'Bearer', description: 'Authorization: Bearer <secret_slot>' },
	{ value: 'api_key_header', label: 'API Key Header', description: '把 key 写入指定 header' },
	{ value: 'api_key_query', label: 'API Key Query', description: '把 key 追加到 query 参数' },
	{ value: 'basic', label: 'Basic', description: 'username/password 来自 secret slots' },
	{ value: 'custom_headers', label: 'Custom Headers', description: '按模板注入多个认证 header' },
	{ value: 'hmac', label: 'HMAC', description: 'method/path/body/timestamp/nonce 签名' },
	{ value: 'aws_sigv4', label: 'AWS SigV4', description: 'AWS Signature Version 4' },
	{ value: 'oauth_client_credentials', label: 'OAuth Client Credentials', description: '先换 token，再注入 Bearer' },
	{ value: 'none', label: 'None', description: '不注入认证' }
];

const DEFAULT_HMAC_PAYLOAD = '{{method}}\n{{path}}\n{{body_sha256}}\n{{timestamp}}\n{{nonce}}';
const DEFAULT_CUSTOM_HEADERS = '{\n  "X-Api-Key": "{{api_key}}"\n}';

export function defaultPluginAuthForm(strategy: PluginAuthStrategy = 'bearer'): PluginAuthForm {
	return {
		strategy,
		secret_slot: 'primary',
		header_name: 'X-Api-Key',
		query_name: 'api_key',
		username_slot: 'username',
		password_slot: 'primary',
		custom_headers: DEFAULT_CUSTOM_HEADERS,
		hmac_signature_header: 'X-Signature',
		hmac_timestamp_header: 'X-Timestamp',
		hmac_nonce_header: 'X-Nonce',
		hmac_signed_payload: DEFAULT_HMAC_PAYLOAD,
		hmac_signature_encoding: 'hex',
		aws_service: 'bedrock',
		aws_region: '',
		aws_access_key_slot: 'primary',
		aws_secret_key_slot: 'aws_secret_key',
		aws_session_token_slot: 'aws_session_token',
		oauth_token_url: '',
		oauth_client_id_slot: 'client_id',
		oauth_client_secret_slot: 'client_secret',
		oauth_scope: '',
		oauth_audience: '',
		oauth_expiry_skew_seconds: 60
	};
}

export function defaultPluginAuthForPreset(preset: string): PluginAuthForm {
	if (preset === 'azure_openai') {
		return { ...defaultPluginAuthForm('api_key_header'), header_name: 'api-key' };
	}
	if (preset === 'anthropic_messages') {
		return { ...defaultPluginAuthForm('api_key_header'), header_name: 'x-api-key' };
	}
	if (preset === 'bedrock_converse') {
		return defaultPluginAuthForm('aws_sigv4');
	}
	return defaultPluginAuthForm('bearer');
}

export function providerCapabilities(providerType: string): ProviderCapabilities {
	return cloneCapabilities(PROVIDER_CAPABILITIES[normalizeProvider(providerType)] ?? OPENAI_COMPAT_CORE);
}

export function pluginCapabilitiesForPreset(preset: string): ProviderCapabilities {
	return cloneCapabilities(PLUGIN_PRESET_CAPABILITIES[normalizeProvider(preset)] ?? CHAT_STREAM);
}

export function providerBaseUrlSuggestion(providerType: string): string {
	return PROVIDER_BASE_URL_SUGGESTIONS[normalizeProvider(providerType)] ?? '';
}

export function pluginPresetBaseUrlSuggestion(preset: string): string {
	return PLUGIN_PRESET_BASE_URL_SUGGESTIONS[normalizeProvider(preset)] ?? '';
}

export function capabilityList(caps: ProviderCapabilities | undefined): ProviderCapabilityKey[] {
	if (!caps) return [];
	return (Object.keys(CAPABILITY_LABELS) as ProviderCapabilityKey[]).filter(key => !!caps[key]);
}

export function missingCapabilityList(
	caps: ProviderCapabilities | undefined,
	keys: ProviderCapabilityKey[]
): ProviderCapabilityKey[] {
	return keys.filter(key => !caps?.[key]);
}

export function pluginManifestFromPreset(
	provider: string,
	authForm?: PluginAuthForm
): Record<string, unknown> {
	return provider
		? {
				plugin: {
					version: 1,
					capabilities: pluginCapabilitiesForPreset(provider),
					auth: authForm ? buildPluginAuthManifest(authForm) : { strategy: 'bearer', secret_slot: 'primary' },
					preset: { provider }
				}
			}
		: {};
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

export function authFormFromManifest(mapping: Record<string, unknown> | undefined): PluginAuthForm {
	const preset = manifestPreset(mapping);
	const auth = pluginSection(mapping)?.auth;
	if (!auth || typeof auth !== 'object' || Array.isArray(auth)) {
		return defaultPluginAuthForPreset(preset);
	}
	const source = auth as Record<string, unknown>;
	const strategy = stringValue(source.strategy) as PluginAuthStrategy | '';
	const form = defaultPluginAuthForm(isKnownAuthStrategy(strategy) ? strategy : 'bearer');
	form.secret_slot = stringValue(source.secret_slot) || form.secret_slot;
	form.header_name = stringValue(source.header_name) || form.header_name;
	form.query_name = stringValue(source.query_name) || form.query_name;
	form.username_slot = stringValue(source.username_slot) || form.username_slot;
	form.password_slot = stringValue(source.password_slot) || form.password_slot;
	if (isPlainObject(source.headers)) {
		form.custom_headers = JSON.stringify(source.headers, null, 2);
	}
	if (isPlainObject(source.hmac)) {
		const hmac = source.hmac as Record<string, unknown>;
		form.hmac_signature_header = stringValue(hmac.signature_header) || form.hmac_signature_header;
		form.hmac_timestamp_header = stringValue(hmac.timestamp_header) || form.hmac_timestamp_header;
		form.hmac_nonce_header = stringValue(hmac.nonce_header) || form.hmac_nonce_header;
		form.hmac_signed_payload = stringValue(hmac.signed_payload) || form.hmac_signed_payload;
		const encoding = stringValue(hmac.signature_encoding);
		if (encoding === 'hex' || encoding === 'base64') form.hmac_signature_encoding = encoding;
	}
	if (isPlainObject(source.aws_sigv4)) {
		const aws = source.aws_sigv4 as Record<string, unknown>;
		form.aws_service = stringValue(aws.service) || form.aws_service;
		form.aws_region = stringValue(aws.region);
		form.aws_access_key_slot = stringValue(aws.access_key_slot) || form.aws_access_key_slot;
		form.aws_secret_key_slot = stringValue(aws.secret_key_slot) || form.aws_secret_key_slot;
		form.aws_session_token_slot = stringValue(aws.session_token_slot);
	}
	if (isPlainObject(source.oauth)) {
		const oauth = source.oauth as Record<string, unknown>;
		form.oauth_token_url = stringValue(oauth.token_url);
		form.oauth_client_id_slot = stringValue(oauth.client_id_slot) || form.oauth_client_id_slot;
		form.oauth_client_secret_slot = stringValue(oauth.client_secret_slot) || form.oauth_client_secret_slot;
		form.oauth_scope = stringValue(oauth.scope);
		form.oauth_audience = stringValue(oauth.audience);
		const skew = numberValue(oauth.expiry_skew_seconds);
		if (skew != null) form.oauth_expiry_skew_seconds = skew;
	}
	return form;
}

export function selectedPluginMapping(
	preset: string,
	input: string,
	authForm?: PluginAuthForm
): Record<string, unknown> {
	if (!preset) return withPluginAuth(parsePluginManifest(input), authForm);
	if (input.trim()) {
		const parsed = parsePluginManifest(input);
		if (manifestPreset(parsed) === preset) return withPluginAuth(parsed, authForm);
	}
	return pluginManifestFromPreset(preset, authForm);
}

export function defaultPluginBuilderDraft(preset = ''): PluginBuilderDraft {
	return {
		preset,
		auth: defaultPluginAuthForPreset(preset),
		request_path: preset ? '' : '/private/chat',
		request_body: preset
			? ''
			: JSON.stringify(
					{
						modelName: '{{model}}',
						prompt: '{{last_user_message}}',
						stream: '{{stream}}',
						limit: '{{max_tokens}}'
					},
					null,
					2
				),
		response_sample: '',
		response_content_path: '',
		response_finish_reason_path: '',
		response_prompt_tokens_path: '',
		response_completion_tokens_path: '',
		response_total_tokens_path: '',
		raw_sse: '',
		probe_path: '',
		probe_model: '',
		probe_body: '',
		probe_success_status: '200',
		probe_max_cost_micros: '',
		target_group_id: ''
	};
}

export function buildPluginBuilderManifest(draft: PluginBuilderDraft): Record<string, unknown> {
	const plugin: Record<string, unknown> = {
		version: 1,
		capabilities: pluginCapabilitiesForPreset(draft.preset),
		auth: buildPluginAuthManifest(draft.auth)
	};
	if (draft.preset) {
		plugin.preset = { provider: draft.preset };
	} else {
		const request: Record<string, unknown> = {};
		if (draft.request_path.trim()) request.path = draft.request_path.trim();
		const body = parseOptionalJson(draft.request_body, 'Request body');
		if (body != null) request.body = body;
		if (Object.keys(request).length > 0) plugin.request = request;
	}

	const response = responseMappingFromSample(draft);
	if (Object.keys(response).length > 0) plugin.response = response;

	const probe = probeManifestFromDraft(draft);
	if (Object.keys(probe).length > 0) plugin.probe = probe;

	return { plugin };
}

export function suggestResponsePaths(sampleInput: string): PluginResponsePathSuggestion[] {
	const sample = parseOptionalJson(sampleInput, 'Response sample');
	if (!isPlainObject(sample)) return [];
	const suggestions: PluginResponsePathSuggestion[] = [];
	collectLeafPaths(sample, '', suggestions);
	const ranked = suggestions.sort((a, b) => responsePathScore(b.path) - responsePathScore(a.path));
	return ranked.slice(0, 24);
}

export function buildPluginAuthManifest(form: PluginAuthForm): Record<string, unknown> {
	switch (form.strategy) {
		case 'bearer':
			return { strategy: 'bearer', secret_slot: slotOrDefault(form.secret_slot, 'primary') };
		case 'api_key_header':
			return {
				strategy: 'api_key_header',
				header_name: requiredText(form.header_name, 'API key header name'),
				secret_slot: slotOrDefault(form.secret_slot, 'primary')
			};
		case 'api_key_query':
			return {
				strategy: 'api_key_query',
				query_name: requiredText(form.query_name, 'API key query name'),
				secret_slot: slotOrDefault(form.secret_slot, 'primary')
			};
		case 'basic':
			return {
				strategy: 'basic',
				username_slot: requiredSlot(form.username_slot, 'username_slot'),
				password_slot: slotOrDefault(form.password_slot, 'primary')
			};
		case 'custom_headers':
			return { strategy: 'custom_headers', headers: parseCustomHeaders(form.custom_headers) };
		case 'hmac':
			return {
				strategy: 'hmac',
				secret_slot: slotOrDefault(form.secret_slot, 'primary'),
				hmac: {
					signature_header: requiredText(form.hmac_signature_header, 'HMAC signature header'),
					timestamp_header: requiredText(form.hmac_timestamp_header, 'HMAC timestamp header'),
					nonce_header: requiredText(form.hmac_nonce_header, 'HMAC nonce header'),
					signed_payload: requiredText(form.hmac_signed_payload, 'HMAC signed payload'),
					signature_encoding: form.hmac_signature_encoding
				}
			};
		case 'aws_sigv4': {
			const aws: Record<string, unknown> = {
				service: requiredText(form.aws_service, 'AWS service'),
				access_key_slot: slotOrDefault(form.aws_access_key_slot, 'primary'),
				secret_key_slot: requiredSlot(form.aws_secret_key_slot, 'secret_key_slot')
			};
			if (form.aws_region.trim()) aws.region = form.aws_region.trim();
			if (form.aws_session_token_slot.trim()) {
				aws.session_token_slot = requiredSlot(form.aws_session_token_slot, 'session_token_slot');
			}
			return { strategy: 'aws_sigv4', aws_sigv4: aws };
		}
		case 'oauth_client_credentials': {
			const tokenUrl = requiredText(form.oauth_token_url, 'OAuth token URL');
			if (!isAllowedOauthTokenUrl(tokenUrl)) {
				throw new Error('OAuth token URL 必须使用 HTTPS；本地测试仅允许 localhost / 127.0.0.1');
			}
			const skew = Math.trunc(Number(form.oauth_expiry_skew_seconds));
			if (!Number.isFinite(skew) || skew < 0 || skew > 3600) {
				throw new Error('OAuth expiry_skew_seconds 必须在 0-3600 之间');
			}
			const oauth: Record<string, unknown> = {
				token_url: tokenUrl,
				client_id_slot: requiredSlot(form.oauth_client_id_slot, 'client_id_slot'),
				client_secret_slot: requiredSlot(form.oauth_client_secret_slot, 'client_secret_slot'),
				expiry_skew_seconds: skew
			};
			if (form.oauth_scope.trim()) oauth.scope = form.oauth_scope.trim();
			if (form.oauth_audience.trim()) oauth.audience = form.oauth_audience.trim();
			return { strategy: 'oauth_client_credentials', oauth };
		}
		case 'none':
			return { strategy: 'none' };
		default:
			throw new Error(`不支持的 auth strategy: ${form.strategy}`);
	}
}

function withPluginAuth(
	mapping: Record<string, unknown>,
	authForm?: PluginAuthForm
): Record<string, unknown> {
	if (!authForm) return mapping;
	const auth = buildPluginAuthManifest(authForm);
	if (isPlainObject(mapping.plugin)) {
		return {
			...mapping,
			plugin: {
				...(mapping.plugin as Record<string, unknown>),
				version: (mapping.plugin as Record<string, unknown>).version ?? 1,
				auth
			}
		};
	}
	return { ...mapping, version: mapping.version ?? 1, auth };
}

function pluginSection(mapping: Record<string, unknown> | undefined): Record<string, unknown> | undefined {
	if (!mapping) return undefined;
	if (isPlainObject(mapping.plugin)) return mapping.plugin as Record<string, unknown>;
	return mapping;
}

function parseCustomHeaders(input: string): Record<string, unknown> {
	const parsed = parsePluginManifest(input);
	if (Object.keys(parsed).length === 0) {
		throw new Error('Custom headers 至少需要一个 header');
	}
	return parsed;
}

function responseMappingFromSample(draft: PluginBuilderDraft): Record<string, unknown> {
	const suggestions = suggestResponsePaths(draft.response_sample);
	const pick = (patterns: RegExp[]) => suggestions.find(s => patterns.some(pattern => pattern.test(s.path)))?.path;
	const response: Record<string, unknown> = { openai_compatible: false };
	const contentPath = draft.response_content_path.trim() || pick([
		/^choices\.0\.message\.content$/,
		/^(result|data|output|message|response)\.(text|content|answer)$/,
		/(^|\.)(text|content|answer)$/
	]);
	if (contentPath) response.content_path = contentPath;
	const finishPath = draft.response_finish_reason_path.trim() || pick([/(^|\.)finish(_reason)?$/, /(^|\.)stop_reason$/]);
	if (finishPath) response.finish_reason_path = finishPath;
	const usage: Record<string, unknown> = {};
	const prompt = draft.response_prompt_tokens_path.trim() || pick([/^usage\.(prompt_tokens|input_tokens|input)$/]);
	const completion =
		draft.response_completion_tokens_path.trim() || pick([/^usage\.(completion_tokens|output_tokens|output)$/]);
	const total = draft.response_total_tokens_path.trim() || pick([/^usage\.total_tokens$/]);
	if (prompt) usage.prompt_tokens_path = prompt;
	if (completion) usage.completion_tokens_path = completion;
	if (total) usage.total_tokens_path = total;
	if (Object.keys(usage).length > 0) response.usage = usage;
	if (Object.keys(response).length === 1) return {};
	return response;
}

function probeManifestFromDraft(draft: PluginBuilderDraft): Record<string, unknown> {
	const probe: Record<string, unknown> = {};
	if (draft.probe_path.trim()) probe.path = draft.probe_path.trim();
	if (draft.probe_model.trim()) probe.model = draft.probe_model.trim();
	const probeBody = parseOptionalJson(draft.probe_body, 'Probe body');
	if (probeBody != null) probe.body = probeBody;
	const statuses = draft.probe_success_status
		.split(',')
		.map(s => Number.parseInt(s.trim(), 10))
		.filter(n => Number.isInteger(n) && n >= 100 && n <= 599);
	if (statuses.length > 0) probe.success_status = statuses;
	if (draft.probe_max_cost_micros.trim()) {
		const cost = Number.parseInt(draft.probe_max_cost_micros.trim(), 10);
		if (!Number.isFinite(cost) || cost < 0) throw new Error('Probe max_cost_micros 必须是非负整数');
		probe.max_cost_micros = cost;
	}
	return probe;
}

function parseOptionalJson(input: string, label: string): unknown {
	if (!input.trim()) return null;
	try {
		return JSON.parse(input);
	} catch (err: any) {
		throw new Error(`${label} 必须是合法 JSON: ${err?.message ?? err}`);
	}
}

function collectLeafPaths(value: unknown, prefix: string, out: PluginResponsePathSuggestion[]) {
	if (Array.isArray(value)) {
		value.slice(0, 3).forEach((item, index) => collectLeafPaths(item, joinPath(prefix, String(index)), out));
		return;
	}
	if (isPlainObject(value)) {
		for (const [key, child] of Object.entries(value)) {
			collectLeafPaths(child, joinPath(prefix, key), out);
		}
		return;
	}
	if (value == null) return;
	out.push({ label: prefix, path: prefix, value: String(value).slice(0, 80) });
}

function joinPath(prefix: string, key: string): string {
	return prefix ? `${prefix}.${key}` : key;
}

function responsePathScore(path: string): number {
	if (/^choices\.0\.message\.content$/.test(path)) return 100;
	if (/^(result|data|output|message|response)\.(text|content|answer)$/.test(path)) return 95;
	if (/(^|\.)(text|content|answer)$/.test(path)) return 80;
	if (/^usage\.(prompt_tokens|completion_tokens|total_tokens|input|output)$/.test(path)) return 70;
	if (/(^|\.)finish(_reason)?$/.test(path)) return 60;
	return 10;
}

function requiredText(value: string, label: string): string {
	const trimmed = value.trim();
	if (!trimmed) throw new Error(`${label} 不能为空`);
	return trimmed;
}

function requiredSlot(value: string, label: string): string {
	const slot = requiredText(value, label);
	if (!/^[a-zA-Z0-9_-]+$/.test(slot)) {
		throw new Error(`${label} 只能使用 [a-zA-Z0-9_-]`);
	}
	return slot;
}

function slotOrDefault(value: string, fallback: string): string {
	return requiredSlot(value.trim() || fallback, 'secret_slot');
}

function cloneCapabilities(caps: ProviderCapabilities): ProviderCapabilities {
	return { ...NO_CAPABILITIES, ...caps };
}

function normalizeProvider(value: string): string {
	return value.trim().toLowerCase().replaceAll('-', '_');
}

function isPlainObject(value: unknown): value is Record<string, unknown> {
	return !!value && typeof value === 'object' && !Array.isArray(value);
}

function stringValue(value: unknown): string {
	return typeof value === 'string' ? value : '';
}

function numberValue(value: unknown): number | null {
	return typeof value === 'number' && Number.isFinite(value) ? value : null;
}

function isAllowedOauthTokenUrl(value: string): boolean {
	try {
		const url = new URL(value);
		if (url.protocol === 'https:') return true;
		if (url.protocol !== 'http:') return false;
		return url.hostname === 'localhost' || url.hostname === '127.0.0.1';
	} catch {
		return false;
	}
}

function isKnownAuthStrategy(value: string): value is PluginAuthStrategy {
	return PLUGIN_AUTH_STRATEGY_OPTIONS.some(option => option.value === value);
}
