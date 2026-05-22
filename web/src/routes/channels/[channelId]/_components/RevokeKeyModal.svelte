<script lang="ts">
	// 0.4.17：从 channels/[channelId]/+page.svelte 抽出的 Revoke Key modal。
	import { Button, Card, Input } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { rawId } from '$lib/id.js';

	interface Props {
		revokingId: string | null;
		revokeConfirmation: string;
		revoking: boolean;
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		revokingId,
		revokeConfirmation = $bindable(),
		revoking,
		onClose,
		onConfirm,
	}: Props = $props();

	const expected = $derived(revokingId ? `revoke:${rawId(revokingId)}` : '');
</script>

{#if revokingId}
	<ModalFrame close={onClose} class="z-40">
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认撤销</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">撤销后此 Key 将立即失效。请输入确认短语：</p>
			<div class="mb-4 space-y-2">
				<code class="block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expected}</code>
				<Input id="revoke-confirm" bind:value={revokeConfirmation} disabled={revoking} placeholder={expected} class="font-mono" />
			</div>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={onClose} disabled={revoking}>取消</Button>
				<Button variant="destructive" onclick={onConfirm} disabled={revoking || revokeConfirmation.trim() !== expected}>
					{revoking ? '撤销中...' : '确认撤销'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}
