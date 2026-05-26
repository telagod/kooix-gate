// 0.4.145：capability store + gating helper sanity tests
import { afterEach, describe, expect, it, vi } from 'vitest';
import {
	_resetProviderCapabilitiesCache,
	findCapability,
	getProviderCapabilities
} from '$lib/stores/provider-capabilities';
import * as api from '$lib/api';

describe('provider-capabilities store', () => {
	afterEach(() => {
		_resetProviderCapabilitiesCache();
		vi.restoreAllMocks();
	});

	it('findCapability 按 id 查到对应 entry', () => {
		const rows = [
			{
				id: 'openai',
				name: 'openai',
				capabilities: { chat: true } as never,
				base_url_hint: 'https://api.openai.com',
				kind: 'compile_time' as const
			},
			{
				id: 'plugin:google_gemini',
				name: 'google_gemini',
				capabilities: { chat: true } as never,
				base_url_hint: null,
				kind: 'plugin_preset' as const
			}
		];
		expect(findCapability(rows, 'openai')?.kind).toBe('compile_time');
		expect(findCapability(rows, 'plugin:google_gemini')?.kind).toBe('plugin_preset');
		expect(findCapability(rows, 'unknown')).toBeUndefined();
	});

	it('getProviderCapabilities 缓存 — 第二次调用不再 fetch', async () => {
		const mockRows = [
			{
				id: 'openai',
				name: 'openai',
				capabilities: { chat: true } as never,
				base_url_hint: null,
				kind: 'compile_time' as const
			}
		];
		const spy = vi
			.spyOn(api, 'listProviderCapabilities')
			.mockResolvedValue(mockRows);

		const a = await getProviderCapabilities();
		const b = await getProviderCapabilities();
		expect(a).toBe(b); // 同 reference
		expect(spy).toHaveBeenCalledTimes(1); // 仅 1 次 fetch
	});

	it('getProviderCapabilities 并发合并 — 同时调用只 fetch 1 次', async () => {
		const spy = vi
			.spyOn(api, 'listProviderCapabilities')
			.mockImplementation(
				() =>
					new Promise((resolve) => setTimeout(() => resolve([]), 10))
			);

		const [a, b, c] = await Promise.all([
			getProviderCapabilities(),
			getProviderCapabilities(),
			getProviderCapabilities()
		]);
		expect(a).toBe(b);
		expect(b).toBe(c);
		expect(spy).toHaveBeenCalledTimes(1); // 防雷暴：仅 1 次
	});
});
