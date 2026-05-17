<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, listUsers, updateUserStatus } from '$lib/api.js';
	import type { UserDetail } from '$lib/api.js';
	import { Button, Card, Skeleton } from '$lib/components/ui';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { Users, ShieldCheck, ShieldOff } from 'lucide-svelte';

	let users = $state<UserDetail[]>([]);
	let loading = $state(true);
	let error = $state('');
	let actionError = $state('');

	onMount(async () => {
		try {
			const me = await getMe();
			if (!me.is_platform_admin) { error = '需要平台管理员权限'; loading = false; return; }
			users = await listUsers(200);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	async function toggleStatus(user: UserDetail) {
		actionError = '';
		const next = user.status === 'active' ? 'suspended' : 'active';
		try {
			const updated = await updateUserStatus(user.id, next);
			users = users.map(u => u.id === updated.id ? updated : u);
		} catch (err: any) {
			actionError = err?.message ?? '操作失败';
		}
	}

	function fmtDate(d: string | null): string {
		if (!d) return '—';
		return new Date(d).toLocaleDateString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
	}
</script>

<PageShell title="用户管理" description={`${users.length} 用户`} icon={Users}>
	{#if actionError}
		<Card padding="sm" variant="danger" class="mb-4">
			<p class="text-xs text-red-600 dark:text-red-400">{actionError}</p>
		</Card>
	{/if}

	{#if loading}
		<div class="space-y-2">
			{#each Array(5) as _}
				<Skeleton class="h-12" />
			{/each}
		</div>
	{:else if error}
		<StatePanel variant="danger" description={error} />
	{:else if users.length === 0}
		<StatePanel title="暂无用户" icon={Users} />
	{:else}
		<Card class="overflow-x-auto" padding="none">
			<table class="w-full text-sm">
				<thead>
					<tr class="border-b border-zinc-200 dark:border-zinc-700 text-left">
						<th class="px-4 py-3 font-medium text-zinc-600 dark:text-zinc-300">邮箱</th>
						<th class="px-4 py-3 font-medium text-zinc-600 dark:text-zinc-300">昵称</th>
						<th class="px-4 py-3 font-medium text-zinc-600 dark:text-zinc-300">状态</th>
						<th class="px-4 py-3 font-medium text-zinc-600 dark:text-zinc-300">MFA</th>
						<th class="px-4 py-3 font-medium text-zinc-600 dark:text-zinc-300">最后登录</th>
						<th class="px-4 py-3 font-medium text-zinc-600 dark:text-zinc-300">注册时间</th>
						<th class="px-4 py-3"></th>
					</tr>
				</thead>
				<tbody>
					{#each users as user}
						<tr class="border-b border-zinc-100 dark:border-zinc-800 hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
							<td class="px-4 py-3 font-mono text-zinc-900 dark:text-zinc-100">{user.email}</td>
							<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400">{user.display_name ?? '—'}</td>
							<td class="px-4 py-3">
								<span class="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full {
									user.status === 'active'
										? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
										: 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400'
								}">
									{user.status}
								</span>
							</td>
							<td class="px-4 py-3 text-zinc-600 dark:text-zinc-300">{user.mfa_enabled ? '是' : '否'}</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-300">{fmtDate(user.last_login_at)}</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-300">{fmtDate(user.created_at)}</td>
							<td class="px-4 py-3 text-right">
								<Button
									variant={user.status === 'active' ? 'outline' : 'default'}
									size="sm"
									onclick={() => toggleStatus(user)}
								>
									{#if user.status === 'active'}
										<ShieldOff size={12} />
									{:else}
										<ShieldCheck size={12} />
									{/if}
								</Button>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</Card>
	{/if}
</PageShell>
