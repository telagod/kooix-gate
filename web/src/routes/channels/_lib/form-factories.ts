// 0.4.110（followup B2 step 2）：channels page createForm / editForm 默认值
// 工厂。之前默认值散在 page.svelte 多处（line 158-162 初始化 + 770 重置），
// 任何字段调整需改两处，且无类型检查兜底。
//
// 抽到 _lib 后：
// - 单一定义点
// - 显式 `CreateChannelRequest` 类型保护
// - reset 时 spread `defaultCreateForm()` 一次到位

import type { CreateChannelRequest, UpdateChannelRequest } from '$lib/api.js';

export function defaultCreateForm(): CreateChannelRequest {
	return {
		code: '',
		provider_type: 'openai',
		base_url: '',
		supported_models: [],
		rpm_limit: null,
		tpm_limit: null,
		timeout_ms: 60000,
		max_retries: 2,
		tags: [],
		model_mapping: {}
	};
}

export function defaultEditForm(): UpdateChannelRequest {
	return {};
}
