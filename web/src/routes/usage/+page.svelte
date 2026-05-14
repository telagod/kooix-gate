<!-- /usage — 用量仪表盘：三 stat cards + SVG 折线图 + 范围切换 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getUsage, getMe } from '$lib/api.js';
	import type { UsageResponse } from '$lib/api.js';
	import { getAccessToken, clearTokens } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Stat from '$lib/components/Stat.svelte';

	let usage = $state<UsageResponse | null>(null);
	let loading = $state(true);
	let error = $state('');
	let range = $state<'7d' | '30d'>('7d');
	let currentOrg = $state<string | null>(null);

	onMount(async () => {
		if (!getAccessToken()) {
			goto('/login');
			return;
		}
		try {
			const me = await getMe();
			currentOrg = me.current_org ?? me.orgs[0] ?? null;
			if (!currentOrg && !me.is_platform_admin) {
				error = '当前账号没有加入任何组织，无法查看用量';
				loading = false;
				return;
			}
		} catch (err: any) {
			if (err?.status === 401) {
				clearTokens();
				goto('/login');
				return;
			}
			error = err?.message ?? '加载身份失败';
			loading = false;
			return;
		}
		await load();
	});

	async function load() {
		loading = true;
		error = '';
		try {
			usage = await getUsage(currentOrg, range);
		} catch (err: any) {
			if (err?.status === 401) {
				clearTokens();
				goto('/login');
				return;
			}
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	async function switchRange(next: '7d' | '30d') {
		if (range === next) return;
		range = next;
		await load();
	}

	function formatCost(n: number): string {
		return `$${n.toFixed(4)}`;
	}

	function formatNum(n: number): string {
		return n.toLocaleString('en-US');
	}

	// SVG 折线图配置
	const W = 720;
	const H = 220;
	const PAD = { top: 16, right: 24, bottom: 32, left: 56 };

	let chartPaths = $derived.by(() => {
		if (!usage || usage.series.length === 0) return null;
		const series = usage.series;
		const innerW = W - PAD.left - PAD.right;
		const innerH = H - PAD.top - PAD.bottom;

		const costs = series.map((p) => p.cost_usd);
		const maxCost = Math.max(0.0001, ...costs);
		const n = series.length;

		const x = (i: number) =>
			PAD.left + (n === 1 ? innerW / 2 : (i * innerW) / (n - 1));
		const y = (v: number) => PAD.top + innerH - (v / maxCost) * innerH;

		const path = series
			.map((p, i) => `${i === 0 ? 'M' : 'L'} ${x(i).toFixed(1)} ${y(p.cost_usd).toFixed(1)}`)
			.join(' ');

		// area fill：path + 底边封口
		const area = `${path} L ${x(n - 1).toFixed(1)} ${PAD.top + innerH} L ${x(0).toFixed(1)} ${PAD.top + innerH} Z`;

		const points = series.map((p, i) => ({
			x: x(i),
			y: y(p.cost_usd),
			cost: p.cost_usd,
			key: p.key
		}));

		// y 轴 4 个 ticks
		const yTicks = [0, 0.33, 0.66, 1].map((t) => ({
			y: PAD.top + innerH - t * innerH,
			value: t * maxCost
		}));

		return { path, area, points, yTicks, maxCost };
	});
</script>

<div class="max-w-6xl mx-auto p-6">
	<div class="flex items-center justify-between mb-6">
		<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">用量仪表盘</h1>
		<div class="flex gap-2">
			<Button
				variant={range === '7d' ? 'default' : 'outline'}
				size="sm"
				onclick={() => switchRange('7d')}
				disabled={loading}
			>
				最近 7 天
			</Button>
			<Button
				variant={range === '30d' ? 'default' : 'outline'}
				size="sm"
				onclick={() => switchRange('30d')}
				disabled={loading}
			>
				最近 30 天
			</Button>
		</div>
	</div>

	{#if loading}
		<p class="text-zinc-500 dark:text-zinc-400">加载中...</p>
	{:else if error}
		<Card class="p-6">
			<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
		</Card>
	{:else if usage}
		<div class="grid grid-cols-1 md:grid-cols-3 gap-4 mb-6">
			<Stat
				title="总花费"
				value={formatCost(usage.total_cost_usd)}
				subtitle="USD · {usage.range}"
			/>
			<Stat
				title="Input Tokens"
				value={formatNum(usage.total_tokens_in)}
				subtitle="prompt 输入累计"
			/>
			<Stat
				title="Output Tokens"
				value={formatNum(usage.total_tokens_out)}
				subtitle="completion 输出累计"
			/>
		</div>

		<Card class="p-5">
			<div class="flex items-center justify-between mb-3">
				<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">每日花费 (USD)</h2>
				<p class="text-xs text-zinc-400 dark:text-zinc-500 font-mono">{usage.series.length} buckets</p>
			</div>

			{#if !chartPaths}
				<p class="text-sm text-zinc-500 dark:text-zinc-400 py-12 text-center">此区间无用量记录</p>
			{:else}
				<svg viewBox="0 0 {W} {H}" class="w-full h-auto">
					<!-- Y 轴 grid + 标签 -->
					{#each chartPaths.yTicks as t}
						<line
							x1={PAD.left}
							y1={t.y}
							x2={W - PAD.right}
							y2={t.y}
							class="stroke-zinc-200 dark:stroke-zinc-700"
							stroke-dasharray="3,3"
						/>
						<text
							x={PAD.left - 8}
							y={t.y + 3}
							text-anchor="end"
							class="fill-zinc-400 dark:fill-zinc-500 text-[10px] font-mono"
						>
							${t.value.toFixed(3)}
						</text>
					{/each}

					<!-- area fill -->
					<path d={chartPaths.area} class="fill-zinc-900 dark:fill-zinc-300" fill-opacity="0.06" />

					<!-- line -->
					<path
						d={chartPaths.path}
						fill="none"
						class="stroke-zinc-900 dark:stroke-zinc-300"
						stroke-width="2"
						stroke-linejoin="round"
						stroke-linecap="round"
					/>

					<!-- 数据点 -->
					{#each chartPaths.points as p}
						<circle cx={p.x} cy={p.y} r="3.5" class="fill-zinc-900 dark:fill-zinc-300" />
						<title>{p.key}: ${p.cost.toFixed(4)}</title>
					{/each}

					<!-- X 轴日期标签（最多 7 个，超过则间隔抽样） -->
					{#each chartPaths.points as p, i}
						{#if chartPaths.points.length <= 7 || i % Math.ceil(chartPaths.points.length / 7) === 0 || i === chartPaths.points.length - 1}
							<text
								x={p.x}
								y={H - 8}
								text-anchor="middle"
								class="fill-zinc-500 dark:fill-zinc-400 text-[10px] font-mono"
							>
								{p.key.slice(5)}
							</text>
						{/if}
					{/each}
				</svg>
			{/if}
		</Card>

		<p class="mt-4 text-xs text-zinc-400 dark:text-zinc-500">
			Org：<span class="font-mono">{currentOrg ?? '全平台 (SuperAdmin)'}</span> · 数据范围
			{usage.from.slice(0, 10)} → {usage.to.slice(0, 10)}
		</p>
	{/if}
</div>
