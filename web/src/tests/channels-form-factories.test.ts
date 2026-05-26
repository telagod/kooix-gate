// 0.4.114（followup B2 step 3）：验证 channels form-factories 工厂返回正确
// shape。0.4.110 抽了工厂但没测试。
//
// 这里不只是 type check（TypeScript 已保证）——而是验证默认值业务正确：
// timeout_ms=60000、max_retries=2 这些是 product 决策，工厂必须返这些值。

import { describe, expect, it } from 'vitest';
import {
	defaultCreateForm,
	defaultEditForm
} from '../routes/channels/_lib/form-factories';

describe('channels form-factories', () => {
	it('defaultCreateForm 返业务约定的默认值', () => {
		const f = defaultCreateForm();
		expect(f.code).toBe('');
		expect(f.provider_type).toBe('openai');
		expect(f.base_url).toBe('');
		expect(f.supported_models).toEqual([]);
		expect(f.rpm_limit).toBeNull();
		expect(f.tpm_limit).toBeNull();
		expect(f.timeout_ms).toBe(60000); // 60s 是 channel 默认上游超时（与 ProviderOpts default 不同）
		expect(f.max_retries).toBe(2); // 与 RetryConfig::default().max_retries 对齐
		expect(f.tags).toEqual([]);
		expect(f.model_mapping).toEqual({});
	});

	it('defaultCreateForm 每次调用返新 object（不是 shared ref）', () => {
		const a = defaultCreateForm();
		const b = defaultCreateForm();
		expect(a).not.toBe(b); // 不同 reference
		expect(a.supported_models).not.toBe(b.supported_models); // array 也不共享
		expect(a.model_mapping).not.toBe(b.model_mapping); // object 也不共享

		// mutate a，b 不受影响
		a.supported_models!.push('gpt-4');
		expect(b.supported_models).toEqual([]);
	});

	it('defaultEditForm 返空对象', () => {
		const f = defaultEditForm();
		expect(Object.keys(f)).toHaveLength(0);
	});

	it('defaultEditForm 每次调用返新对象', () => {
		const a = defaultEditForm();
		const b = defaultEditForm();
		expect(a).not.toBe(b);
	});
});
