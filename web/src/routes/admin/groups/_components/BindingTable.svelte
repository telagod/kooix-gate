<script lang="ts">
	// admin/groups/_components/BindingTable.svelte — 0.4.64 抽出
	// 父：admin/groups/+page.svelte 581-694 行 Binding Table 段
	import type { GroupBinding, GroupDetail } from '$lib/api.js';
	import {
		CAPABILITY_LABELS,
		capabilityList,
		type ProviderCapabilities
	} from '$lib/plugin-presets';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import { Plus, Pencil, Trash2, Check, X } from 'lucide-svelte';
	import { PROVIDER_COLOR, capabilityChipClass, formatCanaryPercent } from '../_lib/helpers';

	type Props = {
		detail: GroupDetail;
		refs: string[];
		editingBindingId: string | null;
		editBindingPriority: number;
		editBindingWeight: number;
		editBindingCanaryPercent: number | null;
		bindingCapabilities: (b: GroupBinding) => ProviderCapabilities;
		onOpenAddChannel: () => void;
		onStartEdit: (b: GroupBinding) => void;
		onCancelEdit: () => void;
		onSaveBinding: () => void;
		onRemoveBinding: (channelId: string) => void;
		onUpdatePriority: (value: number) => void;
		onUpdateWeight: (value: number) => void;
		onUpdateCanaryPercent: (value: number | null) => void;
	};

	let {
		detail,
		refs,
		editingBindingId,
		editBindingPriority,
		editBindingWeight,
		editBindingCanaryPercent,
		bindingCapabilities,
		onOpenAddChannel,
		onStartEdit,
		onCancelEdit,
		onSaveBinding,
		onRemoveBinding,
		onUpdatePriority,
		onUpdateWeight,
		onUpdateCanaryPercent
	}: Props = $props();

	function providerColor(_p: string) {
		return PROVIDER_COLOR;
	}
</script>

<div class="p-5">
	<div class="flex items-center justify-between mb-3">
		<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300">渠道列表 ({detail.bindings.length})</h3>
		<button
			onclick={onOpenAddChannel}
			class="inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700"
		>
			<Plus class="w-3 h-3" /> 添加渠道
		</button>
	</div>

	{#if detail.bindings.length === 0}
		<p class="text-center text-sm text-zinc-600 dark:text-zinc-300 py-8">暂无渠道，点击上方按钮添加</p>
	{:else}
		<DataTable class="mb-0">
			{#snippet head()}
				<tr>
					<th class={dataTemplate.th}>状态</th>
					<th class={dataTemplate.th}>渠道</th>
					<th class={dataTemplate.th}>类型</th>
					<th class={dataTemplate.th}>优先级</th>
					<th class={dataTemplate.th}>权重</th>
					<th class={dataTemplate.th}>Canary</th>
					<th class={dataTemplate.th}>模型过滤</th>
					<th class={cn(dataTemplate.th, 'text-right')}>操作</th>
				</tr>
			{/snippet}

			{#each detail.bindings as b (b.channel_id)}
				{@const health = b.channel_health ?? 'healthy'}
				{@const isEditing = editingBindingId === b.channel_id}
				<tr class={dataTemplate.row}>
					<td class={dataTemplate.td}>
						<span class="inline-block w-2.5 h-2.5 rounded-full {health === 'healthy' ? 'bg-green-500' : 'bg-red-500'}" title={health}></span>
					</td>
					<td class={dataTemplate.tdStrong}>
						<div class="font-medium">{b.channel_name}</div>
						<div class="text-xs text-zinc-600 dark:text-zinc-300">{b.channel_code}</div>
					</td>
					<td class={dataTemplate.td}>
						<div class="space-y-1">
							<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {providerColor(b.provider_type)}">
								{b.provider_type}
							</span>
							<div class="flex max-w-44 flex-wrap gap-1">
								{#each capabilityList(bindingCapabilities(b)).slice(0, 3) as cap}
									<span class="rounded px-1.5 py-0.5 text-[10px] font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
								{/each}
								{#if capabilityList(bindingCapabilities(b)).length > 3}
									<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-zinc-100 text-zinc-500 ring-1 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:ring-zinc-700">+{capabilityList(bindingCapabilities(b)).length - 3}</span>
								{/if}
							</div>
						</div>
					</td>
					<td class={dataTemplate.td}>
						{#if isEditing}
							<input
								type="number"
								value={editBindingPriority}
								oninput={(e) => onUpdatePriority(Number((e.currentTarget as HTMLInputElement).value))}
								class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100"
							/>
						{:else}
							<button onclick={() => onStartEdit(b)} class="text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 cursor-pointer font-mono">{b.priority}</button>
						{/if}
					</td>
					<td class={dataTemplate.td}>
						{#if isEditing}
							<input
								type="number"
								value={editBindingWeight}
								oninput={(e) => onUpdateWeight(Number((e.currentTarget as HTMLInputElement).value))}
								class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100"
							/>
						{:else}
							<button onclick={() => onStartEdit(b)} class="text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 cursor-pointer font-mono">{b.weight}</button>
						{/if}
					</td>
					<td class={dataTemplate.td}>
						{#if isEditing}
							<input
								type="number"
								min="1"
								max="5"
								step="0.5"
								placeholder="关闭"
								value={editBindingCanaryPercent ?? ''}
								oninput={(e) => {
									const v = (e.currentTarget as HTMLInputElement).value;
									onUpdateCanaryPercent(v === '' ? null : Number(v));
								}}
								class="w-20 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100"
							/>
						{:else if b.canary_percent_bps !== null && b.canary_percent_bps !== undefined}
							<button onclick={() => onStartEdit(b)} class="rounded border border-amber-200 bg-amber-50 px-2 py-0.5 font-mono text-xs font-medium text-amber-700 hover:bg-amber-100 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-300">
								{formatCanaryPercent(b.canary_percent_bps)}
							</button>
						{:else}
							<button onclick={() => onStartEdit(b)} class="text-xs text-zinc-500 dark:text-zinc-400">关闭</button>
						{/if}
					</td>
					<td class={dataTemplate.td}>
						{#if b.model_filter && b.model_filter.length > 0}
							<div class="flex flex-wrap gap-1">
								{#each b.model_filter.slice(0, 3) as m}
									<span class="px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-700 text-xs text-zinc-600 dark:text-zinc-300">{m}</span>
								{/each}
								{#if b.model_filter.length > 3}
									<span class="text-xs text-zinc-600 dark:text-zinc-300">+{b.model_filter.length - 3}</span>
								{/if}
							</div>
						{:else}
							<span class="text-xs text-zinc-600 dark:text-zinc-300">全部</span>
						{/if}
					</td>
					<td class={cn(dataTemplate.td, 'text-right')}>
						{#if isEditing}
							<button onclick={onSaveBinding} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300"><Check class="w-4 h-4" /></button>
							<button onclick={onCancelEdit} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-700 text-zinc-500"><X class="w-4 h-4" /></button>
						{:else}
							<button onclick={() => onStartEdit(b)} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-700 text-zinc-500" title="编辑"><Pencil class="w-3.5 h-3.5" /></button>
							<button onclick={() => onRemoveBinding(b.channel_id)} class="p-1 rounded hover:bg-red-50 dark:hover:bg-red-900/20 text-red-500" title="移除"><Trash2 class="w-3.5 h-3.5" /></button>
						{/if}
					</td>
				</tr>
			{/each}
		</DataTable>
	{/if}

	<!-- Project references -->
	{#if refs.length > 0}
		<div class="mt-4 text-sm text-zinc-600 dark:text-zinc-300">
			{refs.length} 个项目正在使用此分组
		</div>
	{/if}
</div>
