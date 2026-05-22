<script lang="ts">
	// 0.4.5：从 admin/groups/+page.svelte 抽出的添加渠道 modal。
	import { Button, Field, Input, Select } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { X, Search, Check } from 'lucide-svelte';
	import type { Channel } from '$lib/api.js';

	type SelectOption = { value: string | null; label: string };

	interface Props {
		showAddChannel: boolean;
		channelSearch: string;
		channelProviderFilter: string;
		providerFilterOptions: SelectOption[];
		addPriority: number;
		addWeight: number;
		addCanaryPercent: number | null;
		selectedChannels: Set<string>;
		filteredChannels: Channel[];
		providerColor: (p: string) => string;
		onClose: () => void;
		onToggleChannel: (id: string) => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		showAddChannel = $bindable(),
		channelSearch = $bindable(),
		channelProviderFilter = $bindable(),
		providerFilterOptions,
		addPriority = $bindable(),
		addWeight = $bindable(),
		addCanaryPercent = $bindable(),
		selectedChannels,
		filteredChannels,
		providerColor,
		onClose,
		onToggleChannel,
		onConfirm,
	}: Props = $props();
</script>

{#if showAddChannel}
	<ModalFrame close={onClose}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-xl max-h-[85vh] flex flex-col">
			<div class="p-5 border-b border-zinc-200 dark:border-zinc-700 flex items-center justify-between flex-shrink-0">
				<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">添加渠道</h2>
				<button onclick={onClose} class="p-1 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-700"><X class="w-5 h-5 text-zinc-500" /></button>
			</div>

			<div class="p-4 border-b border-zinc-200 dark:border-zinc-700 space-y-3 flex-shrink-0">
				<div class="flex gap-2">
					<div class="relative flex-1">
						<Search class="absolute left-3 top-2.5 w-4 h-4 text-zinc-400" />
						<Input bind:value={channelSearch} placeholder="搜索渠道..." class="pl-9" />
					</div>
					<Select bind:value={channelProviderFilter} options={providerFilterOptions} class="w-36" />
				</div>
				<div class="flex gap-4">
					<Field label="优先级" for="group-add-priority" class="flex-row items-center gap-2 space-y-0">
						<Input id="group-add-priority" type="number" bind:value={addPriority} size="sm" class="w-20" />
					</Field>
					<Field label="权重" for="group-add-weight" class="flex-row items-center gap-2 space-y-0">
						<Input id="group-add-weight" type="number" bind:value={addWeight} size="sm" class="w-20" />
					</Field>
				</div>
				<Field label="Canary 流量（可选）" for="group-add-canary" hint="留空为关闭；开启时后端限制 1%-5%，未命中流量会走其它渠道。">
					<Input id="group-add-canary" type="number" min="1" max="5" step="0.5" bind:value={addCanaryPercent} placeholder="如 5" size="sm" class="w-28" />
				</Field>
			</div>

			<div class="flex-1 overflow-y-auto p-4 space-y-1">
				{#if filteredChannels.length === 0}
					<p class="text-center text-sm text-zinc-600 dark:text-zinc-300 py-8">没有可用的渠道</p>
				{:else}
					{#each filteredChannels as ch (ch.id)}
						<button
							onclick={() => onToggleChannel(ch.id)}
							class="w-full flex items-center gap-3 p-3 rounded-lg text-left transition-colors
								{selectedChannels.has(ch.id) ? 'bg-zinc-100 dark:bg-zinc-800 border border-zinc-400 dark:border-zinc-500' : 'hover:bg-zinc-50 dark:hover:bg-zinc-900/50 border border-transparent'}"
						>
							<div class="w-5 h-5 rounded border-2 flex items-center justify-center flex-shrink-0
								{selectedChannels.has(ch.id) ? 'border-zinc-900 bg-zinc-900 dark:border-zinc-100 dark:bg-zinc-100' : 'border-zinc-200 dark:border-zinc-700'}">
								{#if selectedChannels.has(ch.id)}<Check class="w-3 h-3 text-white" />{/if}
							</div>
							<div class="flex-1 min-w-0">
								<div class="text-sm font-medium text-zinc-900 dark:text-zinc-100">{ch.name}</div>
								<div class="text-xs text-zinc-600 dark:text-zinc-300">{ch.code}</div>
							</div>
							<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {providerColor(ch.provider_type)}">{ch.provider_type}</span>
						</button>
					{/each}
				{/if}
			</div>

			<div class="p-4 border-t border-zinc-200 dark:border-zinc-700 flex items-center justify-between flex-shrink-0">
				<span class="text-sm text-zinc-500">{selectedChannels.size} 个已选</span>
				<div class="flex gap-2">
					<Button variant="outline" onclick={onClose}>取消</Button>
					<Button onclick={onConfirm} disabled={selectedChannels.size === 0}>
						添加选中 ({selectedChannels.size})
					</Button>
				</div>
			</div>
		</div>
	</ModalFrame>
{/if}
