<script lang="ts">
	// 0.4.17：从 channels/[channelId]/+page.svelte 抽出的 Create Key modal。
	import { Button, Card, Field, Input, Textarea } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';

	interface Props {
		showCreateKey: boolean;
		createSecret: string;
		createAlias: string;
		createKeyError: string;
		creatingKey: boolean;
		onClose: () => void;
		onSubmit: (e: SubmitEvent) => void | Promise<void>;
	}

	let {
		showCreateKey = $bindable(),
		createSecret = $bindable(),
		createAlias = $bindable(),
		createKeyError,
		creatingKey,
		onClose,
		onSubmit,
	}: Props = $props();
</script>

{#if showCreateKey}
	<ModalFrame close={onClose} class="z-40">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">添加 Key</h3>
			<div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800 dark:text-amber-300">Secret 为上游 API Key 明文，加密存储后不可查看。</p>
			</div>
			<form onsubmit={onSubmit} class="space-y-3">
				<Field label="Secret" for="ck-secret" required>
					<Textarea id="ck-secret" bind:value={createSecret} disabled={creatingKey} rows={3} placeholder="sk-..." class="font-mono resize-none" />
				</Field>
				<Field label="别名" for="ck-alias">
					<Input id="ck-alias" bind:value={createAlias} disabled={creatingKey} placeholder="prod-key-1" />
				</Field>
				{#if createKeyError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{createKeyError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={onClose}>取消</Button>
					<Button type="submit" disabled={creatingKey || !createSecret.trim()}>
						{creatingKey ? '创建中...' : '创建'}
					</Button>
				</div>
			</form>
		</Card>
	</ModalFrame>
{/if}
