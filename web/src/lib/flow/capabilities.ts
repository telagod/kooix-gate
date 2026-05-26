// 0.4.159（第四刀 #3 step 1）：playground 节点 capability gating 共享逻辑。
//
// 把 FlowNodeKind 映射到 ProviderCapabilities 字段，给 FlowEditor + 各 node
// 共用判定逻辑：当前所有已知 provider/plugin preset 是否至少有一个支持该
// modality。无可用 channel 时该节点应禁用 / 显示 placeholder。
//
// 注意：当前 ProviderCapabilities 不区分 stt/tts，统一看 audio flag；
// imageGen 看 image flag。后端 0.5.x 拆 audio_in/audio_out 时再细化。

import type { ProviderCapabilityEntry, ProviderCapabilities } from '$lib/api';
import type { FlowNodeKind } from '$lib/flow/types';

/** node kind → 需要的 capability key（input/output 类节点返 null 表示无需 capability） */
export type CapabilityKey = keyof ProviderCapabilities;

const NODE_CAPABILITY_KEY: Record<FlowNodeKind, CapabilityKey | null> = {
	textInput: null,
	imageUpload: null,
	audioUpload: null,
	preview: null,
	llmChat: 'chat',
	imageGen: 'image',
	tts: 'audio',
	stt: 'audio',
};

/** 节点是否需要 channel 端 capability（input/preview 类返 false） */
export function nodeRequiresCapability(kind: FlowNodeKind): boolean {
	return NODE_CAPABILITY_KEY[kind] !== null;
}

/** 拿到节点所需的 capability key（input/preview 返 null） */
export function nodeCapabilityKey(kind: FlowNodeKind): CapabilityKey | null {
	return NODE_CAPABILITY_KEY[kind];
}

/**
 * 检查给定 capability 矩阵中至少有一个 provider/plugin preset 支持该 modality。
 * - 无 rows（capability 未加载）→ true（不阻塞用户）
 * - rows 加载完成但无 provider 支持 → false（禁用节点）
 */
export function isModalitySupported(
	rows: ProviderCapabilityEntry[] | null,
	kind: FlowNodeKind,
): boolean {
	const key = NODE_CAPABILITY_KEY[kind];
	if (key === null) return true;
	if (!rows || rows.length === 0) return true;
	return rows.some((r) => r.capabilities[key]);
}

/**
 * 列出所有支持给定 modality 的 provider id，供 node UI 显示「可用 provider」hint。
 */
export function supportingProviders(
	rows: ProviderCapabilityEntry[] | null,
	kind: FlowNodeKind,
): string[] {
	const key = NODE_CAPABILITY_KEY[kind];
	if (key === null || !rows) return [];
	return rows.filter((r) => r.capabilities[key]).map((r) => r.id);
}
