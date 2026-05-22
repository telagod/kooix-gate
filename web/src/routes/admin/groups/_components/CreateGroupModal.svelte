<script lang="ts">
	// 0.4.5：从 admin/groups/+page.svelte 抽出的新建分组 modal。
	import { Button, Field, Input, Select, Textarea } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { X } from 'lucide-svelte';

	type StrategyMeta = { label: string; color: string; desc: string };
	type CreateForm = {
		name: string;
		strategy: string;
		description: string;
		fallback_group_id: string | null;
	};
	type SelectOption = { value: string | null; label: string };

	interface Props {
		showCreate: boolean;
		createForm: CreateForm;
		strategies: Record<string, StrategyMeta>;
		fallbackOptions: SelectOption[];
		strategyBadgeClass: (color: string) => string;
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		showCreate = $bindable(),
		createForm = $bindable(),
		strategies,
		fallbackOptions,
		strategyBadgeClass,
		onClose,
		onConfirm,
	}: Props = $props();
</script>

{#if showCreate}
	<ModalFrame close={onClose}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-lg max-h-[90vh] overflow-y-auto">
			<div class="p-5 border-b border-zinc-200 dark:border-zinc-700 flex items-center justify-between">
				<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">新建分组</h2>
				<button onclick={onClose} class="p-1 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-700"><X class="w-5 h-5 text-zinc-500" /></button>
			</div>
			<div class="p-5 space-y-4">
				<Field label="名称" for="group-create-name">
					<Input id="group-create-name" bind:value={createForm.name} placeholder="如：默认分组" />
				</Field>
				<Field label="路由策略" for="group-create-strategy">
					<div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
						{#each Object.entries(strategies) as [key, s]}
							<button
								onclick={() => { createForm.strategy = key; }}
								class="text-left p-3 rounded-lg border-2 transition-colors
									{createForm.strategy === key ? 'border-zinc-900 dark:border-zinc-300 bg-zinc-50 dark:bg-zinc-700' : 'border-zinc-200 dark:border-zinc-700 hover:border-zinc-400 dark:hover:border-zinc-500'}"
							>
								<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {strategyBadgeClass(s.color)} mb-1">{s.label}</span>
								<p class="text-xs text-zinc-600 dark:text-zinc-300">{s.desc}</p>
							</button>
						{/each}
					</div>
				</Field>
				<Field label="回退分组（可选）" for="group-create-fallback">
					<Select id="group-create-fallback" bind:value={createForm.fallback_group_id} options={fallbackOptions} />
				</Field>
				<Field label="描述（可选）" for="group-create-description">
					<Textarea id="group-create-description" bind:value={createForm.description} rows={2} placeholder="分组用途说明" />
				</Field>
			</div>
			<div class="p-5 border-t border-zinc-200 dark:border-zinc-700 flex justify-end gap-2">
				<Button variant="outline" onclick={onClose}>取消</Button>
				<Button onclick={onConfirm} disabled={!createForm.name.trim()}>创建</Button>
			</div>
		</div>
	</ModalFrame>
{/if}
