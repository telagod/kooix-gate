<script lang="ts">
	// 0.4.5：从 admin/groups/+page.svelte 抽出的删除确认 modal。
	import { Button } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { AlertTriangle } from 'lucide-svelte';
	import type { ChannelGroup } from '$lib/api.js';

	interface Props {
		deleteTarget: ChannelGroup | null;
		deleteRefs: string[];
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let { deleteTarget, deleteRefs, onClose, onConfirm }: Props = $props();
</script>

{#if deleteTarget}
	<ModalFrame close={onClose}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-sm">
			<div class="p-6 text-center">
				<div class="mx-auto w-12 h-12 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center mb-4">
					<AlertTriangle class="w-6 h-6 text-red-600 dark:text-red-400" />
				</div>
				<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
				<p class="text-sm text-zinc-600 dark:text-zinc-300">
					确定要删除分组「{deleteTarget.name}」吗？此操作不可撤销。
					{#if deleteRefs.length > 0}
						<br /><span class="inline-flex items-center justify-center gap-1 text-red-500 font-medium"><AlertTriangle class="h-3.5 w-3.5" />有 {deleteRefs.length} 个项目正在使用此分组</span>
					{/if}
				</p>
			</div>
			<div class="px-6 pb-6 flex gap-2">
				<Button variant="outline" class="flex-1" onclick={onClose}>取消</Button>
				<Button variant="destructive" class="flex-1" onclick={onConfirm}>删除</Button>
			</div>
		</div>
	</ModalFrame>
{/if}
