<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { getMe, listRequests, getFilterOptions } from '$lib/api.js';
	import type { RequestRecord, RequestPage, RequestListParams, MeResult, FilterOptions } from '$lib/api.js';
	import Card from '$lib/components/ui/Card.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import FilterPills from '$lib/components/ui/FilterPills.svelte';
	import ModalityBadge from '$lib/components/ui/ModalityBadge.svelte';
	import {
		ScrollText,
		Search,
		ArrowRight,
		ArrowLeft,
		ChevronDown,
		ChevronUp,
		XCircle,
		Filter,
		RotateCcw,
		SlidersHorizontal,
		X
	} from 'lucide-svelte';
	import { clsx } from 'clsx';

	let me = $state<MeResult | null>(null);
	let page = $state<RequestPage | null>(null);
	let loading = $state(true);
	let error = $state('');
	let expandedId = $state<string | null>(null);
	let filterOpts = $state<FilterOptions | null>(null);
	let showAdvanced = $state(false);

	// L0: Quick filters
	let search = $state('');
	let filterRange = $state('24h');
	let filterStatusCat = $state('');
	let filterStream = $state('');

	// L1: Dropdown selects
	let filterModel = $state('');
	let filterChannel = $state('');
	let filterProject = $state('');
	let filterOrgId = $state('');
	let filterGroupId = $state('');
	let filterUserId = $state('');
	let filterApiKeyId = $state('');
	let filterErrorCode = $state('');
	let filterModelRequested = $state('');
	let filterHasRetries = $state('');

	// L2: Range filters
	let latencyMin = $state('');
	let latencyMax = $state('');
	let ttfbMin = $state('');
	let ttfbMax = $state('');
	let costMin = $state('');
	let costMax = $state('');
	let tokensMin = $state('');
	let tokensMax = $state('');

	// Pagination
	let cursorStack = $state<string[]>([]);
	let currentCursor = $state<string | undefined>(undefined);

	const statusCatOptions = [
		{ value: '', label: '全部状态' },
		{ value: '2xx', label: '2xx 成功' },
		{ value: '4xx', label: '4xx 客户端' },
		{ value: '5xx', label: '5xx 服务端' }
	];

	const rangeOptions = [
		{ value: '1h', label: '1 小时' },
		{ value: '24h', label: '24 小时' },
		{ value: '7d', label: '7 天' },
		{ value: '30d', label: '30 天' }
	];

	const streamOptions = [
		{ value: '', label: '全部' },
		{ value: 'true', label: 'Stream' },
		{ value: 'false', label: '非 Stream' }
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
		await Promise.all([load(), loadFilterOptions()]);
	});

	async function loadFilterOptions() {
		try {
			filterOpts = await getFilterOptions(undefined, 168);
		} catch { /* silent */ }
	}

	async function load() {
		loading = true;
		error = '';
		try {
			const params: RequestListParams = { limit: 50 };
			if (search.trim()) params.search = search.trim();
			params.from = rangeToDate(filterRange);
			if (currentCursor) params.cursor = currentCursor;

			// status
			if (filterStatusCat) params.status_category = filterStatusCat;

			// stream
			if (filterStream === 'true') params.stream = true;
			else if (filterStream === 'false') params.stream = false;

			// L1 selects
			if (filterModel) params.model = filterModel;
			if (filterModelRequested) params.model_requested = filterModelRequested;
			if (filterChannel) params.channel_id = filterChannel;
			if (filterProject) params.project_id = filterProject;
			if (filterOrgId) params.org_id = filterOrgId;
			if (filterGroupId) params.group_id = filterGroupId;
			if (filterUserId) params.user_id = filterUserId;
			if (filterApiKeyId) params.api_key_id = filterApiKeyId;
			if (filterErrorCode) params.error_code = filterErrorCode;
			if (filterHasRetries === 'true') params.has_retries = true;
			else if (filterHasRetries === 'false') params.has_retries = false;

			// L2 ranges
			if (latencyMin) params.latency_min = Number(latencyMin);
			if (latencyMax) params.latency_max = Number(latencyMax);
			if (ttfbMin) params.ttfb_min = Number(ttfbMin);
			if (ttfbMax) params.ttfb_max = Number(ttfbMax);
			if (costMin) params.cost_min = Number(costMin);
			if (costMax) params.cost_max = Number(costMax);
			if (tokensMin) params.tokens_min = Number(tokensMin);
			if (tokensMax) params.tokens_max = Number(tokensMax);

			page = await listRequests(params);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	function resetFilters() {
		search = '';
		filterRange = '24h';
		filterStatusCat = '';
		filterStream = '';
		filterModel = '';
		filterModelRequested = '';
		filterChannel = '';
		filterProject = '';
		filterOrgId = '';
		filterGroupId = '';
		filterUserId = '';
		filterApiKeyId = '';
		filterErrorCode = '';
		filterHasRetries = '';
		latencyMin = '';
		latencyMax = '';
		ttfbMin = '';
		ttfbMax = '';
		costMin = '';
		costMax = '';
		tokensMin = '';
		tokensMax = '';
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

	// Active advanced filter badges
	let activeAdvanced = $derived(
		[
			filterModel && `模型: ${filterModel}`,
			filterModelRequested && `请求模型: ${filterModelRequested}`,
			filterChannel && `Channel: ${shortId(filterChannel)}`,
			filterProject && `Project: ${shortId(filterProject)}`,
			filterOrgId && `Org: ${shortId(filterOrgId)}`,
			filterGroupId && `Group: ${shortId(filterGroupId)}`,
			filterUserId && `User: ${shortId(filterUserId)}`,
			filterApiKeyId && `API Key: ${shortId(filterApiKeyId)}`,
			filterErrorCode && `错误码: ${filterErrorCode}`,
			filterHasRetries === 'true' && '有重试',
			filterHasRetries === 'false' && '无重试',
			latencyMin && `延迟≥${latencyMin}ms`,
			latencyMax && `延迟≤${latencyMax}ms`,
			ttfbMin && `TTFB≥${ttfbMin}ms`,
			ttfbMax && `TTFB≤${ttfbMax}ms`,
			costMin && `费用≥$${costMin}`,
			costMax && `费用≤$${costMax}`,
			tokensMin && `Tokens≥${tokensMin}`,
			tokensMax && `Tokens≤${tokensMax}`,
		].filter(Boolean) as string[]
	);

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

	<!-- L0: Quick Filters -->
	<div class="flex flex-wrap items-center gap-3 mb-3">
		<div class="relative flex-1 min-w-[200px] max-w-sm">
			<Search size={14} class="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
			<input
				type="text"
				bind:value={search}
				onkeydown={handleSearchKeydown}
				placeholder="搜索 model / error_code / request_id..."
				class="w-full h-9 pl-9 pr-3 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300"
			/>
		</div>

		<FilterPills bind:value={filterRange} options={rangeOptions} />
		<FilterPills bind:value={filterStatusCat} options={statusCatOptions} />
		<FilterPills bind:value={filterStream} options={streamOptions} />

		<button
			onclick={() => showAdvanced = !showAdvanced}
			class={clsx(
				'inline-flex items-center gap-1.5 h-9 px-3 rounded-lg border text-sm transition-colors',
				showAdvanced
					? 'border-zinc-900 dark:border-zinc-100 bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900'
					: 'border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-50 dark:hover:bg-zinc-800'
			)}
		>
			<SlidersHorizontal size={14} />
			高级筛选
			{#if activeAdvanced.length > 0}
				<span class="ml-1 inline-flex items-center justify-center w-5 h-5 rounded-full bg-zinc-700 dark:bg-zinc-300 text-[10px] font-bold text-white dark:text-zinc-900">{activeAdvanced.length}</span>
			{/if}
		</button>

		<Button variant="default" size="sm" onclick={applyFilters} disabled={loading}>
			<Filter size={14} />
			<span class="ml-1">筛选</span>
		</Button>
	</div>

	<!-- Active filter badges -->
	{#if activeAdvanced.length > 0}
		<div class="flex flex-wrap gap-1.5 mb-3">
			{#each activeAdvanced as badge}
				<span class="inline-flex items-center gap-1 px-2 py-0.5 rounded-md bg-zinc-100 dark:bg-zinc-800 text-xs text-zinc-700 dark:text-zinc-300 border border-zinc-200 dark:border-zinc-700">
					{badge}
				</span>
			{/each}
		</div>
	{/if}

	<!-- Advanced Filter Panel -->
	{#if showAdvanced}
		<div class="mb-4 p-4 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/40 space-y-4">
			<!-- Row 1: Select filters -->
			<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Model (actual)</label>
					{#if filterOpts && filterOpts.models.length > 0}
						<select bind:value={filterModel} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100">
							<option value="">全部</option>
							{#each filterOpts.models as m}
								<option value={m}>{m}</option>
							{/each}
						</select>
					{:else}
						<input type="text" bind:value={filterModel} onkeydown={handleSearchKeydown} placeholder="model_actual" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					{/if}
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Model (requested)</label>
					<input type="text" bind:value={filterModelRequested} onkeydown={handleSearchKeydown} placeholder="model_requested" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Channel</label>
					{#if filterOpts && filterOpts.channels.length > 0}
						<select bind:value={filterChannel} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100">
							<option value="">全部</option>
							{#each filterOpts.channels as ch}
								<option value={ch.id}>{ch.label ?? shortId(ch.id)}</option>
							{/each}
						</select>
					{:else}
						<input type="text" bind:value={filterChannel} onkeydown={handleSearchKeydown} placeholder="channel_id" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					{/if}
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Project</label>
					{#if filterOpts && filterOpts.projects.length > 0}
						<select bind:value={filterProject} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100">
							<option value="">全部</option>
							{#each filterOpts.projects as p}
								<option value={p.id}>{p.label ?? shortId(p.id)}</option>
							{/each}
						</select>
					{:else}
						<input type="text" bind:value={filterProject} onkeydown={handleSearchKeydown} placeholder="project_id" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					{/if}
				</div>
			</div>

			<!-- Row 2: More selects -->
			<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Org ID</label>
					<input type="text" bind:value={filterOrgId} onkeydown={handleSearchKeydown} placeholder="org_id" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 font-mono text-xs" />
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">API Key ID</label>
					<input type="text" bind:value={filterApiKeyId} onkeydown={handleSearchKeydown} placeholder="api_key_id" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 font-mono text-xs" />
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Error Code</label>
					{#if filterOpts && filterOpts.error_codes.length > 0}
						<select bind:value={filterErrorCode} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100">
							<option value="">全部</option>
							{#each filterOpts.error_codes as ec}
								<option value={ec}>{ec}</option>
							{/each}
						</select>
					{:else}
						<input type="text" bind:value={filterErrorCode} onkeydown={handleSearchKeydown} placeholder="error_code" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					{/if}
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">重试</label>
					<select bind:value={filterHasRetries} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100">
						<option value="">全部</option>
						<option value="true">有重试</option>
						<option value="false">无重试</option>
					</select>
				</div>
			</div>

			<!-- Row 3: Range filters -->
			<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">延迟 (ms)</label>
					<div class="flex gap-1">
						<input type="number" bind:value={latencyMin} placeholder="min" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
						<input type="number" bind:value={latencyMax} placeholder="max" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					</div>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">TTFB (ms)</label>
					<div class="flex gap-1">
						<input type="number" bind:value={ttfbMin} placeholder="min" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
						<input type="number" bind:value={ttfbMax} placeholder="max" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					</div>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">费用 ($)</label>
					<div class="flex gap-1">
						<input type="number" step="0.0001" bind:value={costMin} placeholder="min" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
						<input type="number" step="0.0001" bind:value={costMax} placeholder="max" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					</div>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Tokens (in+out)</label>
					<div class="flex gap-1">
						<input type="number" bind:value={tokensMin} placeholder="min" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
						<input type="number" bind:value={tokensMax} placeholder="max" class="w-1/2 h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
					</div>
				</div>
			</div>

			<!-- Row 4: ID filters -->
			<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">User ID</label>
					<input type="text" bind:value={filterUserId} onkeydown={handleSearchKeydown} placeholder="user_id" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 font-mono text-xs" />
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">Group ID</label>
					<input type="text" bind:value={filterGroupId} onkeydown={handleSearchKeydown} placeholder="group_id" class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 font-mono text-xs" />
				</div>
			</div>
		</div>
	{/if}

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
									<div class="flex items-center gap-1.5">
										<span class="text-xs font-medium text-zinc-900 dark:text-zinc-100 truncate max-w-[180px]">{req.model_actual}</span>
										<ModalityBadge model={req.model_actual} metadata={req.metadata} />
									</div>
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
							<td class="px-4 py-3 text-xs text-zinc-500 dark:text-zinc-400 font-mono truncate max-w-[100px]">{shortId(req.channel_id)}...</td>
							<td class="px-4 py-3 text-right">
								{#if expandedId === req.request_id}
									<ChevronUp size={14} class="text-zinc-400" />
								{:else}
									<ChevronDown size={14} class="text-zinc-400" />
								{/if}
							</td>
						</tr>
						{#if expandedId === req.request_id}
							<tr class="bg-zinc-50 dark:bg-zinc-800/50">
								<td colspan="8" class="px-4 py-4">
									<div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-xs mb-3">
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Request ID</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100 break-all">{req.request_id}</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Org / Project</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{shortId(req.org_id)}... / {shortId(req.project_id)}...</p>
										</div>
										<div>
											<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">API Key</p>
											<p class="font-mono text-zinc-900 dark:text-zinc-100">{shortId(req.api_key_id)}...</p>
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
									{#if req.user_id}
										<div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-xs mb-3">
											<div>
												<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">User ID</p>
												<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.user_id}</p>
											</div>
											{#if req.group_id}
												<div>
													<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Group ID</p>
													<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.group_id}</p>
												</div>
											{/if}
											{#if req.channel_key_id}
												<div>
													<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Channel Key ID</p>
													<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.channel_key_id}</p>
												</div>
											{/if}
										</div>
									{/if}
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
