<!-- /orgs — 登录后首页：显示当前 org + org 列表 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getMe } from '$lib/api.js';
	import { getAccessToken, clearTokens } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let me: { subject: any; current_org: string | null; orgs: string[] } | null = $state(null);
	let loading = $state(true);
	let error = $state('');
	let activeOrg = $state<string | null>(null);

	onMount(async () => {
		if (!getAccessToken()) {
			goto('/login');
			return;
		}
		try {
			me = await getMe();
			activeOrg = me.current_org ?? me.orgs[0] ?? null;
		} catch (err: any) {
			if (err?.status === 401) {
				clearTokens();
				goto('/login');
			} else {
				error = err?.message ?? '加载失败';
			}
		} finally {
			loading = false;
		}
	});

	async function switchOrg(orgId: string) {
		try {
			me = await getMe(orgId);
			activeOrg = orgId;
		} catch (err: any) {
			error = err?.message ?? '切换 Org 失败';
		}
	}

	function goToProjects(orgId: string) {
		goto(`/orgs/${orgId}/projects`);
	}
</script>

<div class="max-w-4xl mx-auto p-6">
	{#if loading}
		<p class="text-zinc-500 dark:text-zinc-400">加载中...</p>
	{:else if error}
		<p class="text-red-600 dark:text-red-400">{error}</p>
	{:else if me}
		<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 mb-1">我的组织</h1>
		<p class="text-sm text-zinc-500 dark:text-zinc-400 mb-6">
			当前激活：<span class="font-mono font-medium text-zinc-700 dark:text-zinc-300">{activeOrg ?? '—'}</span>
		</p>

		{#if me.orgs.length === 0}
			<Card class="p-6">
				<p class="text-zinc-500 dark:text-zinc-400 text-sm">暂无组织，请联系管理员。</p>
			</Card>
		{:else}
			<div class="space-y-3">
				{#each me.orgs as orgId}
					<Card class="p-4 flex items-center justify-between">
						<div>
							<p class="font-mono text-sm text-zinc-700 dark:text-zinc-300">{orgId}</p>
							{#if orgId === activeOrg}
								<span
									class="inline-block mt-1 text-xs bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 px-2 py-0.5 rounded"
								>
									当前
								</span>
							{/if}
						</div>
						<div class="flex gap-2">
							{#if orgId !== activeOrg}
								<Button variant="outline" size="sm" onclick={() => switchOrg(orgId)}>
									切换
								</Button>
							{/if}
							<Button size="sm" onclick={() => goToProjects(orgId)}>查看项目</Button>
						</div>
					</Card>
				{/each}
			</div>
		{/if}
	{/if}
</div>
