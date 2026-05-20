<script lang="ts">
	import { onMount } from 'svelte';
	import { Copy, RefreshCcw, Send, Trash2, UserPlus } from 'lucide-svelte';
	import {
		createOrgInvitation,
		createProjectInvitation,
		listOrgInvitations,
		listProjectInvitations,
		revokeOrgInvitation,
		revokeProjectInvitation
	} from '$lib/api.js';
	import type { CreatedInvitation, Invitation } from '$lib/api.js';
	import { shortId } from '$lib/id.js';
	import { Alert, Badge, Button, Card, Field, Input, Select } from '$lib/components/ui';
	import SectionCard from '$lib/components/templates/SectionCard.svelte';
	import { cn, dataTemplate, text } from '$lib/design';
	import type { BadgeVariant } from '$lib/design';

	let {
		scope,
		orgId,
		projectId = '',
		class: className = ''
	}: {
		scope: 'org' | 'project';
		orgId: string;
		projectId?: string;
		class?: string;
	} = $props();

	let invitations = $state<Invitation[]>([]);
	let loading = $state(true);
	let refreshing = $state(false);
	let creating = $state(false);
	let revokingId = $state('');
	let error = $state('');
	let success = $state('');
	let copied = $state(false);
	let createdInvite = $state<CreatedInvitation | null>(null);
	let form = $state({
		email: '',
		role: 'developer',
		ttl_hours: '168'
	});

	const orgRoles = [
		{ value: 'member', label: 'member' },
		{ value: 'billing_viewer', label: 'billing_viewer' },
		{ value: 'admin', label: 'admin' },
		{ value: 'owner', label: 'owner' }
	];
	const projectRoles = [
		{ value: 'viewer', label: 'viewer' },
		{ value: 'developer', label: 'developer' },
		{ value: 'admin', label: 'admin' },
		{ value: 'owner', label: 'owner' }
	];

	let pendingCount = $derived(invitations.filter((i) => i.status === 'pending').length);
	let activeRoles = $derived(Array.from(new Set(invitations.filter((i) => i.status === 'pending').map((i) => i.role))));

	onMount(() => {
		form.role = scope === 'org' ? 'member' : 'developer';
		void loadInvitations();
	});

	async function loadInvitations() {
		refreshing = true;
		error = '';
		try {
			invitations =
				scope === 'org'
					? await listOrgInvitations(orgId, true)
					: await listProjectInvitations(orgId, projectId, true);
		} catch (err: any) {
			error = err?.message ?? '加载邀请失败';
		} finally {
			loading = false;
			refreshing = false;
		}
	}

	async function submitInvite() {
		if (!form.email.trim()) {
			error = '邮箱必填';
			return;
		}
		creating = true;
		error = '';
		success = '';
		createdInvite = null;
		try {
			const ttl = Number(form.ttl_hours) || 168;
			const payload = { email: form.email.trim(), role: form.role, ttl_hours: ttl };
			const created =
				scope === 'org'
					? await createOrgInvitation(orgId, payload)
					: await createProjectInvitation(orgId, projectId, payload);
			createdInvite = created;
			invitations = [created, ...invitations.filter((i) => i.id !== created.id)];
			success = `邀请已创建：${created.email}`;
			form.email = '';
		} catch (err: any) {
			error = err?.message ?? '创建邀请失败';
		} finally {
			creating = false;
		}
	}

	async function revoke(invitation: Invitation) {
		revokingId = invitation.id;
		error = '';
		success = '';
		try {
			const updated =
				scope === 'org'
					? await revokeOrgInvitation(orgId, invitation.id)
					: await revokeProjectInvitation(orgId, projectId, invitation.id);
			invitations = invitations.map((i) => (i.id === updated.id ? updated : i));
			success = `已撤销 ${updated.email} 的邀请`;
		} catch (err: any) {
			error = err?.message ?? '撤销失败';
		} finally {
			revokingId = '';
		}
	}

	async function copyInvite() {
		if (!createdInvite) return;
		const value = createdInvite.accept_url ?? createdInvite.token;
		await navigator.clipboard.writeText(value);
		copied = true;
		setTimeout(() => (copied = false), 1800);
	}

	function statusVariant(status: Invitation['status']): BadgeVariant {
		if (status === 'pending') return 'success';
		if (status === 'expired') return 'warning';
		if (status === 'revoked') return 'danger';
		return 'default';
	}

	function fmtDate(value: string): string {
		return new Date(value).toLocaleString('zh-CN', {
			month: 'short',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit'
		});
	}
</script>

<SectionCard
	title={scope === 'org' ? 'Org 邀请' : 'Project 邀请'}
	description="创建一次性邀请 token，支持过期与撤销；明文 token 仅创建时显示。"
	icon={UserPlus}
	class={className}
>
	{#snippet actions()}
		<Button variant="ghost" size="sm" onclick={loadInvitations} disabled={refreshing}>
			<RefreshCcw size={14} class={refreshing ? 'animate-spin' : ''} />
		</Button>
	{/snippet}

	<div class="grid gap-3 md:grid-cols-[minmax(0,1.5fr)_160px_120px_auto]">
		<Field label="邮箱" for={`invite-email-${scope}`}>
			<Input id={`invite-email-${scope}`} bind:value={form.email} placeholder="teammate@example.com" disabled={creating} />
		</Field>
		<Field label="角色" for={`invite-role-${scope}`}>
			<Select id={`invite-role-${scope}`} bind:value={form.role} options={scope === 'org' ? orgRoles : projectRoles} disabled={creating} />
		</Field>
		<Field label="TTL 小时" for={`invite-ttl-${scope}`}>
			<Input id={`invite-ttl-${scope}`} bind:value={form.ttl_hours} placeholder="168" disabled={creating} />
		</Field>
		<div class="flex items-end">
			<Button onclick={submitInvite} disabled={creating} class="w-full">
				<Send size={14} />
				{creating ? '创建中' : '邀请'}
			</Button>
		</div>
	</div>

	<div class="mt-3 flex flex-wrap gap-2">
		<Badge>{pendingCount} pending</Badge>
		{#each activeRoles as role}
			<Badge variant="default">{role}</Badge>
		{/each}
	</div>

	{#if error}
		<Alert variant="danger" class="mt-3">{error}</Alert>
	{/if}
	{#if success}
		<Alert variant="success" class="mt-3">{success}</Alert>
	{/if}

	{#if createdInvite}
		<Card padding="sm" variant="success" class="mt-4">
			<div class="flex flex-col gap-3 md:flex-row md:items-center md:justify-between">
				<div class="min-w-0">
					<p class="text-xs font-semibold text-green-700 dark:text-green-400">明文邀请，仅显示一次</p>
					<code class="mt-1 block break-all rounded-md bg-white px-2 py-1 font-mono text-xs text-zinc-800 dark:bg-zinc-900 dark:text-zinc-100">
						{createdInvite.accept_url ?? createdInvite.token}
					</code>
				</div>
				<Button variant="outline" size="sm" onclick={copyInvite}>
					<Copy size={14} />
					{copied ? '已复制' : '复制'}
				</Button>
			</div>
		</Card>
	{/if}

	<div class={cn(dataTemplate.tableWrap, 'mt-4 mb-0')}>
		<table class={dataTemplate.table}>
			<thead class={dataTemplate.head}>
				<tr>
					<th class={dataTemplate.th}>Invitee</th>
					<th class={dataTemplate.th}>Role</th>
					<th class={dataTemplate.th}>Status</th>
					<th class={dataTemplate.th}>Expires</th>
					<th class={dataTemplate.th}>ID</th>
					<th class={cn(dataTemplate.th, 'text-right')}>Action</th>
				</tr>
			</thead>
			<tbody class={dataTemplate.body}>
				{#if loading}
					<tr><td class={dataTemplate.emptyCell} colspan="6">加载中...</td></tr>
				{:else if invitations.length === 0}
					<tr><td class={dataTemplate.emptyCell} colspan="6">暂无邀请</td></tr>
				{:else}
					{#each invitations as invitation}
						<tr class={dataTemplate.row}>
							<td class={dataTemplate.tdStrong}>{invitation.email}</td>
							<td class={dataTemplate.tdMono}>{invitation.role}</td>
							<td class={dataTemplate.td}>
								<Badge variant={statusVariant(invitation.status)}>{invitation.status}</Badge>
							</td>
							<td class={dataTemplate.td}>{fmtDate(invitation.expires_at)}</td>
							<td class={dataTemplate.tdMono}>{shortId(invitation.id)}</td>
							<td class={cn(dataTemplate.td, 'text-right')}>
								{#if invitation.status === 'pending'}
									<Button variant="ghost" size="sm" onclick={() => revoke(invitation)} disabled={revokingId === invitation.id}>
										<Trash2 size={13} class="text-red-500" />
										{revokingId === invitation.id ? '撤销中' : '撤销'}
									</Button>
								{:else}
									<span class={text.muted}>—</span>
								{/if}
							</td>
						</tr>
					{/each}
				{/if}
			</tbody>
		</table>
	</div>
</SectionCard>
