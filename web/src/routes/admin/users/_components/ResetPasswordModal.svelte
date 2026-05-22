<script lang="ts">
	// 0.4.7：从 admin/users/+page.svelte 抽出的重置密码 modal。
	import { Button, Card, Field, Input } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import { Check, KeyRound } from 'lucide-svelte';

	interface Props {
		resetTarget: { id: string; email: string } | null;
		resetPasswordValue: string;
		resetPasswordError: string;
		passwordBusy: Record<string, boolean>;
		textPrimary: string;
		textSecondary: string;
		textMuted: string;
		onClose: () => void;
		onConfirm: () => void | Promise<void>;
	}

	let {
		resetTarget,
		resetPasswordValue = $bindable(),
		resetPasswordError,
		passwordBusy,
		textPrimary,
		textSecondary,
		textMuted,
		onClose,
		onConfirm,
	}: Props = $props();
</script>

{#if resetTarget}
	<ModalFrame close={onClose} class="bg-zinc-950/40" panelClass="w-full max-w-md">
		<Card padding="lg" class="w-full max-w-md">
			<div class="mb-4 flex items-center gap-2">
				<KeyRound size={18} class={textSecondary} />
				<div>
					<p class="font-semibold {textPrimary}">重置密码</p>
					<p class="text-xs {textMuted}">{resetTarget.email}</p>
				</div>
			</div>
			<Field label="新密码" for="reset-password" error={resetPasswordError} required>
				<Input id="reset-password" type="password" placeholder="至少 8 位" bind:value={resetPasswordValue} autocomplete="new-password" invalid={!!resetPasswordError} />
			</Field>
			<div class="mt-5 flex justify-end gap-2">
				<Button variant="ghost" onclick={onClose}>取消</Button>
				<Button onclick={onConfirm} disabled={passwordBusy[resetTarget.id]}>
					<Check size={14} />确认重置
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}
