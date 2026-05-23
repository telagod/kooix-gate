<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import {
		listGroups, createGroup, updateGroup, deleteGroup,
		getGroupDetail, listGroupBindings, addGroupBinding,
		updateGroupBinding, removeGroupBinding, listAdminChannels
	} from '$lib/api.js';
	import type { CanaryStats, ChannelGroup, FallbackChainNode, GroupBinding, GroupDetail, Channel } from '$lib/api.js';
	import { CAPABILITY_LABELS, capabilityList, providerCapabilities } from '$lib/plugin-presets';
	import type { ProviderCapabilities, ProviderCapabilityKey } from '$lib/plugin-presets';
	import { addToast } from '$lib/stores/toast.js';
	import { Button, Field, Input, Select, Skeleton, Textarea } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import {
		Layers, Plus, Trash2, Pencil, X, ChevronRight, Search,
		ArrowRight, AlertTriangle, Check
	} from 'lucide-svelte';
	import CreateGroupModal from './_components/CreateGroupModal.svelte';
	import DeleteGroupModal from './_components/DeleteGroupModal.svelte';
	import DisableGroupModal from './_components/DisableGroupModal.svelte';
	import AddChannelModal from './_components/AddChannelModal.svelte';
	import GroupCard from './_components/GroupCard.svelte';
	import FallbackChainPanel from './_components/FallbackChainPanel.svelte';
	import {
		STRATEGIES,
		PROVIDER_COLOR,
		strategyMeta,
		strategyBadgeClass,
		capabilityChipClass,
		formatNumber,
		formatPercent,
		formatCanaryPercent
	} from './_lib/helpers';

	// ── Strategy metadata ──
	// 已抽到 ./_lib/helpers.ts（0.4.61）

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
	let editDisableConfirmation = $state('');

	// create modal
	let showCreate = $state(false);
	let createForm = $state({ name: '', strategy: 'priority', description: '', fallback_group_id: null as string | null });

	// delete confirm
	let deleteTarget = $state<ChannelGroup | null>(null);
	let disableTarget = $state<ChannelGroup | null>(null);
	let disableConfirmation = $state('');

	// add channel modal
	let showAddChannel = $state(false);
	let channelSearch = $state('');
	let channelProviderFilter = $state('');
	let selectedChannels = $state<Set<string>>(new Set());
	let addPriority = $state(100);
	let addWeight = $state(1);
	let addCanaryPercent = $state<number | null>(null);

	// inline editing binding
	let editingBindingId = $state<string | null>(null);
	let editBindingPriority = $state(0);
	let editBindingWeight = $state(0);
	let editBindingCanaryPercent = $state<number | null>(null);

	// ── Helpers ──
	// strategyMeta / strategyBadgeClass / capabilityChipClass / formatNumber 已抽到 ./_lib/helpers.ts（0.4.61）
	function providerColor(_p: string) { return PROVIDER_COLOR; }
	function bindingCapabilities(b: GroupBinding): ProviderCapabilities {
		return b.capabilities ?? providerCapabilities(b.provider_type);
	}

	function groupName(id: string | null | undefined): string {
		if (!id) return '';
		return groups.find(g => g.id === id)?.name ?? shortId(id);
	}

	function percentToBps(percent: number | string | null | undefined): number | null {
		if (percent === null || percent === undefined) return null;
		const value = Number(percent);
		if (!Number.isFinite(value) || value <= 0) return null;
		return Math.round(value * 100);
	}

	function bpsToPercent(bps: number | null | undefined): number | null {
		if (bps === null || bps === undefined) return null;
		return bps / 100;
	}

	function canaryRows(d: GroupDetail | null): CanaryStats[] {
		return d?.canary_stats ?? [];
	}

	function baselineRows(d: GroupDetail | null): CanaryStats[] {
		return canaryRows(d).filter((row) => !row.is_canary);
	}

	function metricDelta(row: CanaryStats, baseline: CanaryStats | null, field: 'error_rate' | 'avg_latency_ms' | 'avg_cost_micros'): number | null {
		if (!baseline) return null;
		const current = row[field];
		const base = baseline[field];
		if (current === null || current === undefined || base === null || base === undefined) return null;
		return current - base;
	}

	function weightedBaseline(rows: CanaryStats[]): CanaryStats | null {
		if (rows.length === 0) return null;
		const totalRequests = rows.reduce((sum, row) => sum + row.requests, 0);
		const weighted = (field: 'error_rate' | 'avg_latency_ms' | 'avg_cost_micros'): number | null => {
			const values = rows.filter((row) => row[field] !== null && row[field] !== undefined);
			if (values.length === 0) return null;
			if (totalRequests > 0) {
				const weightedRequests = values.reduce((sum, row) => sum + row.requests, 0);
				if (weightedRequests > 0) {
					return values.reduce((sum, row) => sum + (row[field] ?? 0) * row.requests, 0) / weightedRequests;
				}
			}
			return values.reduce((sum, row) => sum + (row[field] ?? 0), 0) / values.length;
		};
		return {
			channel_id: 'baseline',
			channel_code: 'baseline',
			channel_name: 'Baseline',
			provider_type: 'baseline',
			canary_percent_bps: null,
			is_canary: false,
			requests: totalRequests,
			error_rate: weighted('error_rate') ?? 0,
			avg_latency_ms: weighted('avg_latency_ms'),
			avg_cost_micros: weighted('avg_cost_micros')
		};
	}

	function formatMaybeMs(value: number | null | undefined): string {
		return value === null || value === undefined ? '—' : `${Math.round(value)}ms`;
	}

	function formatMaybeMicros(value: number | null | undefined): string {
		return value === null || value === undefined ? '—' : `${Math.round(value).toLocaleString('zh-CN')}µ`;
	}

	function formatSignedPercentDelta(delta: number | null): string {
		if (delta === null) return '—';
		const value = delta * 100;
		return `${value >= 0 ? '+' : ''}${value.toFixed(1)}pp`;
	}

	function formatSignedNumberDelta(delta: number | null, suffix = ''): string {
		if (delta === null) return '—';
		return `${delta >= 0 ? '+' : ''}${Math.round(delta).toLocaleString('zh-CN')}${suffix}`;
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
				},
				canary_stats: []
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
			const disablesCurrentGroup = detail?.group.enabled === true && editForm.enabled === false;
			const expectedConfirmation = disablesCurrentGroup ? `disable:${detail?.group.name ?? ''}` : '';
			if (disablesCurrentGroup && editDisableConfirmation.trim() !== expectedConfirmation) {
				addToast('请输入正确的禁用确认短语', 'error');
				return;
			}
			await updateGroup(selectedId, {
				name: editForm.name,
				strategy: editForm.strategy,
				description: editForm.description,
				fallback_group_id: editForm.fallback_group_id,
				enabled: editForm.enabled,
			}, disablesCurrentGroup ? editDisableConfirmation : undefined);
			addToast('分组已更新', 'success');
			editing = false;
			editDisableConfirmation = '';
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
		if (group.enabled) {
			disableTarget = group;
			disableConfirmation = '';
			return;
		}
		try {
			await updateGroup(group.id, { enabled: !group.enabled });
			group.enabled = !group.enabled;
			groups = [...groups];
		} catch (e: any) {
			addToast(e.message || '切换失败', 'error');
		}
	}

	async function confirmDisableGroup() {
		if (!disableTarget) return;
		try {
			await updateGroup(disableTarget.id, { enabled: false }, disableConfirmation);
			disableTarget.enabled = false;
			groups = [...groups];
			if (detail?.group.id === disableTarget.id) {
				await loadDetail(disableTarget.id);
			}
			disableTarget = null;
			disableConfirmation = '';
			addToast('分组已禁用', 'success');
		} catch (e: any) {
			addToast(e.message || '禁用失败', 'error');
		}
	}

	function startEdit() {
		if (!detail) return;
		const g = detail.group;
		editDisableConfirmation = '';
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
			const canaryBps = percentToBps(addCanaryPercent);
			for (const chId of selectedChannels) {
				await addGroupBinding(selectedId, chId, addPriority, addWeight, canaryBps);
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
		editBindingCanaryPercent = bpsToPercent(b.canary_percent_bps);
	}

	async function saveBinding() {
		if (!selectedId || !editingBindingId) return;
		try {
			await updateGroupBinding(selectedId, editingBindingId, {
				priority: editBindingPriority,
				weight: editBindingWeight,
				canary_percent_bps: percentToBps(editBindingCanaryPercent),
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

<PageShell
	title="渠道分组管理"
	description={`管理 Provider group、fallback chain 与 canary binding · ${groups.length} groups`}
	icon={Layers}
	max="full"
>
	{#snippet actions()}
		<Button onclick={() => { showCreate = true; }}>
			<Plus size={14} /> 新建分组
		</Button>
	{/snippet}

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
		<StatePanel title="分组加载失败" description={error} icon={Layers} variant="danger" />

	{:else if groups.length === 0}
		<StatePanel title="暂无渠道分组" description="还没有分组，点击右上角创建。" icon={Layers} />

	{:else}

		<!-- ═══ Group Grid ═══ -->
		<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-4">
			{#each groups as group (group.id)}
				<GroupCard
					{group}
					isSelected={selectedId === group.id}
					{groupName}
					onSelect={selectGroup}
					onToggleEnabled={toggleEnabled}
				/>
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
		{@const canary = canaryRows(detail)}
		{@const canaryOnly = canary.filter((row) => row.is_canary)}
		{@const baseline = weightedBaseline(baselineRows(detail))}

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
					{@const expectedEditDisableConfirmation = g.enabled && !editForm.enabled ? `disable:${g.name}` : ''}
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
								<button id="group-edit-enabled" aria-pressed={editForm.enabled} aria-label="切换分组启用状态" onclick={() => { editForm.enabled = !editForm.enabled; if (editForm.enabled) editDisableConfirmation = ''; }}
									class="relative inline-flex h-5 w-9 flex-shrink-0 cursor-pointer rounded-full transition-colors
										{editForm.enabled ? 'bg-zinc-900 dark:bg-zinc-100' : 'bg-zinc-300 dark:bg-zinc-600'}">
								<span class="pointer-events-none inline-block h-4 w-4 transform rounded-full bg-white shadow transition-transform mt-0.5
									{editForm.enabled ? 'translate-x-4 ml-0.5' : 'translate-x-0.5'}"></span>
							</button>
						</div>
						{#if expectedEditDisableConfirmation}
							<div class="rounded-lg border border-amber-200 bg-amber-50 p-3 dark:border-amber-900/60 dark:bg-amber-950/30">
								<p class="text-xs font-medium text-amber-800 dark:text-amber-300">高危操作二次确认</p>
								<p class="mt-1 text-xs text-amber-700 dark:text-amber-300">保存时会禁用当前分组。请输入确认短语：</p>
								<code class="mt-2 block rounded-md border border-amber-200 bg-white px-3 py-2 font-mono text-xs text-zinc-800 dark:border-amber-900/60 dark:bg-zinc-900 dark:text-zinc-200">{expectedEditDisableConfirmation}</code>
								<Input id="group-edit-disable-confirm" bind:value={editDisableConfirmation} placeholder={expectedEditDisableConfirmation} class="mt-2 font-mono" />
							</div>
						{/if}
						<div class="flex gap-2">
								<Button onclick={handleUpdate} disabled={Boolean(expectedEditDisableConfirmation) && editDisableConfirmation.trim() !== expectedEditDisableConfirmation}>保存</Button>
								<Button variant="outline" onclick={() => { editing = false; editDisableConfirmation = ''; }}>取消</Button>
							</div>
						</div>
				{/if}
			</div>

			<!-- ═══ Canary Stats ═══ -->
			{#if canary.length > 0}
				<div class="p-5 border-t border-zinc-200 dark:border-zinc-700">
					<div class="mb-4 flex items-start justify-between gap-4">
						<div>
							<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300">Canary 对比</h3>
							<p class="mt-1 text-xs text-zinc-600 dark:text-zinc-400">
								{stats?.window_hours ?? 24}h 窗口，按 request_events 比较错误率 / 延迟 / 平均成本
							</p>
						</div>
						<span class="rounded-lg border border-zinc-200 bg-zinc-50 px-2.5 py-1 text-xs font-medium text-zinc-600 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
							{canaryOnly.length} canary
						</span>
					</div>

					{#if canaryOnly.length === 0}
						<p class="rounded-lg border border-zinc-200 bg-zinc-50 px-3 py-2 text-sm text-zinc-600 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
							暂无 Canary binding；编辑渠道后把 Canary 设置为 1%-5% 即可开始小流量验证。
						</p>
					{:else}
						<DataTable class="mb-0">
							{#snippet head()}
								<tr>
									<th class={dataTemplate.th}>渠道</th>
									<th class={dataTemplate.th}>流量</th>
									<th class={cn(dataTemplate.th, 'text-right')}>请求</th>
									<th class={cn(dataTemplate.th, 'text-right')}>错误率</th>
									<th class={cn(dataTemplate.th, 'text-right')}>延迟</th>
									<th class={cn(dataTemplate.th, 'text-right')}>平均成本</th>
								</tr>
							{/snippet}

							{#each canaryOnly as row (row.channel_id)}
								<tr class={dataTemplate.row}>
									<td class={dataTemplate.tdStrong}>
										<div class="font-medium">{row.channel_name}</div>
										<div class="text-xs text-zinc-600 dark:text-zinc-400">{row.channel_code}</div>
									</td>
									<td class={dataTemplate.td}>
										<span class="rounded border border-amber-200 bg-amber-50 px-2 py-0.5 font-mono text-xs font-medium text-amber-700 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-300">{formatCanaryPercent(row.canary_percent_bps)}</span>
									</td>
									<td class={cn(dataTemplate.tdMono, 'text-right')}>{formatNumber(row.requests)}</td>
									<td class={cn(dataTemplate.td, 'text-right')}>
										<div class="font-mono text-zinc-900 dark:text-zinc-100">{formatPercent(row.error_rate)}</div>
										<div class="font-mono text-[11px] text-zinc-500 dark:text-zinc-400">{formatSignedPercentDelta(metricDelta(row, baseline, 'error_rate'))}</div>
									</td>
									<td class={cn(dataTemplate.td, 'text-right')}>
										<div class="font-mono text-zinc-900 dark:text-zinc-100">{formatMaybeMs(row.avg_latency_ms)}</div>
										<div class="font-mono text-[11px] text-zinc-500 dark:text-zinc-400">{formatSignedNumberDelta(metricDelta(row, baseline, 'avg_latency_ms'), 'ms')}</div>
									</td>
									<td class={cn(dataTemplate.td, 'text-right')}>
										<div class="font-mono text-zinc-900 dark:text-zinc-100">{formatMaybeMicros(row.avg_cost_micros)}</div>
										<div class="font-mono text-[11px] text-zinc-500 dark:text-zinc-400">{formatSignedNumberDelta(metricDelta(row, baseline, 'avg_cost_micros'), 'µ')}</div>
									</td>
								</tr>
							{/each}
						</DataTable>
						<p class="mt-3 text-xs text-zinc-500 dark:text-zinc-400">
							下方小字为相对 baseline 的差值；负值代表错误率 / 延迟 / 成本低于 baseline。
						</p>
					{/if}
				</div>
			{/if}

			<!-- ═══ Fallback Chain ═══ -->
			{#if chain.length > 0}
				<FallbackChainPanel {chain} {stats} {selectedId} />
			{/if}

			<!-- ═══ Binding Table ═══ -->
			<div class="p-5">
				<div class="flex items-center justify-between mb-3">
					<h3 class="text-sm font-medium text-zinc-700 dark:text-zinc-300">渠道列表 ({detail.bindings.length})</h3>
					<button onclick={() => { showAddChannel = true; selectedChannels = new Set(); channelSearch = ''; channelProviderFilter = ''; addPriority = 100; addWeight = 1; addCanaryPercent = null; }}
						class="inline-flex items-center gap-1 px-2.5 py-1 text-xs font-medium rounded-lg border border-zinc-200 dark:border-zinc-700 text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-700">
						<Plus class="w-3 h-3" /> 添加渠道
					</button>
				</div>

				{#if detail.bindings.length === 0}
					<p class="text-center text-sm text-zinc-600 dark:text-zinc-300 py-8">暂无渠道，点击上方按钮添加</p>
				{:else}
					<DataTable class="mb-0">
						{#snippet head()}
							<tr>
								<th class={dataTemplate.th}>状态</th>
								<th class={dataTemplate.th}>渠道</th>
								<th class={dataTemplate.th}>类型</th>
								<th class={dataTemplate.th}>优先级</th>
								<th class={dataTemplate.th}>权重</th>
								<th class={dataTemplate.th}>Canary</th>
								<th class={dataTemplate.th}>模型过滤</th>
								<th class={cn(dataTemplate.th, 'text-right')}>操作</th>
							</tr>
						{/snippet}

						{#each detail.bindings as b (b.channel_id)}
							{@const health = b.channel_health ?? 'healthy'}
							{@const isEditing = editingBindingId === b.channel_id}
							<tr class={dataTemplate.row}>
								<td class={dataTemplate.td}>
									<span class="inline-block w-2.5 h-2.5 rounded-full {health === 'healthy' ? 'bg-green-500' : 'bg-red-500'}" title={health}></span>
								</td>
								<td class={dataTemplate.tdStrong}>
									<div class="font-medium">{b.channel_name}</div>
									<div class="text-xs text-zinc-600 dark:text-zinc-300">{b.channel_code}</div>
								</td>
								<td class={dataTemplate.td}>
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
								<td class={dataTemplate.td}>
									{#if isEditing}
										<input type="number" bind:value={editBindingPriority} class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100" />
									{:else}
										<button onclick={() => startEditBinding(b)} class="text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 cursor-pointer font-mono">{b.priority}</button>
									{/if}
								</td>
								<td class={dataTemplate.td}>
									{#if isEditing}
										<input type="number" bind:value={editBindingWeight} class="w-16 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100" />
									{:else}
										<button onclick={() => startEditBinding(b)} class="text-zinc-700 dark:text-zinc-300 hover:text-zinc-900 dark:hover:text-zinc-100 cursor-pointer font-mono">{b.weight}</button>
									{/if}
								</td>
								<td class={dataTemplate.td}>
									{#if isEditing}
										<input type="number" min="1" max="5" step="0.5" placeholder="关闭" bind:value={editBindingCanaryPercent} class="w-20 rounded border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-2 py-1 text-sm text-zinc-900 dark:text-zinc-100" />
									{:else if b.canary_percent_bps !== null && b.canary_percent_bps !== undefined}
										<button onclick={() => startEditBinding(b)} class="rounded border border-amber-200 bg-amber-50 px-2 py-0.5 font-mono text-xs font-medium text-amber-700 hover:bg-amber-100 dark:border-amber-900/60 dark:bg-amber-950/30 dark:text-amber-300">
											{formatCanaryPercent(b.canary_percent_bps)}
										</button>
									{:else}
										<button onclick={() => startEditBinding(b)} class="text-xs text-zinc-500 dark:text-zinc-400">关闭</button>
									{/if}
								</td>
								<td class={dataTemplate.td}>
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
								<td class={cn(dataTemplate.td, 'text-right')}>
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
					</DataTable>
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
</PageShell>


<!-- Modals -->
<CreateGroupModal
	bind:showCreate
	bind:createForm
	strategies={STRATEGIES}
	fallbackOptions={createFallbackOptions}
	{strategyBadgeClass}
	onClose={() => (showCreate = false)}
	onConfirm={handleCreate}
/>

<DeleteGroupModal
	{deleteTarget}
	deleteRefs={selectedId === deleteTarget?.id ? projectRefs(detail) : []}
	onClose={() => (deleteTarget = null)}
	onConfirm={handleDelete}
/>

<DisableGroupModal
	{disableTarget}
	bind:disableConfirmation
	onClose={() => { disableTarget = null; disableConfirmation = ''; }}
	onConfirm={confirmDisableGroup}
/>

<AddChannelModal
	bind:showAddChannel
	bind:channelSearch
	bind:channelProviderFilter
	{providerFilterOptions}
	bind:addPriority
	bind:addWeight
	bind:addCanaryPercent
	{selectedChannels}
	{filteredChannels}
	{providerColor}
	onClose={() => (showAddChannel = false)}
	onToggleChannel={toggleChannel}
	onConfirm={handleAddChannels}
/>

