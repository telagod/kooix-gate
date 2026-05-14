<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, listRequests } from '$lib/api.js';
	import type { RequestRecord, RequestPage, RequestListParams, MeResult } from '$lib/api.js';
	import Card from '$lib/components/ui/Card.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import FilterPills from '$lib/components/ui/FilterPills.svelte';
	import {
		ScrollText,
		Search,
		AlertTriangle,
		Clock,
		ArrowRight,
		ArrowLeft,
		ChevronDown,
		ChevronUp,
		Zap,
		XCircle,
		CheckCircle2,
		Filter,
		RotateCcw
	} from 'lucide-svelte';
	import { clsx } from 'clsx';

	let me = $state<MeResult | null>(null);
	let page = $state<RequestPage | null>(null);
	let loading = $state(true);
	let error = $state('');
	let expandedId = $state<string | null>(null);

	// Filters
	let search = $state('');
	let filterStatus = $state('');
	let filterModel = $state('');
	let filterRange = $state('24h');
	let cursorStack = $state<string[]>([]);
	let currentCursor = $state<string | undefined>(undefined);

	const statusOptions = [
		{ value: '', label: '全部状态' },
		{ value: 'success', label: '成功' },
		{ value: 'error', label: '错误' }
	];

	const rangeOptions = [
		{ value: '1h', label: '1 小时' },
		{ value: '24h', label: '24 小时' },
		{ value: '7d', label: '7 天' },
		{ value: '30d', label: '30 天' }
	];

	function rangeToDate(range: string): string {
		const now = new Date();
		switch (range) {
			case '1h': return new Date(now.getTime() - 3600_000).toISOString();
			case '24h': return new Date(now.getTime() - 86400_000).toISOString();
			case '7d': return new Date(now.getTime() - 7 * 86400_000).toISOString();
			case '30d': return new Date(now.getTime() - 30 * 86400_000).toISOString();
			default: return new Date(now.getTime() - 86400_000).toISOString();
		}
	}

	onMount(async () => {
		try {
			me = await getMe();
			if (!me.is_platform_admin) {
				error = '仅 Platform Admin 可查看请求日志';
				loading = false;
				return;
			}
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
			const params: RequestListParams = { limit: 50 };
			if (search.trim()) params.search = search.trim();
			if (filterStatus === 'error') params.error_only = true;
			else if (filterStatus === 'success') { params.status_min = 200; params.status_max = 299; }
			if (filterModel.trim()) params.model = filterModel.trim();
			params.from = rangeToDate(filterRange);
			if (currentCursor) params.cursor = currentCursor;
			page = await listRequests(params);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	function resetFilters() {
		search = '';
		filterStatus = '';
		filterModel = '';
		filterRange = '24h';
		currentCursor = undefined;
		cursorStack = [];
		load();
	}

	function nextPage() {
		if (!page?.next_cursor) return;
		if (currentCursor) cursorStack = [...cursorStack, currentCursor];
		currentCursor = page.next_cursor;
		load();
	}

	function prevPage() {
		if (cursorStack.length === 0) return;
		const prev = cursorStack[cursorStack.length - 1];
		cursorStack = cursorStack.slice(0, -1);
		currentCursor = prev || undefined;
		load();
	}

	function applyFilters() {
		currentCursor = undefined;
		cursorStack = [];
		load();
	}

	function handleSearchKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter') applyFilters();
	}

	function toggleExpand(id: string) {
		expandedId = expandedId === id ? null : id;
	}

	function statusBadgeCls(status: number): string {
		if (status >= 200 && status < 300) return 'bg-emerald-50 dark:bg-emerald-900/30 text-emerald-700 dark:text-emerald-400 ring-1 ring-emerald-200 dark:ring-emerald-800';
		if (status >= 400 && status < 500) return 'bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400 ring-1 ring-amber-200 dark:ring-amber-800';
		if (status >= 500) return 'bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400 ring-1 ring-red-200 dark:ring-red-800';
		return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300';
	}

	function formatDate(s: string): string {
		try {
			return new Date(s).toLocaleString('zh-CN', {
				month: '2-digit', day: '2-digit',
				hour: '2-digit', minute: '2-digit', second: '2-digit'
			});
		} catch { return s; }
	}

	function formatLatency(ms: number | null): string {
		if (ms == null) return '—';
		if (ms < 1000) return `${ms}ms`;
		return `${(ms / 1000).toFixed(1)}s`;
	}

	function formatCost(n: number): string {
		if (n < 0.0001) return '$0';
		return `$${n.toFixed(4)}`;
	}

	function formatTokens(n: number): string {
		if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
		if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
		return String(n);
	}

	let pageNum = $derived(cursorStack.length + 1);
</script>

<div class="px-6 py-6">
	<!-- Header -->
	<div class="flex items-center justify-between mb-6">
		<div class="flex items-center gap-3">
			<div class="flex items-center justify-center w-9 h-9 rounded-lg bg-zinc-900 dark:bg-zinc-100">
				<ScrollText size={18} class="text-white dark:text-zinc-900" />
			</div>
			<div>
				<h1 class="text-xl font-bold text-zinc-900 dark:text-zinc-100">请求日志</h1>
				<p class="text-xs text-zinc-500 dark:text-zinc-400">查看所有 API 请求记录，支持多维筛选</p>
			</div>
		</div>
		<Button variant="outline" size="sm" onclick={resetFilters}>
			<RotateCcw size={14} />
			<span class="ml-1">重置</span>
		</Button>
	</div>

	<!-- Filters -->
	<div class="flex flex-wrap items-center gap-3 mb-4">
		<div class="relative flex-1 min-w-[200px] max-w-sm">
			<Search size={14} class="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
			<input
				type="text"
				bind:value={search}
				onkeydown={handleSearchKeydown}
				placeholder="搜索 model / error_code..."
				class="w-full h-9 pl-9 pr-3 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300"
			/>
		</div>

		<FilterPills bind:value={filterRange} options={rangeOptions} />

		<FilterPills bind:value={filterStatus} options={statusOptions} />

		<input
			type="text"
			bind:value={filterModel}
			onkeydown={handleSearchKeydown}
			placeholder="model 过滤"
			class="h-9 w-40 px-3 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300"
		/>

		<Button variant="default" size="sm" onclick={applyFilters} disabled={loading}>
			<Filter size={14} />
			<span class="ml-1">筛选</span>
		</Button>
	</div>

	<!-- Content -->
	{#if loading && !page}
		<div class="space-y-2">
			{#each Array(8) as _}
				<div class="h-12 bg-zinc-100 dark:bg-zinc-800 rounded-lg animate-pulse"></div>
			{/each}
		</div>
	{:else if error}
		<Card class="p-8 text-center">
			<XCircle size={24} class="mx-auto mb-2 text-red-400" />
			<p class="text-sm text-red-600 dark:text-red-400">{error}</p>
		</Card>
	{:else if page && page.data.length === 0}
		<Card class="p-12 text-center">
			<ScrollText size={32} class="mx-auto mb-3 text-zinc-300 dark:text-zinc-600" />
			<p class="text-base font-medium text-zinc-900 dark:text-zinc-100 mb-1">暂无请求记录</p>
			<p class="text-sm text-zinc-500 dark:text-zinc-400">调整筛选条件或时间范围试试</p>
		</Card>
	{:else if page}
		<!-- Table -->
		<div class="overflow-hidden rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-sm mb-4">
			<table class="w-full text-sm">
				<thead class="bg-zinc-50 dark:bg-zinc-800/60 border-b border-zinc-200 dark:border-zinc-700">
					<tr>
						<th class="px-4 py-3 text-left font-medium text-zinc-500 dark:text-zinc-400 text-xs uppercase tracking-wider">时间</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-500 dark:text-zinc-400 text-xs uppercase tracking-wider">模型</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-500 dark:text-zinc-400 text-xs uppercase tracking-wider">状态</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-500 dark:text-zinc-400 text-xs uppercase tracking-wider">延迟</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-500 dark:text-zinc-400 text-xs uppercase tracking-wider">Tokens</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-500 dark:text-zinc-400 text-xs uppercase tracking-wider">花费</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-500 dark:text-zinc-400 text-xs uppercase tracking-wider">Channel</th>
						<th class="px-4 py-3 w-8"></th>
					</tr>
				</thead>
				<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
					{#each page.data as req}
						<tr
							class={clsx(
								'transition-colors cursor-pointer',
								expandedId === req.request_id
									? 'bg-zinc-50 dark:bg-zinc-800/50'
									: 'hover:bg-zinc-50 dark:hover:bg-zinc-800/30'
							)}
							onclick={() => toggleExpand(req.request_id)}
						>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 whitespace-nowrap font-mono">{formatDate(req.ts)}</td>
							<td class="px-4 py-3">
								<div class="flex flex-col">
									<span class="text-xs font-medium text-zinc-900 dark:text-zinc-100 truncate max-w-[180px]">{req.model_actual}</span>
									{#if req.model_requested !== req.model_actual}
										<span class="text-[10px] text-zinc-400 dark:text-zinc-500 truncate max-w-[180px]">{req.model_requested}</span>
									{/if}
								</div>
							</td>
							<td class="px-4 py-3">
								<span class={clsx('inline-block px-2 py-0.5 rounded-full text-xs font-medium', statusBadgeCls(req.status))}>
									{req.status}
								</span>
							</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 font-mono whitespace-nowrap">
								{formatLatency(req.latency_ms)}
								{#if req.stream}
									<span class="ml-1 text-[10px] text-zinc-400 dark:text-zinc-500">stream</span>
								{/if}
							</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 font-mono whitespace-nowrap">
								<span class="text-zinc-500 dark:text-zinc-400">{formatTokens(req.tokens_in)}</span>
								<span class="text-zinc-300 dark:text-zinc-600 mx-0.5">/</span>
								<span class="text-zinc-900 dark:text-zinc-100">{formatTokens(req.tokens_out)}</span>
							</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 font-mono">{formatCost(req.cost_usd)}</td>
							<td class="px-4 py-3 text-xs text-zinc-500 dark:text-zinc-400 font-mono truncate max-w-[100px]">{req.channel_id.slice(0, 8)}...</td>
							<td class="px-4 py-3 text-right">
								{#if expandedId === req.request_id}
									<ChevronUp size={14} class="text-zinc-400" />
								{:else}
									<ChevronDown size={14} class="text-zinc-400" />
								{/if}
							</td>
						</tr>
						{#if expandedId === req.request_id}
							<tr class="bg-zinc-50 dark:bg-zinc-800/50 animate-expand">
								<td colspan="8" class="px-4 py-4">
									<div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-xs mb-3">
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Request ID</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100 break-all">{req.request_id}</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Org / Project</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.org_id.slice(0, 8)}... / {req.project_id.slice(0, 8)}...</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">API Key</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.api_key_id.slice(0, 8)}...</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Client IP</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.client_ip ?? '—'}</p>
										</div>
									</div>
									<div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-xs mb-3">
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">TTFB</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{formatLatency(req.ttfb_ms)}</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Cached Tokens</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{formatTokens(req.tokens_cached)}</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Retries</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.retries}</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Stream</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.stream ? '是' : '否'}</p>
										</div>
									</div>
									{#if req.error_code}
										<div class="mt-2 p-3 rounded-lg bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800">
											<p class="text-xs font-medium text-red-700 dark:text-red-400 mb-1">Error</p>
											<p class="text-xs font-mono text-red-600 dark:text-red-400">{req.error_code}</p>
										</div>
									{/if}
									{#if req.metadata}
										<details class="mt-2">
											<summary class="text-xs text-zinc-500 dark:text-zinc-400 cursor-pointer hover:text-zinc-700 dark:hover:text-zinc-300">Metadata</summary>
											<pre class="mt-1 p-3 rounded-lg bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-700 text-[11px] font-mono text-zinc-800 dark:text-zinc-200 overflow-x-auto whitespace-pre-wrap break-all">{JSON.stringify(req.metadata, null, 2)}</pre>
										</details>
									{/if}
								</td>
							</tr>
						{/if}
					{/each}
				</tbody>
			</table>
		</div>

		<!-- Pagination -->
		<div class="flex items-center justify-between text-sm text-zinc-600 dark:text-zinc-400">
			<span class="text-xs">
				第 {pageNum} 页 · {page.data.length} 条
				{#if loading}
					<span class="ml-2 text-zinc-400">加载中...</span>
				{/if}
			</span>
			<div class="flex items-center gap-2">
				<Button variant="outline" size="sm" onclick={prevPage} disabled={cursorStack.length === 0 && !currentCursor || loading}>
					<ArrowLeft size={14} />
					<span class="ml-1">上一页</span>
				</Button>
				<Button variant="outline" size="sm" onclick={nextPage} disabled={!page.has_more || loading}>
					<span class="mr-1">下一页</span>
					<ArrowRight size={14} />
				</Button>
			</div>
		</div>
	{/if}
</div>
