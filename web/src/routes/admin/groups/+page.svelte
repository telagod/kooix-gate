<script lang="ts">
	import { onMount } from 'svelte';
	import {
		getMe, listGroups, createGroup, updateGroup, deleteGroup,
		getGroupDetail, listGroupBindings, addGroupBinding,
		updateGroupBinding, removeGroupBinding, listAdminChannels
	} from '$lib/api.js';
	import type { ChannelGroup, GroupBinding, GroupDetail, Channel } from '$lib/api.js';
	import { addToast } from '$lib/stores/toast.js';
	import Skeleton from '$lib/components/ui/Skeleton.svelte';
	import {
		Layers, Plus, Trash2, Pencil, X, ChevronRight, Search,
		ToggleLeft, ToggleRight, ArrowRight, AlertTriangle, Check, RefreshCw
	} from 'lucide-svelte';

	// ── Strategy metadata ──
	const STRATEGIES: Record<string, { label: string; color: string; desc: string }> = {
		priority:        { label: '优先级',     color: 'blue',   desc: '按优先级选择，总是使用优先级最高的可用渠道' },
		weighted_random: { label: '加权随机',   color: 'green',  desc: '按权重随机分配，权重越高被选中概率越大' },
		round_robin:     { label: '轮询',       color: 'purple', desc: '轮询分配，依次使用每个渠道' },
		least_conn:      { label: '最少连接',   color: 'orange', desc: '优先使用当前并发最少的渠道' },
		least_latency:   { label: '最低延迟',   color: 'yellow', desc: '优先使用平均响应最快的渠道' },
	};

	const PROVIDER_COLOR = 'bg-zinc-100 text-zinc-700 dark:bg-zinc-700 dark:text-zinc-300';

	// ── State ──
	let groups = $state<ChannelGroup[]>([]);
	let allChannels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');

	// selected group detail
	let selectedId = $state<string | null>(null);
	let detail = $state<GroupDetail | null>(null);
	let detailLoading = $state(false);

	// editing
	let editing = $state(false);
	let editForm = $state({ name: '', strategy: 'priority', description: '', fallback_group_id: null as string | null, enabled: true });

	// create modal
	let showCreate = $state(false);
	let createForm = $state({ name: '', strategy: 'priority', description: '', fallback_group_id: null as string | null });

	// delete confirm
	let deleteTarget = $state<ChannelGroup | null>(null);

	// add channel modal
	let showAddChannel = $state(false);
	let channelSearch = $state('');
	let channelProviderFilter = $state('');
	let selectedChannels = $state<Set<string>>(new Set());
	let addPriority = $state(100);
	let addWeight = $state(1);

	// inline editing binding
	let editingBindingId = $state<string | null>(null);
	let editBindingPriority = $state(0);
	let editBindingWeight = $state(0);

	// ── Helpers ──
	function strategyMeta(s: string) { return STRATEGIES[s] ?? { label: s, color: 'gray', desc: '' }; }
	function providerColor(_p: string) { return PROVIDER_COLOR; }

	function strategyBadgeClass(_color: string) {
		return 'bg-zinc-100 text-zinc-700 dark:bg-zinc-700 dark:text-zinc-300';
	}

	function healthDot(bindings: GroupBinding[]): string {
		if (!bindings || bindings.length === 0) return 'bg-gray-300 dark:bg-gray-600';
		const healthy = bindings.filter(b => (b.channel_health ?? 'healthy') === 'healthy').length;
		if (healthy === bindings.length) return 'bg-green-500';
		if (healthy > 0) return 'bg-yellow-500';
		return 'bg-red-500';
	}

	function groupName(id: string | null | undefined): string {
		if (!id) return '';
		return groups.find(g => g.id === id)?.name ?? id.slice(0, 8);
	}

	// Fallback chain builder
	function buildFallbackChain(groupId: string): ChannelGroup[] {
		const chain: ChannelGroup[] = [];
		const visited = new Set<string>();
		let current: string | null | undefined = groupId;
		while (current && !visited.has(current) && chain.length < 6) {
			visited.add(current);
			const g = groups.find(gr => gr.id === current);
			if (!g) break;
			chain.push(g);
			current = g.fallback_group_id;
		}
		return chain;
	}

	// Filtered channels for add modal
	let filteredChannels = $derived.by(() => {
		const boundIds = new Set(detail?.bindings?.map(b => b.channel_id) ?? []);
		return allChannels
			.filter(c => !boundIds.has(c.id))
			.filter(c => !channelProviderFilter || c.provider_type === channelProviderFilter)
			.filter(c => !channelSearch || c.name.toLowerCase().includes(channelSearch.toLowerCase()) || c.code.toLowerCase().includes(channelSearch.toLowerCase()));
	});

	let providerTypes = $derived([...new Set(allChannels.map(c => c.provider_type))].sort());

	// ── Data loading ──
	async function loadGroups() {
		loading = true;
		try {
			const [g, ch] = await Promise.all([listGroups(), listAdminChannels()]);
			groups = g;
			allChannels = ch;
		} catch (e: any) {
			error = e.message || '加载失败';
		} finally {
			loading = false;
		}
	}

	async function loadDetail(id: string) {
		detailLoading = true;
		try {
			detail = await getGroupDetail(id);
		} catch {
			const bindings = await listGroupBindings(id);
			const group = groups.find(g => g.id === id)!;
			detail = { group, bindings, project_ids: [] };
		} finally {
			detailLoading = false;
		}
	}

	async function selectGroup(id: string) {
		if (selectedId === id) {
			selectedId = null;
			detail = null;
			editing = false;
			return;
		}
		selectedId = id;
		editing = false;
		await loadDetail(id);
	}

	// ── CRUD actions ──
	async function handleCreate() {
		try {
			await createGroup(createForm.name, createForm.strategy, createForm.description, createForm.fallback_group_id);
			addToast('分组已创建', 'success');
			showCreate = false;
			createForm = { name: '', strategy: 'priority', description: '', fallback_group_id: null };
			await loadGroups();
		} catch (e: any) {
			addToast(e.message || '创建失败', 'error');
		}
	}

	async function handleUpdate() {
		if (!selectedId) return;
		try {
			await updateGroup(selectedId, {
				name: editForm.name,
				strategy: editForm.strategy,
				description: editForm.description,
				fallback_group_id: editForm.fallback_group_id,
				enabled: editForm.enabled,
			});
			addToast('分组已更新', 'success');
			editing = false;
			await loadGroups();
			await loadDetail(selectedId);
		} catch (e: any) {
			addToast(e.message || '更新失败', 'error');
		}
	}

	async function handleDelete() {
		if (!deleteTarget) return;
		try {
			await deleteGroup(deleteTarget.id);
			addToast('分组已删除', 'success');
			if (selectedId === deleteTarget.id) {
				selectedId = null;
				detail = null;
			}
			deleteTarget = null;
			await loadGroups();
		} catch (e: any) {
			addToast(e.message || '删除失败', 'error');
		}
	}

	async function toggleEnabled(group: ChannelGroup) {
		try {
			await updateGroup(group.id, { enabled: !group.enabled });
			group.enabled = !group.enabled;
			groups = [...groups];
		} catch (e: any) {
			addToast(e.message || '切换失败', 'error');
		}
	}

	function startEdit() {
		if (!detail) return;
		const g = detail.group;
		editForm = {
			name: g.name,
			strategy: g.strategy,
			description: g.description ?? '',
			fallback_group_id: g.fallback_group_id ?? null,
			enabled: g.enabled,
		};
		editing = true;
	}

	// ── Binding actions ──
	async function handleAddChannels() {
		if (!selectedId || selectedChannels.size === 0) return;
		try {
			for (const chId of selectedChannels) {
				await addGroupBinding(selectedId, chId, addPriority, addWeight);
			}
			addToast(`已添加 ${selectedChannels.size} 个渠道`, 'success');
			showAddChannel = false;
			selectedChannels = new Set();
			channelSearch = '';
			channelProviderFilter = '';
			await loadDetail(selectedId);
		} catch (e: any) {
			addToast(e.message || '添加失败', 'error');
		}
	}

	async function handleRemoveBinding(channelId: string) {
		if (!selectedId) return;
		try {
			await removeGroupBinding(selectedId, channelId);
			addToast('已移除渠道', 'success');
			await loadDetail(selectedId);
		} catch (e: any) {
			addToast(e.message || '移除失败', 'error');
		}
	}

	function startEditBinding(b: GroupBinding) {
		editingBindingId = b.channel_id;
		editBindingPriority = b.priority;
		editBindingWeight = b.weight;
	}

	async function saveBinding() {
		if (!selectedId || !editingBindingId) return;
		try {
			await updateGroupBinding(selectedId, editingBindingId, {
				priority: editBindingPriority,
				weight: editBindingWeight,
			});
			editingBindingId = null;
			addToast('已更新', 'success');
			await loadDetail(selectedId);
		} catch (e: any) {
			addToast(e.message || '更新失败', 'error');
		}
	}

	function toggleChannel(id: string) {
		const next = new Set(selectedChannels);
		if (next.has(id)) next.delete(id); else next.add(id);
		selectedChannels = next;
	}

	onMount(loadGroups);
</script>

<svelte:head><title>渠道分组管理 | Kooix Gate</title></svelte:head>

<div class="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-6 space-y-6">

	<!-- Header -->
	<div class="flex items-center justify-between">
		<div class="flex items-center gap-3">
			<Layers class="w-6 h-6 text-zinc-500" />
			<h1 class="text-xl font-semibold text-zinc-900 dark:text-zinc-100">渠道分组管理</h1>
		</div>
		<button onclick={() => { showCreate = true; }} class="inline-flex items-center gap-1.5 px-3 py-1.5 text-sm font-medium rounded-lg bg-zinc-900 text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200 transition-colors">
			<Plus class="w-4 h-4" /> 新建分组
		</button>
	</div>

	<!-- Loading -->
	{#if loading}
		<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
			{#each Array(4) as _}
				<div class="bg-white dark:bg-zinc-800 rounded-lg border border-zinc-200 dark:border-zinc-700 p-4 space-y-3">
					<Skeleton class="h-5 w-2/3" /><Skeleton class="h-4 w-1/2" /><Skeleton class="h-4 w-1/3" />
				</div>
			{/each}
		</div>

	{:else if error}
		<div class="bg-red-50 dark:bg-red-900/20 border border-red-200 dark:border-red-800 rounded-lg p-4 text-red-700 dark:text-red-300">{error}</div>

	{:else if groups.length === 0}
		<div class="text-center py-16">
			<Layers class="w-12 h-12 mx-auto text-zinc-300 dark:text-zinc-600 mb-3" />
			<p class="text-zinc-600 dark:text-zinc-300 mb-4">还没有分组，点击右上角创建</p>
		</div>

	{:else}

		<!-- ═══ Group Grid ═══ -->
		<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
			{#each groups as group (group.id)}
				{@const meta = strategyMeta(group.strategy)}
				{@const count = group.channel_count ?? 0}
				{@const isSelected = selectedId === group.id}
				<button
					onclick={() => selectGroup(group.id)}
					class="text-left bg-white dark:bg-zinc-800 rounded-lg border-2 p-4 transition-all hover:shadow-md
						{isSelected ? 'border-zinc-900 dark:border-zinc-100 shadow-md ring-1 ring-zinc-900/20 dark:ring-zinc-100/20' : 'border-zinc-200 dark:border-zinc-700'}"
				>
					<!-- Top row: name + toggle -->
					<div class="flex items-start justify-between gap-2">
						<h3 class="font-medium text-zinc-900 dark:text-zinc-100 truncate">{group.name}</h3>
						<div
							role="switch"
							aria-checked={group.enabled}
							tabindex="0"
							onclick={(e: MouseEvent) => { e.stopPropagation(); toggleEnabled(group); }}
							onkeydown={(e: KeyboardEvent) => { e.stopPropagation(); if (e.key === 'Enter') toggleEnabled(group); }}
							class="relative inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full transition-colors
								{group.enabled ? 'bg-zinc-900 dark:bg-zinc-100' : 'bg-zinc-300 dark:bg-zinc-600'}"
						>
							<span class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow ring-0 transition-transform mt-0.5
								{group.enabled ? 'translate-x-4 ml-0.5' : 'translate-x-0.5'}"></span>
						</div>
					</div>

					<!-- Strategy badge -->
					<div class="mt-2 flex items-center gap-2">
						<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {strategyBadgeClass(meta.color)}">
							{meta.label}
						</span>
						<span class="text-sm text-zinc-600 dark:text-zinc-300">{count} 渠道</span>
					</div>

					<!-- Description -->
					{#if group.description}
						<p class="mt-2 text-sm text-zinc-600 dark:text-zinc-300 truncate">{group.description}</p>
					{/if}

					<!-- Fallback -->
					{#if group.fallback_group_id}
						<div class="mt-2 flex items-center gap-1 text-sm text-zinc-600 dark:text-zinc-300">
							<ArrowRight class="w-3 h-3" />
							<span class="truncate">回退: {groupName(group.fallback_group_id)}</span>
						</div>
					{/if}
				</button>
			{/each}
		</div>
	{/if}

	<!-- ═══ Group Detail Panel ═══ -->
	{#if selectedId && detail}
		{@const g = detail.group}
		{@const meta = strategyMeta(g.strategy)}
		{@const chain = buildFallbackChain(selectedId)}

		<div class="bg-white dark:bg-zinc-800 rounded-lg border border-zinc-200 dark:border-zinc-700 divide-y divide-zinc-200 dark:divide-zinc-700">

			<!-- Detail header -->
			<div class="p-5">
				{#if detailLoading}
					<Skeleton class="h-6 w-48" />
				{:else if !editing}
					<div class="flex items-center justify-between">
						<div>
							<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">{g.name}</h2>
							{#if g.description}<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-1">{g.description}</p>{/if}
							<div class="flex items-center gap-3 mt-2">
								<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {strategyBadgeClass(meta.color)}">{meta.label}</span>
								<span class="text-xs text-zinc-600 dark:text-zinc-300">{meta.desc}</span>
							</div>
						</div>
						<div class="flex items-center gap-2">
							<button onclick={startEdit} class="p-2 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-700 text-zinc-500">
								<Pencil class="w-4 h-4" />
							</button>
							<button onclick={() => { deleteTarget = g; }} class="p-2 rounded-lg hover:bg-red-50 dark:hover:bg-red-900/20 text-red-500">
								<Trash2 class="w-4 h-4" />
							</button>
						</div>
					</div>

					<!-- Strategy impact notice -->
					{#if g.strategy === 'weighted_random'}
						<div class="mt-3 text-xs text-zinc-600 dark:text-zinc-400 bg-zinc-50 dark:bg-zinc-900 rounded px-3 py-1.5">
							当前策略「加权随机」：weight 字段生效，priority 仅作排序参考
						</div>
					{:else if g.strategy === 'priority'}
						<div class="mt-3 text-xs text-zinc-600 dark:text-zinc-400 bg-zinc-50 dark:bg-zinc-900 rounded px-3 py-1.5">
							当前策略「优先级」：priority 越小越优先，weight 字段不生效
						</div>
					{/if}

				{:else}
					<!-- Edit form -->
					<div class="space-y-4">
						<div>
							<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
							<input bind:value={editForm.name} class="w-full rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm" />
						</div>
						<div>
							<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">策略</label>
							<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2">
								{#each Object.entries(STRATEGIES) as [key, s]}
									<button
										onclick={() => { editForm.strategy = key; }}
										class="text-left p-3 rounded-lg border-2 transition-colors
											{editForm.strategy === key ? 'border-zinc-900 dark:border-zinc-100 bg-zinc-50 dark:bg-zinc-900' : 'border-zinc-200 dark:border-zinc-700 hover:border-zinc-300'}"
									>
										<span class="text-sm font-medium {editForm.strategy === key ? 'text-zinc-900 dark:text-zinc-100 font-semibold' : 'text-zinc-600 dark:text-zinc-400'}">{s.label}</span>
										<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-0.5">{s.desc}</p>
									</button>
								{/each}
							</div>
						</div>
						<div>
							<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">回退分组</label>
							<select bind:value={editForm.fallback_group_id} class="w-full rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm">
								<option value={null}>无</option>
								{#each groups.filter(gr => gr.id !== selectedId) as gr}
									<option value={gr.id}>{gr.name}</option>
								{/each}
							</select>
						</div>
						<div>
							<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">描述</label>
							<textarea bind:value={editForm.description} rows="2" class="w-full rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm"></textarea>
						</div>
						<div class="flex items-center gap-2">
							<label class="text-sm text-zinc-700 dark:text-zinc-300">启用</label>
							<button onclick={() => { editForm.enabled = !editForm.enabled; }}
								class="relative inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full transition-colors
									{editForm.enabled ? 'bg-zinc-900 dark:bg-zinc-100' : 'bg-zinc-300 dark:bg-zinc-600'}">
								<span class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform mt-0.5
									{editForm.enabled ? 'translate-x-4 ml-0.5' : 'translate-x-0.5'}"></span>
							</button>
						</div>
						<div class="flex gap-2">
							<button onclick={handleUpdate} class="px-4 py-2 text-sm font-medium rounded-lg bg-zinc-900 text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200">保存</button>
							<button onclick={() => { editing = false; }} class="px-4 py-2 text-sm font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700">取消</button>
						</div>
					</div>
				{/if}
			</div>

			<!-- ═══ Fallback Chain ═══ -->
			{#if chain.length > 1}
				<div class="p-5">
					<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-3">回退链路</h3>
					<div class="flex items-center gap-2 overflow-x-auto pb-2">
						{#each chain as node, i}
							<div class="flex items-center gap-2 flex-shrink-0">
								<div class="px-3 py-2 rounded-lg border text-sm
									{node.id === selectedId
										? 'border-zinc-900 dark:border-zinc-100 bg-zinc-50 dark:bg-zinc-900 text-zinc-900 dark:text-zinc-100 font-medium'
										: 'border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 text-zinc-700 dark:text-zinc-300'}">
									<div>{node.name}</div>
									<div class="text-xs text-zinc-600 dark:text-zinc-300 mt-0.5">{strategyMeta(node.strategy).label}</div>
								</div>
								{#if i < chain.length - 1}
									<ChevronRight class="w-4 h-4 text-zinc-400 flex-shrink-0" />
								{/if}
							</div>
						{/each}
						{#if !chain[chain.length - 1].fallback_group_id}
							<ChevronRight class="w-4 h-4 text-zinc-400 flex-shrink-0" />
							<span class="text-zinc-600 dark:text-zinc-300 text-sm">∅</span>
						{/if}
					</div>
				</div>
			{/if}

			<!-- ═══ Binding Table ═══ -->
			<div class="p-5">
				<div class="flex items-center justify-between mb-3">
					<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300">渠道列表 ({detail.bindings.length})</h3>
					<button onclick={() => { showAddChannel = true; selectedChannels = new Set(); channelSearch = ''; channelProviderFilter = ''; addPriority = 100; addWeight = 1; }}
						class="inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700">
						<Plus class="w-3 h-3" /> 添加渠道
					</button>
				</div>

				{#if detail.bindings.length === 0}
					<p class="text-center text-sm text-zinc-600 dark:text-zinc-300 py-8">暂无渠道，点击上方按钮添加</p>
				{:else}
					<div class="overflow-x-auto">
						<table class="w-full text-sm">
							<thead>
								<tr class="border-b border-zinc-200 dark:border-zinc-700 text-zinc-600 dark:text-zinc-300">
									<th class="text-left py-2 px-2 font-medium">状态</th>
									<th class="text-left py-2 px-2 font-medium">渠道</th>
									<th class="text-left py-2 px-2 font-medium">类型</th>
									<th class="text-left py-2 px-2 font-medium">优先级</th>
									<th class="text-left py-2 px-2 font-medium">权重</th>
									<th class="text-left py-2 px-2 font-medium">模型过滤</th>
									<th class="text-right py-2 px-2 font-medium">操作</th>
								</tr>
							</thead>
							<tbody>
								{#each detail.bindings as b (b.channel_id)}
									{@const health = b.channel_health ?? 'healthy'}
									{@const isEditing = editingBindingId === b.channel_id}
									<tr class="border-b border-zinc-100 dark:border-zinc-800 hover:bg-zinc-50 dark:hover:bg-zinc-900/50">
										<td class="py-2.5 px-2">
											<span class="inline-block w-2.5 h-2.5 rounded-full {health === 'healthy' ? 'bg-green-500' : 'bg-red-500'}" title={health}></span>
										</td>
										<td class="py-2.5 px-2">
											<div class="font-medium text-zinc-900 dark:text-zinc-100">{b.channel_name}</div>
											<div class="text-xs text-zinc-600 dark:text-zinc-300">{b.channel_code}</div>
										</td>
										<td class="py-2.5 px-2">
											<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {providerColor(b.provider_type)}">
												{b.provider_type}
											</span>
										</td>
										<td class="py-2.5 px-2">
											{#if isEditing}
												<input type="number" bind:value={editBindingPriority} class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm" />
											{:else}
												<button onclick={() => startEditBinding(b)} class="text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 cursor-pointer font-mono">{b.priority}</button>
											{/if}
										</td>
										<td class="py-2.5 px-2">
											{#if isEditing}
												<input type="number" bind:value={editBindingWeight} class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm" />
											{:else}
												<button onclick={() => startEditBinding(b)} class="text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 cursor-pointer font-mono">{b.weight}</button>
											{/if}
										</td>
										<td class="py-2.5 px-2">
											{#if b.model_filter && b.model_filter.length > 0}
												<div class="flex flex-wrap gap-1">
													{#each b.model_filter.slice(0, 3) as m}
														<span class="px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-700 text-xs text-zinc-600 dark:text-zinc-300">{m}</span>
													{/each}
													{#if b.model_filter.length > 3}
														<span class="text-xs text-zinc-600 dark:text-zinc-300">+{b.model_filter.length - 3}</span>
													{/if}
												</div>
											{:else}
												<span class="text-xs text-zinc-600 dark:text-zinc-300">全部</span>
											{/if}
										</td>
										<td class="py-2.5 px-2 text-right">
											{#if isEditing}
												<button onclick={saveBinding} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-700 text-zinc-700 dark:text-zinc-300"><Check class="w-4 h-4" /></button>
												<button onclick={() => { editingBindingId = null; }} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-700 text-zinc-500"><X class="w-4 h-4" /></button>
											{:else}
												<button onclick={() => startEditBinding(b)} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-700 text-zinc-500" title="编辑"><Pencil class="w-3.5 h-3.5" /></button>
												<button onclick={() => handleRemoveBinding(b.channel_id)} class="p-1 rounded hover:bg-red-50 dark:hover:bg-red-900/20 text-red-500" title="移除"><Trash2 class="w-3.5 h-3.5" /></button>
											{/if}
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}

				<!-- Project references -->
				{#if detail.project_ids && detail.project_ids.length > 0}
					<div class="mt-4 text-sm text-zinc-600 dark:text-zinc-300">
						{detail.project_ids.length} 个项目正在使用此分组
					</div>
				{/if}
			</div>
		</div>
	{/if}
</div>

<!-- ═══ Create Group Modal ═══ -->
{#if showCreate}
	<div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) showCreate = false; }}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-lg max-h-[90vh] overflow-y-auto">
			<div class="p-5 border-b border-zinc-200 dark:border-zinc-700 flex items-center justify-between">
				<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">新建分组</h2>
				<button onclick={() => { showCreate = false; }} class="p-1 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-700"><X class="w-5 h-5 text-zinc-500" /></button>
			</div>
			<div class="p-5 space-y-4">
				<div>
					<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
					<input bind:value={createForm.name} placeholder="如：默认分组" class="w-full rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm" />
				</div>
				<div>
					<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-2">路由策略</label>
					<div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
						{#each Object.entries(STRATEGIES) as [key, s]}
							<button
								onclick={() => { createForm.strategy = key; }}
								class="text-left p-3 rounded-lg border-2 transition-colors
									{createForm.strategy === key ? 'border-zinc-900 dark:border-zinc-100 bg-zinc-50 dark:bg-zinc-900' : 'border-zinc-200 dark:border-zinc-700 hover:border-zinc-300'}"
							>
								<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {strategyBadgeClass(s.color)} mb-1">{s.label}</span>
								<p class="text-xs text-zinc-600 dark:text-zinc-300">{s.desc}</p>
							</button>
						{/each}
					</div>
				</div>
				<div>
					<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">回退分组（可选）</label>
					<select bind:value={createForm.fallback_group_id} class="w-full rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm">
						<option value={null}>无</option>
						{#each groups as gr}
							<option value={gr.id}>{gr.name}</option>
						{/each}
					</select>
				</div>
				<div>
					<label class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">描述（可选）</label>
					<textarea bind:value={createForm.description} rows="2" placeholder="分组用途说明" class="w-full rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm"></textarea>
				</div>
			</div>
			<div class="p-5 border-t border-zinc-200 dark:border-zinc-700 flex justify-end gap-2">
				<button onclick={() => { showCreate = false; }} class="px-4 py-2 text-sm font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700">取消</button>
				<button onclick={handleCreate} disabled={!createForm.name.trim()} class="px-4 py-2 text-sm font-medium rounded-lg bg-zinc-900 text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200 disabled:opacity-50">创建</button>
			</div>
		</div>
	</div>
{/if}

<!-- ═══ Delete Confirm Modal ═══ -->
{#if deleteTarget}
	<div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) deleteTarget = null; }}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-sm">
			<div class="p-6 text-center">
				<div class="mx-auto w-12 h-12 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center mb-4">
					<AlertTriangle class="w-6 h-6 text-red-600 dark:text-red-400" />
				</div>
				<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
				<p class="text-sm text-zinc-600 dark:text-zinc-300">
					确定要删除分组「{deleteTarget.name}」吗？此操作不可撤销。
					{#if detail?.project_ids && detail.project_ids.length > 0}
						<br /><span class="text-red-500 font-medium">⚠ 有 {detail.project_ids.length} 个项目正在使用此分组！</span>
					{/if}
				</p>
			</div>
			<div class="px-6 pb-6 flex gap-2">
				<button onclick={() => { deleteTarget = null; }} class="flex-1 px-4 py-2 text-sm font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700">取消</button>
				<button onclick={handleDelete} class="flex-1 px-4 py-2 text-sm font-medium rounded-lg bg-red-600 text-white hover:bg-red-700">删除</button>
			</div>
		</div>
	</div>
{/if}

<!-- ═══ Add Channel Modal ═══ -->
{#if showAddChannel}
	<div class="fixed inset-0 bg-black/50 flex items-center justify-center z-50 p-4" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) showAddChannel = false; }}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-xl max-h-[85vh] flex flex-col">
			<div class="p-5 border-b border-zinc-200 dark:border-zinc-700 flex items-center justify-between flex-shrink-0">
				<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">添加渠道</h2>
				<button onclick={() => { showAddChannel = false; }} class="p-1 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-700"><X class="w-5 h-5 text-zinc-500" /></button>
			</div>

			<!-- Filters -->
			<div class="p-4 border-b border-zinc-200 dark:border-zinc-700 space-y-3 flex-shrink-0">
				<div class="flex gap-2">
					<div class="relative flex-1">
						<Search class="absolute left-3 top-2.5 w-4 h-4 text-zinc-400" />
						<input bind:value={channelSearch} placeholder="搜索渠道..." class="w-full pl-9 pr-3 py-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm" />
					</div>
					<select bind:value={channelProviderFilter} class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm">
						<option value="">全部类型</option>
						{#each providerTypes as pt}
							<option value={pt}>{pt}</option>
						{/each}
					</select>
				</div>
				<div class="flex gap-4">
					<div class="flex items-center gap-2">
						<label class="text-xs text-zinc-500">优先级</label>
						<input type="number" bind:value={addPriority} class="w-20 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm" />
					</div>
					<div class="flex items-center gap-2">
						<label class="text-xs text-zinc-500">权重</label>
						<input type="number" bind:value={addWeight} class="w-20 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm" />
					</div>
				</div>
			</div>

			<!-- Channel list -->
			<div class="flex-1 overflow-y-auto p-4 space-y-1">
				{#if filteredChannels.length === 0}
					<p class="text-center text-sm text-zinc-600 dark:text-zinc-300 py-8">没有可用的渠道</p>
				{:else}
					{#each filteredChannels as ch (ch.id)}
						<button
							onclick={() => toggleChannel(ch.id)}
							class="w-full flex items-center gap-3 p-3 rounded-lg text-left transition-colors
								{selectedChannels.has(ch.id) ? 'bg-zinc-100 dark:bg-zinc-800 border border-zinc-400 dark:border-zinc-500' : 'hover:bg-zinc-50 dark:hover:bg-zinc-900/50 border border-transparent'}"
						>
							<div class="w-5 h-5 rounded border-2 flex items-center justify-center flex-shrink-0
								{selectedChannels.has(ch.id) ? 'border-zinc-900 bg-zinc-900 dark:border-zinc-100 dark:bg-zinc-100' : 'border-zinc-200 dark:border-zinc-700'}">
								{#if selectedChannels.has(ch.id)}<Check class="w-3 h-3 text-white" />{/if}
							</div>
							<div class="flex-1 min-w-0">
								<div class="text-sm font-medium text-zinc-900 dark:text-zinc-100">{ch.name}</div>
								<div class="text-xs text-zinc-600 dark:text-zinc-300">{ch.code}</div>
							</div>
							<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {providerColor(ch.provider_type)}">{ch.provider_type}</span>
						</button>
					{/each}
				{/if}
			</div>

			<!-- Footer -->
			<div class="p-4 border-t border-zinc-200 dark:border-zinc-700 flex items-center justify-between flex-shrink-0">
				<span class="text-sm text-zinc-500">{selectedChannels.size} 个已选</span>
				<div class="flex gap-2">
					<button onclick={() => { showAddChannel = false; }} class="px-4 py-2 text-sm font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700">取消</button>
					<button onclick={handleAddChannels} disabled={selectedChannels.size === 0} class="px-4 py-2 text-sm font-medium rounded-lg bg-zinc-900 text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200 disabled:opacity-50">
						添加选中 ({selectedChannels.size})
					</button>
				</div>
			</div>
		</div>
	</div>
{/if}
