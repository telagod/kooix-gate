<!-- /channels — 渠道管理：搜索/过滤/分页/批量操作/创建/编辑 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import {
		getMe,
		listAdminChannels,
		listChannels,
		createChannel,
		updateChannel,
		deleteChannel,
		batchEnableChannels,
		batchDisableChannels,
		batchDeleteChannels,
		probeChannel,
		testChannel,
		getChannelBalance
	} from '$lib/api.js';
	import type {
		Channel,
		PaginatedChannels,
		ChannelListParams,
		CreateChannelRequest,
		UpdateChannelRequest,
		TestResponse,
		ProbeResponse,
		BalanceResponse
	} from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	const PROVIDERS = ['openai', 'anthropic', 'gemini', 'azure', 'bedrock', 'deepseek', 'ollama', 'mistral', 'cohere'];
	const STRATEGIES_LABEL: Record<string, string> = {
		priority: '优先级',
		weighted_random: '加权随机',
		round_robin: '轮询',
		least_conn: '最少连接',
		least_latency: '最低延迟'
	};

	// ── State ────────────────────────────────────────
	let channels = $state<Channel[]>([]);
	let total = $state(0);
	let loading = $state(true);
	let error = $state('');
	let currentOrg = $state<string | null>(null);
	let isPlatformAdmin = $state(false);

	// Pagination & filter
	let search = $state('');
	let filterProvider = $state('');
	let filterStatus = $state('');
	let filterHealth = $state('');
	let page = $state(1);
	let pageSize = $state(20);
	let sortBy = $state('created_at');
	let sortDir = $state<'asc' | 'desc'>('desc');

	// Selection
	let selectedIds = $state<Set<string>>(new Set());
	let selectAll = $derived(channels.length > 0 && channels.every(c => selectedIds.has(c.id)));

	// Per-channel test
	let testResults = $state<Record<string, TestResponse>>({});
	let testingIds = $state<Set<string>>(new Set());
	let balances = $state<Record<string, BalanceResponse>>({});
	let loadingBalanceIds = $state<Set<string>>(new Set());

	// Probe
	let probeResult = $state<ProbeResponse | null>(null);
	let probeChannelName = $state('');
	let probingId = $state<string | null>(null);
	let syncingProbe = $state(false);

	// Batch
	let batchTesting = $state(false);
	let batchProgress = $state('');

	// Create drawer
	let showCreate = $state(false);
	let createForm = $state<CreateChannelRequest>({
		code: '', provider_type: 'openai', base_url: '', supported_models: [],
		rpm_limit: null, tpm_limit: null, timeout_ms: 60000, max_retries: 2,
		tags: [], model_mapping: {}
	});
	let creating = $state(false);
	let createError = $state('');
	let modelsInput = $state('');
	let tagsInput = $state('');

	// Edit drawer
	let editingChannel = $state<Channel | null>(null);
	let editForm = $state<UpdateChannelRequest>({});
	let editing = $state(false);
	let editError = $state('');
	let editModelsInput = $state('');
	let editTagsInput = $state('');

	// Delete confirm
	let deletingId = $state<string | null>(null);
	let deleting = $state(false);

	// Batch confirm
	let batchAction = $state<'enable' | 'disable' | 'delete' | null>(null);
	let batchProcessing = $state(false);

	// Toast
	let toast = $state('');
	let toastType = $state<'ok' | 'err'>('ok');

	// ── Computed ──────────────────────────────────────
	let totalPages = $derived(Math.max(1, Math.ceil(total / pageSize)));

	// ── Init ─────────────────────────────────────────
	onMount(async () => {
		try {
			const me = await getMe();
			currentOrg = me.current_org ?? me.orgs[0] ?? null;
			isPlatformAdmin = me.is_platform_admin;
		} catch (err: any) {
			error = err?.message ?? '加载身份失败';
			loading = false;
			return;
		}
		if (!currentOrg && !isPlatformAdmin) {
			error = '当前账号没有加入任何组织';
			loading = false;
			return;
		}
		await loadChannels();
	});

	async function loadChannels() {
		loading = true;
		error = '';
		try {
			if (isPlatformAdmin) {
				const result = await listAdminChannels({
					search: search || undefined,
					provider: filterProvider || undefined,
					status: filterStatus || undefined,
					health: filterHealth || undefined,
					page,
					page_size: pageSize,
					sort_by: sortBy,
					sort_dir: sortDir
				});
				channels = result.data ?? [];
				total = result.total ?? 0;
			} else {
				const list = await listChannels(currentOrg!);
				channels = Array.isArray(list) ? list : (list as any).data ?? [];
				total = channels.length;
			}
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	function showToast(msg: string, type: 'ok' | 'err' = 'ok') {
		toast = msg;
		toastType = type;
		setTimeout(() => (toast = ''), 3500);
	}

	// ── Search / Filter ──────────────────────────────
	let searchTimer: ReturnType<typeof setTimeout>;
	function onSearchInput() {
		clearTimeout(searchTimer);
		searchTimer = setTimeout(() => {
			page = 1;
			selectedIds = new Set();
			loadChannels();
		}, 300);
	}

	function onFilterChange() {
		page = 1;
		selectedIds = new Set();
		loadChannels();
	}

	function onSort(col: string) {
		if (sortBy === col) {
			sortDir = sortDir === 'asc' ? 'desc' : 'asc';
		} else {
			sortBy = col;
			sortDir = 'asc';
		}
		page = 1;
		loadChannels();
	}

	function goPage(p: number) {
		page = Math.max(1, Math.min(p, totalPages));
		selectedIds = new Set();
		loadChannels();
	}

	// ── Selection ────────────────────────────────────
	function toggleSelectAll() {
		if (selectAll) {
			selectedIds = new Set();
		} else {
			selectedIds = new Set(channels.map(c => c.id));
		}
	}

	function toggleSelect(id: string) {
		const s = new Set(selectedIds);
		if (s.has(id)) s.delete(id); else s.add(id);
		selectedIds = s;
	}

	// ── Toggle enabled inline ────────────────────────
	async function handleToggleEnabled(ch: Channel) {
		try {
			const updated = await updateChannel(ch.id, { enabled: ch.status !== 'active' });
			channels = channels.map(c => c.id === updated.id ? updated : c);
		} catch (err: any) {
			showToast(err?.message ?? '切换失败', 'err');
		}
	}

	// ── Test ─────────────────────────────────────────
	async function handleTest(ch: Channel) {
		testingIds = new Set([...testingIds, ch.id]);
		try {
			const result = await testChannel(ch.id);
			testResults = { ...testResults, [ch.id]: result };
		} catch (err: any) {
			testResults = {
				...testResults,
				[ch.id]: { success: false, model: '', response_time_ms: 0, message: null, error: err?.message ?? '请求失败' }
			};
		} finally {
			testingIds = new Set([...testingIds].filter(id => id !== ch.id));
		}
	}

	async function handleBatchTest() {
		batchTesting = true;
		const list = [...channels];
		for (let i = 0; i < list.length; i++) {
			const ch = list[i];
			batchProgress = `${i + 1}/${list.length}: ${ch.code}`;
			await handleTest(ch);
		}
		batchProgress = '';
		batchTesting = false;
		showToast(`批量测试完成，共 ${list.length} 个 channel`);
	}

	// ── Probe ────────────────────────────────────────
	async function handleProbe(ch: Channel) {
		probingId = ch.id;
		probeResult = null;
		probeChannelName = ch.name || ch.code;
		try {
			probeResult = await probeChannel(ch.id);
		} catch (err: any) {
			showToast(err?.message ?? 'Probe 失败', 'err');
			probingId = null;
		}
	}

	async function handleSyncModels() {
		if (!probeResult || !probingId) return;
		syncingProbe = true;
		try {
			const updated = await updateChannel(probingId, { supported_models: probeResult.models });
			channels = channels.map(c => c.id === updated.id ? updated : c);
			showToast(`已同步 ${probeResult.models.length} 个模型`);
			probeResult = null;
			probingId = null;
		} catch (err: any) {
			showToast(err?.message ?? '同步失败', 'err');
		} finally {
			syncingProbe = false;
		}
	}

	// ── Balance ──────────────────────────────────────
	async function handleRefreshBalance(ch: Channel) {
		loadingBalanceIds = new Set([...loadingBalanceIds, ch.id]);
		try {
			const result = await getChannelBalance(ch.id);
			balances = { ...balances, [ch.id]: result };
		} catch (err: any) {
			showToast(err?.message ?? '余额查询失败', 'err');
		} finally {
			loadingBalanceIds = new Set([...loadingBalanceIds].filter(id => id !== ch.id));
		}
	}

	// ── Batch actions ────────────────────────────────
	async function executeBatch() {
		if (!batchAction || selectedIds.size === 0) return;
		batchProcessing = true;
		try {
			const ids = [...selectedIds];
			let result: { affected: number };
			if (batchAction === 'enable') result = await batchEnableChannels(ids);
			else if (batchAction === 'disable') result = await batchDisableChannels(ids);
			else result = await batchDeleteChannels(ids);
			showToast(`操作完成，影响 ${result.affected} 个 channel`);
			selectedIds = new Set();
			batchAction = null;
			await loadChannels();
		} catch (err: any) {
			showToast(err?.message ?? '批量操作失败', 'err');
		} finally {
			batchProcessing = false;
		}
	}

	// ── Create ───────────────────────────────────────
	async function handleCreate(e: SubmitEvent) {
		e.preventDefault();
		if (!createForm.code.trim() || !createForm.base_url.trim()) return;
		creating = true;
		createError = '';
		try {
			const models = modelsInput.split(',').map(s => s.trim()).filter(Boolean);
			const tags = tagsInput.split(',').map(s => s.trim()).filter(Boolean);
			await createChannel({ ...createForm, supported_models: models, tags });
			showCreate = false;
			createForm = { code: '', provider_type: 'openai', base_url: '', supported_models: [], rpm_limit: null, tpm_limit: null, timeout_ms: 60000, max_retries: 2, tags: [], model_mapping: {} };
			modelsInput = '';
			tagsInput = '';
			showToast('Channel 创建成功');
			await loadChannels();
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	// ── Edit ─────────────────────────────────────────
	function startEdit(ch: Channel) {
		editingChannel = ch;
		editForm = {
			name: ch.name,
			base_url: ch.base_url,
			enabled: ch.status === 'active',
			rpm_limit: ch.rpm_limit,
			tpm_limit: ch.tpm_limit,
			timeout_ms: ch.timeout_ms,
			max_retries: ch.max_retries,
			tags: ch.tags,
			model_mapping: ch.model_mapping
		};
		editModelsInput = (ch.supported_models || []).join(', ');
		editTagsInput = (ch.tags || []).join(', ');
		editError = '';
	}

	async function handleEdit(e: SubmitEvent) {
		e.preventDefault();
		if (!editingChannel) return;
		editing = true;
		editError = '';
		try {
			const models = editModelsInput.split(',').map(s => s.trim()).filter(Boolean);
			const tags = editTagsInput.split(',').map(s => s.trim()).filter(Boolean);
			const updated = await updateChannel(editingChannel.id, { ...editForm, supported_models: models, tags });
			channels = channels.map(c => c.id === updated.id ? updated : c);
			editingChannel = null;
			showToast('Channel 更新成功');
		} catch (err: any) {
			editError = err?.message ?? '更新失败';
		} finally {
			editing = false;
		}
	}

	// ── Delete ───────────────────────────────────────
	async function handleDelete() {
		if (!deletingId) return;
		deleting = true;
		try {
			await deleteChannel(deletingId);
			deletingId = null;
			showToast('Channel 已删除');
			await loadChannels();
		} catch (err: any) {
			showToast(err?.message ?? '删除失败', 'err');
			deletingId = null;
		} finally {
			deleting = false;
		}
	}

	// ── Helpers ──────────────────────────────────────
	function statusColor(status: string): string {
		if (status === 'active') return 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400';
		if (status === 'disabled') return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300';
		return 'bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400';
	}

	function healthColor(health: string): string {
		if (health === 'healthy') return 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400';
		if (health === 'degraded') return 'bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400';
		if (health === 'unhealthy') return 'bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400';
		return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300';
	}

	function healthDot(health: string): string {
		if (health === 'healthy') return 'bg-green-500';
		if (health === 'degraded') return 'bg-amber-500';
		if (health === 'unhealthy') return 'bg-red-500';
		return 'bg-zinc-400';
	}

	function fmtBalance(b: BalanceResponse): string {
		if (!b.supported) return '不支持';
		if (b.balance_usd == null) return b.message ?? '—';
		return `$${b.balance_usd.toFixed(2)}`;
	}

	function fmtLimit(v: number | null): string {
		if (v == null) return '∞';
		return v.toLocaleString();
	}

	function sortIcon(col: string): string {
		if (sortBy !== col) return '↕';
		return sortDir === 'asc' ? '↑' : '↓';
	}

	function fmtDate(s: string | null): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
		} catch { return s; }
	}
</script>

<!-- Toast -->
{#if toast}
	<div
		class="fixed top-4 right-4 z-50 px-4 py-2 rounded-lg shadow-lg text-sm animate-fade-in {toastType === 'err'
			? 'bg-red-600 text-white'
			: 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900'}"
	>
		{toast}
	</div>
{/if}

<!-- Probe modal -->
{#if probingId && probeResult}
	<div class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) { probeResult = null; probingId = null; } }}>
		<Card class="p-6 max-w-md w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">Probe — {probeChannelName}</h3>
			<p class="text-xs text-zinc-600 dark:text-zinc-300 mb-3 font-mono">{probeResult.provider_type}</p>
			<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-2">发现 {probeResult.models.length} 个模型</p>
			<div class="max-h-56 overflow-y-auto rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 p-2 space-y-0.5">
				{#each probeResult.models as m}
					<div class="text-xs font-mono text-zinc-700 dark:text-zinc-300 px-1 py-0.5 hover:bg-zinc-100 dark:hover:bg-zinc-700 rounded">{m}</div>
				{/each}
			</div>
			<div class="flex gap-2 justify-end mt-4">
				<Button variant="outline" type="button" onclick={() => { probeResult = null; probingId = null; }}>关闭</Button>
				<Button type="button" disabled={syncingProbe} onclick={handleSyncModels}>
					{syncingProbe ? '同步中...' : '同步到 Channel'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Probing spinner -->
{#if probingId && !probeResult}
	<div class="fixed inset-0 z-50 bg-black/40 flex items-center justify-center">
		<Card class="p-6 max-w-xs w-full mx-4 flex flex-col items-center gap-3">
			<svg class="animate-spin h-8 w-8 text-zinc-600 dark:text-zinc-300" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
				<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
				<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z"></path>
			</svg>
			<p class="text-sm text-zinc-600 dark:text-zinc-300">Probe {probeChannelName}...</p>
			<Button variant="outline" size="sm" onclick={() => (probingId = null)}>取消</Button>
		</Card>
	</div>
{/if}

<!-- Delete confirm -->
{#if deletingId}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) deletingId = null; }}>
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">此操作将禁用该 channel 并软删除，无法恢复。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (deletingId = null)} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Batch confirm -->
{#if batchAction}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) batchAction = null; }}>
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">
				批量{batchAction === 'enable' ? '启用' : batchAction === 'disable' ? '禁用' : '删除'}
			</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">将对 {selectedIds.size} 个 channel 执行操作。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (batchAction = null)} disabled={batchProcessing}>取消</Button>
				<Button variant={batchAction === 'delete' ? 'destructive' : 'default'} onclick={executeBatch} disabled={batchProcessing}>
					{batchProcessing ? '处理中...' : '确认'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Create drawer -->
{#if showCreate}
	<div class="fixed inset-0 z-40 bg-black/50 flex justify-end" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) showCreate = false; }}>
		<div class="w-full max-w-lg bg-white dark:bg-zinc-900 h-full overflow-y-auto shadow-xl border-l border-zinc-200 dark:border-zinc-700">
			<div class="p-6">
				<div class="flex items-center justify-between mb-6">
					<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">新建 Channel</h2>
					<button onclick={() => (showCreate = false)} class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 text-xl">&times;</button>
				</div>
				<form onsubmit={handleCreate} class="space-y-5">
					<!-- 基础信息 -->
					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">基础信息</p>
						<div class="space-y-3">
							<div>
								<label for="ch-code" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Code <span class="text-red-500">*</span></label>
								<Input id="ch-code" placeholder="openai-prod" bind:value={createForm.code} disabled={creating} />
							</div>
							<div>
								<label for="ch-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
								<Input id="ch-name" placeholder="OpenAI Production" bind:value={createForm.name} disabled={creating} />
							</div>
							<div>
								<label for="ch-provider" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Provider <span class="text-red-500">*</span></label>
								<select
									id="ch-provider"
									bind:value={createForm.provider_type}
									disabled={creating}
									class="flex h-10 w-full rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300"
								>
									{#each PROVIDERS as p}
										<option value={p}>{p}</option>
									{/each}
								</select>
							</div>
							<div>
								<label for="ch-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL <span class="text-red-500">*</span></label>
								<Input id="ch-url" placeholder="https://api.openai.com/v1" bind:value={createForm.base_url} disabled={creating} />
							</div>
						</div>
					</div>

					<!-- 限速 -->
					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">限速 & 超时</p>
						<div class="grid grid-cols-2 gap-3">
							<div>
								<label for="ch-rpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">RPM</label>
								<Input id="ch-rpm" type="number" placeholder="无限制" bind:value={createForm.rpm_limit} disabled={creating} />
							</div>
							<div>
								<label for="ch-tpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">TPM</label>
								<Input id="ch-tpm" type="number" placeholder="无限制" bind:value={createForm.tpm_limit} disabled={creating} />
							</div>
							<div>
								<label for="ch-timeout" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">超时(ms)</label>
								<Input id="ch-timeout" type="number" bind:value={createForm.timeout_ms} disabled={creating} />
							</div>
							<div>
								<label for="ch-retries" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">重试次数</label>
								<Input id="ch-retries" type="number" bind:value={createForm.max_retries} disabled={creating} />
							</div>
						</div>
					</div>

					<!-- 模型 -->
					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">模型</p>
						<div>
							<label for="ch-models" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
								支持的模型 <span class="text-zinc-400 font-normal">(逗号分隔)</span>
							</label>
							<Input id="ch-models" placeholder="gpt-4o, gpt-4o-mini" bind:value={modelsInput} disabled={creating} />
						</div>
					</div>

					<!-- 标签 -->
					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">标签</p>
						<Input placeholder="production, us-east" bind:value={tagsInput} disabled={creating} />
					</div>

					{#if createError}
						<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{createError}</p>
					{/if}
					<div class="flex gap-2 justify-end pt-2 border-t border-zinc-200 dark:border-zinc-700">
						<Button variant="outline" type="button" onclick={() => (showCreate = false)}>取消</Button>
						<Button type="submit" disabled={creating}>
							{creating ? '创建中...' : '创建'}
						</Button>
					</div>
				</form>
			</div>
		</div>
	</div>
{/if}

<!-- Edit drawer -->
{#if editingChannel}
	<div class="fixed inset-0 z-40 bg-black/50 flex justify-end" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) editingChannel = null; }}>
		<div class="w-full max-w-lg bg-white dark:bg-zinc-900 h-full overflow-y-auto shadow-xl border-l border-zinc-200 dark:border-zinc-700">
			<div class="p-6">
				<div class="flex items-center justify-between mb-6">
					<div>
						<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">编辑 Channel</h2>
						<p class="text-xs font-mono text-zinc-500 dark:text-zinc-400 mt-0.5">{editingChannel.code}</p>
					</div>
					<button onclick={() => (editingChannel = null)} class="text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 text-xl">&times;</button>
				</div>
				<form onsubmit={handleEdit} class="space-y-5">
					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">基础信息</p>
						<div class="space-y-3">
							<div>
								<label for="ed-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
								<Input id="ed-name" bind:value={editForm.name} disabled={editing} />
							</div>
							<div>
								<label for="ed-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL</label>
								<Input id="ed-url" bind:value={editForm.base_url} disabled={editing} />
							</div>
							<div class="flex items-center gap-2">
								<input type="checkbox" id="ed-enabled" bind:checked={editForm.enabled} disabled={editing}
									class="w-4 h-4 rounded border-zinc-300 dark:border-zinc-600" />
								<label for="ed-enabled" class="text-sm text-zinc-700 dark:text-zinc-300">启用</label>
							</div>
						</div>
					</div>

					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">限速 & 超时</p>
						<div class="grid grid-cols-2 gap-3">
							<div>
								<label for="ed-rpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">RPM</label>
								<Input id="ed-rpm" type="number" placeholder="无限制" bind:value={editForm.rpm_limit} disabled={editing} />
							</div>
							<div>
								<label for="ed-tpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">TPM</label>
								<Input id="ed-tpm" type="number" placeholder="无限制" bind:value={editForm.tpm_limit} disabled={editing} />
							</div>
							<div>
								<label for="ed-timeout" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">超时(ms)</label>
								<Input id="ed-timeout" type="number" bind:value={editForm.timeout_ms} disabled={editing} />
							</div>
							<div>
								<label for="ed-retries" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">重试次数</label>
								<Input id="ed-retries" type="number" bind:value={editForm.max_retries} disabled={editing} />
							</div>
						</div>
					</div>

					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">模型</p>
						<div class="flex gap-2 items-end">
							<div class="flex-1">
								<Input placeholder="gpt-4o, gpt-4o-mini" bind:value={editModelsInput} disabled={editing} />
							</div>
							<Button variant="outline" size="sm" type="button" disabled={editing || !!probingId}
								onclick={() => handleProbe(editingChannel!)}>Probe</Button>
						</div>
					</div>

					<div>
						<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400 mb-3">标签</p>
						<Input placeholder="production, us-east" bind:value={editTagsInput} disabled={editing} />
					</div>

					{#if editError}
						<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{editError}</p>
					{/if}
					<div class="flex gap-2 justify-end pt-2 border-t border-zinc-200 dark:border-zinc-700">
						<Button variant="outline" type="button" onclick={() => (editingChannel = null)}>取消</Button>
						<Button type="submit" disabled={editing}>
							{editing ? '保存中...' : '保存'}
						</Button>
					</div>
				</form>
			</div>
		</div>
	</div>
{/if}

<!-- Main content -->
<div class="max-w-7xl mx-auto p-6">
	<!-- Header -->
	<div class="flex items-center justify-between mb-6">
		<div>
			<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">渠道管理</h1>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-1">
				{#if isPlatformAdmin}
					管理上游 LLM 服务商连接。共 {total} 个渠道。
				{:else}
					只读视图。编辑需平台管理员权限。
				{/if}
			</p>
		</div>
		{#if isPlatformAdmin}
			<div class="flex items-center gap-2">
				<Button size="sm" variant="outline" onclick={handleBatchTest} disabled={batchTesting || loading}>
					{batchTesting ? batchProgress : '批量测试'}
				</Button>
				<Button size="sm" onclick={() => (showCreate = true)}>+ 新建</Button>
			</div>
		{/if}
	</div>

	<!-- Search & Filters -->
	{#if isPlatformAdmin}
		<div class="flex flex-wrap gap-3 mb-4">
			<div class="flex-1 min-w-[200px]">
				<Input placeholder="搜索 code / 名称..." bind:value={search} oninput={onSearchInput} />
			</div>
			<select
				bind:value={filterProvider}
				onchange={onFilterChange}
				class="h-10 rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300"
			>
				<option value="">全部 Provider</option>
				{#each PROVIDERS as p}
					<option value={p}>{p}</option>
				{/each}
			</select>
			<select
				bind:value={filterStatus}
				onchange={onFilterChange}
				class="h-10 rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300"
			>
				<option value="">全部状态</option>
				<option value="active">active</option>
				<option value="disabled">disabled</option>
			</select>
			<select
				bind:value={filterHealth}
				onchange={onFilterChange}
				class="h-10 rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300"
			>
				<option value="">全部健康度</option>
				<option value="healthy">healthy</option>
				<option value="degraded">degraded</option>
				<option value="unhealthy">unhealthy</option>
			</select>
		</div>
	{/if}

	<!-- Batch toolbar -->
	{#if isPlatformAdmin && selectedIds.size > 0}
		<div class="flex items-center gap-3 mb-4 px-4 py-2 rounded-lg bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700">
			<span class="text-sm text-zinc-600 dark:text-zinc-300">已选 {selectedIds.size} 项</span>
			<div class="flex gap-2 ml-auto">
				<Button size="sm" variant="outline" onclick={() => (batchAction = 'enable')}>启用</Button>
				<Button size="sm" variant="outline" onclick={() => (batchAction = 'disable')}>禁用</Button>
				<Button size="sm" variant="destructive" onclick={() => (batchAction = 'delete')}>删除</Button>
				<Button size="sm" variant="ghost" onclick={() => (selectedIds = new Set())}>取消</Button>
			</div>
		</div>
	{/if}

	<!-- Table -->
	{#if loading}
		<div class="flex items-center justify-center py-16">
			<svg class="animate-spin h-6 w-6 text-zinc-400" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
				<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
				<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z"></path>
			</svg>
		</div>
	{:else if error}
		<Card class="p-6"><p class="text-red-600 dark:text-red-400 text-sm">{error}</p></Card>
	{:else if channels.length === 0}
		<Card class="p-10 text-center">
			<p class="text-zinc-600 dark:text-zinc-300 text-sm mb-4">暂无渠道。</p>
			{#if isPlatformAdmin}
				<Button size="sm" onclick={() => (showCreate = true)}>+ 创建第一个 Channel</Button>
			{/if}
		</Card>
	{:else}
		<div class="overflow-x-auto rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
			<table class="w-full text-sm">
				<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
					<tr>
						{#if isPlatformAdmin}
							<th class="px-3 py-3 w-8">
								<input type="checkbox" checked={selectAll} onchange={toggleSelectAll}
									class="w-4 h-4 rounded border-zinc-300 dark:border-zinc-600" />
							</th>
						{/if}
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400 cursor-pointer select-none" onclick={() => onSort('code')}>
							Code <span class="text-xs text-zinc-400">{sortIcon('code')}</span>
						</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400 cursor-pointer select-none" onclick={() => onSort('provider_type')}>
							Provider <span class="text-xs text-zinc-400">{sortIcon('provider_type')}</span>
						</th>
						<th class="px-3 py-3 text-center font-medium text-zinc-600 dark:text-zinc-400">状态</th>
						<th class="px-3 py-3 text-center font-medium text-zinc-600 dark:text-zinc-400">健康</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">模型</th>
						<th class="px-3 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">RPM / TPM</th>
						<th class="px-3 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">响应</th>
						{#if isPlatformAdmin}
							<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">操作</th>
						{/if}
					</tr>
				</thead>
				<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
					{#each channels as ch}
						{@const testRes = testResults[ch.id]}
						{@const bal = balances[ch.id]}
						{@const isTesting = testingIds.has(ch.id)}
						<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800/50 transition-colors">
							{#if isPlatformAdmin}
								<td class="px-3 py-3">
									<input type="checkbox" checked={selectedIds.has(ch.id)} onchange={() => toggleSelect(ch.id)}
										class="w-4 h-4 rounded border-zinc-300 dark:border-zinc-600" />
								</td>
							{/if}
							<!-- Code + name -->
							<td class="px-4 py-3">
								<a href="/channels/{ch.id}" class="font-mono text-zinc-900 dark:text-zinc-100 hover:underline">{ch.code}</a>
								{#if ch.name && ch.name !== ch.code}
									<div class="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5">{ch.name}</div>
								{/if}
								{#if ch.tags && ch.tags.length > 0}
									<div class="flex flex-wrap gap-1 mt-1">
										{#each ch.tags.slice(0, 3) as tag}
											<span class="px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-300 rounded text-[10px]">{tag}</span>
										{/each}
										{#if ch.tags.length > 3}
											<span class="text-[10px] text-zinc-400">+{ch.tags.length - 3}</span>
										{/if}
									</div>
								{/if}
							</td>
							<!-- Provider -->
							<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400 font-mono text-xs">{ch.provider_type}</td>
							<!-- Status toggle -->
							<td class="px-3 py-3 text-center">
								{#if isPlatformAdmin}
									<button
										onclick={() => handleToggleEnabled(ch)}
										class="relative inline-flex h-5 w-9 items-center rounded-full transition-colors {ch.status === 'active' ? 'bg-green-500' : 'bg-zinc-300 dark:bg-zinc-600'}"
										title={ch.status === 'active' ? '点击禁用' : '点击启用'}
									>
										<span class="inline-block h-3.5 w-3.5 transform rounded-full bg-white transition-transform {ch.status === 'active' ? 'translate-x-4.5' : 'translate-x-0.5'}"></span>
									</button>
								{:else}
									<span class="inline-block px-2 py-0.5 rounded text-xs font-medium {statusColor(ch.status)}">{ch.status}</span>
								{/if}
							</td>
							<!-- Health -->
							<td class="px-3 py-3 text-center">
								<div class="flex items-center justify-center gap-1.5">
									<span class="w-2 h-2 rounded-full {healthDot(ch.health)}"></span>
									<span class="text-xs text-zinc-600 dark:text-zinc-400">{ch.health}</span>
								</div>
							</td>
							<!-- Models -->
							<td class="px-4 py-3 max-w-[180px]">
								{#if ch.supported_models && ch.supported_models.length > 0}
									<div class="flex flex-wrap gap-1">
										{#each ch.supported_models.slice(0, 2) as m}
											<span class="inline-block px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 rounded text-[10px] font-mono truncate max-w-[90px]" title={m}>{m}</span>
										{/each}
										{#if ch.supported_models.length > 2}
											<span class="inline-block px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 rounded text-[10px]">+{ch.supported_models.length - 2}</span>
										{/if}
									</div>
								{:else}
									<span class="text-xs text-zinc-400">—</span>
								{/if}
							</td>
							<!-- RPM/TPM -->
							<td class="px-3 py-3 text-right text-xs text-zinc-600 dark:text-zinc-400 tabular-nums font-mono">
								{fmtLimit(ch.rpm_limit)} / {fmtLimit(ch.tpm_limit)}
							</td>
							<!-- Test result -->
							<td class="px-3 py-3 text-right min-w-[90px]">
								{#if isTesting}
									<span class="inline-flex items-center gap-1 text-xs text-zinc-500">
										<svg class="animate-spin h-3 w-3" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
											<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
											<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z"></path>
										</svg>
									</span>
								{:else if testRes}
									{#if testRes.success}
										<span class="text-xs text-green-600 dark:text-green-400 font-mono">{testRes.response_time_ms}ms</span>
									{:else}
										<span class="text-xs text-red-600 dark:text-red-400 max-w-[80px] truncate inline-block" title={testRes.error ?? undefined}>✗</span>
									{/if}
								{:else if ch.balance != null}
									<span class="text-xs text-zinc-500 dark:text-zinc-400 font-mono">${ch.balance.toFixed(2)}</span>
								{:else}
									<span class="text-xs text-zinc-400">—</span>
								{/if}
							</td>
							<!-- Actions -->
							{#if isPlatformAdmin}
								<td class="px-4 py-3 text-right">
									<div class="flex gap-1 justify-end">
										<Button variant="ghost" size="sm" disabled={isTesting} onclick={() => handleTest(ch)} class="text-zinc-600 dark:text-zinc-400">测试</Button>
										<Button variant="ghost" size="sm" onclick={() => handleProbe(ch)} class="text-zinc-600 dark:text-zinc-400">Probe</Button>
										<Button variant="ghost" size="sm" onclick={() => startEdit(ch)} class="text-zinc-600 dark:text-zinc-400">编辑</Button>
										<Button variant="ghost" size="sm" onclick={() => goto(`/channels/${ch.id}`)} class="text-zinc-600 dark:text-zinc-400">Keys</Button>
										<Button variant="ghost" size="sm" onclick={() => (deletingId = ch.id)}>
											<span class="text-red-600 dark:text-red-400">删除</span>
										</Button>
									</div>
								</td>
							{/if}
						</tr>
					{/each}
				</tbody>
			</table>
		</div>

		<!-- Pagination -->
		{#if totalPages > 1}
			<div class="flex items-center justify-between mt-4 px-1">
				<p class="text-xs text-zinc-500 dark:text-zinc-400">
					第 {(page - 1) * pageSize + 1}–{Math.min(page * pageSize, total)} / {total} 条
				</p>
				<div class="flex items-center gap-1">
					<Button variant="outline" size="sm" disabled={page <= 1} onclick={() => goPage(page - 1)}>上一页</Button>
					{#each Array.from({ length: Math.min(5, totalPages) }, (_, i) => {
						const start = Math.max(1, Math.min(page - 2, totalPages - 4));
						return start + i;
					}).filter(p => p >= 1 && p <= totalPages) as p}
						<button
							onclick={() => goPage(p)}
							class="w-8 h-8 rounded-md text-sm transition-colors {p === page
								? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 font-medium'
								: 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
						>{p}</button>
					{/each}
					<Button variant="outline" size="sm" disabled={page >= totalPages} onclick={() => goPage(page + 1)}>下一页</Button>
				</div>
			</div>
		{/if}
	{/if}
</div>
