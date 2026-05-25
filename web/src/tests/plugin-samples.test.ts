// 0.4.77（product-review B2 step 2）：plugin-samples 的格式 sanity 检查。
// 这些示例字符串会被用户复制到 channel manifest，必须保证至少 JSON
// 可解析、SSE 样例符合 `event:`/`data:` 结构。

import { describe, expect, it } from 'vitest';
import {
	PLUGIN_MANIFEST_EXAMPLE,
	PRIVATE_PLUGIN_MANIFEST_EXAMPLE,
	PLUGIN_REPLAY_SAMPLE,
	RESPONSE_SAMPLE_PLACEHOLDER,
	PROBE_BODY_PLACEHOLDER,
	PLUGIN_BUILDER_STEPS,
} from '../routes/channels/_lib/plugin-samples';

describe('plugin-samples', () => {
	it('PLUGIN_MANIFEST_EXAMPLE 是合法 JSON', () => {
		expect(() => JSON.parse(PLUGIN_MANIFEST_EXAMPLE)).not.toThrow();
		const parsed = JSON.parse(PLUGIN_MANIFEST_EXAMPLE);
		expect(parsed.plugin).toBeDefined();
	});

	it('PRIVATE_PLUGIN_MANIFEST_EXAMPLE 是合法 JSON 且含 stream / response 块', () => {
		const parsed = JSON.parse(PRIVATE_PLUGIN_MANIFEST_EXAMPLE);
		expect(parsed.plugin).toBeDefined();
		expect(parsed.plugin.request).toBeDefined();
		expect(parsed.plugin.response).toBeDefined();
		expect(parsed.plugin.stream).toBeDefined();
	});

	it('RESPONSE_SAMPLE_PLACEHOLDER 是合法 JSON', () => {
		const parsed = JSON.parse(RESPONSE_SAMPLE_PLACEHOLDER);
		expect(parsed.result).toBeDefined();
		expect(parsed.usage).toBeDefined();
	});

	it('PROBE_BODY_PLACEHOLDER 含 model 占位符', () => {
		expect(PROBE_BODY_PLACEHOLDER).toContain('{{model}}');
		expect(PROBE_BODY_PLACEHOLDER).toContain('messages');
	});

	it('PLUGIN_REPLAY_SAMPLE 含 SSE 必要标记', () => {
		expect(PLUGIN_REPLAY_SAMPLE).toContain('event:');
		expect(PLUGIN_REPLAY_SAMPLE).toContain('data:');
		expect(PLUGIN_REPLAY_SAMPLE).toContain('payload');
	});

	it('PLUGIN_BUILDER_STEPS 长度为 7 且顺序固定', () => {
		expect(PLUGIN_BUILDER_STEPS.length).toBe(7);
		expect(PLUGIN_BUILDER_STEPS[0]).toBe('Preset');
		expect(PLUGIN_BUILDER_STEPS[PLUGIN_BUILDER_STEPS.length - 1]).toBe('Save');
	});
});
