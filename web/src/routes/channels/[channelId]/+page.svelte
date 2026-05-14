<!-- /channels/[channelId] — Channel Keys 管理 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import {
		getMe,
		listChannelKeys,
		createChannelKey,
		rotateChannelKey,
		revokeChannelKey
	} from '$lib/api.js';
	import type { ChannelKeySummary } from '$lib/api.js';
	import { getAccessToken, clearTokens } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let channelId = $derived($page.params.channelId ?? '');

	let keys = $state<ChannelKeySummary[]>([]);
	let loading = $state(true);
	let error = $state('');
	let isPlatformAdmin = $state(false);

	// Create modal
	let showCreate = $state(false);
	let createSecret = $state('');
	let createAlias = $state('');
	let creating = $state(false);
	let createError = $state('');

	// Rotate modal
	let showRotate = $state(false);
	let rotateSecret = $state('');
	let rotateAlias = $state('');
	let rotating = $state(false);
	let rotateError = $state('');

	// Revoke confirm
	let revokingId = $state<string | null>(null);
	let revoking = $state(false);

	// Toast
	let toast = $state('');

	onMount(async () => {
		try {
			const me = await getMe();
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
		await loadKeys();
	});

	async function loadKeys() {
		loading = true;
		error = '';
		try {
			keys = await listChannelKeys(channelId);
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
		if (!createSecret.trim()) return;
		creating = true;
		createError = '';
		try {
			const key = await createChannelKey(channelId, createSecret.trim(), createAlias.trim() || undefined);
			keys = [...keys, key];
			showCreate = false;
			createSecret = '';
			createAlias = '';
			showToast('Key 创建成功');
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	async function handleRotate(e: SubmitEvent) {
		e.preventDefault();
		if (!rotateSecret.trim()) return;
		rotating = true;
		rotateError = '';
		try {
			const key = await rotateChannelKey(channelId, rotateSecret.trim(), rotateAlias.trim() || undefined);
			// rotate 会撤销旧 key，重新拉列表
			keys = await listChannelKeys(channelId);
			showRotate = false;
			rotateSecret = '';
			rotateAlias = '';
			showToast('Key 轮转成功，旧 Key 已自动撤销');
		} catch (err: any) {
			rotateError = err?.message ?? '轮转失败';
		} finally {
			rotating = false;
		}
	}

	async function handleRevoke() {
		if (!revokingId) return;
		revoking = true;
		try {
			await revokeChannelKey(channelId, revokingId);
			keys = keys.filter((k) => k.id !== revokingId);
			revokingId = null;
			showToast('Key 已撤销');
		} catch (err: any) {
			error = err?.message ?? '撤销失败';
			revokingId = null;
		} finally {
			revoking = false;
		}
	}

	function closeCreate() {
		showCreate = false;
		createSecret = '';
		createAlias = '';
		createError = '';
	}

	function closeRotate() {
		showRotate = false;
		rotateSecret = '';
		rotateAlias = '';
		rotateError = '';
	}

	function healthBadge(health: string): string {
		if (health === 'healthy') return 'bg-green-50 text-green-700';
		if (health === 'degraded') return 'bg-amber-50 text-amber-700';
		if (health === 'unhealthy') return 'bg-red-50 text-red-700';
		return 'bg-zinc-100 text-zinc-500';
	}

	function formatDate(s: string): string {
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

<!-- Revoke confirm -->
{#if revokingId}
	<div class="fixed inset-0 z-40 bg-black/30 flex items-center justify-center">
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 mb-2">确认撤销</h3>
			<p class="text-sm text-zinc-600 mb-4">撤销后此 Key 将立即失效，所有使用该 Key 的请求都会失败。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (revokingId = null)} disabled={revoking}>取消</Button>
				<Button variant="destructive" onclick={handleRevoke} disabled={revoking}>
					{revoking ? '撤销中...' : '确认撤销'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Create modal -->
{#if showCreate}
	<div class="fixed inset-0 z-40 bg-black/30 flex items-center justify-center">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 mb-1">创建 Channel Key</h3>
			<div class="bg-amber-50 border border-amber-200 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800">
					Secret 为上游服务商 API Key 明文（如 OpenAI sk-...），将加密存储，提交后不可查看。
				</p>
			</div>
			<form onsubmit={handleCreate} class="space-y-3">
				<div>
					<label for="create-secret" class="block text-sm font-medium text-zinc-700 mb-1">
						Secret <span class="text-red-500">*</span>
					</label>
					<textarea
						id="create-secret"
						bind:value={createSecret}
						disabled={creating}
						rows="3"
						placeholder="sk-..."
						class="flex w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-zinc-900 disabled:opacity-50 resize-none"
					></textarea>
				</div>
				<div>
					<label for="create-alias" class="block text-sm font-medium text-zinc-700 mb-1">
						别名（可选）
					</label>
					<Input
						id="create-alias"
						bind:value={createAlias}
						disabled={creating}
						placeholder="prod-key-1"
					/>
				</div>
				{#if createError}
					<p class="text-sm text-red-600 bg-red-50 rounded-md px-3 py-2">{createError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={closeCreate} disabled={creating}>取消</Button>
					<Button type="submit" disabled={creating || !createSecret.trim()}>
						{creating ? '创建中...' : '创建'}
					</Button>
				</div>
			</form>
		</Card>
	</div>
{/if}

<!-- Rotate modal -->
{#if showRotate}
	<div class="fixed inset-0 z-40 bg-black/30 flex items-center justify-center">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 mb-1">轮转 Channel Key</h3>
			<div class="bg-amber-50 border border-amber-200 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800">
					轮转将创建新 Key 并自动撤销当前所有旧 Key。Secret 为上游 API Key 明文。
				</p>
			</div>
			<form onsubmit={handleRotate} class="space-y-3">
				<div>
					<label for="rotate-secret" class="block text-sm font-medium text-zinc-700 mb-1">
						新 Secret <span class="text-red-500">*</span>
					</label>
					<textarea
						id="rotate-secret"
						bind:value={rotateSecret}
						disabled={rotating}
						rows="3"
						placeholder="sk-..."
						class="flex w-full rounded-md border border-zinc-300 bg-white px-3 py-2 text-sm font-mono focus:outline-none focus:ring-2 focus:ring-zinc-900 disabled:opacity-50 resize-none"
					></textarea>
				</div>
				<div>
					<label for="rotate-alias" class="block text-sm font-medium text-zinc-700 mb-1">
						别名（可选）
					</label>
					<Input
						id="rotate-alias"
						bind:value={rotateAlias}
						disabled={rotating}
						placeholder="prod-key-2"
					/>
				</div>
				{#if rotateError}
					<p class="text-sm text-red-600 bg-red-50 rounded-md px-3 py-2">{rotateError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={closeRotate} disabled={rotating}>取消</Button>
					<Button variant="destructive" type="submit" disabled={rotating || !rotateSecret.trim()}>
						{rotating ? '轮转中...' : '确认轮转'}
					</Button>
				</div>
			</form>
		</Card>
	</div>
{/if}

<div>
	<!-- 面包屑 -->
	<div class="bg-white border-b border-zinc-200 px-6 py-2 flex items-center gap-3">
		<button onclick={() => goto('/channels')} class="text-sm text-zinc-500 hover:text-zinc-900 transition-colors">
			← 渠道列表
		</button>
		<span class="text-zinc-300">/</span>
		<span class="text-sm font-mono text-zinc-600">{channelId}</span>
		<span class="text-zinc-300">/</span>
		<span class="text-sm font-medium text-zinc-900">Keys</span>
	</div>

	<div class="max-w-5xl mx-auto p-6">
		<div class="flex items-center justify-between mb-1">
			<h1 class="text-2xl font-bold text-zinc-900">Channel Keys 管理</h1>
			{#if isPlatformAdmin}
				<div class="flex gap-2">
					<Button variant="outline" size="sm" onclick={() => { showRotate = true; }}>
						轮转 Key
					</Button>
					<Button size="sm" onclick={() => { showCreate = true; }}>
						+ 添加 Key
					</Button>
				</div>
			{/if}
		</div>
		<p class="text-sm text-zinc-500 mb-6">
			{#if isPlatformAdmin}
				平台管理员可添加、轮转和撤销 Key。
			{:else}
				只读视图。操作需平台管理员权限。
			{/if}
		</p>

		{#if loading}
			<p class="text-zinc-500">加载中...</p>
		{:else if error}
			<Card class="p-6">
				<p class="text-red-600 text-sm">{error}</p>
			</Card>
		{:else if keys.length === 0}
			<Card class="p-6">
				<p class="text-zinc-500 text-sm">此 Channel 暂无 Key。
					{#if isPlatformAdmin}点击「+ 添加 Key」创建第一个。{/if}
				</p>
			</Card>
		{:else}
			<div class="overflow-hidden rounded-lg border border-zinc-200 bg-white">
				<table class="w-full text-sm">
					<thead class="bg-zinc-50 border-b border-zinc-200">
						<tr>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">Label</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">Fingerprint</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">Weight</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">Health</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">创建时间</th>
							{#if isPlatformAdmin}
								<th class="px-4 py-3 text-right font-medium text-zinc-600">操作</th>
							{/if}
						</tr>
					</thead>
					<tbody class="divide-y divide-zinc-100">
						{#each keys as key}
							<tr class="hover:bg-zinc-50 transition-colors">
								<td class="px-4 py-3 text-zinc-900">{key.label ?? '—'}</td>
								<td class="px-4 py-3 font-mono text-xs text-zinc-600">{key.fingerprint}</td>
								<td class="px-4 py-3 text-zinc-700">{key.weight}</td>
								<td class="px-4 py-3">
									<span class="inline-block px-2 py-0.5 rounded text-xs font-medium {healthBadge(key.health)}">
										{key.health}
									</span>
								</td>
								<td class="px-4 py-3 text-zinc-600 text-xs">{formatDate(key.created_at)}</td>
								{#if isPlatformAdmin}
									<td class="px-4 py-3 text-right">
										<Button variant="ghost" size="sm" onclick={() => (revokingId = key.id)}>
											<span class="text-red-600">撤销</span>
										</Button>
									</td>
								{/if}
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
