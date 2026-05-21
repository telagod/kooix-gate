<script lang="ts">
	import { DollarSign, Trash2 } from 'lucide-svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import { cn, dataTemplate } from '$lib/design';

	interface PricingRule {
		id: string;
		model: string;
		dimension: string;
		unit: string;
		rate: number;
		channel_id: string | null;
		priority: number;
		description?: string | null;
	}

	interface Props {
		loading: boolean;
		rules: PricingRule[];
		deletingId: string;
		channelName: (id: string | null) => string;
		onDelete: (id: string) => void;
	}

	let { loading, rules, deletingId, channelName, onDelete }: Props = $props();
</script>

{#if loading}
	<div class="space-y-2">
		{#each Array(5) as _}
			<div class="h-12 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
		{/each}
	</div>
{:else}
	<DataTable isEmpty={rules.length === 0} emptyColspan={8}>
		{#snippet head()}
			<tr>
				<th class={dataTemplate.th}>模型</th>
				<th class={dataTemplate.th}>维度</th>
				<th class={dataTemplate.th}>单位</th>
				<th class={cn(dataTemplate.th, 'text-right')}>费率</th>
				<th class={dataTemplate.th}>渠道</th>
				<th class={cn(dataTemplate.th, 'text-center')}>优先级</th>
				<th class={dataTemplate.th}>描述</th>
				<th class="px-4 py-3 w-12"></th>
			</tr>
		{/snippet}

		{#snippet empty()}
			<DollarSign size={24} class="mx-auto mb-2 text-zinc-300 dark:text-zinc-600" />
			暂无定价规则
		{/snippet}

		{#each rules as rule}
			<tr class={dataTemplate.row}>
				<td class={dataTemplate.tdMonoStrong}>{rule.model}</td>
				<td class={dataTemplate.td}>{rule.dimension}</td>
				<td class={dataTemplate.td}>{rule.unit}</td>
				<td class={cn(dataTemplate.tdMonoStrong, 'text-right')}>{rule.rate.toFixed(4)}</td>
				<td class={dataTemplate.td}>{channelName(rule.channel_id)}</td>
				<td class={cn(dataTemplate.td, 'text-center')}>{rule.priority}</td>
				<td class={cn(dataTemplate.td, 'truncate max-w-[200px]')}>{rule.description ?? '—'}</td>
				<td class={dataTemplate.td}>
					<button
						type="button"
						aria-label="删除定价规则"
						onclick={() => onDelete(rule.id)}
						disabled={deletingId === rule.id}
						class="p-1 rounded text-zinc-400 hover:text-red-600 dark:hover:text-red-400 transition-colors disabled:pointer-events-none disabled:opacity-50"
					>
						<Trash2 size={14} />
					</button>
				</td>
			</tr>
		{/each}
	</DataTable>
	<p class="text-xs text-zinc-500 dark:text-zinc-400 mt-2">{rules.length} 条规则</p>
{/if}
