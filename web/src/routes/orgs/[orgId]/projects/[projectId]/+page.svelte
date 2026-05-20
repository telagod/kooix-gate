<script lang="ts">
	import { rawId, shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { getProject, updateProject, listKeys, createKey, revokeKey, listModelAliases, upsertModelAlias, deleteModelAlias } from '$lib/api.js';
	import type { Project, ApiKey, CreateKeyResponse, ModelAlias } from '$lib/api.js';
	import { Button, Card, Field, Input, Select } from '$lib/components/ui';
	import InvitationPanel from '$lib/components/InvitationPanel.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { Settings, Key, Plus, Trash2, Copy, Check, ArrowRight, ArrowLeft } from 'lucide-svelte';

	let orgId = $derived($page.params.orgId ?? '');
	let projectId = $derived($page.params.projectId ?? '');

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

	const statusOptions = [
		{ value: 'active', label: 'Active 启用' },
		{ value: 'archived', label: 'Archived 归档' }
	];

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

<PageShell
	title={project?.name ?? '项目设置'}
	description={project ? `${project.slug} · ${shortId(projectId)}` : `Project: ${shortId(projectId)}`}
	eyebrow={`组织 / ${shortId(orgId)} / 项目`}
	icon={Settings}
	max="wide"
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/projects`)}>
			<ArrowLeft size={14} />
			项目列表
		</Button>
		<Button variant="outline" size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/projects/${rawId(projectId)}/keys`)}>
			<Key size={14} />
			API Keys
		</Button>
	{/snippet}

	{#if loading}
		<StatePanel title="正在读取项目设置" description="吾正在拉取 Project 设置、API Keys 与模型别名。" icon={Settings} />
	{:else if error && !project}
		<StatePanel title="项目加载失败" description={error} icon={Settings} variant="danger" />
	{:else if project}
		<!-- Settings -->
		<Card class="p-5 mb-6">
			<div class="flex items-center gap-2 mb-4">
				<Settings size={16} class="text-zinc-400" />
				<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">项目设置</h2>
			</div>
			<div class="grid grid-cols-1 md:grid-cols-2 gap-4">
				<Field label="名称" for="project-name">
					<Input id="project-name" bind:value={editName} placeholder="项目名称" />
				</Field>
				<Field label="状态" for="project-status">
					<Select id="project-status" bind:value={editStatus} options={statusOptions} />
				</Field>
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

		<InvitationPanel scope="project" {orgId} {projectId} class="mb-6" />

		<!-- API Keys -->
		<Card class="p-5">
			<div class="flex items-center justify-between mb-4">
				<div class="flex items-center gap-2">
					<Key size={16} class="text-zinc-400" />
					<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">API Keys 凭据</h2>
				</div>
				<span class="text-xs text-zinc-600 dark:text-zinc-300">{keys.filter(k => !k.revoked).length} 活跃</span>
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
				<p class="text-sm text-zinc-500 dark:text-zinc-400 py-4 text-center">暂无 API Key，创建一个开始使用</p>
			{:else}
				<div class="space-y-1.5">
					{#each keys as key}
						<div class="flex items-center justify-between py-2 px-3 rounded-md {key.revoked ? 'opacity-50' : ''} hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
							<div class="flex items-center gap-3">
								<span class="text-xs font-mono text-zinc-600 dark:text-zinc-300">{key.prefix}...{key.last4}</span>
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
				<span class="text-xs text-zinc-600 dark:text-zinc-300">{aliases.length} 条</span>
			</div>

			<div class="flex gap-2 mb-4">
				<Input bind:value={newAlias} placeholder="别名 (如 gpt-4)" class="flex-1" />
				<Input bind:value={newTarget} placeholder="目标模型 (如 gpt-4o-mini)" class="flex-1" />
				<Button size="sm" onclick={handleAddAlias} disabled={!newAlias.trim() || !newTarget.trim()}>
					<Plus size={14} />
				</Button>
			</div>

			{#if aliases.length === 0}
				<p class="text-sm text-zinc-500 dark:text-zinc-400 py-4 text-center">暂无别名，添加后请求中的 model 会自动映射</p>
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
</PageShell>
