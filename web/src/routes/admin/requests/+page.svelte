<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount, type Snippet } from 'svelte';
	import { getMe, listRequests, getFilterOptions } from '$lib/api.js';
	import type { RequestRecord, RequestPage, RequestListParams, MeResult, FilterOptions } from '$lib/api.js';
	import { Badge, Button, Card, Field, FilterPills, Input, ModalityBadge, Select } from '$lib/components/ui';
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
		SlidersHorizontal
	} from 'lucide-svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import FilterPanel from '$lib/components/templates/FilterPanel.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import {
		formatCost,
		formatLatency,
		formatRequestDate,
		formatTokens,
		rangeToDate,
		statusBadgeCls,
	} from '$lib/requests-helpers';

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



	let channelOptions = $derived([
		{ value: '', label: '全部' },
		...(filterOpts?.channels ?? []).map((ch) => ({ value: ch.id, label: ch.label ?? shortId(ch.id) }))
	]);

	let modelOptions = $derived([
		{ value: '', label: '全部' },
		...(filterOpts?.models ?? []).map((value) => ({ value, label: value }))
	]);

	let projectOptions = $derived([
		{ value: '', label: '全部' },
		...(filterOpts?.projects ?? []).map((project) => ({ value: project.id, label: project.label ?? shortId(project.id) }))
	]);

	let errorCodeOptions = $derived([
		{ value: '', label: '全部' },
		...(filterOpts?.error_codes ?? []).map((value) => ({ value, label: value }))
	]);

	const retriesOptions = [
		{ value: '', label: '全部' },
		{ value: 'true', label: '有重试' },
		{ value: 'false', label: '无重试' }
	];

	let pageNum = $derived(cursorStack.length + 1);
</script>

<PageShell title="请求日志" description="查看所有 API 请求记录，支持多维筛选" icon={ScrollText} max="full">
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={resetFilters}>
			<RotateCcw size={14} />
			<span class="ml-1">重置</span>
		</Button>
	{/snippet}

	<!-- L0: Quick Filters -->
	<DataToolbar badgesVisible={activeAdvanced.length > 0}>
		{#snippet query()}
			<Search size={14} class="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
			<Input
				id="admin-request-search"
				size="sm"
				bind:value={search}
				onkeydown={handleSearchKeydown}
				placeholder="搜索 model / error_code / request_id..."
				class="pl-9"
			/>
		{/snippet}

		{#snippet controls()}
			<FilterPills bind:value={filterRange} options={rangeOptions} />
			<FilterPills bind:value={filterStatusCat} options={statusCatOptions} />
			<FilterPills bind:value={filterStream} options={streamOptions} />
		{/snippet}

		{#snippet actions()}
			<Button variant={showAdvanced ? 'default' : 'outline'} size="sm" onclick={() => showAdvanced = !showAdvanced}>
				<SlidersHorizontal size={14} />
				高级筛选
				{#if activeAdvanced.length > 0}
					<span class="ml-1 inline-flex h-5 w-5 items-center justify-center rounded-full bg-zinc-700 text-[10px] font-bold text-white dark:bg-zinc-300 dark:text-zinc-900">{activeAdvanced.length}</span>
				{/if}
			</Button>

			<Button variant="default" size="sm" onclick={applyFilters} disabled={loading}>
				<Filter size={14} />
				<span class="ml-1">筛选</span>
			</Button>
		{/snippet}

		{#snippet badges()}
			{#each activeAdvanced as badge}
				<Badge>{badge}</Badge>
			{/each}
		{/snippet}
	</DataToolbar>

	<!-- Advanced Filter Panel -->
	<FilterPanel open={showAdvanced}>
		<!-- Row 1: Select filters -->
		<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
			<Field label="Model (actual)" for="admin-filter-model">
				{#if filterOpts && filterOpts.models.length > 0}
					<Select id="admin-filter-model" bind:value={filterModel} options={modelOptions} size="sm" />
				{:else}
					<Input id="admin-filter-model" size="sm" bind:value={filterModel} onkeydown={handleSearchKeydown} placeholder="model_actual" />
				{/if}
			</Field>
			<Field label="Model (requested)" for="admin-filter-model-requested">
				<Input id="admin-filter-model-requested" size="sm" bind:value={filterModelRequested} onkeydown={handleSearchKeydown} placeholder="model_requested" />
			</Field>
			<Field label="Channel" for="admin-filter-channel">
				{#if filterOpts && filterOpts.channels.length > 0}
					<Select id="admin-filter-channel" bind:value={filterChannel} options={channelOptions} size="sm" />
				{:else}
					<Input id="admin-filter-channel" size="sm" bind:value={filterChannel} onkeydown={handleSearchKeydown} placeholder="channel_id" />
				{/if}
			</Field>
			<Field label="Project" for="admin-filter-project">
				{#if filterOpts && filterOpts.projects.length > 0}
					<Select id="admin-filter-project" bind:value={filterProject} options={projectOptions} size="sm" />
				{:else}
					<Input id="admin-filter-project" size="sm" bind:value={filterProject} onkeydown={handleSearchKeydown} placeholder="project_id" />
				{/if}
			</Field>
		</div>

		<!-- Row 2: More selects -->
		<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
			<Field label="Org ID" for="admin-filter-org">
				<Input id="admin-filter-org" size="sm" bind:value={filterOrgId} onkeydown={handleSearchKeydown} placeholder="org_id" class="font-mono text-xs" />
			</Field>
			<Field label="API Key ID" for="admin-filter-api-key">
				<Input id="admin-filter-api-key" size="sm" bind:value={filterApiKeyId} onkeydown={handleSearchKeydown} placeholder="api_key_id" class="font-mono text-xs" />
			</Field>
			<Field label="Error Code" for="admin-filter-error-code">
				{#if filterOpts && filterOpts.error_codes.length > 0}
					<Select id="admin-filter-error-code" bind:value={filterErrorCode} options={errorCodeOptions} size="sm" />
				{:else}
					<Input id="admin-filter-error-code" size="sm" bind:value={filterErrorCode} onkeydown={handleSearchKeydown} placeholder="error_code" />
				{/if}
			</Field>
			<Field label="重试" for="admin-filter-retries">
				<Select id="admin-filter-retries" bind:value={filterHasRetries} options={retriesOptions} size="sm" />
			</Field>
		</div>

		<!-- Row 3: Range filters -->
		<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
			<Field label="延迟 (ms)" for="admin-filter-latency-min">
				<div class="flex gap-1">
					<Input id="admin-filter-latency-min" type="number" size="sm" bind:value={latencyMin} placeholder="min" />
					<Input id="admin-filter-latency-max" type="number" size="sm" bind:value={latencyMax} placeholder="max" />
				</div>
			</Field>
			<Field label="TTFB (ms)" for="admin-filter-ttfb-min">
				<div class="flex gap-1">
					<Input id="admin-filter-ttfb-min" type="number" size="sm" bind:value={ttfbMin} placeholder="min" />
					<Input id="admin-filter-ttfb-max" type="number" size="sm" bind:value={ttfbMax} placeholder="max" />
				</div>
			</Field>
			<Field label="费用 ($)" for="admin-filter-cost-min">
				<div class="flex gap-1">
					<Input id="admin-filter-cost-min" type="number" step="0.0001" size="sm" bind:value={costMin} placeholder="min" />
					<Input id="admin-filter-cost-max" type="number" step="0.0001" size="sm" bind:value={costMax} placeholder="max" />
				</div>
			</Field>
			<Field label="Tokens (in+out)" for="admin-filter-tokens-min">
				<div class="flex gap-1">
					<Input id="admin-filter-tokens-min" type="number" size="sm" bind:value={tokensMin} placeholder="min" />
					<Input id="admin-filter-tokens-max" type="number" size="sm" bind:value={tokensMax} placeholder="max" />
				</div>
			</Field>
		</div>

		<!-- Row 4: ID filters -->
		<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
			<Field label="User ID" for="admin-filter-user">
				<Input id="admin-filter-user" size="sm" bind:value={filterUserId} onkeydown={handleSearchKeydown} placeholder="user_id" class="font-mono text-xs" />
			</Field>
			<Field label="Group ID" for="admin-filter-group">
				<Input id="admin-filter-group" size="sm" bind:value={filterGroupId} onkeydown={handleSearchKeydown} placeholder="group_id" class="font-mono text-xs" />
			</Field>
		</div>
	</FilterPanel>

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
		<!-- 0.4.156（第四刀 #2 step 1）：抽 requestRowSnippet + expandedRowSnippet。
		     为 0.4.157 引入「展开为空时 → virtualize / 展开有效时 → legacy」双轨打底。
		     本步只重构 snippet 结构，行为不变（仍 #each 渲染）。 -->
		{#snippet requestRowSnippet(req: RequestRecord, _i: number)}
			<tr
				class={cn(dataTemplate.rowInteractive, expandedId === req.request_id && dataTemplate.rowSelected)}
				onclick={() => toggleExpand(req.request_id)}
			>
				<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 whitespace-nowrap font-mono">{formatRequestDate(req.ts)}</td>
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
					<span class={cn('inline-block px-2 py-0.5 rounded-full text-xs font-medium', statusBadgeCls(req.status))}>
						{req.status}
					</span>
				</td>
				<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 font-mono whitespace-nowrap">
					{formatLatency(req.latency_ms)}
					{#if req.stream}
						<span class="ml-1 text-[10px] text-zinc-400 dark:text-zinc-500">stream 流式</span>
					{/if}
				</td>
				<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 font-mono whitespace-nowrap">
					<span class="text-zinc-500 dark:text-zinc-400">{formatTokens(req.tokens_in)}</span>
					<span class="text-zinc-300 dark:text-zinc-600 mx-0.5">/</span>
					<span class="text-zinc-900 dark:text-zinc-100">{formatTokens(req.tokens_out)}</span>
				</td>
				<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 font-mono">{formatCost(req.cost_usd)}</td>
				<td class="px-4 py-3 text-xs text-zinc-500 dark:text-zinc-400 font-mono truncate max-w-[100px]">{req.channel_id ? `${shortId(req.channel_id)}...` : '—'}</td>
				<td class="px-4 py-3 text-right">
					{#if expandedId === req.request_id}
						<ChevronUp size={14} class="text-zinc-400" />
					{:else}
						<ChevronDown size={14} class="text-zinc-400" />
					{/if}
				</td>
			</tr>
		{/snippet}

		{#snippet expandedRowSnippet(req: RequestRecord)}
			<tr class={dataTemplate.rowSelected}>
				<td colspan="8" class="px-4 py-4">
					<div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-xs mb-3">
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Request ID 请求 ID</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100 break-all">{req.request_id}</p>
						</div>
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Org / Project 归属</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100">{shortId(req.org_id)}... / {shortId(req.project_id)}...</p>
						</div>
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">API Key 凭据</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100">{shortId(req.api_key_id)}...</p>
						</div>
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Client IP 客户端 IP</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.client_ip ?? '—'}</p>
						</div>
					</div>

					<div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-xs mb-3">
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">TTFB</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100">{formatLatency(req.ttfb_ms)}</p>
						</div>
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Cached tokens 缓存</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100">{formatTokens(req.tokens_cached)}</p>
						</div>
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Retries 重试</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.retries}</p>
						</div>
						<div>
							<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Stream 流式</p>
							<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.stream ? '是' : '否'}</p>
						</div>
					</div>

					{#if req.user_id}
						<div class="grid grid-cols-2 md:grid-cols-4 gap-4 text-xs mb-3">
							<div>
								<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">User ID 用户</p>
								<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.user_id}</p>
							</div>
							{#if req.group_id}
								<div>
									<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Group ID 分组</p>
									<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.group_id}</p>
								</div>
							{/if}
							{#if req.channel_key_id}
								<div>
									<p class="text-zinc-500 dark:text-zinc-400 mb-0.5">Channel Key ID 凭据</p>
									<p class="font-mono text-zinc-900 dark:text-zinc-100">{req.channel_key_id}</p>
								</div>
							{/if}
						</div>
					{/if}

					{#if req.error_code}
						<div class="mt-2 p-3 rounded-lg bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800">
							<p class="text-xs font-medium text-red-700 dark:text-red-400 mb-1">Error 错误</p>
							<p class="text-xs font-mono text-red-600 dark:text-red-400">{req.error_code}</p>
						</div>
					{/if}

					{#if req.metadata}
						<details class="mt-2">
							<summary class="text-xs text-zinc-500 dark:text-zinc-400 cursor-pointer hover:text-zinc-700 dark:hover:text-zinc-300">Metadata 元数据</summary>
							<pre class="mt-1 p-3 rounded-lg bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-700 text-[11px] font-mono text-zinc-800 dark:text-zinc-200 overflow-x-auto whitespace-pre-wrap break-all">{JSON.stringify(req.metadata, null, 2)}</pre>
						</details>
					{/if}
				</td>
			</tr>
		{/snippet}

		<!-- 0.4.157（第四刀 #2 step 2）：双轨 — 无展开走 virtualize（等高 48），
		     有展开退 legacy（变高破坏虚拟定位）。
		     默认 page_size=50 已小于 virtualize 触发阈值，本步在大 page 时收益明显。 -->
		{#if expandedId === null && page.data.length >= 40}
			<DataTable
				stickyHead
				maxHeight="70vh"
				rows={page.data}
				rowSnippet={requestRowSnippet as unknown as Snippet<[RequestRecord, number]>}
				rowHeight={48}
				overscan={6}
			>
				{#snippet head()}
					<tr>
						<th class={dataTemplate.th}>时间</th>
						<th class={dataTemplate.th}>模型</th>
						<th class={dataTemplate.th}>状态</th>
						<th class={dataTemplate.th}>延迟</th>
						<th class={dataTemplate.th}>Tokens 用量</th>
						<th class={dataTemplate.th}>花费</th>
						<th class={dataTemplate.th}>Channel</th>
						<th class="px-4 py-3 w-8"></th>
					</tr>
				{/snippet}
			</DataTable>
		{:else}
			<DataTable stickyHead maxHeight="70vh">
				{#snippet head()}
					<tr>
						<th class={dataTemplate.th}>时间</th>
						<th class={dataTemplate.th}>模型</th>
						<th class={dataTemplate.th}>状态</th>
						<th class={dataTemplate.th}>延迟</th>
						<th class={dataTemplate.th}>Tokens 用量</th>
						<th class={dataTemplate.th}>花费</th>
						<th class={dataTemplate.th}>Channel</th>
						<th class="px-4 py-3 w-8"></th>
					</tr>
				{/snippet}

				{#each page.data as req, i (req.request_id)}
					{@render requestRowSnippet(req, i)}
					{#if expandedId === req.request_id}
						{@render expandedRowSnippet(req)}
					{/if}
				{/each}
			</DataTable>
		{/if}

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
</PageShell>
