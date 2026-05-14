<!-- /orgs/[orgId]/projects/[projectId]/keys — API Key 管理 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { listKeys, createKey, revokeKey } from '$lib/api.js';
	import type { ApiKey, CreateKeyResponse } from '$lib/api.js';
	import { getAccessToken, clearTokens } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let orgId = $derived($page.params.orgId ?? '');
	let projectId = $derived($page.params.projectId ?? '');

	let keys = $state<ApiKey[]>([]);
	let loading = $state(true);
	let error = $state('');

	// Create
	let showCreate = $state(false);
	let newName = $state('');
	let creating = $state(false);
	let createError = $state('');
	let createdKey = $state<CreateKeyResponse | null>(null);
	let copied = $state(false);

	// Revoke confirm
	let revokingId = $state<string | null>(null);
	let revoking = $state(false);

	// Toast
	let toast = $state('');

	onMount(async () => {
		if (!getAccessToken()) {
			goto('/login');
			return;
		}
		await loadKeys();
	});

	async function loadKeys() {
		loading = true;
		error = '';
		try {
			keys = await listKeys(orgId, projectId);
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
	}

	function showToast(msg: string) {
		toast = msg;
		setTimeout(() => (toast = ''), 3000);
	}

	async function handleCreate(e: SubmitEvent) {
		e.preventDefault();
		if (!newName.trim()) return;
		creating = true;
		createError = '';
		try {
			const result = await createKey(orgId, projectId, newName.trim());
			createdKey = result;
			// Refresh list
			keys = await listKeys(orgId, projectId);
			newName = '';
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	async function copyKey() {
		if (!createdKey) return;
		try {
			await navigator.clipboard.writeText(createdKey.plaintext);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			// fallback
		}
	}

	function dismissCreated() {
		createdKey = null;
		showCreate = false;
		showToast('API Key 已创建');
	}

	async function handleRevoke() {
		if (!revokingId) return;
		revoking = true;
		try {
			await revokeKey(orgId, projectId, revokingId);
			keys = await listKeys(orgId, projectId);
			revokingId = null;
			showToast('API Key 已撤销');
		} catch (err: any) {
			error = err?.message ?? '撤销失败';
			revokingId = null;
		} finally {
			revoking = false;
		}
	}

	function formatDate(s: string | null): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleDateString('zh-CN', {
				year: 'numeric',
				month: '2-digit',
				day: '2-digit',
				hour: '2-digit',
				minute: '2-digit'
			});
		} catch {
			return s;
		}
	}
</script>

<!-- Toast -->
{#if toast}
	<div class="fixed top-4 right-4 z-50 bg-zinc-900 text-white px-4 py-2 rounded-lg shadow-lg text-sm">
		{toast}
	</div>
{/if}

<!-- Revoke confirmation -->
{#if revokingId}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center">
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认撤销</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-400 mb-4">撤销后此 API Key 将立即失效，使用该 Key 的所有请求都会被拒绝。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (revokingId = null)} disabled={revoking}>取消</Button>
				<Button variant="destructive" onclick={handleRevoke} disabled={revoking}>
					{revoking ? '撤销中...' : '确认撤销'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<div>
	<!-- 面包屑 -->
	<div class="bg-white dark:bg-zinc-900 border-b border-zinc-200 dark:border-zinc-700 px-6 py-2 flex items-center gap-3">
		<button onclick={() => goto('/orgs')} class="text-sm text-zinc-500 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 transition-colors">
			← 组织
		</button>
		<span class="text-zinc-300 dark:text-zinc-600">/</span>
		<button onclick={() => goto(`/orgs/${orgId}/projects`)} class="text-sm text-zinc-500 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100 transition-colors font-mono">
			{orgId.slice(0, 8)}...
		</button>
		<span class="text-zinc-300 dark:text-zinc-600">/</span>
		<span class="text-sm font-medium text-zinc-900 dark:text-zinc-100">API Keys</span>
	</div>

	<div class="max-w-5xl mx-auto p-6">
		<div class="flex items-center justify-between mb-6">
			<div>
				<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">API Keys</h1>
				<p class="text-sm text-zinc-500 dark:text-zinc-400 mt-1">Project: <span class="font-mono">{projectId.slice(0, 8)}...</span></p>
			</div>
			<Button onclick={() => { showCreate = !showCreate; createdKey = null; }}>
				{showCreate ? '取消' : '+ 创建 Key'}
			</Button>
		</div>

		<!-- Warning banner -->
		<div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-lg px-4 py-3 mb-6">
			<p class="text-sm text-amber-800 dark:text-amber-300">API Key 明文仅在创建时显示一次，请立即复制保存。</p>
		</div>

		<!-- Create form -->
		{#if showCreate && !createdKey}
			<Card class="p-5 mb-6">
				<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 mb-4">创建新 Key</h2>
				<form onsubmit={handleCreate} class="space-y-3">
					<div>
						<label for="key-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
						<Input id="key-name" placeholder="production-backend" bind:value={newName} disabled={creating} />
					</div>
					{#if createError}
						<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{createError}</p>
					{/if}
					<div class="flex gap-2 justify-end">
						<Button variant="outline" type="button" onclick={() => (showCreate = false)}>取消</Button>
						<Button type="submit" disabled={creating}>
							{creating ? '创建中...' : '创建'}
						</Button>
					</div>
				</form>
			</Card>
		{/if}

		<!-- Created key display (show once) -->
		{#if createdKey}
			<Card class="p-5 mb-6 border-green-200 dark:border-green-700 bg-green-50 dark:bg-green-900/20">
				<h2 class="text-base font-semibold text-green-900 dark:text-green-300 mb-2">Key 已创建</h2>
				<p class="text-sm text-green-800 dark:text-green-400 mb-3">请立即复制以下密钥，此页面关闭后将无法再次查看。</p>
				<div class="flex items-center gap-2">
					<code class="flex-1 bg-white dark:bg-zinc-900 border border-green-300 dark:border-green-700 rounded px-3 py-2 text-sm font-mono text-zinc-900 dark:text-zinc-100 break-all select-all">
						{createdKey.plaintext}
					</code>
					<Button size="sm" onclick={copyKey}>
						{copied ? '已复制' : '复制'}
					</Button>
				</div>
				<p class="text-xs text-green-700 dark:text-green-400 mt-2">Name: {createdKey.name} | Prefix: {createdKey.prefix}</p>
				<div class="flex justify-end mt-3">
					<Button variant="outline" size="sm" onclick={dismissCreated}>我已保存，关闭</Button>
				</div>
			</Card>
		{/if}

		<!-- Key list -->
		{#if loading}
			<p class="text-zinc-500 dark:text-zinc-400">加载中...</p>
		{:else if error}
			<Card class="p-6">
				<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
			</Card>
		{:else if keys.length === 0}
			<Card class="p-6">
				<p class="text-zinc-500 dark:text-zinc-400 text-sm">暂无 API Key，点击右上角创建。</p>
			</Card>
		{:else}
			<div class="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
				<table class="w-full text-sm">
					<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
						<tr>
							<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">名称</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">Key</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">创建时间</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">最后使用</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">状态</th>
							<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">操作</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
						{#each keys as key}
							<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800 transition-colors {key.revoked ? 'opacity-50' : ''}">
								<td class="px-4 py-3 font-medium text-zinc-900 dark:text-zinc-100">{key.name}</td>
								<td class="px-4 py-3 font-mono text-zinc-600 dark:text-zinc-400">
									{key.prefix ? `${key.prefix}...${key.last4}` : '—'}
								</td>
								<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400 text-xs">{formatDate(key.created_at)}</td>
								<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400 text-xs">{formatDate(key.last_used_at)}</td>
								<td class="px-4 py-3">
									{#if key.revoked}
										<span class="inline-block px-2 py-0.5 rounded text-xs font-medium bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400">已撤销</span>
									{:else}
										<span class="inline-block px-2 py-0.5 rounded text-xs font-medium bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400">活跃</span>
									{/if}
								</td>
								<td class="px-4 py-3 text-right">
									{#if !key.revoked}
										<Button variant="ghost" size="sm" onclick={() => (revokingId = key.id)}>
											<span class="text-red-600 dark:text-red-400">撤销</span>
										</Button>
									{/if}
								</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
