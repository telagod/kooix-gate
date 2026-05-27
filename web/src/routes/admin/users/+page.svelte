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
	import ResetPasswordModal from './_components/ResetPasswordModal.svelte';
	import SuspendUserModal from './_components/SuspendUserModal.svelte';
	import SessionModal from './_components/SessionModal.svelte';
	import CreateUserForm from './_components/CreateUserForm.svelte';
	import UserStatsCards from './_components/UserStatsCards.svelte';
	import UserTableRow from './_components/UserTableRow.svelte';
	import { dataTemplate, text } from '$lib/design';
	import type { BadgeVariant } from '$lib/design';
	import {
		Eye,
		EyeOff,
		RefreshCcw,
		Search,
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
		<UserStatsCards total={users.length} active={activeCount} suspended={suspendedCount} />

		<CreateUserForm
			form={createForm}
			errors={createErrors}
			{creating}
			{statusOptions}
			onSubmit={submitCreate}
			onUpdateField={(key, value) => {
				createForm = { ...createForm, [key]: value };
			}}
		/>

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

			{#each filteredUsers as user (user.id)}
				<UserTableRow
					{user}
					{isVisible}
					{statusVariant}
					{fmtDate}
					{currentUserId}
					{statusBusy}
					{passwordBusy}
					onToggleStatus={toggleStatus}
					onOpenReset={openReset}
					onOpenSessions={openSessions}
				/>
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

		<ResetPasswordModal
			{resetTarget}
			bind:resetPasswordValue
			{resetPasswordError}
			{passwordBusy}
			textPrimary={text.primary}
			textSecondary={text.secondary}
			textMuted={text.muted}
			onClose={() => (resetTarget = null)}
			onConfirm={submitResetPassword}
		/>

		<SuspendUserModal
			{statusConfirmTarget}
			bind:statusConfirmation
			{statusBusy}
			textPrimary={text.primary}
			textSecondary={text.secondary}
			textMuted={text.muted}
			textDanger={text.danger}
			onClose={() => { statusConfirmTarget = null; statusConfirmation = ''; pendingStatus = ''; }}
			onConfirm={confirmStatusChange}
		/>

		<SessionModal
			{sessionTarget}
			{sessions}
			{sessionsLoading}
			{sessionError}
			{revokeAllBusy}
			{sessionBusy}
			text={{ primary: text.primary, secondary: text.secondary, muted: text.muted }}
			onClose={() => (sessionTarget = null)}
			onRefresh={refreshSessions}
			onRevokeAll={revokeAllSessions}
			onRevokeSession={revokeSession}
		/>

	{/if}
</PageShell>
