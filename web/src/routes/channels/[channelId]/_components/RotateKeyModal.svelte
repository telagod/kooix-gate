<script lang="ts">
	// 0.4.17：从 channels/[channelId]/+page.svelte 抽出的 Rotate Key modal。
	import { Button, Card, Field, Input, Textarea } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';

	interface Props {
		showRotate: boolean;
		rotateSecret: string;
		rotateAlias: string;
		rotateConfirmation: string;
		rotateError: string;
		rotating: boolean;
		channelCode: string;
		onClose: () => void;
		onSubmit: (e: SubmitEvent) => void | Promise<void>;
	}

	let {
		showRotate = $bindable(),
		rotateSecret = $bindable(),
		rotateAlias = $bindable(),
		rotateConfirmation = $bindable(),
		rotateError,
		rotating,
		channelCode,
		onClose,
		onSubmit,
	}: Props = $props();

	const expected = $derived(`rotate:${channelCode}`);
</script>

{#if showRotate}
	<ModalFrame close={onClose} class="z-40">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">轮转 Key</h3>
			<div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800 dark:text-amber-300">将创建新 Key 并自动撤销所有旧 Key。</p>
			</div>
			<form onsubmit={onSubmit} class="space-y-3">
				<Field label="新 Secret" for="rk-secret" required>
					<Textarea id="rk-secret" bind:value={rotateSecret} disabled={rotating} rows={3} placeholder="sk-..." class="font-mono resize-none" />
				</Field>
				<Field label="别名" for="rk-alias">
					<Input id="rk-alias" bind:value={rotateAlias} disabled={rotating} placeholder="prod-key-2" />
				</Field>
				<Field label="二次确认" for="rk-confirm" hint="轮转会禁用旧 healthy key；请输入下方短语。">
					<code class="mb-2 block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expected}</code>
					<Input id="rk-confirm" bind:value={rotateConfirmation} disabled={rotating} placeholder={expected} class="font-mono" />
				</Field>
				{#if rotateError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{rotateError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={onClose}>取消</Button>
					<Button variant="destructive" type="submit" disabled={rotating || !rotateSecret.trim() || rotateConfirmation.trim() !== expected}>
						{rotating ? '轮转中...' : '确认轮转'}
					</Button>
				</div>
			</form>
		</Card>
	</ModalFrame>
{/if}
