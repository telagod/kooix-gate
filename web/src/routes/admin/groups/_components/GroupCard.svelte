<script lang="ts">
	// admin/groups/_components/GroupCard.svelte — 0.4.61 抽出
	// 父：admin/groups/+page.svelte。grid item 53 行整体抽出。
	import { ArrowRight } from 'lucide-svelte';
	import type { ChannelGroup } from '$lib/api.js';
	import { strategyMeta, strategyBadgeClass } from '../_lib/helpers';

	type Props = {
		group: ChannelGroup;
		isSelected: boolean;
		groupName: (id: string | null | undefined) => string;
		onSelect: (id: string) => void;
		onToggleEnabled: (group: ChannelGroup) => void;
	};

	let { group, isSelected, groupName, onSelect, onToggleEnabled }: Props = $props();

	let meta = $derived(strategyMeta(group.strategy));
	let count = $derived(group.channel_count ?? 0);
</script>

<button
	onclick={() => onSelect(group.id)}
	class="text-left bg-white dark:bg-zinc-800 rounded-lg border-2 p-4 transition-all hover:shadow-md
		{isSelected ? 'border-zinc-900 dark:border-zinc-100 shadow-md ring-1 ring-zinc-900/20 dark:ring-zinc-100/20' : 'border-zinc-200 dark:border-zinc-700'}"
>
	<!-- Top row: name + toggle -->
	<div class="flex items-start justify-between gap-2">
		<h3 class="font-medium text-zinc-900 dark:text-zinc-100 truncate">{group.name}</h3>
		<div
			role="switch"
			aria-checked={group.enabled}
			tabindex="0"
			onclick={(e: MouseEvent) => {
				e.stopPropagation();
				onToggleEnabled(group);
			}}
			onkeydown={(e: KeyboardEvent) => {
				e.stopPropagation();
				if (e.key === 'Enter') onToggleEnabled(group);
			}}
			class="relative inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full transition-colors
				{group.enabled ? 'bg-zinc-900 dark:bg-zinc-100' : 'bg-zinc-300 dark:bg-zinc-600'}"
		>
			<span
				class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition-transform mt-0.5
				{group.enabled ? 'translate-x-4 ml-0.5' : 'translate-x-0.5'}"
			></span>
		</div>
	</div>

	<!-- Strategy badge -->
	<div class="mt-2 flex items-center gap-2">
		<span
			class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {strategyBadgeClass(
				meta.color
			)}"
		>
			{meta.label}
		</span>
		<span class="text-sm text-zinc-600 dark:text-zinc-300">{count} 渠道</span>
	</div>

	<!-- Description -->
	{#if group.description}
		<p class="mt-2 text-sm text-zinc-600 dark:text-zinc-300 truncate">{group.description}</p>
	{/if}

	<!-- Fallback -->
	{#if group.fallback_group_id}
		<div class="mt-2 flex items-center gap-1 text-sm text-zinc-600 dark:text-zinc-300">
			<ArrowRight class="w-3 h-3" />
			<span class="truncate">回退: {groupName(group.fallback_group_id)}</span>
		</div>
	{/if}
</button>
