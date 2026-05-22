<script lang="ts">
	// 0.4.8：从 admin/pricing/+page.svelte 抽出的删除 pricing rule 确认 modal。
	import { Button, Card, Input } from '$lib/components/ui';

	type DeletingRule = { id: string; model: string; dimension: string } | null;

	interface Props {
		deletingId: string;
		deletingRule: DeletingRule;
		deleteConfirmation: string;
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		deletingId,
		deletingRule,
		deleteConfirmation = $bindable(),
		onClose,
		onConfirm,
	}: Props = $props();

	const expected = $derived(
		deletingRule ? `pricing:${deletingRule.model}:${deletingRule.dimension}` : '',
	);
</script>

{#if deletingId}
	<div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
		<Card class="w-full max-w-sm p-6">
			<h3 class="mb-2 text-lg font-semibold text-zinc-900 dark:text-zinc-100">删除 Pricing rule</h3>
			<p class="mb-4 text-sm text-zinc-600 dark:text-zinc-300">删除后将立即影响命中的 usage 计费。请输入确认短语：</p>
			<code class="mb-2 block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expected}</code>
			<Input id="pricing-delete-confirm" bind:value={deleteConfirmation} placeholder={expected} class="font-mono" />
			<div class="mt-5 flex justify-end gap-2">
				<Button variant="outline" onclick={onClose}>取消</Button>
				<Button variant="destructive" onclick={onConfirm} disabled={deleteConfirmation.trim() !== expected}>删除</Button>
			</div>
		</Card>
	</div>
{/if}
