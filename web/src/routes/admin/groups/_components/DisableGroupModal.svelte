<script lang="ts">
	// 0.4.5：从 admin/groups/+page.svelte 抽出的禁用分组确认 modal。
	import { Button, Input } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { AlertTriangle } from 'lucide-svelte';
	import type { ChannelGroup } from '$lib/api.js';

	interface Props {
		disableTarget: ChannelGroup | null;
		disableConfirmation: string;
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		disableTarget,
		disableConfirmation = $bindable(),
		onClose,
		onConfirm,
	}: Props = $props();

	const expected = $derived(disableTarget ? `disable:${disableTarget.name}` : '');
</script>

{#if disableTarget}
	<ModalFrame close={onClose}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-sm">
			<div class="p-6 text-center">
				<div class="mx-auto w-12 h-12 rounded-full bg-amber-100 dark:bg-amber-900/30 flex items-center justify-center mb-4">
					<AlertTriangle class="w-6 h-6 text-amber-600 dark:text-amber-400" />
				</div>
				<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认禁用分组</h3>
				<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">禁用后该分组不会继续承载新路由。请输入确认短语：</p>
				<code class="mb-2 block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expected}</code>
				<Input id="group-disable-confirm" bind:value={disableConfirmation} placeholder={expected} class="font-mono text-left" />
			</div>
			<div class="px-6 pb-6 flex gap-2">
				<Button variant="outline" class="flex-1" onclick={onClose}>取消</Button>
				<Button variant="destructive" class="flex-1" onclick={onConfirm} disabled={disableConfirmation.trim() !== expected}>禁用</Button>
			</div>
		</div>
	</ModalFrame>
{/if}
