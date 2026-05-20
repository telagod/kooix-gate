<!-- /admin/channels — 渠道仪表盘：总览 + 健康地图 + 导入/导出 -->
<script lang="ts">
	import { rawId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { listAdminChannels } from '$lib/api.js';
	import type { Channel } from '$lib/api.js';
	import Card from '$lib/components/ui/Card.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import Stat from '$lib/components/Stat.svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import { Cable } from 'lucide-svelte';

	let channels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');

	// Derived stats
	let totalCount = $derived(channels.length);
	let healthyCount = $derived(channels.filter(c => c.health === 'healthy').length);
	let degradedCount = $derived(channels.filter(c => c.health === 'degraded').length);
	let unhealthyCount = $derived(channels.filter(c => c.health === 'unhealthy').length);
	let activeCount = $derived(channels.filter(c => c.status === 'active').length);
	let drainingCount = $derived(channels.filter(c => c.status === 'draining').length);
	let disabledCount = $derived(channels.filter(c => c.status === 'disabled').length);

	// Group by provider
	let byProvider = $derived.by(() => {
		const map: Record<string, { total: number; healthy: number; degraded: number; unhealthy: number }> = {};
		for (const ch of channels) {
			if (!map[ch.provider_type]) map[ch.provider_type] = { total: 0, healthy: 0, degraded: 0, unhealthy: 0 };
			map[ch.provider_type].total++;
			if (ch.health === 'healthy') map[ch.provider_type].healthy++;
			else if (ch.health === 'degraded') map[ch.provider_type].degraded++;
			else map[ch.provider_type].unhealthy++;
		}
		return Object.entries(map).sort((a, b) => b[1].total - a[1].total);
	});

	// Recent errors
	let recentErrors = $derived(
		channels
			.filter(c => c.last_error)
			.sort((a, b) => (b.last_error_at ?? '').localeCompare(a.last_error_at ?? ''))
			.slice(0, 5)
	);

	// Export
	let exporting = $state(false);

	onMount(async () => {
		try {
			const result = await listAdminChannels({ page_size: 100 });
			channels = result.data ?? [];
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	function handleExport() {
		exporting = true;
		try {
			const exportData = channels.map(ch => ({
				code: ch.code,
				name: ch.name,
				provider_type: ch.provider_type,
				base_url: ch.base_url,
				supported_models: ch.supported_models,
				rpm_limit: ch.rpm_limit,
				tpm_limit: ch.tpm_limit,
				timeout_ms: ch.timeout_ms,
				max_retries: ch.max_retries,
				tags: ch.tags,
				model_mapping: ch.model_mapping,
				status: ch.status
			}));
			const blob = new Blob([JSON.stringify(exportData, null, 2)], { type: 'application/json' });
			const url = URL.createObjectURL(blob);
			const a = document.createElement('a');
			a.href = url;
			a.download = `channels-export-${new Date().toISOString().slice(0, 10)}.json`;
			a.click();
			URL.revokeObjectURL(url);
		} finally {
			exporting = false;
		}
	}

	// Import
	let importFile = $state<File | null>(null);
	let importing = $state(false);
	let importResult = $state('');

	async function handleImport() {
		if (!importFile) return;
		importing = true;
		importResult = '';
		try {
			const text = await importFile.text();
			const data = JSON.parse(text);
			if (!Array.isArray(data)) throw new Error('JSON 必须是数组');
			const { createChannel } = await import('$lib/api.js');
			let created = 0;
			let skipped = 0;
			for (const item of data) {
				try {
					await createChannel({
						code: item.code,
						provider_type: item.provider_type,
						base_url: item.base_url,
						name: item.name,
						supported_models: item.supported_models ?? [],
						rpm_limit: item.rpm_limit,
						tpm_limit: item.tpm_limit,
						timeout_ms: item.timeout_ms,
						max_retries: item.max_retries,
						tags: item.tags ?? [],
						model_mapping: item.model_mapping
					});
					created++;
				} catch (err: any) {
					if (err?.code === 'conflict' || err?.message?.includes('already exists')) {
						skipped++;
					} else {
						skipped++;
					}
				}
			}
			importResult = `导入完成：创建 ${created}，跳过 ${skipped}`;
			// Reload
			const result = await listAdminChannels({ page_size: 100 });
			channels = result.data ?? [];
		} catch (err: any) {
			importResult = `导入失败：${err?.message ?? '格式错误'}`;
		} finally {
			importing = false;
			importFile = null;
		}
	}

	function healthColor(health: string): string {
		if (health === 'healthy') return 'bg-green-500';
		if (health === 'degraded') return 'bg-amber-500';
		return 'bg-red-500';
	}

	function fmtDate(s: string | null): string {
		if (!s) return '—';
		try { return new Date(s).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' }); }
		catch { return s; }
	}
</script>

<PageShell title="渠道仪表盘" description="全局渠道健康状态与运维概览" icon={Cable} max="full">
	{#snippet actions()}
		<div class="flex gap-2">
			<Button variant="outline" size="sm" onclick={handleExport} disabled={exporting || channels.length === 0}>
				导出 JSON
			</Button>
			<label class="cursor-pointer">
				<input type="file" accept=".json" class="hidden" onchange={(e: Event) => { importFile = (e.target as HTMLInputElement).files?.[0] ?? null; }} />
				<span class="inline-flex items-center justify-center h-8 px-3 text-xs font-medium rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 hover:bg-zinc-50 dark:hover:bg-zinc-800 transition-colors">
					导入 JSON
				</span>
			</label>
		</div>
	{/snippet}

	{#if importFile}
		<Card class="p-4 mb-4 flex items-center justify-between">
			<p class="text-sm text-zinc-700 dark:text-zinc-300">
				准备导入：<span class="font-mono">{importFile.name}</span>
			</p>
			<div class="flex gap-2">
				<Button variant="outline" size="sm" onclick={() => (importFile = null)}>取消</Button>
				<Button size="sm" onclick={handleImport} disabled={importing}>
					{importing ? '导入中...' : '确认导入'}
				</Button>
			</div>
		</Card>
	{/if}

	{#if importResult}
		<div class="mb-4 px-4 py-2 rounded-lg text-sm bg-zinc-100 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300">
			{importResult}
		</div>
	{/if}

	{#if loading}
		<StatePanel title="正在读取渠道" description="吾正在拉取全局渠道健康状态与运维概览。" icon={Cable} />
	{:else if error}
		<StatePanel title="渠道加载失败" description={error} icon={Cable} variant="danger" />
	{:else}
		<!-- Stats cards -->
		<div class="grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4 mb-8">
			<Stat title="总渠道" value={String(totalCount)} />
			<Stat title="Active" value={String(activeCount)} subtitle="{drainingCount} draining · {disabledCount} disabled" />
			<Stat title="Draining" value={String(drainingCount)} class="border-amber-200 dark:border-amber-900" />
			<Stat title="Healthy" value={String(healthyCount)} class="border-green-200 dark:border-green-900" />
			<Stat title="Degraded" value={String(degradedCount)} class="border-amber-200 dark:border-amber-900" />
			<Stat title="Unhealthy" value={String(unhealthyCount)} class="border-red-200 dark:border-red-900" />
			<Stat title="Providers" value={String(byProvider.length)} />
		</div>

		<!-- Provider health map -->
		<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">Provider 健康分布</h2>
		<div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4 mb-8">
			{#each byProvider as [provider, stats]}
				<Card class="p-4">
					<div class="flex items-center justify-between mb-3">
						<h3 class="text-sm font-medium text-zinc-900 dark:text-zinc-100 font-mono">{provider}</h3>
						<span class="text-xs text-zinc-500 dark:text-zinc-400">{stats.total} 渠道</span>
					</div>
					<div class="flex gap-1 h-3 rounded-full overflow-hidden bg-zinc-100 dark:bg-zinc-800">
						{#if stats.healthy > 0}
							<div class="bg-green-500 transition-all" style="width: {(stats.healthy / stats.total) * 100}%"></div>
						{/if}
						{#if stats.degraded > 0}
							<div class="bg-amber-500 transition-all" style="width: {(stats.degraded / stats.total) * 100}%"></div>
						{/if}
						{#if stats.unhealthy > 0}
							<div class="bg-red-500 transition-all" style="width: {(stats.unhealthy / stats.total) * 100}%"></div>
						{/if}
					</div>
					<div class="flex gap-4 mt-2 text-xs text-zinc-500 dark:text-zinc-400">
						<span class="flex items-center gap-1"><span class="w-2 h-2 rounded-full bg-green-500"></span>{stats.healthy}</span>
						<span class="flex items-center gap-1"><span class="w-2 h-2 rounded-full bg-amber-500"></span>{stats.degraded}</span>
						<span class="flex items-center gap-1"><span class="w-2 h-2 rounded-full bg-red-500"></span>{stats.unhealthy}</span>
					</div>
				</Card>
			{/each}
		</div>

		<!-- Recent errors -->
		{#if recentErrors.length > 0}
			<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">最近错误 TOP 5</h2>
			<DataTable class="mb-8">
				{#snippet head()}
					<tr>
						<th class={dataTemplate.th}>Channel</th>
						<th class={dataTemplate.th}>Provider</th>
						<th class={dataTemplate.th}>错误</th>
						<th class={dataTemplate.th}>时间</th>
					</tr>
				{/snippet}

				{#each recentErrors as ch}
					<tr class={dataTemplate.row}>
						<td class={dataTemplate.tdMonoStrong}>
							<a href="/channels/{rawId(ch.id)}" class="hover:underline">{ch.code}</a>
						</td>
						<td class={dataTemplate.td}>{ch.provider_type}</td>
						<td class={cn(dataTemplate.td, 'max-w-[300px] truncate text-red-600 dark:text-red-400')}>{ch.last_error}</td>
						<td class={dataTemplate.td}>{fmtDate(ch.last_error_at)}</td>
					</tr>
				{/each}
			</DataTable>
		{/if}

		<!-- Quick links -->
		<div class="flex gap-4">
			<a href="/channels" class="text-sm text-zinc-600 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 underline">渠道列表 →</a>
			<a href="/admin/groups" class="text-sm text-zinc-600 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 underline">分组管理 →</a>
		</div>
	{/if}
</PageShell>
