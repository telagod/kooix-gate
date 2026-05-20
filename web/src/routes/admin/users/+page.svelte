<script lang="ts">
	import { onMount } from 'svelte';
	import {
		createUser,
		getMe,
		listUserSessions,
		listUsers,
		resetUserPassword,
		revokeUserSession,
		revokeUserSessions,
		updateUserStatus
	} from '$lib/api.js';
	import type { UserDetail, UserSession } from '$lib/api.js';
	import { Alert, Badge, Button, Card, Field, Input, Select, Skeleton } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { dataTemplate, text } from '$lib/design';
	import type { BadgeVariant } from '$lib/design';
	import {
		Check,
		Eye,
		EyeOff,
		KeyRound,
		LogOut,
		MonitorSmartphone,
		Plus,
		RefreshCcw,
		Search,
		ShieldCheck,
		ShieldOff,
		UserPlus,
		Users
	} from 'lucide-svelte';
	import {
		currentPageFromOffset,
		hiddenColumnsFromVisible,
		loadTableState,
		normalizePageSize,
		saveTableState,
		toggleColumnVisibility,
		visibleColumnsFromHidden
	} from '$lib/table-state.js';
	import type { TableColumn } from '$lib/table-state.js';

	type UserTableFilters = {
		search: string;
		status: string;
	};

	const TABLE_KEY = 'admin-users';
	const PAGE_SIZES = [25, 50, 100, 200] as const;
	const DEFAULT_PAGE_SIZE = 50;
	const USER_STATUS_FILTERS = ['all', 'active', 'suspended', 'pending_verification'] as const;
	const columns: TableColumn[] = [
		{ id: 'email', label: '邮箱', required: true },
		{ id: 'display_name', label: '昵称' },
		{ id: 'status', label: '状态', required: true },
		{ id: 'mfa', label: 'MFA' },
		{ id: 'last_login_at', label: '最后登录' },
		{ id: 'created_at', label: '注册时间' },
		{ id: 'actions', label: '操作', required: true }
	];
	const defaultVisibleColumns = columns.map((column) => column.id);
	const pageSizeOptions = PAGE_SIZES.map((size) => ({ value: String(size), label: `${size} / 页` }));

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
	let statusConfirmTarget = $state<UserDetail | null>(null);
	let statusConfirmation = $state('');
	let pendingStatus = $state('');
	let resetTarget = $state<UserDetail | null>(null);
	let resetPasswordValue = $state('');
	let resetPasswordError = $state('');
	let sessionTarget = $state<UserDetail | null>(null);
	let sessions = $state<UserSession[]>([]);
	let sessionsLoading = $state(false);
	let sessionBusy = $state<Record<string, boolean>>({});
	let revokeAllBusy = $state(false);
	let sessionError = $state('');
	let search = $state('');
	let statusFilter = $state('all');
	let limit = $state('50');
	let offset = $state(0);
	let hiddenColumns = $state<string[]>([]);

	const statusOptions = [
		{ value: 'active', label: 'Active 启用' },
		{ value: 'suspended', label: 'Suspended 停用' },
		{ value: 'pending_verification', label: 'Pending verification 待验证' }
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
	let visibleColumns = $derived(visibleColumnsFromHidden(columns, hiddenColumns));
	let pageSizeNumber = $derived(normalizePageSize(Number(limit), PAGE_SIZES, DEFAULT_PAGE_SIZE));
	let currentPage = $derived(currentPageFromOffset(offset, pageSizeNumber));
	let hasPrev = $derived(offset > 0);
	let hasNext = $derived(users.length === pageSizeNumber);
	let hasHiddenColumns = $derived(hiddenColumns.length > 0);
	let hasActiveFilters = $derived(search.trim() !== '' || statusFilter !== 'all');

	onMount(async () => {
		const saved = loadTableState<UserTableFilters>(TABLE_KEY, {
			pageSize: DEFAULT_PAGE_SIZE,
			sortBy: 'created_at',
			sortDir: 'desc',
			visibleColumns: defaultVisibleColumns,
			filters: {
				search: '',
				status: 'all'
			}
		});
		limit = String(normalizePageSize(saved.pageSize, PAGE_SIZES, DEFAULT_PAGE_SIZE));
		hiddenColumns = hiddenColumnsFromVisible(columns, saved.visibleColumns);
		search = typeof saved.filters.search === 'string' ? saved.filters.search : '';
		statusFilter = USER_STATUS_FILTERS.includes(saved.filters.status as (typeof USER_STATUS_FILTERS)[number])
			? saved.filters.status
			: 'all';

		await boot();
	});

	function persistTableState() {
		saveTableState<UserTableFilters>(TABLE_KEY, {
			pageSize: pageSizeNumber,
			sortBy: 'created_at',
			sortDir: 'desc',
			visibleColumns: visibleColumns.map((column) => column.id),
			filters: {
				search,
				status: statusFilter
			}
		});
	}

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
			users = await listUsers(pageSizeNumber, offset);
			persistTableState();
		} catch (err: any) {
			actionError = err?.message ?? '刷新失败';
		} finally {
			refreshing = false;
		}
	}

	async function handlePageSizeChange() {
		limit = String(normalizePageSize(Number(limit), PAGE_SIZES, DEFAULT_PAGE_SIZE));
		await refreshUsers(0);
	}

	function handleFilterChange() {
		persistTableState();
	}

	async function prevPage() {
		if (!hasPrev) return;
		await refreshUsers(Math.max(0, offset - pageSizeNumber));
	}

	async function nextPage() {
		if (!hasNext) return;
		await refreshUsers(offset + pageSizeNumber);
	}

	function toggleColumn(id: string) {
		hiddenColumns = toggleColumnVisibility(columns, hiddenColumns, id);
		persistTableState();
	}

	function isVisible(id: string): boolean {
		return visibleColumns.some((column) => column.id === id);
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
		if (next === 'suspended') {
			statusConfirmTarget = user;
			pendingStatus = next;
			statusConfirmation = '';
			return;
		}
		await applyStatus(user, next);
	}

	async function applyStatus(user: UserDetail, next: string, confirmation?: string) {
		statusBusy = { ...statusBusy, [user.id]: true };
		try {
			const updated = await updateUserStatus(user.id, next, confirmation);
			users = users.map((u) => (u.id === updated.id ? updated : u));
			actionSuccess = `${updated.email} 已切换为 ${updated.status}`;
			statusConfirmTarget = null;
			statusConfirmation = '';
			pendingStatus = '';
		} catch (err: any) {
			actionError = err?.message ?? '操作失败';
		} finally {
			statusBusy = { ...statusBusy, [user.id]: false };
		}
	}

	async function confirmStatusChange() {
		if (!statusConfirmTarget || !pendingStatus) return;
		await applyStatus(statusConfirmTarget, pendingStatus, statusConfirmation);
	}

	function openReset(user: UserDetail) {
		resetTarget = user;
		resetPasswordValue = '';
		resetPasswordError = '';
		actionError = '';
		actionSuccess = '';
	}

	async function openSessions(user: UserDetail) {
		sessionTarget = user;
		sessionError = '';
		actionError = '';
		actionSuccess = '';
		await refreshSessions();
	}

	async function refreshSessions() {
		if (!sessionTarget) return;
		sessionsLoading = true;
		sessionError = '';
		try {
			sessions = await listUserSessions(sessionTarget.id);
		} catch (err: any) {
			sessionError = err?.message ?? '会话加载失败';
		} finally {
			sessionsLoading = false;
		}
	}

	async function revokeSession(session: UserSession) {
		if (!sessionTarget) return;
		sessionBusy = { ...sessionBusy, [session.id]: true };
		sessionError = '';
		actionSuccess = '';
		try {
			await revokeUserSession(sessionTarget.id, session.id);
			sessions = sessions.filter((s) => s.id !== session.id);
			actionSuccess = `${sessionTarget.email} 的 refresh session 已撤销`;
		} catch (err: any) {
			sessionError = err?.message ?? '撤销失败';
		} finally {
			sessionBusy = { ...sessionBusy, [session.id]: false };
		}
	}

	async function revokeAllSessions() {
		if (!sessionTarget) return;
		revokeAllBusy = true;
		sessionError = '';
		actionSuccess = '';
		try {
			const result = await revokeUserSessions(sessionTarget.id);
			sessions = [];
			actionSuccess = `${sessionTarget.email} 已踢下线 ${result.revoked} 个 session`;
		} catch (err: any) {
			sessionError = err?.message ?? '批量撤销失败';
		} finally {
			revokeAllBusy = false;
		}
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

	function fmtDateTime(d: string | null): string {
		if (!d) return '—';
		return new Date(d).toLocaleString('zh-CN', {
			month: 'short',
			day: 'numeric',
			hour: '2-digit',
			minute: '2-digit'
		});
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
				<p class="text-xs uppercase tracking-wider {text.muted}">总用户</p>
				<p class="mt-1 text-2xl font-semibold {text.primary}">{users.length}</p>
			</Card>
			<Card padding="md" variant="success">
				<p class="text-xs uppercase tracking-wider {text.success}">Active 启用</p>
				<p class="mt-1 text-2xl font-semibold {text.primary}">{activeCount}</p>
			</Card>
			<Card padding="md" variant={suspendedCount > 0 ? 'danger' : 'default'}>
				<p class="text-xs uppercase tracking-wider {suspendedCount > 0 ? text.danger : text.muted}">Suspended 停用</p>
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

		<DataToolbar badgesVisible={hasActiveFilters || hasHiddenColumns}>
			{#snippet query()}
				<Search size={14} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
				<Input
					class="pl-9"
					placeholder="搜索邮箱 / 昵称 / ID"
					bind:value={search}
					oninput={handleFilterChange}
				/>
			{/snippet}

			{#snippet controls()}
				<Select
					class="w-52"
					bind:value={statusFilter}
					options={[{ value: 'all', label: '全部状态' }, ...statusOptions]}
					onchange={handleFilterChange}
					size="sm"
				/>
				<Select class="w-36" bind:value={limit} options={pageSizeOptions} onchange={handlePageSizeChange} size="sm" />
			{/snippet}

			{#snippet actions()}
				<div class="flex flex-wrap items-center gap-1 rounded-lg border border-zinc-200 bg-white p-1 dark:border-zinc-700 dark:bg-zinc-900">
					{#each columns as column}
						<Button
							variant={isVisible(column.id) ? 'outline' : 'ghost'}
							size="sm"
							disabled={column.required}
							onclick={() => toggleColumn(column.id)}
							class="h-7 px-2"
						>
							{#if isVisible(column.id)}
								<Eye size={12} />
							{:else}
								<EyeOff size={12} />
							{/if}
							{column.label}
						</Button>
					{/each}
				</div>
			{/snippet}

			{#snippet badges()}
				{#if search.trim()}
					<Badge>搜索：{search.trim()}</Badge>
				{/if}
				{#if statusFilter !== 'all'}
					<Badge>状态：{statusFilter}</Badge>
				{/if}
				{#if hasHiddenColumns}
					<Badge>隐藏列：{hiddenColumns.length}</Badge>
				{/if}
				<Badge>已保存筛选</Badge>
			{/snippet}
		</DataToolbar>

		<DataTable
			isEmpty={users.length === 0 || filteredUsers.length === 0}
			emptyColspan={visibleColumns.length}
		>
			{#snippet head()}
				<tr>
					{#if isVisible('email')}
						<th class={dataTemplate.th}>邮箱</th>
					{/if}
					{#if isVisible('display_name')}
						<th class={dataTemplate.th}>昵称</th>
					{/if}
					{#if isVisible('status')}
						<th class={dataTemplate.th}>状态</th>
					{/if}
					{#if isVisible('mfa')}
						<th class={dataTemplate.th}>MFA</th>
					{/if}
					{#if isVisible('last_login_at')}
						<th class={dataTemplate.th}>最后登录</th>
					{/if}
					{#if isVisible('created_at')}
						<th class={dataTemplate.th}>注册时间</th>
					{/if}
					{#if isVisible('actions')}
						<th class="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">操作</th>
					{/if}
				</tr>
			{/snippet}

			{#snippet empty()}
				<div class="flex flex-col items-center gap-2 py-4">
					{#if users.length === 0}
						<Users size={28} class={text.disabled} />
						<p>暂无用户，先创建一个平台账户再继续分配组织成员。</p>
					{:else}
						<Search size={28} class={text.disabled} />
						<p>无匹配用户，换个搜索词或状态过滤。</p>
					{/if}
				</div>
			{/snippet}

			{#each filteredUsers as user}
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
								<Button variant="outline" size="sm" onclick={() => openSessions(user)}>
									<MonitorSmartphone size={12} />Sessions
								</Button>
							</div>
						</td>
					{/if}
				</tr>
			{/each}
		</DataTable>

		<div class={dataTemplate.pagination}>
			<span class="text-xs">
				第 {currentPage} 页 · 当前页 {filteredUsers.length}/{users.length} 条 · 每页 {pageSizeNumber} 条
				{#if refreshing}<span class="ml-2 text-zinc-400">加载中...</span>{/if}
			</span>
			<div class="flex gap-2">
				<Button variant="outline" size="sm" disabled={!hasPrev || refreshing} onclick={prevPage}>上一页</Button>
				<Button variant="outline" size="sm" disabled={!hasNext || refreshing} onclick={nextPage}>下一页</Button>
			</div>
		</div>

		{#if resetTarget}
			<ModalFrame close={() => (resetTarget = null)} class="bg-zinc-950/40" panelClass="w-full max-w-md">
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
			</ModalFrame>
		{/if}

		{#if statusConfirmTarget}
			{@const expectedStatusConfirmation = `suspend:${statusConfirmTarget.email}`}
			<ModalFrame close={() => { statusConfirmTarget = null; statusConfirmation = ''; pendingStatus = ''; }} class="bg-zinc-950/40" panelClass="w-full max-w-md">
				<Card padding="lg" class="w-full max-w-md">
					<div class="mb-4 flex items-center gap-2">
						<ShieldOff size={18} class={text.danger} />
						<div>
							<p class="font-semibold {text.primary}">停用用户</p>
							<p class="text-xs {text.muted}">{statusConfirmTarget.email}</p>
						</div>
					</div>
					<p class="mb-3 text-sm {text.secondary}">停用后该用户无法继续登录。请输入确认短语：</p>
					<code class="mb-2 block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expectedStatusConfirmation}</code>
					<Field label="确认短语" for="user-status-confirm" required>
						<Input id="user-status-confirm" bind:value={statusConfirmation} placeholder={expectedStatusConfirmation} class="font-mono" />
					</Field>
					<div class="mt-5 flex justify-end gap-2">
						<Button variant="ghost" onclick={() => { statusConfirmTarget = null; statusConfirmation = ''; pendingStatus = ''; }}>取消</Button>
						<Button variant="destructive" onclick={confirmStatusChange} disabled={statusBusy[statusConfirmTarget.id] || statusConfirmation.trim() !== expectedStatusConfirmation}>
							<ShieldOff size={14} />确认停用
						</Button>
					</div>
				</Card>
			</ModalFrame>
		{/if}

		{#if sessionTarget}
			<ModalFrame close={() => (sessionTarget = null)} class="bg-zinc-950/40" panelClass="w-full max-w-4xl">
				<Card padding="lg" class="max-h-[85vh] w-full max-w-4xl overflow-y-auto">
					<div class="mb-4 flex flex-col gap-3 sm:flex-row sm:items-start sm:justify-between">
						<div class="flex items-center gap-2">
							<MonitorSmartphone size={18} class={text.secondary} />
							<div>
								<p class="font-semibold {text.primary}">活跃 refresh sessions 会话</p>
								<p class="text-xs {text.muted}">{sessionTarget.email} · 撤销后仅阻断后续 refresh，已签发 access token 会自然过期。</p>
							</div>
						</div>
						<div class="flex gap-2">
							<Button variant="outline" size="sm" onclick={refreshSessions} disabled={sessionsLoading}>
								<RefreshCcw size={12} class={sessionsLoading ? 'animate-spin' : ''} />刷新
							</Button>
							<Button variant="destructive" size="sm" onclick={revokeAllSessions} disabled={revokeAllBusy || sessions.length === 0}>
								<LogOut size={12} />全部踢下线
							</Button>
						</div>
					</div>

					{#if sessionError}
						<Alert variant="danger" class="mb-3">{sessionError}</Alert>
					{/if}

					{#if sessionsLoading}
						<div class="space-y-2">
							{#each Array(3) as _}
								<Skeleton class="h-14" />
							{/each}
						</div>
					{:else if sessions.length === 0}
						<StatePanel title="暂无活跃 session 会话" description="该用户没有可继续 refresh 的登录态。" icon={MonitorSmartphone} />
					{:else}
						<DataTable class="mb-0">
							{#snippet head()}
								<tr>
									<th class={dataTemplate.th}>Session 会话</th>
									<th class={dataTemplate.th}>IP / UA</th>
									<th class={dataTemplate.th}>最后使用</th>
									<th class={dataTemplate.th}>过期</th>
									<th class="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">操作</th>
								</tr>
							{/snippet}

							{#each sessions as session}
								<tr class={dataTemplate.row}>
									<td class="px-4 py-3">
										<div class="font-mono text-xs {text.primary}">{session.id}</div>
										{#if session.current}
											<Badge variant="admin">当前</Badge>
										{/if}
									</td>
									<td class="px-4 py-3">
										<div class="font-mono text-xs {text.primary}">{session.ip ?? '—'}</div>
										<div class="mt-1 max-w-md truncate text-xs {text.muted}">{session.user_agent ?? '未知 User-Agent'}</div>
									</td>
									<td class={dataTemplate.td}>{fmtDateTime(session.last_used_at)}</td>
									<td class={dataTemplate.td}>{fmtDateTime(session.expires_at)}</td>
									<td class="px-4 py-3 text-right">
										<Button variant="destructive" size="sm" onclick={() => revokeSession(session)} disabled={sessionBusy[session.id]}>
											<LogOut size={12} />撤销
										</Button>
									</td>
								</tr>
							{/each}
						</DataTable>
					{/if}

					<div class="mt-5 flex justify-end">
						<Button variant="ghost" onclick={() => (sessionTarget = null)}>关闭</Button>
					</div>
				</Card>
			</ModalFrame>
		{/if}
	{/if}
</PageShell>
