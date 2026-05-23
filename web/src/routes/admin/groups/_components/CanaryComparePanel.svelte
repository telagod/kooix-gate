<script lang="ts">
	// admin/groups/_components/CanaryComparePanel.svelte — 0.4.63 抽出
	// 父：admin/groups/+page.svelte 617-682 行 canary 对比段
	import type { CanaryStats, FallbackStats } from '$lib/api.js';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import {
		formatNumber,
		formatPercent,
		formatCanaryPercent,
		formatMaybeMs,
		formatMaybeMicros,
		formatSignedPercentDelta,
		formatSignedNumberDelta,
		metricDelta
	} from '../_lib/helpers';

	type Props = {
		canary: CanaryStats[];
		canaryOnly: CanaryStats[];
		baseline: CanaryStats | null;
		stats: FallbackStats | null | undefined;
	};

	let { canary, canaryOnly, baseline, stats }: Props = $props();
</script>

{#if canary.length > 0}
	<div class="p-5 border-t border-zinc-200 dark:border-zinc-700">
		<div class="mb-4 flex items-start justify-between gap-4">
			<div>
				<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300">Canary 对比</h3>
				<p class="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
					{stats?.window_hours ?? 24}h 窗口，按 request_events 比较错误率 / 延迟 / 平均成本
				</p>
			</div>
			<span class="rounded-lg border border-zinc-200 bg-zinc-50 px-2.5 py-1 text-xs font-medium text-zinc-600 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
				{canaryOnly.length} canary
			</span>
		</div>

		{#if canaryOnly.length === 0}
			<p class="rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-600 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
				暂无 Canary binding；编辑渠道后把 Canary 设置为 1%-5% 即可开始小流量验证。
			</p>
		{:else}
			<DataTable class="mb-0">
				{#snippet head()}
					<tr>
						<th class={dataTemplate.th}>渠道</th>
						<th class={dataTemplate.th}>流量</th>
						<th class={cn(dataTemplate.th, 'text-right')}>请求</th>
						<th class={cn(dataTemplate.th, 'text-right')}>错误率</th>
						<th class={cn(dataTemplate.th, 'text-right')}>延迟</th>
						<th class={cn(dataTemplate.th, 'text-right')}>平均成本</th>
					</tr>
				{/snippet}

				{#each canaryOnly as row (row.channel_id)}
					<tr class={dataTemplate.row}>
						<td class={dataTemplate.tdStrong}>
							<div class="font-medium">{row.channel_name}</div>
							<div class="text-xs text-zinc-600 dark:text-zinc-400">{row.channel_code}</div>
						</td>
						<td class={dataTemplate.td}>
							<span class="rounded border border-amber-200 bg-amber-50 px-2 py-0.5 font-mono text-xs font-medium text-amber-700 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-300">{formatCanaryPercent(row.canary_percent_bps)}</span>
						</td>
						<td class={cn(dataTemplate.tdMono, 'text-right')}>{formatNumber(row.requests)}</td>
						<td class={cn(dataTemplate.td, 'text-right')}>
							<div class="font-mono text-zinc-900 dark:text-zinc-100">{formatPercent(row.error_rate)}</div>
							<div class="font-mono text-[11px] text-zinc-500 dark:text-zinc-400">{formatSignedPercentDelta(metricDelta(row, baseline, 'error_rate'))}</div>
						</td>
						<td class={cn(dataTemplate.td, 'text-right')}>
							<div class="font-mono text-zinc-900 dark:text-zinc-100">{formatMaybeMs(row.avg_latency_ms)}</div>
							<div class="font-mono text-[11px] text-zinc-500 dark:text-zinc-400">{formatSignedNumberDelta(metricDelta(row, baseline, 'avg_latency_ms'), 'ms')}</div>
						</td>
						<td class={cn(dataTemplate.td, 'text-right')}>
							<div class="font-mono text-zinc-900 dark:text-zinc-100">{formatMaybeMicros(row.avg_cost_micros)}</div>
							<div class="font-mono text-[11px] text-zinc-500 dark:text-zinc-400">{formatSignedNumberDelta(metricDelta(row, baseline, 'avg_cost_micros'), 'µ')}</div>
						</td>
					</tr>
				{/each}
			</DataTable>
			<p class="mt-3 text-xs text-zinc-500 dark:text-zinc-400">
				下方小字为相对 baseline 的差值；负值代表错误率 / 延迟 / 成本低于 baseline。
			</p>
		{/if}
	</div>
{/if}
