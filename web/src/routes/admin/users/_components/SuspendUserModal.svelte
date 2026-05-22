<script lang="ts">
	// 0.4.7：从 admin/users/+page.svelte 抽出的停用用户确认 modal。
	import { Button, Card, Field, Input } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { ShieldOff } from 'lucide-svelte';

	interface Props {
		statusConfirmTarget: { id: string; email: string } | null;
		statusConfirmation: string;
		statusBusy: Record<string, boolean>;
		textPrimary: string;
		textSecondary: string;
		textMuted: string;
		textDanger: string;
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		statusConfirmTarget,
		statusConfirmation = $bindable(),
		statusBusy,
		textPrimary,
		textSecondary,
		textMuted,
		textDanger,
		onClose,
		onConfirm,
	}: Props = $props();

	const expected = $derived(
		statusConfirmTarget ? `suspend:${statusConfirmTarget.email}` : '',
	);
</script>

{#if statusConfirmTarget}
	<ModalFrame close={onClose} class="bg-zinc-950/40" panelClass="w-full max-w-md">
		<Card padding="lg" class="w-full max-w-md">
			<div class="mb-4 flex items-center gap-2">
				<ShieldOff size={18} class={textDanger} />
				<div>
					<p class="font-semibold {textPrimary}">停用用户</p>
					<p class="text-xs {textMuted}">{statusConfirmTarget.email}</p>
				</div>
			</div>
			<p class="mb-3 text-sm {textSecondary}">停用后该用户无法继续登录。请输入确认短语：</p>
			<code class="mb-2 block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expected}</code>
			<Field label="确认短语" for="user-status-confirm" required>
				<Input id="user-status-confirm" bind:value={statusConfirmation} placeholder={expected} class="font-mono" />
			</Field>
			<div class="mt-5 flex justify-end gap-2">
				<Button variant="ghost" onclick={onClose}>取消</Button>
				<Button variant="destructive" onclick={onConfirm} disabled={statusBusy[statusConfirmTarget.id] || statusConfirmation.trim() !== expected}>
					<ShieldOff size={14} />确认停用
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}
