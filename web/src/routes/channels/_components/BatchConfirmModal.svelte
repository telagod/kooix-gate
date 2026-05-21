<script lang="ts">
	import { Button, Card } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';

	type BatchAction = 'enable' | 'disable' | 'delete' | null;

	interface Props {
		batchAction: BatchAction;
		selectedCount: number;
		batchProcessing: boolean;
		onClose: () => void;
		onConfirm: () => void;
	}

	let { batchAction, selectedCount, batchProcessing, onClose, onConfirm }: Props = $props();
</script>

{#if batchAction}
	<ModalFrame close={onClose} class="z-50 bg-black/60 backdrop-blur-sm animate-backdrop">
		<Card class="p-6 max-w-sm w-full mx-4 animate-fade-in shadow-2xl">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">
				批量{batchAction === 'enable' ? '启用' : batchAction === 'disable' ? '禁用' : '删除'}
			</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">将对 {selectedCount} 个 channel 执行操作。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={onClose} disabled={batchProcessing}>取消</Button>
				<Button variant={batchAction === 'delete' ? 'destructive' : 'default'} onclick={onConfirm} disabled={batchProcessing}>
					{batchProcessing ? '处理中...' : '确认'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}
