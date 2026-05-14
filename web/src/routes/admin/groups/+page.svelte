<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, listGroups, createGroup, updateGroup, deleteGroup, listGroupBindings, addGroupBinding, removeGroupBinding, listAdminChannels } from '$lib/api.js';
	import type { ChannelGroup, GroupBinding, Channel } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import { Layers, Plus, Trash2, Settings, Link } from 'lucide-svelte';

	let groups = $state<ChannelGroup[]>([]);
	let channels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');

	let showCreate = $state(false);
	let newName = $state('');
	let newStrategy = $state('priority');

	let selectedGroup = $state<string | null>(null);
	let bindings = $state<GroupBinding[]>([]);
	let bindingLoading = $state(false);

	let addChannelId = $state('');
	let addPriority = $state(100);
	let addWeight = $state(1);

	const STRATEGIES = ['priority', 'weighted_random', 'round_robin', 'least_conn'];

	onMount(async () => {
		try {
			const me = await getMe();
			if (!me.is_platform_admin) { error = '需要平台管理员权限'; loading = false; return; }
			[groups, channels] = await Promise.all([listGroups(), listAdminChannels()]);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	async function handleCreate() {
		if (!newName.trim()) return;
		try {
			const g = await createGroup(newName.trim(), newStrategy);
			groups = [...groups, g];
			showCreate = false;
			newName = '';
		} catch (err: any) {
			error = err?.message ?? '创建失败';
		}
	}

	async function handleDelete(id: string) {
		try {
			await deleteGroup(id);
			groups = groups.filter(g => g.id !== id);
			if (selectedGroup === id) selectedGroup = null;
		} catch (err: any) {
			error = err?.message ?? '删除失败';
		}
	}

	async function selectGroup(id: string) {
		selectedGroup = id;
		bindingLoading = true;
		try {
			bindings = await listGroupBindings(id);
		} catch (err: any) {
			error = err?.message ?? '加载绑定失败';
		} finally {
			bindingLoading = false;
		}
	}

	async function handleAddBinding() {
		if (!selectedGroup || !addChannelId) return;
		try {
			await addGroupBinding(selectedGroup, addChannelId, addPriority, addWeight);
			bindings = await listGroupBindings(selectedGroup);
			addChannelId = '';
		} catch (err: any) {
			error = err?.message ?? '添加失败';
		}
	}

	async function handleRemoveBinding(channelId: string) {
		if (!selectedGroup) return;
		try {
			await removeGroupBinding(selectedGroup, channelId);
			bindings = bindings.filter(b => b.channel_id !== channelId);
		} catch (err: any) {
			error = err?.message ?? '移除失败';
		}
	}
</script>

<div class="max-w-7xl mx-auto p-6">
	<div class="flex items-center justify-between mb-6">
		<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">渠道分组</h1>
		<Button size="sm" onclick={() => (showCreate = !showCreate)}>
			<Plus size={14} />
		</Button>
	</div>

	{#if error}
		<Card class="p-3 mb-4 bg-red-50 dark:bg-red-900/20 border-red-200 dark:border-red-800">
			<p class="text-xs text-red-600 dark:text-red-400">{error}</p>
		</Card>
	{/if}

	{#if showCreate}
		<Card class="p-4 mb-6">
			<div class="flex gap-3 items-end">
				<div class="flex-1">
					<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">名称</label>
					<Input bind:value={newName} placeholder="生产分组" />
				</div>
				<div>
					<label class="block text-xs font-medium text-zinc-500 dark:text-zinc-400 mb-1">策略</label>
					<select bind:value={newStrategy} class="h-10 rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 text-sm text-zinc-900 dark:text-zinc-100">
						{#each STRATEGIES as s}
							<option value={s}>{s}</option>
						{/each}
					</select>
				</div>
				<Button size="sm" onclick={handleCreate}>创建</Button>
			</div>
		</Card>
	{/if}

	{#if loading}
		<div class="space-y-2">
			{#each Array(3) as _}
				<div class="h-14 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
			{/each}
		</div>
	{:else}
		<div class="grid grid-cols-1 lg:grid-cols-3 gap-6">
			<!-- Left: groups list -->
			<div class="space-y-2">
				{#if groups.length === 0}
					<Card class="p-8 text-center">
						<Layers size={32} class="mx-auto mb-2 text-zinc-300 dark:text-zinc-600" />
						<p class="text-sm text-zinc-500 dark:text-zinc-400">暂无分组</p>
					</Card>
				{:else}
					{#each groups as group}
						<button
							onclick={() => selectGroup(group.id)}
							class="w-full text-left p-3 rounded-lg border transition-colors {selectedGroup === group.id ? 'border-zinc-900 dark:border-zinc-100 bg-zinc-50 dark:bg-zinc-800' : 'border-zinc-200 dark:border-zinc-700 hover:border-zinc-400 dark:hover:border-zinc-500'}"
						>
							<div class="flex items-center justify-between">
								<div>
									<p class="text-sm font-medium text-zinc-900 dark:text-zinc-100">{group.name}</p>
									<p class="text-xs text-zinc-500 dark:text-zinc-400">{group.strategy} · {group.enabled ? '启用' : '禁用'}</p>
								</div>
								<Button variant="ghost" size="sm" onclick={(e) => { e.stopPropagation(); handleDelete(group.id); }}>
									<Trash2 size={12} class="text-red-500" />
								</Button>
							</div>
						</button>
					{/each}
				{/if}
			</div>

			<!-- Right: bindings -->
			<div class="lg:col-span-2">
				{#if selectedGroup}
					<Card class="p-5">
						<div class="flex items-center gap-2 mb-4">
							<Link size={16} class="text-zinc-400" />
							<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100">渠道绑定</h2>
						</div>

						<!-- Add binding -->
						<div class="flex gap-2 items-end mb-4">
							<div class="flex-1">
								<select bind:value={addChannelId} class="w-full h-9 rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-2 text-sm text-zinc-900 dark:text-zinc-100">
									<option value="">选择渠道...</option>
									{#each channels as ch}
										<option value={ch.id}>{ch.name} ({ch.provider_type})</option>
									{/each}
								</select>
							</div>
							<Input bind:value={addPriority} type="number" placeholder="优先级" class="w-20" />
							<Input bind:value={addWeight} type="number" placeholder="权重" class="w-20" />
							<Button size="sm" onclick={handleAddBinding} disabled={!addChannelId}>
								<Plus size={14} />
							</Button>
						</div>

						{#if bindingLoading}
							<p class="text-sm text-zinc-400">加载中...</p>
						{:else if bindings.length === 0}
							<p class="text-sm text-zinc-400 dark:text-zinc-500 py-4 text-center">暂无绑定，添加渠道到此分组</p>
						{:else}
							<div class="space-y-1.5">
								{#each bindings as b}
									<div class="flex items-center justify-between py-2 px-3 rounded-md hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
										<div class="flex items-center gap-3">
											<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{b.channel_code}</span>
											<span class="text-sm text-zinc-900 dark:text-zinc-100">{b.channel_name}</span>
											<span class="text-[10px] px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 rounded">{b.provider_type}</span>
										</div>
										<div class="flex items-center gap-3">
											<span class="text-xs text-zinc-400">P:{b.priority} W:{b.weight}</span>
											<Button variant="ghost" size="sm" onclick={() => handleRemoveBinding(b.channel_id)}>
												<Trash2 size={12} class="text-red-500" />
											</Button>
										</div>
									</div>
								{/each}
							</div>
						{/if}
					</Card>
				{:else}
					<Card class="p-12 text-center">
						<Settings size={32} class="mx-auto mb-2 text-zinc-300 dark:text-zinc-600" />
						<p class="text-sm text-zinc-500 dark:text-zinc-400">选择左侧分组查看绑定详情</p>
					</Card>
				{/if}
			</div>
		</div>
	{/if}
</div>
