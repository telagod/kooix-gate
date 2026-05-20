<!-- /admin/audit — 审计日志 (platform admin only) -->
<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { getMe, listAuditLogs } from '$lib/api.js';
	import type { AuditLog, AuditLogSortBy, SortDir } from '$lib/api.js';
	import { Badge, Button, Select, Skeleton } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { dataTemplate, text, cn } from '$lib/design';
	import type { BadgeVariant } from '$lib/design';
	import {
		ChevronDown,
		ChevronUp,
		Eye,
		EyeOff,
		RefreshCcw,
		ScrollText,
		ShieldAlert
	} from 'lucide-svelte';
	import {
		currentPageFromOffset,
		hiddenColumnsFromVisible,
		loadTableState,
		nextSortDir,
		normalizePageSize,
		normalizeSortBy,
		normalizeSortDir,
		toggleColumnVisibility,
		visibleColumnsFromHidden,
		saveTableState
	} from '$lib/table-state.js';
	import type { TableColumn } from '$lib/table-state.js';

	const TABLE_KEY = 'admin-audit';
	const PAGE_SIZES = [25, 50, 100, 200] as const;
	const DEFAULT_PAGE_SIZE = 50;
	const SORT_FIELDS = ['ts', 'actor_kind', 'action', 'resource_kind', 'outcome'] as const;
	const columns: TableColumn[] = [
		{ id: 'ts', label: '时间', required: true },
		{ id: 'actor', label: '操作者' },
		{ id: 'action', label: '动作', required: true },
		{ id: 'resource_kind', label: '资源类型' },
		{ id: 'resource_id', label: '资源 ID' },
		{ id: 'outcome', label: '结果', required: true }
	];
	const defaultVisibleColumns = columns.map((column) => column.id);
	const pageSizeOptions = PAGE_SIZES.map((size) => ({ value: String(size), label: `${size} / 页` }));
	const sortOptions = [
		{ value: 'ts', label: '时间' },
		{ value: 'actor_kind', label: '操作者' },
		{ value: 'action', label: '动作' },
		{ value: 'resource_kind', label: '资源类型' },
		{ value: 'outcome', label: '结果' }
	];
	const sortDirOptions = [
		{ value: 'desc', label: 'Desc 降序' },
		{ value: 'asc', label: 'Asc 升序' }
	];

	let logs = $state<AuditLog[]>([]);
	let loading = $state(true);
	let refreshing = $state(false);
	let error = $state('');
	let isPlatformAdmin = $state(false);
	let orgs = $state<string[]>([]);
	let selectedOrg = $state('');
	let offset = $state(0);
	let pageSize = $state(String(DEFAULT_PAGE_SIZE));
	let sortBy = $state('ts');
	let sortDir = $state('desc');
	let hiddenColumns = $state<string[]>([]);
	let expandedId = $state<string | null>(null);

	onMount(async () => {
		const saved = loadTableState(TABLE_KEY, {
			pageSize: DEFAULT_PAGE_SIZE,
			sortBy: 'ts',
			sortDir: 'desc',
			visibleColumns: defaultVisibleColumns,
			filters: {}
		});
		pageSize = String(normalizePageSize(saved.pageSize, PAGE_SIZES, DEFAULT_PAGE_SIZE));
		sortBy = normalizeSortBy(saved.sortBy, SORT_FIELDS, 'ts');
		sortDir = normalizeSortDir(saved.sortDir, 'desc');
		hiddenColumns = hiddenColumnsFromVisible(columns, saved.visibleColumns);

		try {
			const me = await getMe();
			isPlatformAdmin = me.is_platform_admin;
			orgs = me.orgs ?? [];

			if (!isPlatformAdmin) {
				loading = false;
				return;
			}

			if (orgs.length > 0) {
				selectedOrg = orgs[0];
			}
		} catch (err: any) {
			error = err?.message ?? '加载身份失败';
			loading = false;
			return;
		}

		if (selectedOrg) {
			await loadLogs();
		} else {
			loading = false;
		}
	});

	function persistTableState() {
		saveTableState(TABLE_KEY, {
			pageSize: pageSizeNumber,
			sortBy: auditSortBy,
			sortDir: auditSortDir,
			visibleColumns: visibleColumns.map((column) => column.id),
			filters: {}
		});
	}

	async function loadLogs(nextOffset = offset) {
		if (!selectedOrg) return;
		refreshing = true;
		loading = logs.length === 0;
		error = '';
		try {
			offset = Math.max(0, nextOffset);
			logs = await listAuditLogs(selectedOrg, {
				limit: pageSizeNumber,
				offset,
				sort_by: auditSortBy,
				sort_dir: auditSortDir
			});
			persistTableState();
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
			refreshing = false;
		}
	}

	async function handleOrgChange() {
		expandedId = null;
		await loadLogs(0);
	}

	async function handlePageSizeChange() {
		pageSize = String(normalizePageSize(Number(pageSize), PAGE_SIZES, DEFAULT_PAGE_SIZE));
		expandedId = null;
		await loadLogs(0);
	}

	async function handleSortChange() {
		sortBy = normalizeSortBy(sortBy, SORT_FIELDS, 'ts');
		sortDir = normalizeSortDir(sortDir, 'desc');
		expandedId = null;
		await loadLogs(0);
	}

	async function sortColumn(field: AuditLogSortBy, initial: SortDir = 'asc') {
		sortDir = nextSortDir(auditSortBy, auditSortDir, field, initial);
		sortBy = field;
		expandedId = null;
		await loadLogs(0);
	}

	async function prevPage() {
		if (!hasPrev) return;
		expandedId = null;
		await loadLogs(Math.max(0, offset - pageSizeNumber));
	}

	async function nextPage() {
		if (!hasNext) return;
		expandedId = null;
		await loadLogs(offset + pageSizeNumber);
	}

	function toggleExpand(id: string) {
		expandedId = expandedId === id ? null : id;
	}

	function toggleColumn(id: string) {
		hiddenColumns = toggleColumnVisibility(columns, hiddenColumns, id);
		persistTableState();
	}

	function isVisible(id: string): boolean {
		return visibleColumns.some((column) => column.id === id);
	}

	function sortLabel(field: AuditLogSortBy): string {
		if (sortBy !== field) return '';
		return sortDir === 'asc' ? ' ↑' : ' ↓';
	}

	function formatDate(s: string): string {
		try {
			return new Date(s).toLocaleString('zh-CN', {
				year: 'numeric',
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

	function outcomeVariant(outcome: string): BadgeVariant {
		if (outcome === 'success' || outcome === 'ok') return 'success';
		if (outcome === 'denied' || outcome === 'forbidden') return 'danger';
		return 'default';
	}

	let visibleColumns = $derived(visibleColumnsFromHidden(columns, hiddenColumns));
	let pageSizeNumber = $derived(normalizePageSize(Number(pageSize), PAGE_SIZES, DEFAULT_PAGE_SIZE));
	let auditSortBy = $derived(normalizeSortBy(sortBy, SORT_FIELDS, 'ts') as AuditLogSortBy);
	let auditSortDir = $derived(normalizeSortDir(sortDir, 'desc'));
	let currentPage = $derived(currentPageFromOffset(offset, pageSizeNumber));
	let hasPrev = $derived(offset > 0);
	let hasNext = $derived(logs.length === pageSizeNumber);
	let hasHiddenColumns = $derived(hiddenColumns.length > 0);
</script>

<PageShell title="审计日志" description="平台级审计记录，仅 Platform Admin 可见。" icon={ScrollText} max="wide">
	{#snippet actions()}
		<Button variant="outline" onclick={() => loadLogs()} disabled={!selectedOrg || refreshing || loading}>
			<RefreshCcw size={14} class={refreshing ? 'animate-spin' : ''} />
			刷新
		</Button>
	{/snippet}

	{#if !isPlatformAdmin && !loading}
		<StatePanel title="无访问权限" description="审计日志仅限 Platform Admin 查看。" icon={ShieldAlert} variant="warning" />
	{:else if !selectedOrg && !loading}
		<StatePanel title="缺少 Org" description="当前账号没有关联任何 Org，无法查询审计日志。" icon={ScrollText} />
	{:else}
		<DataToolbar badgesVisible={hasHiddenColumns}>
			{#snippet query()}
				<div class="flex items-center gap-2">
					<span class="text-xs font-medium {text.muted}">Org</span>
					<Select
						id="audit-org-select"
						bind:value={selectedOrg}
						onchange={handleOrgChange}
						disabled={loading || orgs.length === 0}
						options={orgs.map((org) => ({ value: org, label: org }))}
						size="sm"
						class="font-mono text-xs"
					/>
				</div>
			{/snippet}

			{#snippet controls()}
				<Select bind:value={pageSize} options={pageSizeOptions} onchange={handlePageSizeChange} size="sm" class="w-36" />
				<Select bind:value={sortBy} options={sortOptions} onchange={handleSortChange} size="sm" class="w-36" />
				<Select bind:value={sortDir} options={sortDirOptions} onchange={handleSortChange} size="sm" class="w-28" />
			{/snippet}

			{#snippet actions()}
				<div class="flex flex-wrap items-center gap-1 rounded-lg border border-zinc-200 bg-white p-1 dark:border-zinc-700 dark:bg-zinc-900">
					{#each columns as column}
						<Button
							variant={isVisible(column.id) ? 'outline' : 'ghost'}
							size="sm"
							disabled={column.required}
							onclick={() => toggleColumn(column.id)}
							class="h-7 px-2"
						>
							{#if isVisible(column.id)}
								<Eye size={12} />
							{:else}
								<EyeOff size={12} />
							{/if}
							{column.label}
						</Button>
					{/each}
				</div>
			{/snippet}

			{#snippet badges()}
				<Badge>隐藏列：{hiddenColumns.length}</Badge>
				<Badge>已保存筛选</Badge>
			{/snippet}
		</DataToolbar>

		{#if loading}
			<div class="space-y-2">
				{#each Array(6) as _}
					<Skeleton class="h-12" />
				{/each}
			</div>
		{:else if error}
			<StatePanel title="加载失败" description={error} icon={ShieldAlert} variant="danger">
				{#snippet actions()}
					<Button onclick={() => loadLogs()} disabled={refreshing}>重试</Button>
				{/snippet}
			</StatePanel>
		{:else}
			<DataTable isEmpty={logs.length === 0} emptyColspan={visibleColumns.length + 1}>
				{#snippet head()}
					<tr>
						{#if isVisible('ts')}
							<th class={dataTemplate.th}>
								<button type="button" class="uppercase tracking-wider" onclick={() => sortColumn('ts', 'desc')}>时间{sortLabel('ts')}</button>
							</th>
						{/if}
						{#if isVisible('actor')}
							<th class={dataTemplate.th}>
								<button type="button" class="uppercase tracking-wider" onclick={() => sortColumn('actor_kind')}>操作者{sortLabel('actor_kind')}</button>
							</th>
						{/if}
						{#if isVisible('action')}
							<th class={dataTemplate.th}>
								<button type="button" class="uppercase tracking-wider" onclick={() => sortColumn('action')}>动作{sortLabel('action')}</button>
							</th>
						{/if}
						{#if isVisible('resource_kind')}
							<th class={dataTemplate.th}>
								<button type="button" class="uppercase tracking-wider" onclick={() => sortColumn('resource_kind')}>资源类型{sortLabel('resource_kind')}</button>
							</th>
						{/if}
						{#if isVisible('resource_id')}
							<th class={dataTemplate.th}>资源 ID</th>
						{/if}
						{#if isVisible('outcome')}
							<th class={dataTemplate.th}>
								<button type="button" class="uppercase tracking-wider" onclick={() => sortColumn('outcome')}>结果{sortLabel('outcome')}</button>
							</th>
						{/if}
						<th class="px-4 py-3 w-8"></th>
					</tr>
				{/snippet}

				{#snippet empty()}
					<div class="flex flex-col items-center gap-2 py-4">
						<ScrollText size={28} class={text.disabled} />
						<p>此 Org 暂无审计记录。</p>
					</div>
				{/snippet}

				{#each logs as log}
					<tr
						class={cn(dataTemplate.rowInteractive, expandedId === log.id && dataTemplate.rowSelected)}
						onclick={() => toggleExpand(log.id)}
					>
						{#if isVisible('ts')}
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 whitespace-nowrap font-mono">{formatDate(log.ts)}</td>
						{/if}
						{#if isVisible('actor')}
							<td class={dataTemplate.tdMono}>
								<span class={text.muted}>{log.actor_kind}/</span>{log.actor_id ? shortId(log.actor_id) : '—'}
							</td>
						{/if}
						{#if isVisible('action')}
							<td class={dataTemplate.tdMonoStrong}>{log.action}</td>
						{/if}
						{#if isVisible('resource_kind')}
							<td class={dataTemplate.td}>{log.resource_kind}</td>
						{/if}
						{#if isVisible('resource_id')}
							<td class={dataTemplate.tdMono}>{log.resource_id ? shortId(log.resource_id) : '—'}</td>
						{/if}
						{#if isVisible('outcome')}
							<td class="px-4 py-3"><Badge variant={outcomeVariant(log.outcome)}>{log.outcome}</Badge></td>
						{/if}
						<td class="px-4 py-3 text-right">
							{#if log.after !== null}
								{#if expandedId === log.id}
									<ChevronUp size={14} class={text.muted} />
								{:else}
									<ChevronDown size={14} class={text.muted} />
								{/if}
							{/if}
						</td>
					</tr>
					{#if expandedId === log.id && log.after !== null}
						<tr class={dataTemplate.rowSelected}>
							<td colspan={visibleColumns.length + 1} class="px-4 py-3">
								<div class="mb-1 text-xs font-medium {text.muted}">After 变更后</div>
								<pre class="overflow-x-auto whitespace-pre-wrap break-all rounded-md border border-zinc-200 bg-white p-3 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{JSON.stringify(log.after, null, 2)}</pre>
							</td>
						</tr>
					{/if}
				{/each}
			</DataTable>

			<div class={dataTemplate.pagination}>
				<span class="text-xs">
					第 {currentPage} 页 · 显示 {logs.length} 条 · 排序 {sortBy}/{sortDir}
					{#if refreshing}<span class="ml-2 text-zinc-400">加载中...</span>{/if}
				</span>
				<div class="flex gap-2">
					<Button variant="outline" size="sm" onclick={prevPage} disabled={!hasPrev || refreshing}>上一页</Button>
					<Button variant="outline" size="sm" onclick={nextPage} disabled={!hasNext || refreshing}>下一页</Button>
				</div>
			</div>
		{/if}
	{/if}
</PageShell>
