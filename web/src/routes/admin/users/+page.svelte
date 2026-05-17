<script lang="ts">
	import { onMount } from 'svelte';
	import { createUser, getMe, listUsers, resetUserPassword, updateUserStatus } from '$lib/api.js';
	import type { UserDetail } from '$lib/api.js';
	import { Alert, Badge, Button, Card, Field, Input, Select, Skeleton } from '$lib/components/ui';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { dataTemplate, text } from '$lib/design';
	import type { BadgeVariant } from '$lib/design';
	import { Check, KeyRound, Plus, RefreshCcw, Search, ShieldCheck, ShieldOff, UserPlus, Users } from 'lucide-svelte';

	let users = $state<UserDetail[]>([]);
	let loading = $state(true);
	let refreshing = $state(false);
	let error = $state('');
	let actionError = $state('');
	let actionSuccess = $state('');
	let currentUserId = $state('');
	let creating = $state(false);
	let createForm = $state({ email: '', display_name: '', password: '', status: 'active' });
	let createErrors = $state<Record<string, string>>({});
	let statusBusy = $state<Record<string, boolean>>({});
	let passwordBusy = $state<Record<string, boolean>>({});
	let resetTarget = $state<UserDetail | null>(null);
	let resetPasswordValue = $state('');
	let resetPasswordError = $state('');
	let search = $state('');
	let statusFilter = $state('all');
	let limit = $state('50');
	let offset = $state(0);

	const statusOptions = [
		{ value: 'active', label: 'Active' },
		{ value: 'suspended', label: 'Suspended' },
		{ value: 'pending_verification', label: 'Pending verification' }
	];

	const limitOptions = [
		{ value: '25', label: '25 / page' },
		{ value: '50', label: '50 / page' },
		{ value: '100', label: '100 / page' },
		{ value: '200', label: '200 / page' }
	];

	let filteredUsers = $derived(
		users.filter((user) => {
			const q = search.trim().toLowerCase();
			const matchesSearch =
				!q ||
				user.email.toLowerCase().includes(q) ||
				(user.display_name ?? '').toLowerCase().includes(q) ||
				user.id.toLowerCase().includes(q);
			const matchesStatus = statusFilter === 'all' || user.status === statusFilter;
			return matchesSearch && matchesStatus;
		})
	);

	let activeCount = $derived(users.filter((user) => user.status === 'active').length);
	let suspendedCount = $derived(users.filter((user) => user.status === 'suspended').length);

	onMount(async () => {
		await boot();
	});

	async function boot() {
		try {
			const me = await getMe();
			if (!me.is_platform_admin) {
				error = '需要平台管理员权限';
				return;
			}
			if (me.subject.kind === 'user') currentUserId = me.subject.user_id ?? '';
			await refreshUsers();
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	async function refreshUsers(nextOffset = offset) {
		actionError = '';
		refreshing = true;
		try {
			offset = Math.max(0, nextOffset);
			users = await listUsers(Number(limit), offset);
		} catch (err: any) {
			actionError = err?.message ?? '刷新失败';
		} finally {
			refreshing = false;
		}
	}

	function validateCreate(): boolean {
		const errs: Record<string, string> = {};
		if (!createForm.email.trim() || !createForm.email.includes('@')) errs.email = '请输入有效邮箱';
		if (createForm.password.length < 8) errs.password = '密码至少 8 个字符';
		if (!['active', 'suspended', 'pending_verification'].includes(createForm.status)) errs.status = '状态不合法';
		createErrors = errs;
		return Object.keys(errs).length === 0;
	}

	async function submitCreate() {
		actionError = '';
		actionSuccess = '';
		if (!validateCreate()) return;
		creating = true;
		try {
			const created = await createUser({
				email: createForm.email.trim(),
				display_name: createForm.display_name.trim() || null,
				password: createForm.password,
				status: createForm.status
			});
			users = [created, ...users.filter((u) => u.id !== created.id)];
			createForm = { email: '', display_name: '', password: '', status: 'active' };
			createErrors = {};
			actionSuccess = `用户 ${created.email} 已创建`;
		} catch (err: any) {
			actionError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	async function toggleStatus(user: UserDetail) {
		actionError = '';
		actionSuccess = '';
		if (user.id === currentUserId && user.status === 'active') {
			actionError = '不能停用当前登录的平台管理员';
			return;
		}
		const next = user.status === 'active' ? 'suspended' : 'active';
		statusBusy = { ...statusBusy, [user.id]: true };
		try {
			const updated = await updateUserStatus(user.id, next);
			users = users.map((u) => (u.id === updated.id ? updated : u));
			actionSuccess = `${updated.email} 已切换为 ${updated.status}`;
		} catch (err: any) {
			actionError = err?.message ?? '操作失败';
		} finally {
			statusBusy = { ...statusBusy, [user.id]: false };
		}
	}

	function openReset(user: UserDetail) {
		resetTarget = user;
		resetPasswordValue = '';
		resetPasswordError = '';
		actionError = '';
		actionSuccess = '';
	}

	async function submitResetPassword() {
		if (!resetTarget) return;
		resetPasswordError = '';
		actionError = '';
		actionSuccess = '';
		if (resetPasswordValue.length < 8) {
			resetPasswordError = '新密码至少 8 个字符';
			return;
		}
		passwordBusy = { ...passwordBusy, [resetTarget.id]: true };
		try {
			const updated = await resetUserPassword(resetTarget.id, resetPasswordValue);
			users = users.map((u) => (u.id === updated.id ? updated : u));
			actionSuccess = `${updated.email} 密码已重置`;
			resetTarget = null;
			resetPasswordValue = '';
		} catch (err: any) {
			resetPasswordError = err?.message ?? '重置失败';
		} finally {
			if (resetTarget) passwordBusy = { ...passwordBusy, [resetTarget.id]: false };
		}
	}

	function fmtDate(d: string | null): string {
		if (!d) return '—';
		return new Date(d).toLocaleDateString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
	}

	function statusVariant(status: string): BadgeVariant {
		if (status === 'active') return 'success';
		if (status === 'suspended') return 'danger';
		if (status === 'pending_verification') return 'warning';
		return 'default';
	}
</script>

<PageShell title="用户管理" description={`平台账户生命周期 · ${users.length} users`} icon={Users} max="wide">
	{#snippet actions()}
		<Button variant="outline" onclick={() => refreshUsers()} disabled={refreshing || loading}>
			<RefreshCcw size={14} class={refreshing ? 'animate-spin' : ''} />
			刷新
		</Button>
	{/snippet}

	{#if actionError}
		<Alert variant="danger" class="mb-4">{actionError}</Alert>
	{/if}
	{#if actionSuccess}
		<Alert variant="success" class="mb-4">{actionSuccess}</Alert>
	{/if}

	{#if loading}
		<div class="space-y-2">
			{#each Array(5) as _}
				<Skeleton class="h-12" />
			{/each}
		</div>
	{:else if error}
		<StatePanel variant="danger" description={error} />
	{:else}
		<div class="mb-4 grid gap-3 md:grid-cols-3">
			<Card padding="md">
				<p class="text-xs uppercase tracking-wider {text.muted}">Total</p>
				<p class="mt-1 text-2xl font-semibold {text.primary}">{users.length}</p>
			</Card>
			<Card padding="md" variant="success">
				<p class="text-xs uppercase tracking-wider {text.success}">Active</p>
				<p class="mt-1 text-2xl font-semibold {text.primary}">{activeCount}</p>
			</Card>
			<Card padding="md" variant={suspendedCount > 0 ? 'danger' : 'default'}>
				<p class="text-xs uppercase tracking-wider {suspendedCount > 0 ? text.danger : text.muted}">Suspended</p>
				<p class="mt-1 text-2xl font-semibold {text.primary}">{suspendedCount}</p>
			</Card>
		</div>

		<Card padding="md" class="mb-4">
			<div class="mb-4 flex items-center gap-2">
				<UserPlus size={16} class={text.secondary} />
				<div>
					<p class="text-sm font-semibold {text.primary}">创建用户</p>
					<p class="text-xs {text.muted}">密码仅提交给后端做 Argon2id hash，不会回显或入审计明文。</p>
				</div>
			</div>
			<form class="grid gap-3 lg:grid-cols-[1.2fr_1fr_1fr_180px_auto]" onsubmit={(e) => { e.preventDefault(); submitCreate(); }}>
				<Field label="邮箱" for="create-email" error={createErrors.email} required>
					<Input id="create-email" type="email" placeholder="user@example.com" bind:value={createForm.email} disabled={creating} invalid={!!createErrors.email} autocomplete="off" />
				</Field>
				<Field label="昵称" for="create-name">
					<Input id="create-name" placeholder="可选" bind:value={createForm.display_name} disabled={creating} />
				</Field>
				<Field label="初始密码" for="create-password" error={createErrors.password} required>
					<Input id="create-password" type="password" placeholder="至少 8 位" bind:value={createForm.password} disabled={creating} invalid={!!createErrors.password} autocomplete="new-password" />
				</Field>
				<Field label="状态" for="create-status" error={createErrors.status}>
					<Select id="create-status" bind:value={createForm.status} options={statusOptions} disabled={creating} />
				</Field>
				<div class="flex items-end">
					<Button type="submit" disabled={creating} class="w-full">
						<Plus size={14} />
						创建
					</Button>
				</div>
			</form>
		</Card>

		<Card padding="sm" class="mb-4">
			<div class={dataTemplate.toolbarRow}>
				<div class="relative min-w-[220px] flex-1">
					<Search size={14} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
					<Input class="pl-9" placeholder="搜索邮箱 / 昵称 / ID" bind:value={search} />
				</div>
				<Select class="w-52" bind:value={statusFilter} options={[{ value: 'all', label: 'All status' }, ...statusOptions]} />
				<Select class="w-36" bind:value={limit} options={limitOptions} onchange={() => refreshUsers(0)} />
				<div class="flex items-center gap-2">
					<Button variant="outline" size="sm" disabled={offset === 0 || refreshing} onclick={() => refreshUsers(Math.max(0, offset - Number(limit)))}>上一页</Button>
					<Button variant="outline" size="sm" disabled={users.length < Number(limit) || refreshing} onclick={() => refreshUsers(offset + Number(limit))}>下一页</Button>
				</div>
			</div>
		</Card>

		{#if users.length === 0}
			<StatePanel title="暂无用户" description="先创建一个平台账户再继续分配组织成员。" icon={Users} />
		{:else if filteredUsers.length === 0}
			<StatePanel title="无匹配用户" description="换个搜索词或状态过滤。" icon={Search} />
		{:else}
			<Card class="overflow-x-auto" padding="none">
				<table class={dataTemplate.table}>
					<thead class={dataTemplate.head}>
						<tr>
							<th class={dataTemplate.th}>邮箱</th>
							<th class={dataTemplate.th}>昵称</th>
							<th class={dataTemplate.th}>状态</th>
							<th class={dataTemplate.th}>MFA</th>
							<th class={dataTemplate.th}>最后登录</th>
							<th class={dataTemplate.th}>注册时间</th>
							<th class="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">操作</th>
						</tr>
					</thead>
					<tbody class={dataTemplate.body}>
						{#each filteredUsers as user}
							<tr class={dataTemplate.row}>
								<td class="px-4 py-3">
									<div class="font-mono text-xs {text.primary}">{user.email}</div>
									<div class="mt-1 font-mono text-[11px] {text.muted}">{user.id}</div>
								</td>
								<td class={dataTemplate.td}>{user.display_name ?? '—'}</td>
								<td class="px-4 py-3"><Badge variant={statusVariant(user.status)}>{user.status}</Badge></td>
								<td class={dataTemplate.td}>{user.mfa_enabled ? '是' : '否'}</td>
								<td class={dataTemplate.td}>{fmtDate(user.last_login_at)}</td>
								<td class={dataTemplate.td}>{fmtDate(user.created_at)}</td>
								<td class="px-4 py-3">
									<div class="flex justify-end gap-2">
										<Button
											variant={user.status === 'active' ? 'outline' : 'default'}
											size="sm"
											disabled={statusBusy[user.id] || (user.id === currentUserId && user.status === 'active')}
											onclick={() => toggleStatus(user)}
										>
											{#if user.status === 'active'}
												<ShieldOff size={12} />停用
											{:else}
												<ShieldCheck size={12} />启用
											{/if}
										</Button>
										<Button variant="outline" size="sm" disabled={passwordBusy[user.id]} onclick={() => openReset(user)}>
											<KeyRound size={12} />重置密码
										</Button>
									</div>
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</Card>
		{/if}

		{#if resetTarget}
			<div class="fixed inset-0 z-50 flex items-center justify-center bg-zinc-950/40 p-4" role="presentation" onclick={(e) => { if (e.target === e.currentTarget) resetTarget = null; }}>
				<Card padding="lg" class="w-full max-w-md">
					<div class="mb-4 flex items-center gap-2">
						<KeyRound size={18} class={text.secondary} />
						<div>
							<p class="font-semibold {text.primary}">重置密码</p>
							<p class="text-xs {text.muted}">{resetTarget.email}</p>
						</div>
					</div>
					<Field label="新密码" for="reset-password" error={resetPasswordError} required>
						<Input id="reset-password" type="password" placeholder="至少 8 位" bind:value={resetPasswordValue} autocomplete="new-password" invalid={!!resetPasswordError} />
					</Field>
					<div class="mt-5 flex justify-end gap-2">
						<Button variant="ghost" onclick={() => (resetTarget = null)}>取消</Button>
						<Button onclick={submitResetPassword} disabled={passwordBusy[resetTarget.id]}>
							<Check size={14} />确认重置
						</Button>
					</div>
				</Card>
			</div>
		{/if}
	{/if}
</PageShell>
