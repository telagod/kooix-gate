<!-- /orgs/[orgId]/projects — Org 下的 Project 列表 + 创建 -->
<script lang="ts">
	import { shortId, rawId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { listProjects, createProject } from '$lib/api.js';
	import type { Project } from '$lib/api.js';
	import Alert from '$lib/components/ui/Alert.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Field from '$lib/components/ui/Field.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import InvitationPanel from '$lib/components/InvitationPanel.svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate, text } from '$lib/design';
	import { CreditCard, FolderOpen, Gauge, KeyRound, Plus, Settings, X } from 'lucide-svelte';

	let orgId = $derived($page.params.orgId ?? '');
	let projects = $state<Project[]>([]);
	let loading = $state(true);
	let error = $state('');

	// 创建表单
	let showCreate = $state(false);
	let newName = $state('');
	let newSlug = $state('');
	let creating = $state(false);
	let createError = $state('');

	onMount(async () => {
		await loadProjects();
	});

	async function loadProjects() {
		loading = true;
		error = '';
		try {
			projects = await listProjects(orgId);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	async function handleCreate(e: SubmitEvent) {
		e.preventDefault();
		if (!newName.trim() || !newSlug.trim()) return;

		creating = true;
		createError = '';
		try {
			const proj = await createProject(orgId, newName.trim(), newSlug.trim());
			projects = [...projects, proj];
			showCreate = false;
			newName = '';
			newSlug = '';
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	function statusVariant(status: string): 'default' | 'success' | 'warning' | 'danger' {
		if (status === 'active') return 'success';
		if (status === 'suspended') return 'warning';
		if (status === 'deleted') return 'danger';
		return 'default';
	}
</script>

<PageShell
	title="项目列表"
	description="管理当前 Org 下的 Project、邀请入口与项目级 API Key 设置。"
	eyebrow={`组织 / ${shortId(orgId)}`}
	icon={FolderOpen}
	max="wide"
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/billing`)}>
			<CreditCard size={14} />
			账单
		</Button>
		<Button variant="outline" size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/quotas`)}>
			<Gauge size={14} />
			配额管理
		</Button>
		<Button size="sm" onclick={() => (showCreate = !showCreate)}>
			{#if showCreate}
				<X size={14} />
				取消
			{:else}
				<Plus size={14} />
				创建项目
			{/if}
		</Button>
	{/snippet}

	{#if showCreate}
		<Card padding="md" class="mb-6">
			<div class="mb-4 flex items-start justify-between gap-3">
				<div>
					<p class="text-base font-semibold {text.primary}">新建项目</p>
					<p class="text-xs {text.muted}">Project slug 会进入 URL 与 API scope，请使用稳定短标识。</p>
				</div>
				<Button variant="ghost" size="sm" onclick={() => (showCreate = false)}>
					<X size={14} />
					关闭
				</Button>
			</div>
			<form onsubmit={handleCreate} class="space-y-4">
				<div class="grid gap-3 md:grid-cols-2">
					<Field label="项目名称" for="proj-name" required>
						<Input id="proj-name" placeholder="My Project" bind:value={newName} disabled={creating} />
					</Field>
					<Field label="Slug" for="proj-slug" hint="例如 my-project，只提交给后端校验 slug 规则。" required>
						<Input id="proj-slug" placeholder="my-project" bind:value={newSlug} disabled={creating} />
					</Field>
				</div>
				{#if createError}
					<Alert variant="danger">{createError}</Alert>
				{/if}
				<div class="flex justify-end gap-2">
					<Button variant="outline" type="button" onclick={() => (showCreate = false)}>取消</Button>
					<Button type="submit" disabled={creating || !newName.trim() || !newSlug.trim()}>
						<Plus size={14} />
						{creating ? '创建中...' : '创建'}
					</Button>
				</div>
			</form>
		</Card>
	{/if}

	<InvitationPanel scope="org" {orgId} class="mb-6" />

	{#if loading}
		<StatePanel title="正在读取项目" description="吾正在拉取当前 Org 的 Project 列表。" icon={FolderOpen} />
	{:else if error}
		<StatePanel title="项目加载失败" description={error} icon={FolderOpen} variant="danger">
			{#snippet actions()}
				<Button variant="outline" onclick={loadProjects}>重试</Button>
			{/snippet}
		</StatePanel>
	{:else}
		<DataTable isEmpty={projects.length === 0} emptyColspan={5}>
			{#snippet head()}
				<tr>
					<th class={dataTemplate.th}>名称</th>
					<th class={dataTemplate.th}>Slug</th>
					<th class={dataTemplate.th}>状态</th>
					<th class={dataTemplate.th}>ID</th>
					<th class="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">操作</th>
				</tr>
			{/snippet}

			{#snippet empty()}
				<div class="flex flex-col items-center">
					<FolderOpen size={34} class="mb-3 text-zinc-300 dark:text-zinc-600" />
					<p class="text-sm font-medium {text.secondary}">暂无项目</p>
					<p class="mt-1 text-xs {text.muted}">点击右上角「创建项目」开始使用。</p>
				</div>
			{/snippet}

			{#each projects as proj}
				<tr class={dataTemplate.row}>
					<td class={dataTemplate.tdStrong}>
						<div class="font-medium">{proj.name}</div>
					</td>
					<td class={dataTemplate.tdMono}>{proj.slug}</td>
					<td class={dataTemplate.td}>
						<Badge variant={statusVariant(proj.status)}>{proj.status}</Badge>
					</td>
					<td class={cn(dataTemplate.tdMono, 'max-w-[260px] truncate')}>{proj.id}</td>
					<td class="px-4 py-3 text-right">
						<div class="flex justify-end gap-1">
							<Button variant="ghost" size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/projects/${rawId(proj.id)}`)}>
								<Settings size={14} />
								设置
							</Button>
							<Button variant="ghost" size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/projects/${rawId(proj.id)}/keys`)}>
								<KeyRound size={14} />
								API Keys
							</Button>
						</div>
					</td>
				</tr>
			{/each}
		</DataTable>
	{/if}
</PageShell>
