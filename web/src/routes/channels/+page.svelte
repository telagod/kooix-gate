<!-- /channels — Channel 管理页面：列表 + 创建 + 编辑 + 删除 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getMe, listChannels, createChannel, updateChannel, deleteChannel } from '$lib/api.js';
	import type { Channel, CreateChannelRequest, UpdateChannelRequest } from '$lib/api.js';
	import { getAccessToken, clearTokens } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let channels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');
	let currentOrg = $state<string | null>(null);
	let isPlatformAdmin = $state(false);

	// Create modal
	let showCreate = $state(false);
	let createForm = $state<CreateChannelRequest>({ code: '', provider_type: 'openai', base_url: '' });
	let creating = $state(false);
	let createError = $state('');

	// Edit modal
	let editingChannel = $state<Channel | null>(null);
	let editForm = $state<UpdateChannelRequest>({});
	let editing = $state(false);
	let editError = $state('');

	// Delete confirm
	let deletingId = $state<string | null>(null);
	let deleting = $state(false);

	// Toast
	let toast = $state('');

	onMount(async () => {
		if (!getAccessToken()) {
			goto('/login');
			return;
		}
		try {
			const me = await getMe();
			currentOrg = me.current_org ?? me.orgs[0] ?? null;
			isPlatformAdmin = me.is_platform_admin;
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

		await loadChannels();
	});

	async function loadChannels() {
		loading = true;
		error = '';
		try {
			channels = await listChannels(currentOrg!);
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
	}

	function showToast(msg: string) {
		toast = msg;
		setTimeout(() => (toast = ''), 3000);
	}

	async function handleCreate(e: SubmitEvent) {
		e.preventDefault();
		if (!createForm.code.trim() || !createForm.base_url.trim()) return;
		creating = true;
		createError = '';
		try {
			const ch = await createChannel(createForm);
			channels = [...channels, ch];
			showCreate = false;
			createForm = { code: '', provider_type: 'openai', base_url: '' };
			showToast('Channel 创建成功');
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	function startEdit(ch: Channel) {
		editingChannel = ch;
		editForm = { name: ch.name, base_url: ch.base_url, enabled: ch.status === 'active' };
		editError = '';
	}

	async function handleEdit(e: SubmitEvent) {
		e.preventDefault();
		if (!editingChannel) return;
		editing = true;
		editError = '';
		try {
			const updated = await updateChannel(editingChannel.id, editForm);
			channels = channels.map((c) => (c.id === updated.id ? updated : c));
			editingChannel = null;
			showToast('Channel 更新成功');
		} catch (err: any) {
			editError = err?.message ?? '更新失败';
		} finally {
			editing = false;
		}
	}

	async function handleDelete() {
		if (!deletingId) return;
		deleting = true;
		try {
			await deleteChannel(deletingId);
			channels = channels.filter((c) => c.id !== deletingId);
			deletingId = null;
			showToast('Channel 已删除');
		} catch (err: any) {
			error = err?.message ?? '删除失败';
			deletingId = null;
		} finally {
			deleting = false;
		}
	}

	function statusBadge(status: string): string {
		if (status === 'active') return 'bg-green-50 text-green-700';
		if (status === 'disabled') return 'bg-zinc-100 text-zinc-500';
		return 'bg-amber-50 text-amber-700';
	}

	function healthBadge(health: string): string {
		if (health === 'healthy') return 'bg-green-50 text-green-700';
		if (health === 'degraded') return 'bg-amber-50 text-amber-700';
		if (health === 'unhealthy') return 'bg-red-50 text-red-700';
		return 'bg-zinc-100 text-zinc-500';
	}
</script>

<!-- Toast -->
{#if toast}
	<div class="fixed top-4 right-4 z-50 bg-zinc-900 text-white px-4 py-2 rounded-lg shadow-lg text-sm animate-fade-in">
		{toast}
	</div>
{/if}

<!-- Delete confirmation overlay -->
{#if deletingId}
	<div class="fixed inset-0 z-40 bg-black/30 flex items-center justify-center">
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 mb-2">确认删除</h3>
			<p class="text-sm text-zinc-600 mb-4">此操作将禁用该 channel 并软删除，无法恢复。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (deletingId = null)} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Edit modal -->
{#if editingChannel}
	<div class="fixed inset-0 z-40 bg-black/30 flex items-center justify-center">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 mb-4">编辑 Channel: {editingChannel.code}</h3>
			<form onsubmit={handleEdit} class="space-y-3">
				<div>
					<label for="edit-name" class="block text-sm font-medium text-zinc-700 mb-1">名称</label>
					<Input id="edit-name" bind:value={editForm.name} disabled={editing} />
				</div>
				<div>
					<label for="edit-url" class="block text-sm font-medium text-zinc-700 mb-1">Base URL</label>
					<Input id="edit-url" bind:value={editForm.base_url} disabled={editing} />
				</div>
				<div class="flex items-center gap-2">
					<input type="checkbox" id="edit-enabled" bind:checked={editForm.enabled} disabled={editing}
						class="w-4 h-4 rounded border-zinc-300" />
					<label for="edit-enabled" class="text-sm text-zinc-700">启用</label>
				</div>
				{#if editError}
					<p class="text-sm text-red-600 bg-red-50 rounded-md px-3 py-2">{editError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={() => (editingChannel = null)}>取消</Button>
					<Button type="submit" disabled={editing}>
						{editing ? '保存中...' : '保存'}
					</Button>
				</div>
			</form>
		</Card>
	</div>
{/if}

<div class="max-w-6xl mx-auto p-6">
	<div class="flex items-center justify-between mb-1">
		<h1 class="text-2xl font-bold text-zinc-900">渠道管理</h1>
		<div class="flex items-center gap-3">
			<p class="text-xs text-zinc-400 font-mono">{currentOrg ?? '—'}</p>
			{#if isPlatformAdmin}
				<Button size="sm" onclick={() => (showCreate = !showCreate)}>
					{showCreate ? '取消' : '+ 创建 Channel'}
				</Button>
			{/if}
		</div>
	</div>
	<p class="text-sm text-zinc-500 mb-6">
		{#if isPlatformAdmin}
			平台管理员可创建、编辑和删除 channel。
		{:else}
			只读视图。编辑需平台管理员权限。
		{/if}
	</p>

	<!-- Create form -->
	{#if showCreate}
		<Card class="p-5 mb-6">
			<h2 class="text-base font-semibold text-zinc-900 mb-4">新建 Channel</h2>
			<form onsubmit={handleCreate} class="space-y-3">
				<div class="grid grid-cols-1 md:grid-cols-2 gap-3">
					<div>
						<label for="ch-code" class="block text-sm font-medium text-zinc-700 mb-1">Code</label>
						<Input id="ch-code" placeholder="openai-prod" bind:value={createForm.code} disabled={creating} />
					</div>
					<div>
						<label for="ch-provider" class="block text-sm font-medium text-zinc-700 mb-1">Provider</label>
						<select id="ch-provider" bind:value={createForm.provider_type} disabled={creating}
							class="flex h-10 w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-zinc-900">
							<option value="openai">OpenAI</option>
							<option value="anthropic">Anthropic</option>
							<option value="gemini">Gemini</option>
							<option value="azure">Azure</option>
							<option value="bedrock">Bedrock</option>
						</select>
					</div>
				</div>
				<div>
					<label for="ch-url" class="block text-sm font-medium text-zinc-700 mb-1">Base URL</label>
					<Input id="ch-url" placeholder="https://api.openai.com/v1" bind:value={createForm.base_url} disabled={creating} />
				</div>
				{#if createError}
					<p class="text-sm text-red-600 bg-red-50 rounded-md px-3 py-2">{createError}</p>
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

	{#if loading}
		<p class="text-zinc-500">加载中...</p>
	{:else if error}
		<Card class="p-6">
			<p class="text-red-600 text-sm">{error}</p>
		</Card>
	{:else if channels.length === 0}
		<Card class="p-6">
			<p class="text-zinc-500 text-sm">暂无 channel。请使用上方按钮创建上游连接。</p>
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
						{#if isPlatformAdmin}
							<th class="px-4 py-3 text-right font-medium text-zinc-600">操作</th>
						{/if}
					</tr>
				</thead>
				<tbody class="divide-y divide-zinc-100">
					{#each channels as ch}
						<tr class="hover:bg-zinc-50 transition-colors">
							<td class="px-4 py-3 font-mono text-zinc-900">{ch.code}</td>
							<td class="px-4 py-3 text-zinc-700">{ch.name}</td>
							<td class="px-4 py-3 text-zinc-600">{ch.provider_type}</td>
							<td class="px-4 py-3">
								<span class="inline-block px-2 py-0.5 rounded text-xs font-medium {statusBadge(ch.status)}">
									{ch.status}
								</span>
							</td>
							<td class="px-4 py-3">
								<span class="inline-block px-2 py-0.5 rounded text-xs font-medium {healthBadge(ch.health)}">
									{ch.health}
								</span>
							</td>
							{#if isPlatformAdmin}
								<td class="px-4 py-3 text-right">
									<div class="flex gap-1 justify-end">
										<Button variant="ghost" size="sm" onclick={() => goto(`/channels/${ch.id}`)}>Keys</Button>
										<Button variant="ghost" size="sm" onclick={() => startEdit(ch)}>编辑</Button>
										<Button variant="ghost" size="sm" onclick={() => (deletingId = ch.id)}>
											<span class="text-red-600">删除</span>
										</Button>
									</div>
								</td>
							{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	{/if}
</div>
