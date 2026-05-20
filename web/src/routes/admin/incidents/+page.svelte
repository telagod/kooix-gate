<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { getIncidentSummary, getMe } from '$lib/api.js';
	import type {
		IncidentSummaryResponse,
		MeResult,
		QuotaDenySnapshot,
		RequestRecord,
		TopFailingChannel,
		UpstreamErrorClasses,
		UpstreamErrorSnapshot
	} from '$lib/api.js';
	import { shortId } from '$lib/id.js';
	import { Badge, Button, Card, Select } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import {
		Activity,
		ArrowRight,
		CircleAlert,
		Clock,
		DatabaseZap,
		RefreshCw,
		ShieldAlert,
		Siren,
		TriangleAlert,
		XCircle
	} from 'lucide-svelte';

	let me = $state<MeResult | null>(null);
	let summary = $state<IncidentSummaryResponse | null>(null);
	let loading = $state(true);
	let error = $state('');
	let selectedOrg = $state('');
	let hours = $state('24');

	const hourOptions = [
		{ value: '1', label: '1 小时' },
		{ value: '6', label: '6 小时' },
		{ value: '24', label: '24 小时' },
		{ value: '168', label: '7 天' },
		{ value: '720', label: '30 天' }
	];

	let orgOptions = $derived([
		{ value: '', label: '全部组织' },
		...(me?.orgs ?? []).map((org) => ({ value: org, label: `${shortId(org)}...` }))
	]);
	let totalClassified = $derived(sumClasses(summary?.upstream_error_classes));
	let runtimeUpstreamTotal = $derived(
		(summary?.upstream_errors_runtime_top ?? []).reduce((sum, item) => sum + item.errors, 0)
	);
	let quotaTotal = $derived((summary?.quota_denies_top ?? []).reduce((sum, item) => sum + item.denies, 0));
	let topChannelErrors = $derived((summary?.top_failing_channels?.[0]?.errors ?? 0));
	let topChannelRequests = $derived((summary?.top_failing_channels?.[0]?.requests ?? 0));
	let maxTopFailingErrors = $derived(maxErrors(summary?.top_failing_channels ?? []));
	let maxQuotaDenies = $derived(maxDenies(summary?.quota_denies_top ?? []));
	let maxRuntimeUpstreamErrors = $derived(maxRuntimeErrors(summary?.upstream_errors_runtime_top ?? []));

	onMount(async () => {
		try {
			me = await getMe();
			if (!me.is_platform_admin) {
				error = '仅 Platform Admin 可查看事故中心';
				loading = false;
				return;
			}
			selectedOrg = me.current_org ?? me.orgs?.[0] ?? '';
		} catch (err: any) {
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
			summary = await getIncidentSummary(selectedOrg || undefined, Number(hours));
		} catch (err: any) {
			error = err?.message ?? '加载事故摘要失败';
		} finally {
			loading = false;
		}
	}

	function sumClasses(classes: UpstreamErrorClasses | null | undefined): number {
		if (!classes) return 0;
		return classes.auth_401 + classes.rate_limit_429 + classes.upstream_5xx + classes.other_4xx + classes.unknown;
	}

	function fmt(n: number): string {
		return n.toLocaleString('en-US');
	}

	function fmtPct(n: number): string {
		return `${(n * 100).toFixed(1)}%`;
	}

	function formatDate(s: string | null): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleString('zh-CN', {
				month: '2-digit',
				day: '2-digit',
				hour: '2-digit',
				minute: '2-digit',
				second: '2-digit'
			});
		} catch {
			return s;
		}
	}

	function statusBadgeCls(status: number): string {
		if (status === 401 || status === 403) return 'bg-amber-50 text-amber-700 ring-amber-200 dark:bg-amber-900/30 dark:text-amber-400 dark:ring-amber-800';
		if (status === 429) return 'bg-amber-50 text-amber-700 ring-amber-200 dark:bg-amber-900/30 dark:text-amber-400 dark:ring-amber-800';
		if (status >= 500) return 'bg-red-50 text-red-700 ring-red-200 dark:bg-red-900/30 dark:text-red-400 dark:ring-red-800';
		if (status >= 400) return 'bg-red-50 text-red-700 ring-red-200 dark:bg-red-900/30 dark:text-red-400 dark:ring-red-800';
		return 'bg-zinc-100 text-zinc-700 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700';
	}

	function severityFor(status: number): 'danger' | 'warning' | 'default' {
		if (status >= 500) return 'danger';
		if (status === 401 || status === 403 || status === 429) return 'warning';
		if (status >= 400) return 'danger';
		return 'default';
	}

	function classRows(classes: UpstreamErrorClasses | null | undefined) {
		return [
			{ key: 'auth_401', label: '401 Auth', value: classes?.auth_401 ?? 0, tone: 'warning' },
			{ key: 'rate_limit_429', label: '429 Rate limit', value: classes?.rate_limit_429 ?? 0, tone: 'warning' },
			{ key: 'upstream_5xx', label: 'Upstream 5xx', value: classes?.upstream_5xx ?? 0, tone: 'danger' },
			{ key: 'other_4xx', label: 'Other 4xx', value: classes?.other_4xx ?? 0, tone: 'default' },
			{ key: 'unknown', label: 'Unknown', value: classes?.unknown ?? 0, tone: 'default' }
		];
	}

	function classTone(tone: string): string {
		if (tone === 'danger') return 'bg-red-600 dark:bg-red-500';
		if (tone === 'warning') return 'bg-amber-500 dark:bg-amber-400';
		return 'bg-zinc-500 dark:bg-zinc-400';
	}

	function maxErrors(rows: TopFailingChannel[]): number {
		return Math.max(1, ...rows.map((row) => row.errors));
	}

	function maxDenies(rows: QuotaDenySnapshot[]): number {
		return Math.max(1, ...rows.map((row) => row.denies));
	}

	function maxRuntimeErrors(rows: UpstreamErrorSnapshot[]): number {
		return Math.max(1, ...rows.map((row) => row.errors));
	}

	function requestHref(record: RequestRecord): string {
		return `/admin/requests?search=${encodeURIComponent(record.request_id)}`;
	}

	function errorTitle(record: RequestRecord): string {
		return record.error_code ?? `HTTP ${record.status}`;
	}
</script>

<PageShell title="事故中心" eyebrow="Observability" description="聚合最近错误、失败渠道、配额拒绝和上游错误分类，用于生产止血与复盘。" icon={ShieldAlert} max="full">
	{#snippet actions()}
		<div class="flex flex-wrap items-center gap-2">
			<Select bind:value={selectedOrg} options={orgOptions} size="sm" disabled={loading || !me?.is_platform_admin} class="w-40" onchange={() => load()} />
			<Select bind:value={hours} options={hourOptions} size="sm" disabled={loading || !me?.is_platform_admin} class="w-32" onchange={() => load()} />
			<Button variant="outline" size="sm" onclick={load} disabled={loading || !me?.is_platform_admin}>
				<RefreshCw size={14} class={loading ? 'animate-spin' : ''} />
				刷新
			</Button>
		</div>
	{/snippet}

	{#if loading && !summary}
		<div class="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-4">
			{#each Array(4) as _}
				<div class="h-32 animate-pulse rounded-lg bg-zinc-200 dark:bg-zinc-800"></div>
			{/each}
		</div>
	{:else if error}
		<StatePanel variant={me?.is_platform_admin === false ? 'warning' : 'danger'} title="无法打开事故中心" description={error} icon={TriangleAlert} />
	{:else if summary}
		<div class="mb-5 grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-4">
			<Card class="p-4">
				<div class="mb-3 flex items-center justify-between">
					<div class="flex items-center gap-2">
						<Siren size={16} class="text-red-500 dark:text-red-400" />
						<p class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">最近错误</p>
					</div>
					<Badge variant={summary.recent_errors.length > 0 ? 'danger' : 'default'}>{fmt(summary.recent_errors.length)}</Badge>
				</div>
				<p class="text-3xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">{fmt(summary.recent_errors.length)}</p>
				<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">窗口 {summary.hours}h · 取最近 12 条</p>
			</Card>

			<Card class="p-4">
				<div class="mb-3 flex items-center justify-between">
					<div class="flex items-center gap-2">
						<XCircle size={16} class={topChannelErrors > 0 ? 'text-red-500 dark:text-red-400' : 'text-zinc-400'} />
						<p class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">Top failing</p>
					</div>
					<Badge variant={topChannelErrors > 0 ? 'danger' : 'default'}>{fmt(topChannelErrors)}</Badge>
				</div>
				<p class="text-3xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">{fmtPct(topChannelRequests > 0 ? topChannelErrors / topChannelRequests : 0)}</p>
				<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">最高失败率 · {fmt(topChannelRequests)} requests</p>
			</Card>

			<Card class="p-4">
				<div class="mb-3 flex items-center justify-between">
					<div class="flex items-center gap-2">
						<DatabaseZap size={16} class={quotaTotal > 0 ? 'text-amber-600 dark:text-amber-400' : 'text-zinc-400'} />
						<p class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">Quota deny</p>
					</div>
					<Badge variant={quotaTotal > 0 ? 'warning' : 'default'}>{fmt(quotaTotal)}</Badge>
				</div>
				<p class="text-3xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">{fmt(quotaTotal)}</p>
				<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">runtime-local snapshot</p>
			</Card>

			<Card class="p-4">
				<div class="mb-3 flex items-center justify-between">
					<div class="flex items-center gap-2">
						<Activity size={16} class={totalClassified + runtimeUpstreamTotal > 0 ? 'text-red-500 dark:text-red-400' : 'text-zinc-400'} />
						<p class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">Upstream errors</p>
					</div>
					<Badge variant={totalClassified + runtimeUpstreamTotal > 0 ? 'danger' : 'default'}>{fmt(totalClassified)}</Badge>
				</div>
				<p class="text-3xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">{fmt(totalClassified)}</p>
				<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">401 / 429 / 5xx 已分类</p>
			</Card>
		</div>

		<div class="grid grid-cols-1 gap-4 xl:grid-cols-[1.05fr_0.95fr]">
			<Card class="p-4">
				<div class="mb-4 flex items-center justify-between gap-3">
					<div>
						<h2 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">最近错误</h2>
						<p class="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">按时间倒序，点击跳转请求日志定位单条链路。</p>
					</div>
					<Button variant="ghost" size="sm" onclick={() => goto('/admin/requests?error_only=true')}>全部日志 <ArrowRight size={13} /></Button>
				</div>

				{#if summary.recent_errors.length === 0}
					<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-6 text-center text-sm text-zinc-500 dark:border-zinc-700 dark:bg-zinc-800/50 dark:text-zinc-400">当前窗口没有持久化错误。</div>
				{:else}
					<div class="space-y-2">
						{#each summary.recent_errors as record (record.request_id)}
							<a href={requestHref(record)} class="group block rounded-lg border border-zinc-200 bg-white p-3 transition-colors hover:border-zinc-400 dark:border-zinc-800 dark:bg-zinc-900 dark:hover:border-zinc-600">
								<div class="flex items-start gap-3">
									<div class={cn('mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg', severityFor(record.status) === 'warning' ? 'bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400' : severityFor(record.status) === 'danger' ? 'bg-red-50 text-red-700 dark:bg-red-900/30 dark:text-red-400' : 'bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400')}>
										<CircleAlert size={16} />
									</div>
									<div class="min-w-0 flex-1">
										<div class="flex flex-wrap items-center gap-2">
											<span class={cn('rounded-md px-1.5 py-0.5 text-[11px] font-medium ring-1 ring-inset', statusBadgeCls(record.status))}>{record.status}</span>
											<span class="truncate font-mono text-xs font-medium text-zinc-900 dark:text-zinc-100">{errorTitle(record)}</span>
											<span class="text-[11px] text-zinc-400 dark:text-zinc-500">{formatDate(record.ts)}</span>
										</div>
										<div class="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-[11px] text-zinc-500 dark:text-zinc-400">
											<span class="font-mono">model={record.model_actual}</span>
											<span class="font-mono">channel={record.channel_id ? shortId(record.channel_id) : 'fallback'}</span>
											<span class="font-mono">request={shortId(record.request_id)}</span>
										</div>
									</div>
									<ArrowRight size={14} class="mt-2 text-zinc-300 transition-colors group-hover:text-zinc-500 dark:text-zinc-700 dark:group-hover:text-zinc-400" />
								</div>
							</a>
						{/each}
					</div>
				{/if}
			</Card>

			<div class="space-y-4">
				<Card class="p-4">
					<h2 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Upstream 401 / 429 / 5xx 分类</h2>
					<p class="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">来自持久化 request events / usage records。</p>
					<div class="mt-4 space-y-3">
						{#each classRows(summary.upstream_error_classes) as row}
							{@const pct = totalClassified > 0 ? row.value / totalClassified : 0}
							<div>
								<div class="mb-1 flex items-center justify-between text-xs">
									<span class="font-medium text-zinc-700 dark:text-zinc-300">{row.label}</span>
									<span class="font-mono text-zinc-500 dark:text-zinc-400">{fmt(row.value)} · {fmtPct(pct)}</span>
								</div>
								<div class="h-2 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
									<div class={cn('h-full rounded-full transition-all', classTone(row.tone))} style={`width: ${(pct * 100).toFixed(1)}%`}></div>
								</div>
							</div>
						{/each}
					</div>
				</Card>

				<Card class="p-4">
					<div class="mb-3 flex items-center justify-between">
						<div>
							<h2 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">运行时上游错误 Top</h2>
							<p class="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">process-local，自服务启动后累计。</p>
						</div>
						<Badge variant={runtimeUpstreamTotal > 0 ? 'danger' : 'default'}>{fmt(runtimeUpstreamTotal)}</Badge>
					</div>
					{#if summary.upstream_errors_runtime_top.length === 0}
						<p class="rounded-lg bg-zinc-50 p-4 text-sm text-zinc-500 dark:bg-zinc-800/50 dark:text-zinc-400">暂无运行时上游错误快照。</p>
					{:else}
						<div class="space-y-2">
							{#each summary.upstream_errors_runtime_top as item}
								<div class="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
									<div class="flex items-center justify-between gap-2 text-xs">
										<span class="truncate font-mono font-medium text-zinc-900 dark:text-zinc-100">{item.kind}</span>
										<span class="font-mono text-zinc-500 dark:text-zinc-400">{fmt(item.errors)}</span>
									</div>
									<p class="mt-1 truncate text-[11px] text-zinc-500 dark:text-zinc-400">{item.provider_type} · {item.channel} · {item.model}</p>
									<div class="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
										<div class="h-full rounded-full bg-red-600 dark:bg-red-500" style={`width: ${(item.errors / maxRuntimeUpstreamErrors * 100).toFixed(1)}%`}></div>
									</div>
								</div>
							{/each}
						</div>
					{/if}
				</Card>
			</div>
		</div>

		<div class="mt-4 grid grid-cols-1 gap-4 xl:grid-cols-2">
			<Card class="p-4">
				<h2 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Top failing channels</h2>
				<p class="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">按错误数、错误率和最近错误时间排序。</p>
				{#if summary.top_failing_channels.length === 0}
					<div class="mt-4 rounded-lg border border-zinc-200 bg-zinc-50 p-6 text-center text-sm text-zinc-500 dark:border-zinc-700 dark:bg-zinc-800/50 dark:text-zinc-400">暂无失败渠道。</div>
				{:else}
					<DataTable class="mb-0 mt-4">
						{#snippet head()}
							<tr>
								<th class={dataTemplate.th}>Channel</th>
								<th class={dataTemplate.th}>Errors</th>
								<th class={dataTemplate.th}>Rate</th>
								<th class={dataTemplate.th}>Last</th>
							</tr>
						{/snippet}

						{#each summary.top_failing_channels as row}
							<tr class={dataTemplate.row}>
								<td class="px-4 py-3">
									<p class="truncate text-xs font-medium text-zinc-900 dark:text-zinc-100">{row.channel_name ?? 'Fallback / Unknown'}</p>
									<p class="mt-0.5 truncate font-mono text-[11px] text-zinc-500 dark:text-zinc-400">{row.provider_type ?? 'unknown'} · {row.channel_id ? shortId(row.channel_id) : 'no-channel'}</p>
								</td>
								<td class="px-4 py-3">
									<div class="mb-1 flex items-center justify-between gap-2 font-mono text-xs text-zinc-700 dark:text-zinc-300">
										<span>{fmt(row.errors)}</span>
										<span class="text-zinc-400">/{fmt(row.requests)}</span>
									</div>
									<div class="h-1.5 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
										<div class="h-full rounded-full bg-red-600 dark:bg-red-500" style={`width: ${(row.errors / maxTopFailingErrors * 100).toFixed(1)}%`}></div>
									</div>
								</td>
								<td class="px-4 py-3 font-mono text-xs text-zinc-700 dark:text-zinc-300">{fmtPct(row.error_rate)}</td>
								<td class="px-4 py-3">
									<p class="font-mono text-xs text-zinc-700 dark:text-zinc-300">{row.last_error_code ?? '—'}</p>
									<p class="mt-0.5 text-[11px] text-zinc-500 dark:text-zinc-400">{formatDate(row.last_error_at)}</p>
								</td>
							</tr>
						{/each}
					</DataTable>
				{/if}
			</Card>

			<Card class="p-4">
				<h2 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Quota deny top</h2>
				<p class="mt-0.5 text-xs text-zinc-500 dark:text-zinc-400">来自 `quota_denies_total` 同步维护的运行时快照。</p>
				{#if summary.quota_denies_top.length === 0}
					<div class="mt-4 rounded-lg border border-zinc-200 bg-zinc-50 p-6 text-center text-sm text-zinc-500 dark:border-zinc-700 dark:bg-zinc-800/50 dark:text-zinc-400">暂无配额拒绝。</div>
				{:else}
					<div class="mt-4 space-y-2">
						{#each summary.quota_denies_top as row}
							<div class="rounded-lg border border-zinc-200 p-3 dark:border-zinc-800">
								<div class="flex items-center justify-between gap-3">
									<div class="min-w-0">
										<p class="truncate font-mono text-xs font-medium text-zinc-900 dark:text-zinc-100">{row.dimension}</p>
										<p class="mt-0.5 text-[11px] text-zinc-500 dark:text-zinc-400">{row.scope_kind} · {row.mode}</p>
									</div>
									<span class="font-mono text-sm font-semibold text-amber-700 dark:text-amber-400">{fmt(row.denies)}</span>
								</div>
								<div class="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-100 dark:bg-zinc-800">
									<div class="h-full rounded-full bg-amber-500 dark:bg-amber-400" style={`width: ${(row.denies / maxQuotaDenies * 100).toFixed(1)}%`}></div>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</Card>
		</div>

		<Card class="mt-4 p-4">
			<div class="flex items-start gap-3">
				<Clock size={16} class="mt-0.5 text-zinc-400 dark:text-zinc-500" />
				<div class="min-w-0">
					<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">数据边界</p>
					<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">生成时间：{formatDate(summary.generated_at)} · org={selectedOrg ? shortId(selectedOrg) : 'all'} · window={summary.hours}h</p>
					<ul class="mt-2 list-disc space-y-1 pl-4 text-xs text-zinc-500 dark:text-zinc-400">
						{#each summary.data_notes as note}
							<li>{note}</li>
						{/each}
					</ul>
				</div>
			</div>
		</Card>
	{/if}
</PageShell>
