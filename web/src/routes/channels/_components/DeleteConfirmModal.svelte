<script lang="ts">
	import { Button, Card, Input } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import type { Channel } from '$lib/api.js';

	interface Props {
		deletingId: string;
		channels: Channel[];
		deleting: boolean;
		deleteConfirmation: string;
		onClose: () => void;
		onConfirm: () => void;
		updateConfirmation: (val: string) => void;
	}

	let {
		deletingId,
		channels,
		deleting,
		deleteConfirmation = $bindable(),
		onClose,
		onConfirm,
		updateConfirmation,
	}: Props = $props();

	const deletingChannel = $derived(channels.find((ch) => ch.id === deletingId));
	const expectedDeleteConfirmation = $derived(`delete:${deletingChannel?.code ?? ''}`);
</script>

{#if deletingId}
	<ModalFrame close={onClose} class="z-50 bg-black/60 backdrop-blur-sm animate-backdrop">
		<Card class="p-6 max-w-sm w-full mx-4 animate-fade-in shadow-2xl">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">此操作将禁用该 channel 并软删除，无法恢复。请输入确认短语：</p>
			<div class="mb-4 space-y-2">
				<code class="block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expectedDeleteConfirmation}</code>
				<Input id="channel-delete-confirm" value={deleteConfirmation} oninput={(e) => updateConfirmation((e.target as HTMLInputElement).value)} placeholder={expectedDeleteConfirmation} disabled={deleting} class="font-mono" />
			</div>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={onClose} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={onConfirm} disabled={deleting || deleteConfirmation.trim() !== expectedDeleteConfirmation}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}
