<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { getMe, getProject, updateProject, listKeys, createKey, revokeKey, listModelAliases, upsertModelAlias, deleteModelAlias } from '$lib/api.js';
	import type { Project, ApiKey, CreateKeyResponse, ModelAlias } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import { Settings, Key, Plus, Trash2, Copy, Check, ArrowRight } from 'lucide-svelte';

	let orgId = $derived($page.params.orgId);
	let projectId = $derived($page.params.projectId);

	let project = $state<Project | null>(null);
	let keys = $state<ApiKey[]>([]);
	let loading = $state(true);
	let error = $state('');
	let saving = $state(false);
	let saveMsg = $state('');

	let editName = $state('');
	let editStatus = $state('');

	let newKeyName = $state('');
	let createdKey = $state<CreateKeyResponse | null>(null);
	let keyCopied = $state(false);

	let aliases = $state<ModelAlias[]>([]);
	let newAlias = $state('');
	let newTarget = $state('');

	onMount(async () => {
		try {
			const [p, k, a] = await Promise.all([
				getProject(orgId, projectId),
				listKeys(orgId, projectId),
				listModelAliases(orgId, projectId).catch(() => [])
			]);
			project = p;
			keys = k;
			aliases = a;
			editName = p.name;
			editStatus = p.status;
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	async function saveProject() {
		if (!project) return;
		saving = true;
		saveMsg = '';
		try {
			project = await updateProject(orgId, projectId, { name: editName, status: editStatus });
			saveMsg = '已保存';
			setTimeout(() => (saveMsg = ''), 2000);
		} catch (err: any) {
			saveMsg = err?.message ?? '保存失败';
		} finally {
			saving = false;
		}
	}

	async function handleCreateKey() {
		if (!newKeyName.trim()) return;
		try {
			createdKey = await createKey(orgId, projectId, newKeyName.trim());
			newKeyName = '';
			keys = await listKeys(orgId, projectId);
		} catch (err: any) {
			error = err?.message ?? '创建失败';
		}
	}

	async function handleRevoke(keyId: string) {
		try {
			await revokeKey(orgId, projectId, keyId);
			keys = await listKeys(orgId, projectId);
		} catch (err: any) {
			error = err?.message ?? '撤销失败';
		}
	}

	function copyKey() {
		if (createdKey) {
			navigator.clipboard.writeText(createdKey.plaintext);
			keyCopied = true;
			setTimeout(() => (keyCopied = false), 2000);
		}
	}

	async function handleAddAlias() {
		if (!newAlias.trim() || !newTarget.trim()) return;
		try {
			await upsertModelAlias(orgId, projectId, newAlias.trim(), newTarget.trim());
			aliases = await listModelAliases(orgId, projectId);
			newAlias = '';
			newTarget = '';
		} catch (err: any) {
			error = err?.message ?? '添加失败';
		}
	}

	async function handleDeleteAlias(alias: string) {
		try {
			await deleteModelAlias(orgId, projectId, alias);
			aliases = aliases.filter(a => a.alias !== alias);
		} catch (err: any) {
			error = err?.message ?? '删除失败';
		}
	}
</script>

<div class="max-w-5xl mx-auto p-6">
	{#if loading}
		<div class="space-y-4">
			<div class="h-8 w-48 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
			<div class="h-32 bg-zinc-200 dark:bg-zinc-700 rounded-lg animate-pulse"></div>
		</div>
	{:else if error && !project}
		<Card class="p-6">
			<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
		</Card>
	{:else if project}
		<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100 mb-1">{project.name}</h1>
		<p class="text-sm text-zinc-500 dark:text-zinc-400 mb-6 font-mono">{project.slug} · {projectId.slice(0, 8)}...</p>

		<!-- Settings -->
		<Card class="p-5 mb-6">
			<div class="flex items-center gap-2 mb-4">
				<Settings size={16} class="text-zinc-400" />
				<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">项目设置</h2>
			</div>
			<div class="grid grid-cols-1 md:grid-cols-2 gap-4">
				<div>
					<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">名称</label>
					<Input bind:value={editName} placeholder="项目名称" />
				</div>
				<div>
					<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">状态</label>
					<select bind:value={editStatus} class="w-full h-10 rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 text-sm text-zinc-900 dark:text-zinc-100">
						<option value="active">Active</option>
						<option value="archived">Archived</option>
					</select>
				</div>
			</div>
			<div class="flex items-center gap-3 mt-4">
				<Button size="sm" onclick={saveProject} disabled={saving}>
					{saving ? '保存中...' : '保存'}
				</Button>
				{#if saveMsg}
					<span class="text-xs text-green-600 dark:text-green-400">{saveMsg}</span>
				{/if}
			</div>
		</Card>

		<!-- API Keys -->
		<Card class="p-5">
			<div class="flex items-center justify-between mb-4">
				<div class="flex items-center gap-2">
					<Key size={16} class="text-zinc-400" />
					<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">API Keys</h2>
				</div>
				<span class="text-xs text-zinc-500 dark:text-zinc-400">{keys.filter(k => !k.revoked).length} 活跃</span>
			</div>

			{#if createdKey}
				<div class="mb-4 p-3 rounded-md bg-green-50 dark:bg-green-900/20 border border-green-200 dark:border-green-800">
					<p class="text-xs text-green-700 dark:text-green-400 mb-2">Key 已创建，仅显示一次：</p>
					<div class="flex items-center gap-2">
						<code class="flex-1 text-xs bg-white dark:bg-zinc-800 px-2 py-1 rounded font-mono break-all">{createdKey.plaintext}</code>
						<button onclick={copyKey} class="p-1.5 rounded hover:bg-green-100 dark:hover:bg-green-800 transition-colors">
							{#if keyCopied}
								<Check size={14} class="text-green-600" />
							{:else}
								<Copy size={14} class="text-green-600" />
							{/if}
						</button>
					</div>
				</div>
			{/if}

			<div class="flex gap-2 mb-4">
				<Input bind:value={newKeyName} placeholder="Key 名称" class="flex-1" />
				<Button size="sm" onclick={handleCreateKey} disabled={!newKeyName.trim()}>
					<Plus size={14} />
				</Button>
			</div>

			{#if keys.length === 0}
				<p class="text-sm text-zinc-400 dark:text-zinc-500 py-4 text-center">暂无 API Key，创建一个开始使用</p>
			{:else}
				<div class="space-y-1.5">
					{#each keys as key}
						<div class="flex items-center justify-between py-2 px-3 rounded-md {key.revoked ? 'opacity-50' : ''} hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
							<div class="flex items-center gap-3">
								<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{key.prefix}...{key.last4}</span>
								<span class="text-sm text-zinc-900 dark:text-zinc-100">{key.name}</span>
								{#if key.revoked}
									<span class="text-[10px] px-1.5 py-0.5 bg-red-100 dark:bg-red-900/30 text-red-600 dark:text-red-400 rounded">已撤销</span>
								{/if}
							</div>
							{#if !key.revoked}
								<Button variant="ghost" size="sm" onclick={() => handleRevoke(key.id)}>
									<Trash2 size={12} class="text-red-500" />
								</Button>
							{/if}
						</div>
					{/each}
				</div>
			{/if}
		</Card>

		<!-- Model Aliases -->
		<Card class="p-5 mt-6">
			<div class="flex items-center justify-between mb-4">
				<div class="flex items-center gap-2">
					<ArrowRight size={16} class="text-zinc-400" />
					<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">模型别名</h2>
				</div>
				<span class="text-xs text-zinc-500 dark:text-zinc-400">{aliases.length} 条</span>
			</div>

			<div class="flex gap-2 mb-4">
				<Input bind:value={newAlias} placeholder="别名 (如 gpt-4)" class="flex-1" />
				<Input bind:value={newTarget} placeholder="目标模型 (如 gpt-4o-mini)" class="flex-1" />
				<Button size="sm" onclick={handleAddAlias} disabled={!newAlias.trim() || !newTarget.trim()}>
					<Plus size={14} />
				</Button>
			</div>

			{#if aliases.length === 0}
				<p class="text-sm text-zinc-400 dark:text-zinc-500 py-4 text-center">暂无别名，添加后请求中的 model 会自动映射</p>
			{:else}
				<div class="space-y-1.5">
					{#each aliases as a}
						<div class="flex items-center justify-between py-2 px-3 rounded-md hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
							<div class="flex items-center gap-2">
								<span class="text-sm font-mono text-zinc-900 dark:text-zinc-100">{a.alias}</span>
								<ArrowRight size={12} class="text-zinc-400" />
								<span class="text-sm font-mono text-zinc-600 dark:text-zinc-400">{a.target_model}</span>
								{#if !a.enabled}
									<span class="text-[10px] px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-500 rounded">禁用</span>
								{/if}
							</div>
							<Button variant="ghost" size="sm" onclick={() => handleDeleteAlias(a.alias)}>
								<Trash2 size={12} class="text-red-500" />
							</Button>
						</div>
					{/each}
				</div>
			{/if}
		</Card>
	{/if}
</div>
