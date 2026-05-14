<!-- /admin/audit — 审计日志 (platform admin only) -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, listAuditLogs } from '$lib/api.js';
	import type { AuditLog } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	const LIMIT = 50;

	let logs = $state<AuditLog[]>([]);
	let loading = $state(true);
	let error = $state('');
	let isPlatformAdmin = $state(false);
	let orgs = $state<string[]>([]);
	let selectedOrg = $state('');
	let offset = $state(0);
	let expandedId = $state<string | null>(null);

	onMount(async () => {
		try {
			const me = await getMe();
			isPlatformAdmin = me.is_platform_admin;
			orgs = me.orgs ?? [];

			if (!isPlatformAdmin) {
				loading = false;
				return;
			}

			// 默认选第一个 org
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

	async function loadLogs() {
		loading = true;
		error = '';
		try {
			logs = await listAuditLogs(selectedOrg, LIMIT, offset);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	async function handleOrgChange() {
		offset = 0;
		expandedId = null;
		await loadLogs();
	}

	async function prevPage() {
		if (offset === 0) return;
		offset = Math.max(0, offset - LIMIT);
		expandedId = null;
		await loadLogs();
	}

	async function nextPage() {
		if (logs.length < LIMIT) return;
		offset += LIMIT;
		expandedId = null;
		await loadLogs();
	}

	function toggleExpand(id: string) {
		expandedId = expandedId === id ? null : id;
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

	function outcomeBadge(outcome: string): string {
		if (outcome === 'success' || outcome === 'ok') return 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400';
		if (outcome === 'denied' || outcome === 'forbidden') return 'bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400';
		return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400';
	}

	let currentPage = $derived(Math.floor(offset / LIMIT) + 1);
	let hasPrev = $derived(offset > 0);
	let hasNext = $derived(logs.length === LIMIT);
</script>

<div class="max-w-7xl mx-auto p-6">
	<div class="flex items-center justify-between mb-1">
		<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">审计日志</h1>
		{#if isPlatformAdmin && orgs.length > 0}
			<div class="flex items-center gap-2">
				<label for="org-select" class="text-sm text-zinc-600 dark:text-zinc-400">Org：</label>
				<select
					id="org-select"
					bind:value={selectedOrg}
					onchange={handleOrgChange}
					disabled={loading}
					class="h-9 rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-100 disabled:opacity-50"
				>
					{#each orgs as org}
						<option value={org}>{org}</option>
					{/each}
				</select>
			</div>
		{/if}
	</div>
	<p class="text-sm text-zinc-500 dark:text-zinc-400 mb-6">平台级审计记录，仅 Platform Admin 可见。</p>

	{#if !isPlatformAdmin}
		<Card class="p-8 text-center">
			<p class="text-zinc-500 dark:text-zinc-400 text-sm">无访问权限。审计日志仅限 Platform Admin 查看。</p>
		</Card>
	{:else if !selectedOrg}
		<Card class="p-6">
			<p class="text-zinc-500 dark:text-zinc-400 text-sm">当前账号没有关联任何 Org，无法查询审计日志。</p>
		</Card>
	{:else if loading}
		<p class="text-zinc-500 dark:text-zinc-400">加载中...</p>
	{:else if error}
		<Card class="p-6">
			<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
		</Card>
	{:else if logs.length === 0}
		<Card class="p-6">
			<p class="text-zinc-500 dark:text-zinc-400 text-sm">此 Org 暂无审计记录。</p>
		</Card>
	{:else}
		<div class="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 mb-4">
			<table class="w-full text-sm">
				<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
					<tr>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">时间</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">操作者</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">动作</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">资源类型</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">资源 ID</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">结果</th>
						<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400"></th>
					</tr>
				</thead>
				<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
					{#each logs as log}
						<tr
							class="hover:bg-zinc-50 dark:hover:bg-zinc-800 transition-colors cursor-pointer {expandedId === log.id ? 'bg-zinc-50 dark:bg-zinc-800' : ''}"
							onclick={() => toggleExpand(log.id)}
						>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400 whitespace-nowrap">{formatDate(log.ts)}</td>
							<td class="px-4 py-3 font-mono text-xs text-zinc-700 dark:text-zinc-300">
								<span class="text-zinc-400 dark:text-zinc-500">{log.actor_kind}/</span>{log.actor_id?.slice(0, 8) ?? '—'}
							</td>
							<td class="px-4 py-3 font-mono text-xs text-zinc-900 dark:text-zinc-100">{log.action}</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-400">{log.resource_kind}</td>
							<td class="px-4 py-3 font-mono text-xs text-zinc-600 dark:text-zinc-400">{log.resource_id?.slice(0, 8) ?? '—'}</td>
							<td class="px-4 py-3">
								<span class="inline-block px-2 py-0.5 rounded text-xs font-medium {outcomeBadge(log.outcome)}">
									{log.outcome}
								</span>
							</td>
							<td class="px-4 py-3 text-right">
								{#if log.after !== null}
									<span class="text-xs text-zinc-400 dark:text-zinc-500">{expandedId === log.id ? '▲' : '▼'}</span>
								{/if}
							</td>
						</tr>
						{#if expandedId === log.id && log.after !== null}
							<tr class="bg-zinc-50 dark:bg-zinc-800">
								<td colspan="7" class="px-4 py-3">
									<div class="text-xs text-zinc-600 dark:text-zinc-400 font-medium mb-1">After</div>
									<pre class="bg-white dark:bg-zinc-900 border border-zinc-200 dark:border-zinc-700 rounded-md p-3 text-xs font-mono text-zinc-800 dark:text-zinc-200 overflow-x-auto whitespace-pre-wrap break-all">{JSON.stringify(log.after, null, 2)}</pre>
								</td>
							</tr>
						{/if}
					{/each}
				</tbody>
			</table>
		</div>

		<!-- 分页 -->
		<div class="flex items-center justify-between text-sm text-zinc-600 dark:text-zinc-400">
			<span>第 {currentPage} 页 · 显示 {logs.length} 条</span>
			<div class="flex gap-2">
				<Button variant="outline" size="sm" onclick={prevPage} disabled={!hasPrev || loading}>
					← 上一页
				</Button>
				<Button variant="outline" size="sm" onclick={nextPage} disabled={!hasNext || loading}>
					下一页 →
				</Button>
			</div>
		</div>
	{/if}
</div>
