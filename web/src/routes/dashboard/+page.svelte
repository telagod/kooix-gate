<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getMe, getUsage, getQuotaAlerts, listKeys, listProjects } from '$lib/api.js';
	import type { MeResult, UsageResponse, QuotaAlert } from '$lib/api.js';
	import Card from '$lib/components/ui/Card.svelte';
	import Stat from '$lib/components/Stat.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import {
		TrendingUp,
		AlertTriangle,
		Key,
		FolderOpen,
		ArrowRight,
		MessageSquare,
		BarChart3
	} from 'lucide-svelte';

	let me = $state<MeResult | null>(null);
	let usage = $state<UsageResponse | null>(null);
	let alerts = $state<QuotaAlert[]>([]);
	let projectCount = $state(0);
	let keyCount = $state(0);
	let loading = $state(true);
	let error = $state('');

	let currentOrg = $derived(me?.current_org ?? me?.orgs?.[0] ?? null);

	onMount(async () => {
		try {
			me = await getMe();
			const org = me.current_org ?? me.orgs[0] ?? null;
			if (!org) { loading = false; return; }

			const [u, a, p] = await Promise.all([
				getUsage(org, '7d').catch(() => null),
				getQuotaAlerts(org).catch(() => []),
				listProjects(org).catch(() => [])
			]);
			usage = u;
			alerts = a;
			projectCount = p.length;

			let keys = 0;
			for (const proj of p.slice(0, 5)) {
				const k = await listKeys(org, proj.id).catch(() => []);
				keys += k.filter((x: any) => !x.revoked).length;
			}
			keyCount = keys;
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	function fmt(n: number): string { return n.toLocaleString('en-US'); }
	function fmtCost(n: number): string { return `$${n.toFixed(4)}`; }

	let exceededAlerts = $derived(alerts.filter(a => a.level === 'exceeded'));
	let approachingAlerts = $derived(alerts.filter(a => a.level === 'approaching'));
</script>

<div class="max-w-7xl mx-auto p-6">
	{#if loading}
		<div class="space-y-4">
			<div class="h-8 w-48 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
			<div class="grid grid-cols-1 md:grid-cols-4 gap-4">
				{#each Array(4) as _}
					<div class="h-28 bg-zinc-200 dark:bg-zinc-700 rounded-lg animate-pulse"></div>
				{/each}
			</div>
		</div>
	{:else if error}
		<Card class="p-6">
			<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
		</Card>
	{:else}
		<div class="flex items-center justify-between mb-6">
			<div>
				<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">总览</h1>
				<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-0.5">
					{#if currentOrg}
						当前组织：<span class="font-mono">{currentOrg.slice(0, 8)}...</span>
					{:else}
						未加入任何组织
					{/if}
				</p>
			</div>
			{#if me?.is_platform_admin}
				<span class="inline-flex items-center gap-1 text-xs bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-400 px-2 py-1 rounded-md font-medium">
					Platform Admin
				</span>
			{/if}
		</div>

		<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
			<Stat
				title="7 天花费"
				value={usage ? fmtCost(usage.total_cost_usd) : '$0.00'}
				subtitle="USD"
			/>
			<Stat
				title="Token 消耗"
				value={usage ? fmt(usage.total_tokens_in + usage.total_tokens_out) : '0'}
				subtitle="7 天 (in+out)"
			/>
			<Stat
				title="项目数"
				value={String(projectCount)}
				subtitle="当前组织"
			/>
			<Stat
				title="活跃 API Key"
				value={String(keyCount)}
				subtitle="前 5 项目"
			/>
		</div>

		<!-- Quota alerts -->
		{#if exceededAlerts.length > 0}
			<Card class="p-4 mb-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20">
				<div class="flex items-center gap-2 mb-2">
					<AlertTriangle size={16} class="text-red-600 dark:text-red-400" />
					<h3 class="text-sm font-semibold text-red-700 dark:text-red-400">配额超限 ({exceededAlerts.length})</h3>
				</div>
				<div class="space-y-1">
					{#each exceededAlerts.slice(0, 3) as a}
						<p class="text-xs text-red-600 dark:text-red-400">
							{a.dimension} · {a.scope_kind}:{a.scope_id.slice(0, 8)}... — {a.percent.toFixed(0)}%
						</p>
					{/each}
				</div>
			</Card>
		{/if}

		{#if approachingAlerts.length > 0}
			<Card class="p-4 mb-6 border-amber-200 dark:border-amber-800 bg-amber-50 dark:bg-amber-900/20">
				<div class="flex items-center gap-2 mb-2">
					<AlertTriangle size={16} class="text-amber-600 dark:text-amber-400" />
					<h3 class="text-sm font-semibold text-amber-700 dark:text-amber-400">配额接近上限 ({approachingAlerts.length})</h3>
				</div>
				<div class="space-y-1">
					{#each approachingAlerts.slice(0, 3) as a}
						<p class="text-xs text-amber-600 dark:text-amber-400">
							{a.dimension} · {a.scope_kind}:{a.scope_id.slice(0, 8)}... — {a.percent.toFixed(0)}%
						</p>
					{/each}
				</div>
			</Card>
		{/if}

		<!-- Quick actions -->
		<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 mb-3">快捷入口</h2>
		<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
			<button onclick={() => goto('/playground')} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
				<MessageSquare size={20} class="text-zinc-400 group-hover:text-zinc-600 dark:group-hover:text-zinc-300 transition-colors" />
				<div class="flex-1">
					<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">Chat Playground</p>
					<p class="text-xs text-zinc-600 dark:text-zinc-300">在线测试模型对话</p>
				</div>
				<ArrowRight size={14} class="text-zinc-300 dark:text-zinc-600" />
			</button>

			<button onclick={() => goto('/usage')} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
				<BarChart3 size={20} class="text-zinc-400 group-hover:text-zinc-600 dark:group-hover:text-zinc-300 transition-colors" />
				<div class="flex-1">
					<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">用量仪表盘</p>
					<p class="text-xs text-zinc-600 dark:text-zinc-300">查看 Token 消耗趋势</p>
				</div>
				<ArrowRight size={14} class="text-zinc-300 dark:text-zinc-600" />
			</button>

			{#if currentOrg}
				<button onclick={() => goto(`/orgs/${currentOrg}/projects`)} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
					<FolderOpen size={20} class="text-zinc-400 group-hover:text-zinc-600 dark:group-hover:text-zinc-300 transition-colors" />
					<div class="flex-1">
						<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">项目管理</p>
						<p class="text-xs text-zinc-600 dark:text-zinc-300">查看项目和 API Key</p>
					</div>
					<ArrowRight size={14} class="text-zinc-300 dark:text-zinc-600" />
				</button>
			{/if}
		</div>
	{/if}
</div>
