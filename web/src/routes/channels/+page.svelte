<!-- /channels — 当前 Org 视角下的 channels 只读列表 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getMe, listChannels } from '$lib/api.js';
	import type { Channel } from '$lib/api.js';
	import { getAccessToken, clearTokens } from '$lib/auth.js';
	import Card from '$lib/components/ui/Card.svelte';

	let channels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');
	let currentOrg = $state<string | null>(null);

	onMount(async () => {
		if (!getAccessToken()) {
			goto('/login');
			return;
		}
		try {
			const me = await getMe();
			currentOrg = me.current_org ?? me.orgs[0] ?? null;
		} catch (err: any) {
			if (err?.status === 401) {
				clearTokens();
				goto('/login');
				return;
			}
			error = err?.message ?? '加载身份失败';
			loading = false;
			return;
		}

		if (!currentOrg) {
			error = '当前账号没有加入任何组织';
			loading = false;
			return;
		}

		try {
			channels = await listChannels(currentOrg);
		} catch (err: any) {
			if (err?.status === 401) {
				clearTokens();
				goto('/login');
				return;
			}
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	function statusBadge(status: string): string {
		if (status === 'active') return 'bg-green-50 text-green-700';
		if (status === 'disabled') return 'bg-zinc-100 text-zinc-500';
		return 'bg-amber-50 text-amber-700';
	}

	function healthBadge(health: string): string {
		if (health === 'healthy') return 'bg-green-50 text-green-700';
		if (health === 'degraded') return 'bg-amber-50 text-amber-700';
		if (health === 'down') return 'bg-red-50 text-red-700';
		return 'bg-zinc-100 text-zinc-500';
	}
</script>

<div class="max-w-6xl mx-auto p-6">
	<div class="flex items-baseline justify-between mb-1">
		<h1 class="text-2xl font-bold text-zinc-900">渠道列表</h1>
		<p class="text-xs text-zinc-400 font-mono">{currentOrg ?? '—'}</p>
	</div>
	<p class="text-sm text-zinc-500 mb-6">
		只读视图。编辑 channel 需联系平台管理员（kgctl 或 SuperAdmin 控制台）。
	</p>

	{#if loading}
		<p class="text-zinc-500">加载中...</p>
	{:else if error}
		<Card class="p-6">
			<p class="text-red-600 text-sm">{error}</p>
		</Card>
	{:else if channels.length === 0}
		<Card class="p-6">
			<p class="text-zinc-500 text-sm">暂无 channel。请联系平台管理员配置上游连接。</p>
		</Card>
	{:else}
		<div class="overflow-hidden rounded-lg border border-zinc-200 bg-white">
			<table class="w-full text-sm">
				<thead class="bg-zinc-50 border-b border-zinc-200">
					<tr>
						<th class="px-4 py-3 text-left font-medium text-zinc-600">Code</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600">名称</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600">Provider</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600">状态</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600">健康度</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600">ID</th>
					</tr>
				</thead>
				<tbody class="divide-y divide-zinc-100">
					{#each channels as ch}
						<tr class="hover:bg-zinc-50 transition-colors">
							<td class="px-4 py-3 font-mono text-zinc-900">{ch.code}</td>
							<td class="px-4 py-3 text-zinc-700">{ch.name}</td>
							<td class="px-4 py-3 text-zinc-600">{ch.provider_type}</td>
							<td class="px-4 py-3">
								<span
									class="inline-block px-2 py-0.5 rounded text-xs font-medium {statusBadge(
										ch.status
									)}"
								>
									{ch.status}
								</span>
							</td>
							<td class="px-4 py-3">
								<span
									class="inline-block px-2 py-0.5 rounded text-xs font-medium {healthBadge(
										ch.health
									)}"
								>
									{ch.health}
								</span>
							</td>
							<td class="px-4 py-3 font-mono text-xs text-zinc-400">{ch.id}</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>
