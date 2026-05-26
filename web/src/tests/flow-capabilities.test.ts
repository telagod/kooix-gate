// 0.4.163（第四刀 #3 step 5）：playground capability gating helper 单元测试
import { describe, expect, it } from 'vitest';
import {
	isModalitySupported,
	nodeCapabilityKey,
	nodeRequiresCapability,
	supportingProviders
} from '$lib/flow/capabilities';
import type { ProviderCapabilityEntry } from '$lib/api';

function row(
	id: string,
	caps: Partial<ProviderCapabilityEntry['capabilities']>
): ProviderCapabilityEntry {
	return {
		id,
		name: id,
		capabilities: {
			chat: false,
			streaming: false,
			tools: false,
			embeddings: false,
			image: false,
			audio: false,
			vision: false,
			json_mode: false,
			batch: false,
			...caps
		},
		base_url_hint: null,
		kind: 'compile_time' as const
	};
}

describe('flow capabilities helpers', () => {
	describe('nodeRequiresCapability', () => {
		it('input / preview 类节点返 false', () => {
			expect(nodeRequiresCapability('textInput')).toBe(false);
			expect(nodeRequiresCapability('imageUpload')).toBe(false);
			expect(nodeRequiresCapability('audioUpload')).toBe(false);
			expect(nodeRequiresCapability('preview')).toBe(false);
		});

		it('AI 类节点返 true', () => {
			expect(nodeRequiresCapability('llmChat')).toBe(true);
			expect(nodeRequiresCapability('imageGen')).toBe(true);
			expect(nodeRequiresCapability('tts')).toBe(true);
			expect(nodeRequiresCapability('stt')).toBe(true);
		});
	});

	describe('nodeCapabilityKey', () => {
		it('映射符合 NODE_CAPABILITY_KEY 表', () => {
			expect(nodeCapabilityKey('llmChat')).toBe('chat');
			expect(nodeCapabilityKey('imageGen')).toBe('image');
			expect(nodeCapabilityKey('tts')).toBe('audio');
			expect(nodeCapabilityKey('stt')).toBe('audio');
			expect(nodeCapabilityKey('textInput')).toBeNull();
			expect(nodeCapabilityKey('preview')).toBeNull();
		});
	});

	describe('isModalitySupported', () => {
		it('rows null（capability 未加载）→ true，不阻塞用户', () => {
			expect(isModalitySupported(null, 'llmChat')).toBe(true);
			expect(isModalitySupported(null, 'imageGen')).toBe(true);
		});

		it('rows 空数组 → 视同未加载，返 true', () => {
			expect(isModalitySupported([], 'llmChat')).toBe(true);
		});

		it('input/preview 节点恒返 true', () => {
			const rows = [row('openai', { chat: true })];
			expect(isModalitySupported(rows, 'textInput')).toBe(true);
			expect(isModalitySupported(rows, 'preview')).toBe(true);
		});

		it('AI 节点：至少 1 个 provider 支持 → true', () => {
			const rows = [
				row('openai', { chat: true }),
				row('plugin:cohere', { chat: false })
			];
			expect(isModalitySupported(rows, 'llmChat')).toBe(true);
		});

		it('AI 节点：所有 provider 都不支持 → false', () => {
			const rows = [
				row('openai', { chat: false }),
				row('plugin:cohere', { chat: false })
			];
			expect(isModalitySupported(rows, 'llmChat')).toBe(false);
		});

		it('TTS 和 STT 都看 audio flag', () => {
			const rows = [row('openai', { audio: true })];
			expect(isModalitySupported(rows, 'tts')).toBe(true);
			expect(isModalitySupported(rows, 'stt')).toBe(true);

			const noAudio = [row('plugin:cohere', { chat: true, audio: false })];
			expect(isModalitySupported(noAudio, 'tts')).toBe(false);
			expect(isModalitySupported(noAudio, 'stt')).toBe(false);
		});

		it('imageGen 看 image flag', () => {
			expect(
				isModalitySupported([row('openai', { image: true })], 'imageGen')
			).toBe(true);
			expect(
				isModalitySupported([row('openai', { image: false })], 'imageGen')
			).toBe(false);
		});
	});

	describe('supportingProviders', () => {
		it('返支持该 modality 的 provider id 列表', () => {
			const rows = [
				row('openai', { chat: true }),
				row('anthropic', { chat: true }),
				row('plugin:cohere', { chat: false })
			];
			expect(supportingProviders(rows, 'llmChat')).toEqual(['openai', 'anthropic']);
		});

		it('null rows / input 节点 → 空数组', () => {
			expect(supportingProviders(null, 'llmChat')).toEqual([]);
			expect(supportingProviders([row('openai', { chat: true })], 'textInput')).toEqual([]);
		});

		it('audio flag 同时被 tts/stt 用', () => {
			const rows = [
				row('openai', { audio: true, image: false }),
				row('plugin:cohere', { audio: false })
			];
			expect(supportingProviders(rows, 'tts')).toEqual(['openai']);
			expect(supportingProviders(rows, 'stt')).toEqual(['openai']);
		});
	});
});
