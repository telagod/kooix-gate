// admin/groups _lib helpers — 0.4.61 抽出
// 来源：admin/groups/+page.svelte 78-100 行

export type StrategyMeta = { label: string; color: string; desc: string };

export const STRATEGIES: Record<string, StrategyMeta> = {
	priority: { label: '优先级', color: 'blue', desc: '按优先级选择，总是使用优先级最高的可用渠道' },
	weighted_random: { label: '加权随机', color: 'green', desc: '按权重随机分配，权重越高被选中概率越大' },
	round_robin: { label: '轮询', color: 'purple', desc: '轮询分配，依次使用每个渠道' },
	least_conn: { label: '最少连接', color: 'orange', desc: '优先使用当前并发最少的渠道' },
	least_latency: { label: '最低延迟', color: 'yellow', desc: '优先使用平均响应最快的渠道' }
};

export const PROVIDER_COLOR =
	'bg-zinc-200 text-zinc-700 dark:bg-zinc-600 dark:text-zinc-200';

export function strategyMeta(s: string): StrategyMeta {
	return STRATEGIES[s] ?? { label: s, color: 'gray', desc: '' };
}

export function strategyBadgeClass(_color: string): string {
	return 'bg-zinc-200 text-zinc-700 dark:bg-zinc-600 dark:text-zinc-200';
}

export function capabilityChipClass(_key?: string): string {
	return 'bg-zinc-100 text-zinc-700 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700';
}

export function formatNumber(value: number | null | undefined): string {
	return new Intl.NumberFormat('zh-CN').format(value ?? 0);
}

export function formatPercent(value: number | null | undefined): string {
	return `${((value ?? 0) * 100).toFixed(1)}%`;
}

export function formatCanaryPercent(bps: number | null | undefined): string {
	if (bps === null || bps === undefined) return '关闭';
	return `${(bps / 100).toFixed(bps % 100 === 0 ? 0 : 2)}%`;
}

// ── Canary 对比 helpers ──

import type { CanaryStats } from '$lib/api.js';

export function metricDelta(
	row: CanaryStats,
	baseline: CanaryStats | null,
	field: 'error_rate' | 'avg_latency_ms' | 'avg_cost_micros'
): number | null {
	if (!baseline) return null;
	const current = row[field];
	const base = baseline[field];
	if (current === null || current === undefined || base === null || base === undefined) return null;
	return current - base;
}

export function weightedBaseline(rows: CanaryStats[]): CanaryStats | null {
	if (rows.length === 0) return null;
	const totalRequests = rows.reduce((sum, row) => sum + row.requests, 0);
	const weighted = (field: 'error_rate' | 'avg_latency_ms' | 'avg_cost_micros'): number | null => {
		const values = rows.filter((row) => row[field] !== null && row[field] !== undefined);
		if (values.length === 0) return null;
		if (totalRequests > 0) {
			const weightedRequests = values.reduce((sum, row) => sum + row.requests, 0);
			if (weightedRequests > 0) {
				return values.reduce((sum, row) => sum + (row[field] ?? 0) * row.requests, 0) / weightedRequests;
			}
		}
		return values.reduce((sum, row) => sum + (row[field] ?? 0), 0) / values.length;
	};
	return {
		channel_id: 'baseline',
		channel_code: 'baseline',
		channel_name: 'Baseline',
		provider_type: 'baseline',
		canary_percent_bps: null,
		is_canary: false,
		requests: totalRequests,
		error_rate: weighted('error_rate') ?? 0,
		avg_latency_ms: weighted('avg_latency_ms'),
		avg_cost_micros: weighted('avg_cost_micros')
	};
}

export function formatMaybeMs(value: number | null | undefined): string {
	return value === null || value === undefined ? '—' : `${Math.round(value)}ms`;
}

export function formatMaybeMicros(value: number | null | undefined): string {
	return value === null || value === undefined
		? '—'
		: `${Math.round(value).toLocaleString('zh-CN')}µ`;
}

export function formatSignedPercentDelta(delta: number | null): string {
	if (delta === null) return '—';
	const value = delta * 100;
	return `${value >= 0 ? '+' : ''}${value.toFixed(1)}pp`;
}

export function formatSignedNumberDelta(delta: number | null, suffix = ''): string {
	if (delta === null) return '—';
	return `${delta >= 0 ? '+' : ''}${Math.round(delta).toLocaleString('zh-CN')}${suffix}`;
}
