<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getMe, listAllOrgs, createOrg, updateOrg } from '$lib/api.js';
	import type { MeResult, OrgDetail } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import { Building2, Plus, Settings, X } from 'lucide-svelte';

	let me = $state<MeResult | null>(null);
	let orgs = $state<OrgDetail[]>([]);
	let loading = $state(true);
	let error = $state('');
	let activeOrg = $state<string | null>(null);

	let showCreate = $state(false);
	let newName = $state('');
	let newSlug = $state('');
	let createError = $state('');

	let editingOrg = $state<string | null>(null);
	let editName = $state('');
	let editBilling = $state('');
	let editError = $state('');

	onMount(async () => {
		try {
			me = await getMe();
			activeOrg = me.current_org ?? me.orgs[0] ?? null;
			if (me.is_platform_admin) {
				orgs = await listAllOrgs();
			}
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	async function handleCreate() {
		createError = '';
		if (!newName.trim() || !newSlug.trim()) { createError = '名称和标识必填'; return; }
		try {
			const created = await createOrg(newName.trim(), newSlug.trim());
			orgs = [created, ...orgs];
			showCreate = false;
			newName = '';
			newSlug = '';
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		}
	}

	function startEdit(org: OrgDetail) {
		editingOrg = org.id;
		editName = org.name;
		editBilling = org.billing_email ?? '';
		editError = '';
	}

	async function saveEdit() {
		editError = '';
		if (!editingOrg) return;
		try {
			const updated = await updateOrg(editingOrg, {
				name: editName || undefined,
				billing_email: editBilling || undefined
			});
			orgs = orgs.map(o => o.id === updated.id ? updated : o);
			editingOrg = null;
		} catch (err: any) {
			editError = err?.message ?? '更新失败';
		}
	}

	function fmtDate(d: string): string {
		return new Date(d).toLocaleDateString('zh-CN', { year: 'numeric', month: 'short', day: 'numeric' });
	}
</script>

<div class="max-w-7xl mx-auto p-6">
	<div class="flex items-center justify-between mb-6">
		<div>
			<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">组织</h1>
			<p class="text-sm text-zinc-500 dark:text-zinc-400 mt-0.5">
				当前激活：<span class="font-mono font-medium text-zinc-700 dark:text-zinc-300">{activeOrg?.slice(0, 12) ?? '—'}...</span>
			</p>
		</div>
		{#if me?.is_platform_admin}
			<Button size="sm" onclick={() => (showCreate = !showCreate)}>
				{#if showCreate}
					<X size={14} />
				{:else}
					<Plus size={14} />
				{/if}
			</Button>
		{/if}
	</div>

	{#if showCreate}
		<Card class="p-4 mb-6">
			<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 mb-3">创建组织</h3>
			<div class="grid grid-cols-1 md:grid-cols-2 gap-3">
				<Input bind:value={newName} placeholder="组织名称" />
				<Input bind:value={newSlug} placeholder="标识 (slug)" />
			</div>
			{#if createError}
				<p class="text-xs text-red-600 dark:text-red-400 mt-2">{createError}</p>
			{/if}
			<Button size="sm" onclick={handleCreate} class="mt-3">创建</Button>
		</Card>
	{/if}

	{#if loading}
		<div class="space-y-3">
			{#each Array(3) as _}
				<div class="h-20 bg-zinc-200 dark:bg-zinc-700 rounded-lg animate-pulse"></div>
			{/each}
		</div>
	{:else if error}
		<Card class="p-6">
			<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
		</Card>
	{:else if orgs.length === 0 && !me?.is_platform_admin}
		<!-- Non-admin: simple org list from me.orgs -->
		{#if me && me.orgs.length > 0}
			<div class="space-y-3">
				{#each me.orgs as orgId}
					<Card class="p-4 flex items-center justify-between">
						<div class="flex items-center gap-3">
							<Building2 size={18} class="text-zinc-400" />
							<p class="font-mono text-sm text-zinc-700 dark:text-zinc-300">{orgId}</p>
							{#if orgId === activeOrg}
								<span class="text-xs bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 px-2 py-0.5 rounded">当前</span>
							{/if}
						</div>
						<Button size="sm" onclick={() => goto(`/orgs/${orgId}/projects`)}>查看项目</Button>
					</Card>
				{/each}
			</div>
		{:else}
			<Card class="p-12 text-center">
				<Building2 size={40} class="mx-auto mb-3 text-zinc-300 dark:text-zinc-600" />
				<p class="text-sm text-zinc-500 dark:text-zinc-400">暂无组织，请联系管理员</p>
			</Card>
		{/if}
	{:else}
		<!-- Admin: detailed org list -->
		<div class="space-y-3">
			{#each orgs as org}
				<Card class="p-4">
					{#if editingOrg === org.id}
						<div class="space-y-3">
							<div class="grid grid-cols-1 md:grid-cols-2 gap-3">
								<div>
									<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">名称</label>
									<Input bind:value={editName} />
								</div>
								<div>
									<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">账单邮箱</label>
									<Input bind:value={editBilling} placeholder="billing@example.com" />
								</div>
							</div>
							{#if editError}
								<p class="text-xs text-red-600 dark:text-red-400">{editError}</p>
							{/if}
							<div class="flex gap-2">
								<Button size="sm" onclick={saveEdit}>保存</Button>
								<Button variant="ghost" size="sm" onclick={() => (editingOrg = null)}>取消</Button>
							</div>
						</div>
					{:else}
						<div class="flex items-center justify-between">
							<div class="flex items-center gap-3">
								<Building2 size={18} class="text-zinc-400" />
								<div>
									<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">{org.name}</p>
									<p class="text-xs text-zinc-500 dark:text-zinc-400 font-mono">{org.slug} · {org.id.slice(0, 8)}...</p>
								</div>
								<span class="text-[10px] px-1.5 py-0.5 rounded-full {
									org.status === 'active'
										? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
										: 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400'
								}">{org.status}</span>
							</div>
							<div class="flex items-center gap-2">
								<span class="text-xs text-zinc-400 dark:text-zinc-500">{fmtDate(org.created_at)}</span>
								<Button variant="ghost" size="sm" onclick={() => startEdit(org)}>
									<Settings size={14} />
								</Button>
								<Button size="sm" onclick={() => goto(`/orgs/${org.id}/projects`)}>项目</Button>
							</div>
						</div>
					{/if}
				</Card>
			{/each}
		</div>
	{/if}
</div>
