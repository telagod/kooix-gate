<script lang="ts">
	// admin/users/_components/UserTableRow.svelte — 0.4.180 抽出
	// 父：admin/users/+page.svelte 549-597 行 用户表格行模板
	import type { UserDetail } from '$lib/api.js';
	import { Badge, Button } from '$lib/components/ui';
	import { dataTemplate, text } from '$lib/design';
	import type { BadgeVariant } from '$lib/design';
	import { KeyRound, MonitorSmartphone, ShieldCheck, ShieldOff } from 'lucide-svelte';

	type Props = {
		user: UserDetail;
		isVisible: (id: string) => boolean;
		statusVariant: (s: string) => BadgeVariant;
		fmtDate: (d: string | null) => string;
		currentUserId: string | null;
		statusBusy: Record<string, boolean>;
		passwordBusy: Record<string, boolean>;
		onToggleStatus: (u: UserDetail) => void;
		onOpenReset: (u: UserDetail) => void;
		onOpenSessions: (u: UserDetail) => void;
	};

	let {
		user,
		isVisible,
		statusVariant,
		fmtDate,
		currentUserId,
		statusBusy,
		passwordBusy,
		onToggleStatus,
		onOpenReset,
		onOpenSessions
	}: Props = $props();
</script>

<tr class={dataTemplate.row}>
	{#if isVisible('email')}
		<td class="px-4 py-3">
			<div class="font-mono text-xs {text.primary}">{user.email}</div>
			<div class="mt-1 font-mono text-[11px] {text.muted}">{user.id}</div>
		</td>
	{/if}
	{#if isVisible('display_name')}
		<td class={dataTemplate.td}>{user.display_name ?? '—'}</td>
	{/if}
	{#if isVisible('status')}
		<td class="px-4 py-3"><Badge variant={statusVariant(user.status)}>{user.status}</Badge></td>
	{/if}
	{#if isVisible('mfa')}
		<td class={dataTemplate.td}>{user.mfa_enabled ? '是' : '否'}</td>
	{/if}
	{#if isVisible('last_login_at')}
		<td class={dataTemplate.td}>{fmtDate(user.last_login_at)}</td>
	{/if}
	{#if isVisible('created_at')}
		<td class={dataTemplate.td}>{fmtDate(user.created_at)}</td>
	{/if}
	{#if isVisible('actions')}
		<td class="px-4 py-3">
			<div class="flex justify-end gap-2">
				<Button
					variant={user.status === 'active' ? 'outline' : 'default'}
					size="sm"
					disabled={statusBusy[user.id] || (user.id === currentUserId && user.status === 'active')}
					onclick={() => onToggleStatus(user)}
				>
					{#if user.status === 'active'}
						<ShieldOff size={12} />停用
					{:else}
						<ShieldCheck size={12} />启用
					{/if}
				</Button>
				<Button variant="outline" size="sm" disabled={passwordBusy[user.id]} onclick={() => onOpenReset(user)}>
					<KeyRound size={12} />重置密码
				</Button>
				<Button variant="outline" size="sm" onclick={() => onOpenSessions(user)}>
					<MonitorSmartphone size={12} />Sessions
				</Button>
			</div>
		</td>
	{/if}
</tr>
