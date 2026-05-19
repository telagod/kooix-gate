<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import {
		getMe, listGroups, createGroup, updateGroup, deleteGroup,
		getGroupDetail, listGroupBindings, addGroupBinding,
		updateGroupBinding, removeGroupBinding, listAdminChannels
	} from '$lib/api.js';
	import type { ChannelGroup, FallbackChainNode, GroupBinding, GroupDetail, Channel } from '$lib/api.js';
	import { CAPABILITY_LABELS, capabilityList, providerCapabilities } from '$lib/plugin-presets';
	import type { ProviderCapabilities, ProviderCapabilityKey } from '$lib/plugin-presets';
	import { addToast } from '$lib/stores/toast.js';
	import { Button, Field, Input, Select, Skeleton, Textarea } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
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

	const PROVIDER_COLOR = 'bg-zinc-200 text-zinc-700 dark:bg-zinc-600 dark:text-zinc-200';

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
	function bindingCapabilities(b: GroupBinding): ProviderCapabilities {
		return b.capabilities ?? providerCapabilities(b.provider_type);
	}
	function capabilityChipClass(_key: ProviderCapabilityKey): string {
		return 'bg-zinc-100 text-zinc-700 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700';
	}

	function strategyBadgeClass(_color: string) {
		return 'bg-zinc-200 text-zinc-700 dark:bg-zinc-600 dark:text-zinc-200';
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
		return groups.find(g => g.id === id)?.name ?? shortId(id);
	}

	function formatNumber(value: number | null | undefined): string {
		return new Intl.NumberFormat('zh-CN').format(value ?? 0);
	}

	function formatPercent(value: number | null | undefined): string {
		return `${((value ?? 0) * 100).toFixed(1)}%`;
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

	function buildLocalFallbackChain(groupId: string): FallbackChainNode[] {
		return buildFallbackChain(groupId).map((group, index) => ({
			id: group.id,
			name: group.name,
			strategy: group.strategy,
			enabled: group.enabled,
			channel_count: group.channel_count ?? 0,
			requests: 0,
			share: 0,
			is_fallback: index > 0
		}));
	}

	function wouldCreateFallbackCycle(sourceId: string | null, targetId: string | null): boolean {
		if (!sourceId || !targetId) return false;
		if (sourceId === targetId) return true;
		const visited = new Set<string>([sourceId]);
		let current: string | null | undefined = targetId;
		let depth = 0;
		while (current) {
			if (visited.has(current)) return true;
			visited.add(current);
			if (depth >= 5) return true;
			const group = groups.find((g) => g.id === current);
			if (!group) return false;
			current = group.fallback_group_id;
			depth += 1;
		}
		return false;
	}

	function projectRefs(d: GroupDetail | null): string[] {
		return d?.projects_using ?? d?.project_ids ?? [];
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
	let strategyOptions = $derived(Object.entries(STRATEGIES).map(([value, strategy]) => ({ value, label: strategy.label })));
	let editFallbackOptions = $derived([
		{ value: null, label: '无' },
		...groups
			.filter((group) => group.id !== selectedId && !wouldCreateFallbackCycle(selectedId, group.id))
			.map((group) => ({ value: group.id, label: group.name }))
	]);
	let createFallbackOptions = $derived([
		{ value: null, label: '无' },
		...groups.map((group) => ({ value: group.id, label: group.name }))
	]);
	let providerFilterOptions = $derived([
		{ value: '', label: '全部类型' },
		...providerTypes.map((provider) => ({ value: provider, label: provider }))
	]);

	// ── Data loading ──
	async function loadGroups() {
		loading = true;
		try {
			const [g, ch] = await Promise.all([listGroups(), listAdminChannels()]);
			groups = g;
			allChannels = ch.data ?? [];
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
			detail = {
				group,
				bindings,
				project_ids: [],
				projects_using: [],
				fallback_chain: buildLocalFallbackChain(id),
				fallback_stats: {
					window_hours: 24,
					total_requests: 0,
					primary_requests: 0,
					fallback_requests: 0,
					fallback_hit_rate: 0,
					has_cycle: false,
					cycle_at: null
				}
			};
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
			if (wouldCreateFallbackCycle(null, createForm.fallback_group_id)) {
				addToast('回退链路存在循环，请重新选择', 'error');
				return;
			}
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
			if (wouldCreateFallbackCycle(selectedId, editForm.fallback_group_id)) {
				addToast('回退链路存在循环，请重新选择', 'error');
				return;
			}
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

<div class="px-6 py-6 space-y-6">

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
		{@const chain = detail.fallback_chain?.length ? detail.fallback_chain : buildLocalFallbackChain(selectedId)}
		{@const stats = detail.fallback_stats}
		{@const refs = projectRefs(detail)}

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
							<Field label="名称" for="group-edit-name">
								<Input id="group-edit-name" bind:value={editForm.name} />
							</Field>
							<Field label="策略" for="group-edit-strategy">
								<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-2">
									{#each Object.entries(STRATEGIES) as [key, s]}
									<button
										onclick={() => { editForm.strategy = key; }}
										class="text-left p-3 rounded-lg border-2 transition-colors
											{editForm.strategy === key ? 'border-zinc-900 dark:border-zinc-300 bg-zinc-50 dark:bg-zinc-700' : 'border-zinc-200 dark:border-zinc-700 hover:border-zinc-400 dark:hover:border-zinc-500'}"
									>
										<span class="text-sm font-medium {editForm.strategy === key ? 'text-zinc-900 dark:text-zinc-100 font-semibold' : 'text-zinc-600 dark:text-zinc-400'}">{s.label}</span>
										<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-0.5">{s.desc}</p>
										</button>
									{/each}
								</div>
							</Field>
							<Field label="回退分组" for="group-edit-fallback">
								<Select id="group-edit-fallback" bind:value={editForm.fallback_group_id} options={editFallbackOptions} />
							</Field>
							<Field label="描述" for="group-edit-description">
								<Textarea id="group-edit-description" bind:value={editForm.description} rows={2} />
							</Field>
							<div class="flex items-center gap-2">
								<label for="group-edit-enabled" class="text-sm text-zinc-700 dark:text-zinc-300">启用</label>
								<button id="group-edit-enabled" aria-pressed={editForm.enabled} aria-label="切换分组启用状态" onclick={() => { editForm.enabled = !editForm.enabled; }}
									class="relative inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full transition-colors
										{editForm.enabled ? 'bg-zinc-900 dark:bg-zinc-100' : 'bg-zinc-300 dark:bg-zinc-600'}">
								<span class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform mt-0.5
									{editForm.enabled ? 'translate-x-4 ml-0.5' : 'translate-x-0.5'}"></span>
							</button>
						</div>
						<div class="flex gap-2">
								<Button onclick={handleUpdate}>保存</Button>
								<Button variant="outline" onclick={() => { editing = false; }}>取消</Button>
							</div>
						</div>
				{/if}
			</div>

			<!-- ═══ Fallback Chain ═══ -->
			{#if chain.length > 0}
				<div class="p-5">
					<div class="mb-4 flex items-start justify-between gap-4">
						<div>
							<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300">回退链路</h3>
							<p class="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
								{stats?.window_hours ?? 24}h 窗口，命中率按 request_events.group_id 统计
							</p>
						</div>
						{#if stats?.has_cycle}
							<div class="inline-flex items-center gap-1.5 rounded-lg border border-amber-200 bg-amber-50 px-2.5 py-1 text-xs font-medium text-amber-700 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-300">
								<AlertTriangle class="h-3.5 w-3.5" />
								<span>检测到循环 {stats.cycle_at ? shortId(stats.cycle_at) : ''}</span>
							</div>
						{/if}
					</div>

					<div class="mb-4 grid grid-cols-1 gap-3 sm:grid-cols-4">
						<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
							<div class="text-xs text-zinc-600 dark:text-zinc-400">总请求</div>
							<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatNumber(stats?.total_requests)}</div>
						</div>
						<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
							<div class="text-xs text-zinc-600 dark:text-zinc-400">Primary</div>
							<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatNumber(stats?.primary_requests)}</div>
						</div>
						<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
							<div class="text-xs text-zinc-600 dark:text-zinc-400">Fallback</div>
							<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatNumber(stats?.fallback_requests)}</div>
						</div>
						<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-900">
							<div class="text-xs text-zinc-600 dark:text-zinc-400">命中率</div>
							<div class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatPercent(stats?.fallback_hit_rate)}</div>
						</div>
					</div>

					<div class="flex items-stretch gap-2 overflow-x-auto pb-2">
						{#each chain as node, i}
							<div class="flex items-center gap-2 flex-shrink-0">
								<div class="min-w-44 px-3 py-2 rounded-lg border text-sm
									{node.id === selectedId
										? 'border-zinc-900 dark:border-zinc-300 bg-zinc-100 dark:bg-zinc-700 text-zinc-900 dark:text-zinc-100 font-medium'
										: 'border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300'}">
									<div class="flex items-center justify-between gap-3">
										<div class="truncate">{node.name}</div>
										<span class="rounded bg-zinc-200 px-1.5 py-0.5 text-[10px] font-medium text-zinc-700 dark:bg-zinc-600 dark:text-zinc-200">
											{node.is_fallback ? 'Fallback' : 'Primary'}
										</span>
									</div>
									<div class="mt-1 text-xs text-zinc-600 dark:text-zinc-300">{strategyMeta(node.strategy).label} · {node.channel_count} 渠道</div>
									<div class="mt-2 h-1.5 overflow-hidden rounded-full bg-zinc-200 dark:bg-zinc-700">
										<div class="h-full rounded-full bg-zinc-900 dark:bg-zinc-100" style={`width: ${Math.min(100, Math.max(0, node.share * 100))}%`}></div>
									</div>
									<div class="mt-1 flex justify-between font-mono text-[11px] text-zinc-600 dark:text-zinc-400">
										<span>{formatNumber(node.requests)} req</span>
										<span>{formatPercent(node.share)}</span>
									</div>
									{#if !node.enabled}
										<div class="mt-1 inline-flex items-center gap-1 text-[11px] text-amber-700 dark:text-amber-300">
											<AlertTriangle class="h-3 w-3" /> disabled
										</div>
									{/if}
								</div>
								{#if i < chain.length - 1}
									<ChevronRight class="w-4 h-4 text-zinc-400 flex-shrink-0" />
								{/if}
							</div>
						{/each}
						{#if chain.length === 1}
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
											<div class="space-y-1">
												<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {providerColor(b.provider_type)}">
													{b.provider_type}
												</span>
												<div class="flex max-w-44 flex-wrap gap-1">
													{#each capabilityList(bindingCapabilities(b)).slice(0, 3) as cap}
														<span class="rounded px-1.5 py-0.5 text-[10px] font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
													{/each}
													{#if capabilityList(bindingCapabilities(b)).length > 3}
														<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-zinc-100 text-zinc-500 ring-1 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:ring-zinc-700">+{capabilityList(bindingCapabilities(b)).length - 3}</span>
													{/if}
												</div>
											</div>
										</td>
										<td class="py-2.5 px-2">
											{#if isEditing}
												<input type="number" bind:value={editBindingPriority} class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100" />
											{:else}
												<button onclick={() => startEditBinding(b)} class="text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 cursor-pointer font-mono">{b.priority}</button>
											{/if}
										</td>
										<td class="py-2.5 px-2">
											{#if isEditing}
												<input type="number" bind:value={editBindingWeight} class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100" />
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
				{#if refs.length > 0}
					<div class="mt-4 text-sm text-zinc-600 dark:text-zinc-300">
						{refs.length} 个项目正在使用此分组
					</div>
				{/if}
			</div>
		</div>
	{/if}
</div>

<!-- ═══ Create Group Modal ═══ -->
{#if showCreate}
	<ModalFrame close={() => { showCreate = false; }}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-lg max-h-[90vh] overflow-y-auto">
			<div class="p-5 border-b border-zinc-200 dark:border-zinc-700 flex items-center justify-between">
				<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">新建分组</h2>
				<button onclick={() => { showCreate = false; }} class="p-1 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-700"><X class="w-5 h-5 text-zinc-500" /></button>
			</div>
			<div class="p-5 space-y-4">
				<Field label="名称" for="group-create-name">
					<Input id="group-create-name" bind:value={createForm.name} placeholder="如：默认分组" />
				</Field>
				<Field label="路由策略" for="group-create-strategy">
					<div class="grid grid-cols-1 sm:grid-cols-2 gap-2">
						{#each Object.entries(STRATEGIES) as [key, s]}
							<button
								onclick={() => { createForm.strategy = key; }}
								class="text-left p-3 rounded-lg border-2 transition-colors
									{createForm.strategy === key ? 'border-zinc-900 dark:border-zinc-300 bg-zinc-50 dark:bg-zinc-700' : 'border-zinc-200 dark:border-zinc-700 hover:border-zinc-400 dark:hover:border-zinc-500'}"
							>
								<span class="inline-flex items-center px-2 py-0.5 rounded text-xs font-medium {strategyBadgeClass(s.color)} mb-1">{s.label}</span>
								<p class="text-xs text-zinc-600 dark:text-zinc-300">{s.desc}</p>
							</button>
						{/each}
					</div>
				</Field>
				<Field label="回退分组（可选）" for="group-create-fallback">
					<Select id="group-create-fallback" bind:value={createForm.fallback_group_id} options={createFallbackOptions} />
				</Field>
				<Field label="描述（可选）" for="group-create-description">
					<Textarea id="group-create-description" bind:value={createForm.description} rows={2} placeholder="分组用途说明" />
				</Field>
			</div>
			<div class="p-5 border-t border-zinc-200 dark:border-zinc-700 flex justify-end gap-2">
				<Button variant="outline" onclick={() => { showCreate = false; }}>取消</Button>
				<Button onclick={handleCreate} disabled={!createForm.name.trim()}>创建</Button>
			</div>
		</div>
	</ModalFrame>
{/if}

<!-- ═══ Delete Confirm Modal ═══ -->
{#if deleteTarget}
	{@const deleteRefs = selectedId === deleteTarget.id ? projectRefs(detail) : []}
	<ModalFrame close={() => { deleteTarget = null; }}>
		<div class="bg-white dark:bg-zinc-800 rounded-xl shadow-xl w-full max-w-sm">
			<div class="p-6 text-center">
				<div class="mx-auto w-12 h-12 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center mb-4">
					<AlertTriangle class="w-6 h-6 text-red-600 dark:text-red-400" />
				</div>
				<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
				<p class="text-sm text-zinc-600 dark:text-zinc-300">
					确定要删除分组「{deleteTarget.name}」吗？此操作不可撤销。
					{#if deleteRefs.length > 0}
						<br /><span class="inline-flex items-center justify-center gap-1 text-red-500 font-medium"><AlertTriangle class="h-3.5 w-3.5" />有 {deleteRefs.length} 个项目正在使用此分组</span>
					{/if}
				</p>
			</div>
			<div class="px-6 pb-6 flex gap-2">
				<Button variant="outline" class="flex-1" onclick={() => { deleteTarget = null; }}>取消</Button>
				<Button variant="destructive" class="flex-1" onclick={handleDelete}>删除</Button>
			</div>
		</div>
	</ModalFrame>
{/if}

<!-- ═══ Add Channel Modal ═══ -->
{#if showAddChannel}
	<ModalFrame close={() => { showAddChannel = false; }}>
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
						<Input bind:value={channelSearch} placeholder="搜索渠道..." class="pl-9" />
					</div>
					<Select bind:value={channelProviderFilter} options={providerFilterOptions} class="w-36" />
				</div>
				<div class="flex gap-4">
					<Field label="优先级" for="group-add-priority" class="flex-row items-center gap-2 space-y-0">
						<Input id="group-add-priority" type="number" bind:value={addPriority} size="sm" class="w-20" />
					</Field>
					<Field label="权重" for="group-add-weight" class="flex-row items-center gap-2 space-y-0">
						<Input id="group-add-weight" type="number" bind:value={addWeight} size="sm" class="w-20" />
					</Field>
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
					<Button variant="outline" onclick={() => { showAddChannel = false; }}>取消</Button>
					<Button onclick={handleAddChannels} disabled={selectedChannels.size === 0}>
						添加选中 ({selectedChannels.size})
					</Button>
				</div>
			</div>
		</div>
	</ModalFrame>
{/if}
