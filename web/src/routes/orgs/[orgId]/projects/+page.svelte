<!-- /orgs/[orgId]/projects — Org 下的 Project 列表 + 创建 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { listProjects, createProject } from '$lib/api.js';
	import { getAccessToken, clearTokens } from '$lib/auth.js';
	import type { Project } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let orgId = $derived($page.params.orgId);
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
		if (!getAccessToken()) {
			goto('/login');
			return;
		}
		await loadProjects();
	});

	async function loadProjects() {
		loading = true;
		error = '';
		try {
			projects = await listProjects(orgId);
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
</script>

<div>
	<!-- 子导航：面包屑 -->
	<div class="bg-white border-b border-zinc-200 px-6 py-2 flex items-center gap-3">
		<button
			onclick={() => goto('/orgs')}
			class="text-sm text-zinc-500 hover:text-zinc-900 transition-colors"
		>
			← 组织列表
		</button>
		<span class="text-zinc-300">/</span>
		<span class="text-sm font-medium text-zinc-900 font-mono">{orgId}</span>
	</div>

	<div class="max-w-4xl mx-auto p-6">
		<div class="flex items-center justify-between mb-6">
			<h1 class="text-2xl font-bold text-zinc-900">项目列表</h1>
			<Button onclick={() => (showCreate = !showCreate)}>
				{showCreate ? '取消' : '+ 创建项目'}
			</Button>
		</div>

		<!-- 创建表单 -->
		{#if showCreate}
			<Card class="p-5 mb-6">
				<h2 class="text-base font-semibold text-zinc-900 mb-4">新建项目</h2>
				<form onsubmit={handleCreate} class="space-y-3">
					<div>
						<label for="proj-name" class="block text-sm font-medium text-zinc-700 mb-1">项目名称</label>
						<Input id="proj-name" placeholder="My Project" bind:value={newName} disabled={creating} />
					</div>
					<div>
						<label for="proj-slug" class="block text-sm font-medium text-zinc-700 mb-1">Slug</label>
						<Input id="proj-slug" placeholder="my-project" bind:value={newSlug} disabled={creating} />
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

		<!-- 项目列表 -->
		{#if loading}
			<p class="text-zinc-500">加载中...</p>
		{:else if error}
			<p class="text-red-600">{error}</p>
		{:else if projects.length === 0}
			<Card class="p-6">
				<p class="text-zinc-500 text-sm">暂无项目，点击右上角创建。</p>
			</Card>
		{:else}
			<div class="overflow-hidden rounded-lg border border-zinc-200 bg-white">
				<table class="w-full text-sm">
					<thead class="bg-zinc-50 border-b border-zinc-200">
						<tr>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">名称</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">Slug</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">状态</th>
							<th class="px-4 py-3 text-left font-medium text-zinc-600">ID</th>
						</tr>
					</thead>
					<tbody class="divide-y divide-zinc-100">
						{#each projects as proj}
							<tr class="hover:bg-zinc-50 transition-colors">
								<td class="px-4 py-3 font-medium text-zinc-900">{proj.name}</td>
								<td class="px-4 py-3 font-mono text-zinc-600">{proj.slug}</td>
								<td class="px-4 py-3">
									<span class="inline-block px-2 py-0.5 rounded text-xs font-medium
										{proj.status === 'active' ? 'bg-green-50 text-green-700' : 'bg-zinc-100 text-zinc-600'}">
										{proj.status}
									</span>
								</td>
								<td class="px-4 py-3 font-mono text-xs text-zinc-400">{proj.id}</td>
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		{/if}
	</div>
</div>
