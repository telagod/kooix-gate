<script lang="ts">
	import { shortId, rawId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { getMe, listAllOrgs, createOrg, updateOrg } from '$lib/api.js';
	import type { MeResult, OrgDetail } from '$lib/api.js';
	import { Button, Card, Field, Input, Skeleton } from '$lib/components/ui';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
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

<PageShell
	title="组织"
	description={`当前激活：${activeOrg?.slice(0, 12) ?? '—'}...`}
	icon={Building2}
>
	{#snippet actions()}
		{#if me?.is_platform_admin}
			<Button size="sm" onclick={() => (showCreate = !showCreate)}>
				{#if showCreate}
					<X size={14} />
				{:else}
					<Plus size={14} />
				{/if}
			</Button>
		{/if}
	{/snippet}

	{#if showCreate}
		<Card padding="sm" class="mb-6">
			<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 mb-3">创建组织</h3>
			<div class="grid grid-cols-1 md:grid-cols-2 gap-3">
				<Field label="组织名称" for="new-org-name">
					<Input id="new-org-name" bind:value={newName} placeholder="组织名称" />
				</Field>
				<Field label="标识 (slug)" for="new-org-slug">
					<Input id="new-org-slug" bind:value={newSlug} placeholder="kooix" />
				</Field>
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
				<Skeleton class="h-20" />
			{/each}
		</div>
	{:else if error}
		<StatePanel variant="danger" description={error} />
	{:else if orgs.length === 0 && !me?.is_platform_admin}
		{#if me && me.orgs.length > 0}
			<div class="space-y-3">
				{#each me.orgs as orgId}
					<Card padding="sm" class="flex items-center justify-between">
						<div class="flex items-center gap-3">
							<Building2 size={18} class="text-zinc-400" />
							<p class="font-mono text-sm text-zinc-700 dark:text-zinc-300">{orgId}</p>
							{#if orgId === activeOrg}
								<span class="text-xs bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 px-2 py-0.5 rounded">当前</span>
							{/if}
						</div>
						<Button size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/projects`)}>查看项目</Button>
					</Card>
				{/each}
			</div>
		{:else}
			<StatePanel title="暂无组织" description="请联系管理员" icon={Building2} />
		{/if}
	{:else}
		<div class="space-y-3">
			{#each orgs as org}
				<Card padding="sm">
					{#if editingOrg === org.id}
						<div class="space-y-3">
							<div class="grid grid-cols-1 md:grid-cols-2 gap-3">
								<Field label="名称" for={`org-name-${org.id}`}>
									<Input id={`org-name-${org.id}`} bind:value={editName} />
								</Field>
								<Field label="账单邮箱" for={`org-billing-${org.id}`}>
									<Input id={`org-billing-${org.id}`} bind:value={editBilling} placeholder="billing@example.com" />
								</Field>
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
									<p class="text-xs text-zinc-600 dark:text-zinc-300 font-mono">{org.slug} · {shortId(org.id)}...</p>
								</div>
								<span class="text-[10px] px-1.5 py-0.5 rounded-full {
									org.status === 'active'
										? 'bg-green-100 dark:bg-green-900/30 text-green-700 dark:text-green-400'
										: 'bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400'
								}">{org.status}</span>
							</div>
							<div class="flex items-center gap-2">
								<span class="text-xs text-zinc-500 dark:text-zinc-400">{fmtDate(org.created_at)}</span>
								<Button variant="ghost" size="sm" onclick={() => startEdit(org)}>
									<Settings size={14} />
								</Button>
								<Button size="sm" onclick={() => goto(`/orgs/${rawId(org.id)}/projects`)}>项目</Button>
							</div>
						</div>
					{/if}
				</Card>
			{/each}
		</div>
	{/if}
</PageShell>
