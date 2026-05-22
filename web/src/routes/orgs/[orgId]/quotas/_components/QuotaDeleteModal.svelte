<script lang="ts">
	// 0.4.6：从 quotas/+page.svelte 抽出的删除配额确认 modal。
	import { Button, Card } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { AlertTriangle } from 'lucide-svelte';

	interface Props {
		deletingId: string | null;
		deleting: boolean;
		textPrimary: string;
		textSecondary: string;
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		deletingId,
		deleting,
		textPrimary,
		textSecondary,
		onClose,
		onConfirm,
	}: Props = $props();
</script>

{#if deletingId}
	<ModalFrame close={onClose} panelClass="w-full max-w-sm">
		<Card padding="lg">
			<div class="mb-4 flex items-start gap-3">
				<div class="flex h-9 w-9 items-center justify-center rounded-lg bg-red-50 text-red-600 dark:bg-red-900/20 dark:text-red-400">
					<AlertTriangle size={18} />
				</div>
				<div>
					<h3 class="text-lg font-semibold {textPrimary}">确认删除配额</h3>
					<p class="mt-1 text-sm {textSecondary}">删除后该维度限制立即失效，Redis 现有计数不会自动清空。</p>
				</div>
			</div>
			<div class="flex justify-end gap-2">
				<Button variant="outline" onclick={onClose} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={onConfirm} disabled={deleting}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}
