// 0.4.144（按 playground M1.5 路线，followup §1）：capability matrix store。
// 第一次进 playground / channel drawer 时拉 /v1/admin/providers/capabilities
// 缓存到 module-level，后续读 cached。
//
// 不接 svelte runes（lib/stores 是 plain TS，被 component 包到 $state）。

import {
	listProviderCapabilities,
	type ProviderCapabilityEntry
} from '$lib/api';

let cached: ProviderCapabilityEntry[] | null = null;
let inflight: Promise<ProviderCapabilityEntry[]> | null = null;

/**
 * 拉 capability 矩阵；module-level 缓存防 N+1 调用。
 * 同一时刻并发调用会合并到 1 次 fetch（避免雷暴）。
 */
export async function getProviderCapabilities(): Promise<ProviderCapabilityEntry[]> {
	if (cached) return cached;
	if (inflight) return inflight;
	inflight = listProviderCapabilities()
		.then((rows) => {
			cached = rows;
			inflight = null;
			return rows;
		})
		.catch((err) => {
			inflight = null;
			throw err;
		});
	return inflight;
}

/** 仅测试用：清掉 module-level cache */
export function _resetProviderCapabilitiesCache(): void {
	cached = null;
	inflight = null;
}

/**
 * 按 provider id（或 plugin:preset_name）查 capabilities。
 * 未找到返 undefined。
 */
export function findCapability(
	rows: ProviderCapabilityEntry[],
	id: string
): ProviderCapabilityEntry | undefined {
	return rows.find((r) => r.id === id);
}
