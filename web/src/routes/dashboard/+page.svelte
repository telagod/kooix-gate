<script lang="ts">
	import { shortId, rawId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getMe, getUsage, getQuotaAlerts, listProjects, listKeys, getDashboardStats } from '$lib/api.js';
	import type { MeResult, UsageResponse, QuotaAlert, DashboardStatsResponse } from '$lib/api.js';
	import Card from '$lib/components/ui/Card.svelte';
	import Stat from '$lib/components/Stat.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import ModalityBadge from '$lib/components/ui/ModalityBadge.svelte';
	import {
		TrendingUp,
		AlertTriangle,
		Key,
		FolderOpen,
		ArrowRight,
		MessageSquare,
		BarChart3,
		Activity,
		Zap,
		Clock,
		XCircle,
		ScrollText,
		ShieldAlert,
		Gauge
	} from 'lucide-svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn } from '$lib/design';

	let me = $state<MeResult | null>(null);
	let usage = $state<UsageResponse | null>(null);
	let alerts = $state<QuotaAlert[]>([]);
	let stats = $state<DashboardStatsResponse | null>(null);
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

			const promises: Promise<any>[] = [
				getUsage(org, '7d').catch(() => null),
				getQuotaAlerts(org).catch(() => []),
				listProjects(org).catch(() => [])
			];

			if (me.is_platform_admin) {
				promises.push(getDashboardStats(org, 24).catch(() => null));
			}

			const results = await Promise.all(promises);
			usage = results[0];
			alerts = results[1];
			const projects = results[2];
			projectCount = projects.length;

			if (me.is_platform_admin && results[3]) {
				stats = results[3];
			}

			let keys = 0;
			for (const proj of projects.slice(0, 5)) {
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
	function fmtPct(n: number): string { return `${(n * 100).toFixed(1)}%`; }
	function fmtMs(n: number | null): string {
		if (n == null) return '—';
		if (n < 1000) return `${Math.round(n)}ms`;
		return `${(n / 1000).toFixed(1)}s`;
	}

	let exceededAlerts = $derived(alerts.filter(a => a.level === 'exceeded'));
	let approachingAlerts = $derived(alerts.filter(a => a.level === 'approaching'));

	// Mini sparkline for hourly trend
	function sparklinePath(data: { requests: number }[]): string {
		if (data.length < 2) return '';
		const max = Math.max(1, ...data.map(d => d.requests));
		const w = 120, h = 32;
		return data.map((d, i) => {
			const x = (i / (data.length - 1)) * w;
			const y = h - (d.requests / max) * h;
			return `${i === 0 ? 'M' : 'L'} ${x.toFixed(1)} ${y.toFixed(1)}`;
		}).join(' ');
	}
</script>

<PageShell title="总览" description={currentOrg ? `当前组织：${shortId(currentOrg)}...` : '未加入任何组织'} icon={BarChart3}>
	{#snippet actions()}
		{#if me?.is_platform_admin}
			<span class="inline-flex items-center gap-1 rounded-md bg-amber-100 px-2 py-1 text-xs font-medium text-amber-700 dark:bg-amber-900/40 dark:text-amber-400">Platform Admin</span>
		{/if}
	{/snippet}
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
		<StatePanel variant="danger" description={error} />
	{:else}

		<!-- Platform stats (Admin only) -->
		{#if stats}
			<div class="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3 mb-6">
				<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 p-4 shadow-sm">
					<div class="flex items-center gap-2 mb-2">
						<Activity size={14} class="text-zinc-400" />
						<p class="text-[10px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">24h 请求</p>
					</div>
					<p class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{fmt(stats.total_requests)}</p>
					{#if stats.hourly_trend.length > 1}
						<svg viewBox="0 0 120 32" class="w-full h-8 mt-1">
							<path d={sparklinePath(stats.hourly_trend)} fill="none" class="stroke-zinc-400 dark:stroke-zinc-500" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
						</svg>
					{/if}
				</div>

				<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 p-4 shadow-sm">
					<div class="flex items-center gap-2 mb-2">
						<XCircle size={14} class={cn(stats.error_rate > 0.05 ? 'text-red-400' : 'text-zinc-400')} />
						<p class="text-[10px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">错误率</p>
					</div>
					<p class={cn(
						'text-2xl font-bold tabular-nums',
						stats.error_rate > 0.05 ? 'text-red-600 dark:text-red-400' : 'text-zinc-900 dark:text-zinc-100'
					)}>{fmtPct(stats.error_rate)}</p>
					<p class="text-[10px] text-zinc-500 dark:text-zinc-400 mt-1">{fmt(stats.total_errors)} 错误</p>
				</div>

				<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 p-4 shadow-sm">
					<div class="flex items-center gap-2 mb-2">
						<Clock size={14} class="text-zinc-400" />
						<p class="text-[10px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">P50 延迟</p>
					</div>
					<p class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{fmtMs(stats.p50_latency_ms)}</p>
				</div>

				<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 p-4 shadow-sm">
					<div class="flex items-center gap-2 mb-2">
						<Zap size={14} class="text-zinc-400" />
						<p class="text-[10px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">P95 延迟</p>
					</div>
					<p class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{fmtMs(stats.p95_latency_ms)}</p>
				</div>

				<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 p-4 shadow-sm">
					<div class="flex items-center gap-2 mb-2">
						<TrendingUp size={14} class="text-zinc-400" />
						<p class="text-[10px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">24h 花费</p>
					</div>
					<p class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{fmtCost(stats.total_cost_usd)}</p>
				</div>

				<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 p-4 shadow-sm">
					<div class="flex items-center gap-2 mb-2">
						<Gauge size={14} class="text-zinc-400" />
						<p class="text-[10px] font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">24h Tokens</p>
					</div>
					<p class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{fmt(stats.total_tokens)}</p>
				</div>
			</div>

			<!-- Top Models + Recent Errors -->
			<div class="grid grid-cols-1 lg:grid-cols-2 gap-4 mb-6">
				<!-- Top Models -->
				{#if stats.top_models.length > 0}
					<Card class="p-4">
						<h3 class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">活跃模型 Top 5</h3>
						<div class="space-y-2">
							{#each stats.top_models as m, i}
								{@const maxReqs = stats.top_models[0]?.requests ?? 1}
								<div class="flex items-center gap-3">
									<span class="w-5 text-right text-[10px] font-mono text-zinc-400">{i + 1}</span>
									<div class="flex-1 min-w-0">
										<div class="flex items-center justify-between mb-0.5">
											<div class="flex items-center gap-1.5">
												<span class="text-xs font-medium text-zinc-900 dark:text-zinc-100 truncate">{m.model}</span>
												<ModalityBadge model={m.model} />
											</div>
											<span class="text-[10px] text-zinc-500 dark:text-zinc-400 font-mono ml-2 shrink-0">{fmt(m.requests)} · {fmtCost(m.cost_usd)}</span>
										</div>
										<div class="h-1.5 rounded-full bg-zinc-100 dark:bg-zinc-800 overflow-hidden">
											<div
												class="h-full rounded-full bg-zinc-900 dark:bg-zinc-300 transition-all"
												style="width: {(m.requests / maxReqs * 100).toFixed(1)}%"
											></div>
										</div>
									</div>
								</div>
							{/each}
						</div>
					</Card>
				{/if}

				<!-- Recent Errors -->
				{#if stats.recent_errors.length > 0}
					<Card class="p-4">
						<div class="flex items-center justify-between mb-3">
							<h3 class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">最近错误</h3>
							<button onclick={() => goto('/admin/requests?error_only=true')} class="text-[10px] text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300 transition-colors">
								查看全部 →
							</button>
						</div>
						<div class="space-y-2">
							{#each stats.recent_errors as err}
								<div class="flex items-center gap-3 p-2 rounded-lg bg-red-50/50 dark:bg-red-900/10">
									<XCircle size={12} class="text-red-400 shrink-0" />
									<div class="flex-1 min-w-0">
										<div class="flex items-center gap-2">
											<span class="text-xs font-medium text-zinc-900 dark:text-zinc-100 truncate">{err.model_actual}</span>
											<ModalityBadge model={err.model_actual} metadata={err.metadata} />
											<span class="text-[10px] px-1.5 py-px rounded bg-red-100 dark:bg-red-900/40 text-red-600 dark:text-red-400 font-mono">{err.status}</span>
										</div>
										<p class="text-[10px] text-zinc-500 dark:text-zinc-400 font-mono truncate">{err.error_code ?? '—'}</p>
									</div>
									<span class="text-[10px] text-zinc-400 shrink-0">{new Date(err.ts).toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })}</span>
								</div>
							{/each}
						</div>
					</Card>
				{/if}
			</div>
		{/if}

		<!-- Usage stats (all users) -->
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
							{a.dimension} · {a.scope_kind}:{shortId(a.scope_id)}... — {a.percent.toFixed(0)}%
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
							{a.dimension} · {a.scope_kind}:{shortId(a.scope_id)}... — {a.percent.toFixed(0)}%
						</p>
					{/each}
				</div>
			</Card>
		{/if}

		<!-- Quick actions -->
		<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 mb-3">快捷入口</h2>
		<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-3">
			<button onclick={() => goto('/playground')} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
				<MessageSquare size={20} class="text-zinc-400 group-hover:text-zinc-600 dark:group-hover:text-zinc-300 transition-colors" />
				<div class="flex-1">
					<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">Chat Playground 调试台</p>
					<p class="text-xs text-zinc-600 dark:text-zinc-300">在线测试模型对话</p>
				</div>
				<ArrowRight size={14} class="text-zinc-300 dark:text-zinc-600" />
			</button>

			<button onclick={() => goto('/usage')} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
				<BarChart3 size={20} class="text-zinc-400 group-hover:text-zinc-600 dark:group-hover:text-zinc-300 transition-colors" />
				<div class="flex-1">
					<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">用量仪表盘</p>
					<p class="text-xs text-zinc-600 dark:text-zinc-300">查看 Tokens 消耗趋势</p>
				</div>
				<ArrowRight size={14} class="text-zinc-300 dark:text-zinc-600" />
			</button>

			{#if me?.is_platform_admin}
				<button onclick={() => goto('/admin/requests')} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
					<ScrollText size={20} class="text-zinc-400 group-hover:text-zinc-600 dark:group-hover:text-zinc-300 transition-colors" />
					<div class="flex-1">
						<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">请求日志</p>
						<p class="text-xs text-zinc-600 dark:text-zinc-300">查看 API 请求记录</p>
					</div>
					<ArrowRight size={14} class="text-zinc-300 dark:text-zinc-600" />
				</button>

				<button onclick={() => goto('/admin/incidents')} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
					<ShieldAlert size={20} class="text-zinc-400 group-hover:text-zinc-600 dark:group-hover:text-zinc-300 transition-colors" />
					<div class="flex-1">
						<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">事故中心</p>
						<p class="text-xs text-zinc-600 dark:text-zinc-300">定位错误、配额与上游故障</p>
					</div>
					<ArrowRight size={14} class="text-zinc-300 dark:text-zinc-600" />
				</button>
			{/if}

			{#if currentOrg}
				<button onclick={() => goto(`/orgs/${rawId(currentOrg)}/projects`)} class="group flex items-center gap-3 p-4 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors text-left">
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
</PageShell>
