<!-- /usage — 用量仪表盘：stat cards + 双 SVG 折线图 + 维度切换 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { getUsage, getMe } from '$lib/api.js';
	import type { UsageResponse } from '$lib/api.js';
	import Card from '$lib/components/ui/Card.svelte';
	import Stat from '$lib/components/Stat.svelte';
	import FilterPills from '$lib/components/ui/FilterPills.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { BarChart3 } from 'lucide-svelte';

	let usage = $state<UsageResponse | null>(null);
	let loading = $state(true);
	let error = $state('');
	let range = $state<'7d' | '30d'>('7d');
	let groupBy = $state<'day' | 'model' | 'channel'>('day');
	let chartMode = $state<'cost' | 'tokens' | 'requests'>('cost');
	let currentOrg = $state<string | null>(null);

	const rangeOptions = [
		{ value: '7d', label: '7 天' },
		{ value: '30d', label: '30 天' }
	];

	const groupByOptions = [
		{ value: 'day', label: '按天' },
		{ value: 'model', label: '按模型' },
		{ value: 'channel', label: '按渠道' }
	];

	const chartModeOptions = [
		{ value: 'cost', label: '花费' },
		{ value: 'tokens', label: 'Tokens 用量' }
	];

	onMount(async () => {
		try {
			const me = await getMe();
			currentOrg = me.current_org ?? me.orgs[0] ?? null;
			if (!currentOrg && !me.is_platform_admin) {
				error = '当前账号没有加入任何组织，无法查看用量';
				loading = false;
				return;
			}
		} catch (err: any) {
			error = err?.message ?? '加载身份失败';
			loading = false;
			return;
		}
		await load();
		initialized = true;
	});

	async function load() {
		loading = true;
		error = '';
		try {
			usage = await getUsage(currentOrg, range, groupBy);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	let initialized = $state(false);
	$effect(() => {
		range; groupBy;
		if (initialized) load();
	});

	function formatCost(n: number): string { return `$${n.toFixed(4)}`; }
	function formatNum(n: number): string { return n.toLocaleString('en-US'); }

	const W = 720;
	const H = 220;
	const PAD = { top: 16, right: 24, bottom: 32, left: 56 };

	let chartData = $derived.by(() => {
		if (!usage || usage.series.length === 0) return null;
		const series = usage.series;
		const innerW = W - PAD.left - PAD.right;
		const innerH = H - PAD.top - PAD.bottom;

		const getVal = (p: typeof series[0]) => {
			if (chartMode === 'tokens') return p.tokens_in + p.tokens_out;
			return p.cost_usd;
		};
		const values = series.map(getVal);
		const maxVal = Math.max(0.0001, ...values);
		const n = series.length;

		const x = (i: number) => PAD.left + (n === 1 ? innerW / 2 : (i * innerW) / (n - 1));
		const y = (v: number) => PAD.top + innerH - (v / maxVal) * innerH;

		const path = series
			.map((p, i) => `${i === 0 ? 'M' : 'L'} ${x(i).toFixed(1)} ${y(getVal(p)).toFixed(1)}`)
			.join(' ');

		const area = `${path} L ${x(n - 1).toFixed(1)} ${PAD.top + innerH} L ${x(0).toFixed(1)} ${PAD.top + innerH} Z`;

		const points = series.map((p, i) => ({
			x: x(i),
			y: y(getVal(p)),
			value: getVal(p),
			key: p.key,
			cost: p.cost_usd,
			tokens_in: p.tokens_in,
			tokens_out: p.tokens_out
		}));

		const yTicks = [0, 0.33, 0.66, 1].map((t) => ({
			y: PAD.top + innerH - t * innerH,
			value: t * maxVal
		}));

		const formatTickVal = (v: number) => {
			if (chartMode === 'tokens') {
				if (v >= 1_000_000) return `${(v / 1_000_000).toFixed(1)}M`;
				if (v >= 1000) return `${(v / 1000).toFixed(0)}K`;
				return String(Math.round(v));
			}
			return `$${v.toFixed(3)}`;
		};

		return { path, area, points, yTicks, maxVal, formatTickVal };
	});

	// Bar chart for model/channel group_by
	let isBarChart = $derived(groupBy !== 'day');

	let barData = $derived.by(() => {
		if (!usage || usage.series.length === 0 || !isBarChart) return null;
		const series = usage.series.slice(0, 10);
		const getVal = (p: typeof series[0]) => chartMode === 'tokens' ? p.tokens_in + p.tokens_out : p.cost_usd;
		const maxVal = Math.max(0.0001, ...series.map(getVal));
		return series.map(p => ({
			key: p.key.length > 20 ? p.key.slice(0, 18) + '…' : p.key,
			value: getVal(p),
			pct: getVal(p) / maxVal,
			cost: p.cost_usd,
			tokens_in: p.tokens_in,
			tokens_out: p.tokens_out
		}));
	});
</script>

<PageShell
	title="用量仪表盘"
	description={currentOrg ? `Org 用量趋势、模型/渠道分布与 token 成本汇总 · ${currentOrg}` : '全平台用量趋势、模型/渠道分布与 token 成本汇总'}
	icon={BarChart3}
	max="wide"
>
	<DataToolbar>
		{#snippet controls()}
			<FilterPills bind:value={groupBy} options={groupByOptions} />
			<FilterPills bind:value={range} options={rangeOptions} />
		{/snippet}

		{#snippet badges()}
			<span class="rounded-full border border-zinc-200 px-2.5 py-1 text-xs font-medium text-zinc-600 dark:border-zinc-800 dark:text-zinc-300">
				{groupBy === 'day' ? '每日趋势' : groupBy === 'model' ? '模型分布' : '渠道分布'}
			</span>
			<span class="rounded-full border border-zinc-200 px-2.5 py-1 text-xs font-medium text-zinc-600 dark:border-zinc-800 dark:text-zinc-300">
				{range}
			</span>
		{/snippet}
	</DataToolbar>

	{#if loading}
		<div class="space-y-4">
			<div class="grid grid-cols-1 md:grid-cols-3 gap-4">
				{#each Array(3) as _}
					<div class="h-28 bg-zinc-200 dark:bg-zinc-700 rounded-lg animate-pulse"></div>
				{/each}
			</div>
			<div class="h-64 bg-zinc-200 dark:bg-zinc-700 rounded-lg animate-pulse"></div>
		</div>
	{:else if error}
		<StatePanel title="用量加载失败" description={error} icon={BarChart3} variant="danger" />
	{:else if usage}
		<div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
			<Stat title="总花费" value={formatCost(usage.total_cost_usd)} subtitle="USD · {usage.range}" />
			<Stat title="Input tokens 输入" value={formatNum(usage.total_tokens_in)} subtitle="prompt 输入累计" />
			<Stat title="Output tokens 输出" value={formatNum(usage.total_tokens_out)} subtitle="completion 输出累计" />
		</div>

		<Card class="p-5">
			<div class="flex items-center justify-between mb-3">
				<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">
					{#if groupBy === 'day'}每日趋势{:else if groupBy === 'model'}模型分布{:else}渠道分布{/if}
				</h2>
				<div class="flex items-center gap-3">
					<FilterPills bind:value={chartMode} options={chartModeOptions} />
					<p class="text-xs text-zinc-500 dark:text-zinc-400 font-mono">{usage.series.length} 个 buckets</p>
				</div>
			</div>

			{#if usage.series.length === 0}
				<p class="text-sm text-zinc-600 dark:text-zinc-300 py-12 text-center">此区间无用量记录</p>
			{:else if isBarChart && barData}
				<!-- Horizontal bar chart for model/channel -->
				<div class="space-y-2">
					{#each barData as bar}
						<div class="flex items-center gap-3">
							<span class="w-36 text-xs font-mono text-zinc-600 dark:text-zinc-400 text-right truncate" title={bar.key}>{bar.key}</span>
							<div class="flex-1 h-6 bg-zinc-100 dark:bg-zinc-800 rounded overflow-hidden">
								<div
									class="h-full rounded bg-zinc-900 dark:bg-zinc-300 transition-all flex items-center px-2"
									style="width: {Math.max(2, bar.pct * 100).toFixed(1)}%"
								>
									{#if bar.pct > 0.15}
										<span class="text-[10px] font-mono text-white dark:text-zinc-900 truncate">
											{chartMode === 'tokens' ? formatNum(bar.value) : formatCost(bar.value)}
										</span>
									{/if}
								</div>
							</div>
							{#if bar.pct <= 0.15}
								<span class="text-[10px] font-mono text-zinc-500 dark:text-zinc-400 w-16">
									{chartMode === 'tokens' ? formatNum(bar.value) : formatCost(bar.value)}
								</span>
							{/if}
						</div>
					{/each}
				</div>
			{:else if chartData}
				<!-- Line chart for day group_by -->
				<svg viewBox="0 0 {W} {H}" class="w-full h-auto">
					{#each chartData.yTicks as t}
						<line x1={PAD.left} y1={t.y} x2={W - PAD.right} y2={t.y} class="stroke-zinc-200 dark:stroke-zinc-700" stroke-dasharray="3,3" />
						<text x={PAD.left - 8} y={t.y + 3} text-anchor="end" class="fill-zinc-400 dark:fill-zinc-500 text-[10px] font-mono">
							{chartData.formatTickVal(t.value)}
						</text>
					{/each}
					<path d={chartData.area} class="fill-zinc-900 dark:fill-zinc-300" fill-opacity="0.06" />
					<path d={chartData.path} fill="none" class="stroke-zinc-900 dark:stroke-zinc-300" stroke-width="2" stroke-linejoin="round" stroke-linecap="round" />
					{#each chartData.points as p}
						<circle cx={p.x} cy={p.y} r="3.5" class="fill-zinc-900 dark:fill-zinc-300" />
						<title>{p.key}: {chartMode === 'tokens' ? formatNum(p.value) : formatCost(p.value)}</title>
					{/each}
					{#each chartData.points as p, i}
						{#if chartData.points.length <= 7 || i % Math.ceil(chartData.points.length / 7) === 0 || i === chartData.points.length - 1}
							<text x={p.x} y={H - 8} text-anchor="middle" class="fill-zinc-500 dark:fill-zinc-400 text-[10px] font-mono">
								{p.key.slice(5)}
							</text>
						{/if}
					{/each}
				</svg>
			{/if}
		</Card>

		<p class="mt-4 text-xs text-zinc-500 dark:text-zinc-400">
			Org：<span class="font-mono">{currentOrg ?? '全平台 (SuperAdmin)'}</span> · 数据范围
			{usage.from.slice(0, 10)} → {usage.to.slice(0, 10)}
		</p>
	{/if}
</PageShell>
