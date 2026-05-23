<script lang="ts">
	// admin/groups/_components/FallbackChainPanel.svelte — 0.4.62 抽出
	// 父：admin/groups/+page.svelte 688-762 行 fallback chain 面板
	import { AlertTriangle, ChevronRight } from 'lucide-svelte';
	import { shortId } from '$lib/id.js';
	import type { FallbackStats, FallbackChainNode } from '$lib/api.js';
	import { strategyMeta, formatNumber, formatPercent } from '../_lib/helpers';

	type Props = {
		chain: FallbackChainNode[];
		stats: FallbackStats | null | undefined;
		selectedId: string | null;
	};

	let { chain, stats, selectedId }: Props = $props();
</script>

<div class="p-5">
	<div class="mb-4 flex items-start justify-between gap-4">
		<div>
			<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300">回退链路</h3>
			<p class="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
				{stats?.window_hours ?? 24}h 窗口，命中率按 request_events.group_id 统计
			</p>
		</div>
		{#if stats?.has_cycle}
			<div class="inline-flex items-center gap-1.5 rounded-lg border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-medium text-amber-700 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-300">
				<AlertTriangle class="h-3.5 w-3.5" />
				<span>检测到循环 {stats.cycle_at ? shortId(stats.cycle_at) : ''}</span>
			</div>
		{/if}
	</div>

	<div class="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-4">
		<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
			<div class="text-xs text-zinc-600 dark:text-zinc-400">总请求</div>
			<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatNumber(stats?.total_requests)}</div>
		</div>
		<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
			<div class="text-xs text-zinc-600 dark:text-zinc-400">Primary</div>
			<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatNumber(stats?.primary_requests)}</div>
		</div>
		<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
			<div class="text-xs text-zinc-600 dark:text-zinc-400">Fallback</div>
			<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatNumber(stats?.fallback_requests)}</div>
		</div>
		<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
			<div class="text-xs text-zinc-600 dark:text-zinc-400">命中率</div>
			<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatPercent(stats?.fallback_hit_rate)}</div>
		</div>
	</div>

	<div class="flex items-stretch gap-2 overflow-x-auto pb-2">
		{#each chain as node, i}
			<div class="flex items-center gap-2 flex-shrink-0">
				<div class="min-w-44 px-3 py-2 rounded-lg border text-sm
					{node.id === selectedId
						? 'border-zinc-900 dark:border-zinc-300 bg-zinc-100 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 font-medium'
						: 'border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300'}">
					<div class="flex items-center justify-between gap-3">
						<div class="truncate">{node.name}</div>
						<span class="rounded bg-zinc-200 px-1.5 py-0.5 text-[10px] font-medium text-zinc-700 dark:bg-zinc-600 dark:text-zinc-200">
							{node.is_fallback ? 'Fallback' : 'Primary'}
						</span>
					</div>
					<div class="mt-1 text-xs text-zinc-600 dark:text-zinc-300">{strategyMeta(node.strategy).label} · {node.channel_count} 渠道</div>
					<div class="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-700">
						<div class="h-full rounded-full bg-zinc-900 dark:bg-zinc-100" style={`width: ${Math.min(100, Math.max(0, node.share * 100))}%`}></div>
					</div>
					<div class="mt-1 flex justify-between font-mono text-[11px] text-zinc-600 dark:text-zinc-400">
						<span>{formatNumber(node.requests)} req</span>
						<span>{formatPercent(node.share)}</span>
					</div>
					{#if !node.enabled}
						<div class="mt-1 inline-flex items-center gap-1 text-[11px] text-amber-700 dark:text-amber-300">
							<AlertTriangle class="h-3 w-3" /> disabled
						</div>
					{/if}
				</div>
				{#if i < chain.length - 1}
					<ChevronRight class="w-4 h-4 text-zinc-400 flex-shrink-0" />
				{/if}
			</div>
		{/each}
		{#if chain.length === 1}
			<ChevronRight class="w-4 h-4 text-zinc-400 flex-shrink-0" />
			<span class="text-zinc-600 dark:text-zinc-300 text-sm">∅</span>
		{/if}
	</div>
</div>
