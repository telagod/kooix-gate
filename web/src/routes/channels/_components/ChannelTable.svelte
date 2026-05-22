<script lang="ts">
	// 0.4.11：channels/+page.svelte DataTable 段抽出。
	// props 表 ~28，按对象分组：filtersState / metricsState / actions / labels。
	import { Button } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DropdownMenu, { type MenuItem } from '$lib/components/ui/DropdownMenu.svelte';
	import { CirclePause, ArrowUp, ArrowDown, ArrowUpDown } from 'lucide-svelte';
	import { CAPABILITY_LABELS, capabilityList } from '$lib/plugin-presets';
	import { cn, dataTemplate } from '$lib/design';
	import type { Channel, TestResponse } from '$lib/api.js';
	import {
		capabilityFallback,
		capabilityTitle,
		capabilityChipClass,
		statusBadgeCls,
		healthBadgeCls,
		healthDot,
		fmtLimit,
		fmtDate,
	} from '../_lib/helpers';

	export interface ChannelTableActions {
		onToggleEnabled: (ch: Channel) => void;
		onDisableWhenIdle: (ch: Channel) => void;
		onRefreshDrainStatus: (ch: Channel) => void;
		onSelectChannel: (id: string) => void;
		onSelectAll: () => void;
		onSort: (col: string) => void;
		onToggleExpand: (id: string) => void;
		onFocus: (idx: number) => void;
		getMenuItems: (ch: Channel) => MenuItem[];
		sortIconClass: (col: string) => string;
	}

	interface Props {
		channels: Channel[];
		testResults: Record<string, TestResponse>;
		testingIds: Set<string>;
		expandedId: string | null;
		focusedIdx: number;
		isPlatformAdmin: boolean;
		drainStatuses: Record<string, { inflight: number; safe_to_disable: boolean }>;
		disablingIdleIds: Set<string>;
		selectedIds: Set<string>;
		selectAll: boolean;
		sortBy: string;
		sortDir: 'asc' | 'desc';
		actions: ChannelTableActions;
	}

	let {
		channels,
		testResults,
		testingIds,
		expandedId,
		focusedIdx,
		isPlatformAdmin,
		drainStatuses,
		disablingIdleIds,
		selectedIds,
		selectAll,
		sortBy,
		sortDir,
		actions,
	}: Props = $props();
</script>

<DataTable class="min-h-0 flex-1" bodyClass="divide-y-0">
	{#snippet head()}
		<tr>
			{#if isPlatformAdmin}
				<th class="px-4 py-3.5 w-10">
					<input type="checkbox" checked={selectAll} onchange={actions.onSelectAll} class="w-3.5 h-3.5 rounded border-zinc-300 dark:border-zinc-600" />
				</th>
			{/if}
			<th class={cn(dataTemplate.th, 'py-3.5 cursor-pointer select-none')} onclick={() => actions.onSort('code')}>
				<span class="inline-flex items-center gap-1">
					Channel
					{#if sortBy === 'code'}
						{#if sortDir === 'asc'}<ArrowUp size={12} class={actions.sortIconClass('code')} />{:else}<ArrowDown size={12} class={actions.sortIconClass('code')} />{/if}
					{:else}
						<ArrowUpDown size={12} class={actions.sortIconClass('code')} />
					{/if}
				</span>
			</th>
			<th class={cn(dataTemplate.th, 'py-3.5 cursor-pointer select-none')} onclick={() => actions.onSort('provider_type')}>
				<span class="inline-flex items-center gap-1">
					Provider
					{#if sortBy === 'provider_type'}
						{#if sortDir === 'asc'}<ArrowUp size={12} class={actions.sortIconClass('provider_type')} />{:else}<ArrowDown size={12} class={actions.sortIconClass('provider_type')} />{/if}
					{:else}
						<ArrowUpDown size={12} class={actions.sortIconClass('provider_type')} />
					{/if}
				</span>
			</th>
			<th class={cn(dataTemplate.th, 'py-3.5 text-center')}>状态</th>
			<th class={cn(dataTemplate.th, 'py-3.5 text-center')}>健康</th>
			<th class={cn(dataTemplate.th, 'py-3.5')}>模型</th>
			<th class={cn(dataTemplate.th, 'py-3.5 text-right')}>响应</th>
			{#if isPlatformAdmin}<th class="px-4 py-3.5 w-12"></th>{/if}
		</tr>
	{/snippet}

	{#each channels as ch, idx}
		{@const testRes = testResults[ch.id]}
		{@const isTesting = testingIds.has(ch.id)}
		{@const isExpanded = expandedId === ch.id}
		{@const isFocused = focusedIdx === idx}
		<tr
			class={cn('border-b border-zinc-50 dark:border-zinc-800/50', dataTemplate.rowInteractive, isFocused && 'bg-zinc-50 dark:bg-zinc-800/70')}
			onclick={() => actions.onToggleExpand(ch.id)}
		>
			{#if isPlatformAdmin}
				<td class="px-4 py-4" onclick={(e: MouseEvent) => e.stopPropagation()}>
					<input type="checkbox" checked={selectedIds.has(ch.id)} onchange={() => actions.onSelectChannel(ch.id)} class="w-3.5 h-3.5 rounded border-zinc-300 dark:border-zinc-600" />
				</td>
			{/if}
			<td class="px-4 py-4">
				<div class="flex items-center gap-3">
					<div class="w-8 h-8 rounded-lg bg-white dark:bg-zinc-800 border border-zinc-100 dark:border-zinc-700 flex items-center justify-center shrink-0">
						<img src="/providers/{ch.provider_type}.svg" alt="" class="w-5 h-5" />
					</div>
					<div class="min-w-0">
						<p class="font-medium text-zinc-900 dark:text-zinc-100 truncate">{ch.code}</p>
						{#if ch.name && ch.name !== ch.code}
							<p class="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5 truncate">{ch.name}</p>
						{/if}
					</div>
				</div>
			</td>
			<td class="px-4 py-4">
				<div class="space-y-1.5">
					<span class="inline-flex items-center gap-1.5 px-2 py-1 rounded-md bg-zinc-50 dark:bg-zinc-800 text-xs font-mono text-zinc-600 dark:text-zinc-400">{ch.provider_type}</span>
					<div class="flex max-w-[220px] flex-wrap gap-1" title={capabilityTitle(capabilityFallback(ch.provider_type, ch.capabilities))}>
						{#each capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)).slice(0, 4) as cap}
							<span class="rounded px-1.5 py-0.5 text-[10px] font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
						{/each}
						{#if capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)).length > 4}
							<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-zinc-100 text-zinc-500 ring-1 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:ring-zinc-700">+{capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)).length - 4}</span>
						{/if}
					</div>
				</div>
			</td>
			<td class="px-4 py-4 text-center" onclick={(e: MouseEvent) => e.stopPropagation()}>
				{#if isPlatformAdmin}
					<div class="flex flex-col items-center gap-1.5">
						<button type="button" onclick={() => actions.onToggleEnabled(ch)} class="relative inline-flex h-5 w-9 items-center rounded-full transition-colors {ch.status === 'active' ? 'bg-green-500' : ch.status === 'draining' ? 'bg-amber-500' : 'bg-zinc-300 dark:bg-zinc-600'}" title={ch.status === 'active' ? '点击禁用' : '点击启用'}>
							<span class="inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow-sm transition-transform {ch.status === 'active' ? 'translate-x-4.5' : 'translate-x-0.5'}"></span>
						</button>
						{#if ch.status !== 'active'}
							<span class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium {statusBadgeCls(ch.status)}">
								{#if ch.status === 'draining'}<CirclePause size={10} />{/if}
								{ch.status}
							</span>
						{/if}
					</div>
				{:else}
					<span class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium {statusBadgeCls(ch.status)}">
						{#if ch.status === 'draining'}<CirclePause size={12} />{/if}
						{ch.status}
					</span>
				{/if}
			</td>
			<td class="px-4 py-4 text-center">
				<span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium {healthBadgeCls(ch.health)}">
					<span class="w-1.5 h-1.5 rounded-full {healthDot(ch.health)}"></span>
					{ch.health}
				</span>
			</td>
			<td class="px-4 py-4 max-w-[200px]">
				{#if ch.supported_models && ch.supported_models.length > 0}
					<div class="flex flex-wrap gap-1">
						{#each ch.supported_models.slice(0, 2) as m}
							<span class="inline-block px-2 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300 rounded-md text-xs font-mono truncate max-w-[100px]" title={m}>{m}</span>
						{/each}
						{#if ch.supported_models.length > 2}
							<span class="inline-block px-2 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 rounded-md text-xs">+{ch.supported_models.length - 2}</span>
						{/if}
					</div>
				{:else}
					<span class="text-xs text-zinc-400">—</span>
				{/if}
			</td>
			<td class="px-4 py-4 text-right">
				{#if isTesting}
					<div class="w-4 h-4 rounded-full border-2 border-zinc-200 dark:border-zinc-600 border-t-zinc-900 dark:border-t-zinc-100 animate-spin ml-auto"></div>
				{:else if testRes}
					{#if testRes.success}
						<span class="text-xs font-mono font-medium text-green-600 dark:text-green-400">{testRes.response_time_ms}ms</span>
					{:else}
						<span class="text-xs text-red-500 dark:text-red-400" title={testRes.error ?? undefined}>fail</span>
					{/if}
				{:else if ch.balance != null}
					<span class="text-xs text-zinc-500 dark:text-zinc-400 font-mono">${ch.balance.toFixed(2)}</span>
				{:else}
					<span class="text-xs text-zinc-300 dark:text-zinc-600">—</span>
				{/if}
			</td>
			{#if isPlatformAdmin}
				<td class="px-4 py-4" onclick={(e: MouseEvent) => e.stopPropagation()}>
					<DropdownMenu items={actions.getMenuItems(ch)} />
				</td>
			{/if}
		</tr>
		{#if isExpanded}
			<tr class="bg-zinc-50/50 dark:bg-zinc-800/20">
				<td colspan="100" class="px-6 py-5 animate-expand">
					<div class="grid grid-cols-2 md:grid-cols-4 gap-6">
						<div>
							<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">Base URL</p>
							<p class="text-xs font-mono text-zinc-700 dark:text-zinc-300 break-all">{ch.base_url}</p>
						</div>
						<div>
							<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">RPM / TPM</p>
							<p class="text-xs font-mono text-zinc-700 dark:text-zinc-300">{fmtLimit(ch.rpm_limit)} / {fmtLimit(ch.tpm_limit)}</p>
						</div>
						<div>
							<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">超时 / 重试</p>
							<p class="text-xs font-mono text-zinc-700 dark:text-zinc-300">{ch.timeout_ms ?? 60000}ms / {ch.max_retries ?? 2}x</p>
						</div>
						<div>
							<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">创建时间</p>
							<p class="text-xs text-zinc-700 dark:text-zinc-300">{fmtDate(ch.created_at)}</p>
						</div>
					</div>
					{#if ch.status === 'draining'}
						{@const drainInfo = drainStatuses[ch.id]}
						<div class="mt-4 rounded-lg border border-amber-200/70 bg-amber-50 px-3 py-3 dark:border-amber-800/40 dark:bg-amber-900/10">
							<div class="flex flex-wrap items-center justify-between gap-3">
								<div class="flex items-start gap-2">
									<CirclePause size={16} class="mt-0.5 text-amber-600 dark:text-amber-400" />
									<div>
										<p class="text-sm font-medium text-amber-800 dark:text-amber-300">Draining：已禁止新请求</p>
										<p class="mt-1 text-xs text-amber-700 dark:text-amber-400">
											{drainInfo ? `当前 inflight=${drainInfo.inflight}，${drainInfo.safe_to_disable ? '可安全禁用' : '等待现有请求完成'}` : '点击刷新读取当前 inflight。'}
										</p>
									</div>
								</div>
								{#if isPlatformAdmin}
									<div class="flex gap-2">
										<Button variant="outline" size="sm" onclick={() => actions.onRefreshDrainStatus(ch)}>刷新</Button>
										<Button size="sm" onclick={() => actions.onDisableWhenIdle(ch)} disabled={disablingIdleIds.has(ch.id)}>
											{disablingIdleIds.has(ch.id) ? '检查中...' : '空闲后禁用'}
										</Button>
									</div>
								{/if}
							</div>
						</div>
					{/if}
					{#if ch.tags && ch.tags.length > 0}
						<div class="mt-4">
							<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1.5">标签</p>
							<div class="flex flex-wrap gap-1.5">
								{#each ch.tags as tag}
									<span class="px-2 py-0.5 bg-zinc-200/60 dark:bg-zinc-700 text-zinc-700 dark:text-zinc-300 rounded-md text-xs">{tag}</span>
								{/each}
							</div>
						</div>
					{/if}
					<div class="mt-4">
						<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1.5">Capabilities</p>
						<div class="flex flex-wrap gap-1.5">
							{#each capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)) as cap}
								<span class="px-2 py-0.5 rounded-md text-xs font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
							{/each}
						</div>
					</div>
					{#if ch.supported_models && ch.supported_models.length > 2}
						<div class="mt-4">
							<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1.5">全部模型 ({ch.supported_models.length})</p>
							<div class="flex flex-wrap gap-1.5">
								{#each ch.supported_models as m}
									<span class="px-2 py-0.5 bg-zinc-200/60 dark:bg-zinc-700 text-zinc-700 dark:text-zinc-300 rounded-md text-xs font-mono">{m}</span>
								{/each}
							</div>
						</div>
					{/if}
					{#if ch.last_error}
						<div class="mt-4 p-3 rounded-lg bg-red-50 dark:bg-red-900/10 border border-red-200/50 dark:border-red-800/30">
							<p class="text-[10px] font-semibold uppercase tracking-widest text-red-500 dark:text-red-400 mb-1">最近错误</p>
							<p class="text-xs text-red-700 dark:text-red-400 font-mono">{ch.last_error}</p>
							<p class="text-[10px] text-red-400 dark:text-red-500 mt-1">{fmtDate(ch.last_error_at)}</p>
						</div>
					{/if}
				</td>
			</tr>
		{/if}
	{/each}
</DataTable>
