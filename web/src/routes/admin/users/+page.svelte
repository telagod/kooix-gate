<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, listUsers, updateUserStatus } from '$lib/api.js';
	import type { UserDetail } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
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

<div class="max-w-7xl mx-auto p-6">
	<div class="flex items-center justify-between mb-6">
		<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">用户管理</h1>
		<span class="text-sm text-zinc-500 dark:text-zinc-400">{users.length} 用户</span>
	</div>

	{#if actionError}
		<Card class="p-3 mb-4 bg-red-50 dark:bg-red-900/20 border-red-200 dark:border-red-800">
			<p class="text-xs text-red-600 dark:text-red-400">{actionError}</p>
		</Card>
	{/if}

	{#if loading}
		<div class="space-y-2">
			{#each Array(5) as _}
				<div class="h-12 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
			{/each}
		</div>
	{:else if error}
		<Card class="p-8 text-center">
			<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
		</Card>
	{:else if users.length === 0}
		<Card class="p-12 text-center">
			<Users size={40} class="mx-auto mb-3 text-zinc-300 dark:text-zinc-600" />
			<p class="text-sm text-zinc-500 dark:text-zinc-400">暂无用户</p>
		</Card>
	{:else}
		<div class="overflow-x-auto">
			<table class="w-full text-sm">
				<thead>
					<tr class="border-b border-zinc-200 dark:border-zinc-700 text-left">
						<th class="pb-2 font-medium text-zinc-500 dark:text-zinc-400">邮箱</th>
						<th class="pb-2 font-medium text-zinc-500 dark:text-zinc-400">昵称</th>
						<th class="pb-2 font-medium text-zinc-500 dark:text-zinc-400">状态</th>
						<th class="pb-2 font-medium text-zinc-500 dark:text-zinc-400">MFA</th>
						<th class="pb-2 font-medium text-zinc-500 dark:text-zinc-400">最后登录</th>
						<th class="pb-2 font-medium text-zinc-500 dark:text-zinc-400">注册时间</th>
						<th class="pb-2"></th>
					</tr>
				</thead>
				<tbody>
					{#each users as user}
						<tr class="border-b border-zinc-100 dark:border-zinc-800 hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
							<td class="py-3 font-mono text-zinc-900 dark:text-zinc-100">{user.email}</td>
							<td class="py-3 text-zinc-600 dark:text-zinc-400">{user.display_name ?? '—'}</td>
							<td class="py-3">
								<span class="inline-flex items-center gap-1 text-xs px-2 py-0.5 rounded-full {
									user.status === 'active'
										? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
										: 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400'
								}">
									{user.status}
								</span>
							</td>
							<td class="py-3 text-zinc-500 dark:text-zinc-400">{user.mfa_enabled ? '是' : '否'}</td>
							<td class="py-3 text-xs text-zinc-500 dark:text-zinc-400">{fmtDate(user.last_login_at)}</td>
							<td class="py-3 text-xs text-zinc-500 dark:text-zinc-400">{fmtDate(user.created_at)}</td>
							<td class="py-3 text-right">
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
		</div>
	{/if}
</div>
