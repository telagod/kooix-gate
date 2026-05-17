<!-- /channels — 渠道管理 -->
<script lang="ts">
	import { rawId } from '$lib/id.js';
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
		CreateChannelRequest,
		UpdateChannelRequest,
		TestResponse,
		ProbeResponse,
		BalanceResponse
	} from '$lib/api.js';
	import { Alert, Badge, Button, Card, Field, FilterPills, Input, ProviderSelect } from '$lib/components/ui';
	import type { ProviderOption } from '$lib/components/ui/ProviderSelect.svelte';
	import DropdownMenu from '$lib/components/ui/DropdownMenu.svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import {
		Search,
		Plus,
		Pencil,
		Trash2,
		Key,
		Radar,
		X,
		Cable,
		CheckCircle2,
		XCircle,
		ArrowUpDown,
		ArrowUp,
		ArrowDown,
		ChevronLeft,
		ChevronRight,
		Zap
	} from 'lucide-svelte';

	const PROVIDER_OPTIONS: ProviderOption[] = [
		{ value: 'openai', label: 'OpenAI', description: 'GPT-4o / o1 / o3' },
		{ value: 'anthropic', label: 'Anthropic', description: 'Claude 4 / Sonnet / Haiku' },
		{ value: 'gemini', label: 'Google Gemini', description: 'Gemini 2.5 Pro / Flash' },
		{ value: 'azure', label: 'Azure OpenAI', description: 'Azure 托管 GPT 部署' },
		{ value: 'bedrock', label: 'AWS Bedrock', description: 'Claude / Titan / Llama' },
		{ value: 'deepseek', label: 'DeepSeek', description: 'DeepSeek-V3 / R1' },
		{ value: 'ollama', label: 'Ollama', description: '本地模型推理' },
		{ value: 'mistral', label: 'Mistral', description: 'Mistral Large / Codestral' },
		{ value: 'cohere', label: 'Cohere', description: 'Command R+ / Embed' },
		{ value: 'groq', label: 'Groq', description: 'LPU 推理加速' },
		{ value: 'together', label: 'Together AI', description: '开源模型推理' },
		{ value: 'openrouter', label: 'OpenRouter', description: '多 provider 聚合' },
		{ value: 'moonshot', label: 'Moonshot', description: 'Kimi 长上下文' },
		{ value: 'zhipu', label: '智谱 GLM', description: 'GLM-4 / CodeGeex' },
		{ value: 'qwen', label: '通义千问', description: 'Qwen-Max / Qwen-VL' },
		{ value: 'yi', label: '零一万物', description: 'Yi-Large / Yi-Lightning' },
		{ value: 'plugin', label: 'HTTP Plugin', description: '自定义私有协议 / SSE 整流' },
	];

	const FILTER_PROVIDER_OPTIONS: ProviderOption[] = [
		{ value: '', label: '全部 Provider', description: '不过滤' },
		...PROVIDER_OPTIONS,
	];

	const STATUS_OPTIONS = [
		{ value: '', label: '全部状态' },
		{ value: 'active', label: 'Active' },
		{ value: 'disabled', label: 'Disabled' },
	];

	const HEALTH_OPTIONS = [
		{ value: '', label: '全部健康度' },
		{ value: 'healthy', label: 'Healthy' },
		{ value: 'degraded', label: 'Degraded' },
		{ value: 'unhealthy', label: 'Unhealthy' },
	];

	// ── State ────────────────────────────────────────
	let channels = $state<Channel[]>([]);
	let total = $state(0);
	let loading = $state(true);
	let error = $state('');
	let currentOrg = $state<string | null>(null);
	let isPlatformAdmin = $state(false);

	let search = $state('');
	let filterProvider = $state('');
	let filterStatus = $state('');
	let filterHealth = $state('');
	let page = $state(1);
	let pageSize = $state(20);
	let sortBy = $state('created_at');
	let sortDir = $state<'asc' | 'desc'>('desc');

	let selectedIds = $state<Set<string>>(new Set());
	let selectAll = $derived(channels.length > 0 && channels.every(c => selectedIds.has(c.id)));
	let expandedId = $state<string | null>(null);

	let testResults = $state<Record<string, TestResponse>>({});
	let testingIds = $state<Set<string>>(new Set());
	let balances = $state<Record<string, BalanceResponse>>({});
	let loadingBalanceIds = $state<Set<string>>(new Set());

	let probeResult = $state<ProbeResponse | null>(null);
	let probeChannelName = $state('');
	let probingId = $state<string | null>(null);
	let syncingProbe = $state(false);

	let batchTesting = $state(false);
	let batchProgress = $state('');

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
	let pluginManifestInput = $state('');

	let editingChannel = $state<Channel | null>(null);
	let editForm = $state<UpdateChannelRequest>({});
	let editing = $state(false);
	let editError = $state('');
	let editModelsInput = $state('');
	let editTagsInput = $state('');
	let editPluginManifestInput = $state('');

	let deletingId = $state<string | null>(null);
	let deleting = $state(false);

	let batchAction = $state<'enable' | 'disable' | 'delete' | null>(null);
	let batchProcessing = $state(false);

	let toast = $state('');
	let toastType = $state<'ok' | 'err'>('ok');

	let totalPages = $derived(Math.max(1, Math.ceil(total / pageSize)));

	// ── Init ─────────────────────────────────────────
	onMount(() => {
		(async () => {
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
		})();

		document.addEventListener('keydown', handleKeyboard);
		return () => document.removeEventListener('keydown', handleKeyboard);
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

	// ── Keyboard shortcuts ───────────────────────────
	let focusedIdx = $state(-1);

	function handleKeyboard(e: KeyboardEvent) {
		if (e.target instanceof HTMLInputElement || e.target instanceof HTMLTextAreaElement || e.target instanceof HTMLSelectElement) return;
		if (showCreate || editingChannel || deletingId || batchAction || probingId) return;

		if (e.key === 'j' || e.key === 'ArrowDown') {
			e.preventDefault();
			focusedIdx = Math.min(focusedIdx + 1, channels.length - 1);
		} else if (e.key === 'k' || e.key === 'ArrowUp') {
			e.preventDefault();
			focusedIdx = Math.max(focusedIdx - 1, 0);
		} else if (e.key === 'Enter' && focusedIdx >= 0) {
			e.preventDefault();
			expandedId = expandedId === channels[focusedIdx].id ? null : channels[focusedIdx].id;
		} else if (e.key === 'e' && focusedIdx >= 0 && isPlatformAdmin) {
			e.preventDefault();
			startEdit(channels[focusedIdx]);
		} else if (e.key === 't' && focusedIdx >= 0) {
			e.preventDefault();
			handleTest(channels[focusedIdx]);
		} else if (e.key === 'Escape') {
			expandedId = null;
			focusedIdx = -1;
		}
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

	let prevFilterProvider = $state('');
	$effect(() => {
		if (filterProvider !== prevFilterProvider) {
			prevFilterProvider = filterProvider;
			if (!loading) onFilterChange();
		}
	});

	let prevFilterStatus = $state('');
	$effect(() => {
		if (filterStatus !== prevFilterStatus) {
			prevFilterStatus = filterStatus;
			if (!loading) onFilterChange();
		}
	});

	let prevFilterHealth = $state('');
	$effect(() => {
		if (filterHealth !== prevFilterHealth) {
			prevFilterHealth = filterHealth;
			if (!loading) onFilterChange();
		}
	});

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
		if (selectAll) selectedIds = new Set();
		else selectedIds = new Set(channels.map(c => c.id));
	}

	function toggleSelect(id: string) {
		const s = new Set(selectedIds);
		if (s.has(id)) s.delete(id); else s.add(id);
		selectedIds = s;
	}

	// ── Toggle enabled ───────────────────────────────
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
			batchProgress = `${i + 1}/${list.length}`;
			await handleTest(ch);
		}
		batchProgress = '';
		batchTesting = false;
		showToast(`批量测试完成，共 ${list.length} 个`);
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
			showToast(`操作完成，影响 ${result.affected} 个`);
			selectedIds = new Set();
			batchAction = null;
			await loadChannels();
		} catch (err: any) {
			showToast(err?.message ?? '批量操作失败', 'err');
		} finally {
			batchProcessing = false;
		}
	}

	const PLUGIN_MANIFEST_EXAMPLE = `{
  "plugin": {
    "request": {
      "chat_path": "/private/chat",
      "headers": { "X-Api-Key": "{{api_key}}" },
      "body": {
        "modelName": "{{model}}",
        "prompt": "{{last_user_message}}",
        "stream": "{{stream}}",
        "limit": "{{max_tokens}}"
      }
    },
    "response": {
      "openai_compatible": false,
      "content_path": "result.text",
      "finish_reason_path": "result.finish",
      "usage": {
        "prompt_tokens_path": "usage.input",
        "completion_tokens_path": "usage.output"
      }
    },
    "stream": {
      "openai_compatible": false,
      "event_path": "payload",
      "content_path": "token",
      "finish_reason_path": "finish",
      "done": ["[DONE]", "EOF"],
      "usage": {
        "prompt_tokens_path": "usage.input",
        "completion_tokens_path": "usage.output"
      }
    }
  }
}`;

	function isPluginProvider(providerType: string | undefined): boolean {
		return ['plugin', 'custom', 'http', 'http_plugin'].includes(providerType ?? '');
	}

	function parsePluginManifest(input: string): Record<string, unknown> {
		if (!input.trim()) return {};
		const parsed = JSON.parse(input);
		if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
			throw new Error('Plugin manifest 必须是 JSON object');
		}
		return parsed as Record<string, unknown>;
	}

	function resetCreateForm() {
		createForm = { code: '', provider_type: 'openai', base_url: '', supported_models: [], rpm_limit: null, tpm_limit: null, timeout_ms: 60000, max_retries: 2, tags: [], model_mapping: {} };
		modelsInput = '';
		tagsInput = '';
		pluginManifestInput = '';
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
			const model_mapping = isPluginProvider(createForm.provider_type) ? parsePluginManifest(pluginManifestInput) : createForm.model_mapping;
			await createChannel({ ...createForm, supported_models: models, tags, model_mapping });
			showCreate = false;
			resetCreateForm();
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
		editPluginManifestInput = isPluginProvider(ch.provider_type) ? JSON.stringify(ch.model_mapping ?? {}, null, 2) : '';
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
			const model_mapping = isPluginProvider(editingChannel.provider_type) ? parsePluginManifest(editPluginManifestInput) : editForm.model_mapping;
			const updated = await updateChannel(editingChannel.id, { ...editForm, supported_models: models, tags, model_mapping });
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
	function healthBadgeCls(health: string): string {
		if (health === 'healthy') return 'bg-green-50 dark:bg-green-500/10 text-green-700 dark:text-green-400 ring-1 ring-green-600/10 dark:ring-green-400/20';
		if (health === 'degraded') return 'bg-amber-50 dark:bg-amber-500/10 text-amber-700 dark:text-amber-400 ring-1 ring-amber-600/10 dark:ring-amber-400/20';
		if (health === 'unhealthy') return 'bg-red-50 dark:bg-red-500/10 text-red-700 dark:text-red-400 ring-1 ring-red-600/10 dark:ring-red-400/20';
		return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 ring-1 ring-zinc-200 dark:ring-zinc-700';
	}

	function healthDot(health: string): string {
		if (health === 'healthy') return 'bg-green-500';
		if (health === 'degraded') return 'bg-amber-500';
		if (health === 'unhealthy') return 'bg-red-500';
		return 'bg-zinc-400';
	}

	function fmtLimit(v: number | null): string {
		if (v == null) return '—';
		return v.toLocaleString();
	}

	function fmtDate(s: string | null): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
		} catch { return s; }
	}

	function getMenuItems(ch: Channel) {
		const isTesting = testingIds.has(ch.id);
		const items: any[] = [
			{ label: '测试连通性', icon: Zap, disabled: isTesting, onclick: () => handleTest(ch) },
			{ label: 'Probe 模型', icon: Radar, onclick: () => handleProbe(ch) },
			{ label: '编辑', icon: Pencil, onclick: () => startEdit(ch) },
			{ label: '管理 Keys', icon: Key, onclick: () => goto(`/channels/${rawId(ch.id)}`) },
		];
		items.push({ label: '删除', icon: Trash2, danger: true, onclick: () => (deletingId = ch.id) });
		return items;
	}

	function pageNumbers(current: number, total: number): (number | '...')[] {
		if (total <= 7) return Array.from({ length: total }, (_, i) => i + 1);
		const pages: (number | '...')[] = [1];
		if (current > 3) pages.push('...');
		for (let i = Math.max(2, current - 1); i <= Math.min(total - 1, current + 1); i++) {
			pages.push(i);
		}
		if (current < total - 2) pages.push('...');
		pages.push(total);
		return pages;
	}

	function sortIconClass(col: string) {
		return sortBy === col ? 'text-zinc-700 dark:text-zinc-200' : 'text-zinc-400 opacity-40';
	}

	function selectAllChange() {
		toggleSelectAll();
	}

	function channelSelectChange(id: string) {
		toggleSelect(id);
	}
</script>

<!-- Toast -->
{#if toast}
	<div
		class="fixed top-4 right-4 z-[60] px-4 py-2.5 rounded-lg shadow-lg text-sm font-medium animate-fade-in flex items-center gap-2 {toastType === 'err'
			? 'bg-red-600 text-white'
			: 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900'}"
	>
		{#if toastType === 'ok'}<CheckCircle2 size={16} />{:else}<XCircle size={16} />{/if}
		{toast}
	</div>
{/if}

<!-- Modal: Probe result -->
{#if probingId && probeResult}
	<ModalFrame close={() => { probeResult = null; probingId = null; }} class="z-50 bg-black/60 backdrop-blur-sm animate-backdrop">
		<Card class="p-6 max-w-md w-full mx-4 animate-fade-in shadow-2xl">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">Probe — {probeChannelName}</h3>
			<p class="text-xs text-zinc-500 dark:text-zinc-400 mb-3 font-mono">{probeResult.provider_type}</p>
			<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-2">发现 {probeResult.models.length} 个模型</p>
			<div class="max-h-56 overflow-y-auto rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/50 p-2 space-y-0.5">
				{#each probeResult.models as m}
					<div class="text-xs font-mono text-zinc-700 dark:text-zinc-300 px-2 py-1 hover:bg-zinc-100 dark:hover:bg-zinc-700 rounded">{m}</div>
				{/each}
			</div>
			<div class="flex gap-2 justify-end mt-4">
				<Button variant="outline" type="button" onclick={() => { probeResult = null; probingId = null; }}>关闭</Button>
				<Button type="button" disabled={syncingProbe} onclick={handleSyncModels}>
					{syncingProbe ? '同步中...' : '同步到 Channel'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}

<!-- Modal: Probing spinner -->
{#if probingId && !probeResult}
	<div class="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm flex items-center justify-center animate-backdrop">
		<Card class="p-6 max-w-xs w-full mx-4 flex flex-col items-center gap-3 animate-fade-in">
			<div class="w-10 h-10 rounded-full border-2 border-zinc-200 dark:border-zinc-700 border-t-zinc-900 dark:border-t-zinc-100 animate-spin"></div>
			<p class="text-sm text-zinc-600 dark:text-zinc-300">Probe {probeChannelName}...</p>
			<Button variant="outline" size="sm" onclick={() => (probingId = null)}>取消</Button>
		</Card>
	</div>
{/if}

<!-- Modal: Delete confirm -->
{#if deletingId}
	<ModalFrame close={() => { deletingId = null; }} class="z-50 bg-black/60 backdrop-blur-sm animate-backdrop">
		<Card class="p-6 max-w-sm w-full mx-4 animate-fade-in shadow-2xl">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">此操作将禁用该 channel 并软删除，无法恢复。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (deletingId = null)} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}

<!-- Modal: Batch confirm -->
{#if batchAction}
	<ModalFrame close={() => { batchAction = null; }} class="z-50 bg-black/60 backdrop-blur-sm animate-backdrop">
		<Card class="p-6 max-w-sm w-full mx-4 animate-fade-in shadow-2xl">
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
	</ModalFrame>
{/if}

<!-- Drawer: Create -->
{#if showCreate}
	<ModalFrame close={() => { showCreate = false; }} class="z-40 justify-end bg-black/50 backdrop-blur-sm p-0 animate-backdrop">
		<div class="w-full max-w-lg bg-white dark:bg-zinc-900 h-full overflow-y-auto shadow-2xl animate-slide-in-right">
			<div class="p-6">
				<div class="flex items-center justify-between mb-6">
					<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">新建 Channel</h2>
					<button onclick={() => (showCreate = false)} class="p-1.5 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
						<X size={18} />
					</button>
				</div>
				<form onsubmit={handleCreate} class="space-y-6">
					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">基础信息</p>
						<div class="space-y-3">
							<div>
								<label for="ch-code" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Code <span class="text-red-500">*</span></label>
								<Input id="ch-code" placeholder="openai-prod" bind:value={createForm.code} disabled={creating} />
							</div>
							<div>
								<label for="ch-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
								<Input id="ch-name" placeholder="OpenAI Production" bind:value={createForm.name} disabled={creating} />
							</div>
							<Field label="Provider" for="ch-provider" required>
								<ProviderSelect bind:value={createForm.provider_type} options={PROVIDER_OPTIONS} mode="grid" disabled={creating} />
							</Field>
							<div>
								<label for="ch-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL <span class="text-red-500">*</span></label>
								<Input id="ch-url" placeholder="https://api.openai.com/v1" bind:value={createForm.base_url} disabled={creating} />
							</div>
							{#if isPluginProvider(createForm.provider_type)}
								<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
									<label for="ch-plugin" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Plugin Manifest</label>
									<textarea id="ch-plugin" class="min-h-64 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={PLUGIN_MANIFEST_EXAMPLE} bind:value={pluginManifestInput} disabled={creating}></textarea>
									<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">留空则按 OpenAI-compatible；填写 JSON 可映射私有 body、headers、非标准响应和 SSE token 帧。</p>
								</div>
							{/if}
						</div>
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">限速 & 超时</p>
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

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">模型</p>
						<Input placeholder="gpt-4o, gpt-4o-mini (逗号分隔)" bind:value={modelsInput} disabled={creating} />
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">标签</p>
						<Input placeholder="production, us-east" bind:value={tagsInput} disabled={creating} />
					</div>

					{#if createError}
						<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-lg px-3 py-2">{createError}</p>
					{/if}
					<div class="flex gap-2 justify-end pt-4 border-t border-zinc-200 dark:border-zinc-800">
						<Button variant="outline" type="button" onclick={() => (showCreate = false)}>取消</Button>
						<Button type="submit" disabled={creating}>{creating ? '创建中...' : '创建'}</Button>
					</div>
				</form>
			</div>
		</div>
	</ModalFrame>
{/if}

<!-- Drawer: Edit -->
{#if editingChannel}
	<ModalFrame close={() => { editingChannel = null; }} class="z-40 justify-end bg-black/50 backdrop-blur-sm p-0 animate-backdrop">
		<div class="w-full max-w-lg bg-white dark:bg-zinc-900 h-full overflow-y-auto shadow-2xl animate-slide-in-right">
			<div class="p-6">
				<div class="flex items-center justify-between mb-6">
					<div>
						<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">编辑 Channel</h2>
						<p class="text-xs font-mono text-zinc-500 dark:text-zinc-400 mt-0.5">{editingChannel.code}</p>
					</div>
					<button onclick={() => (editingChannel = null)} class="p-1.5 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
						<X size={18} />
					</button>
				</div>
				<form onsubmit={handleEdit} class="space-y-6">
					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">基础信息</p>
						<div class="space-y-3">
							<div>
								<label for="ed-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
								<Input id="ed-name" bind:value={editForm.name} disabled={editing} />
							</div>
							<div>
								<label for="ed-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL</label>
								<Input id="ed-url" bind:value={editForm.base_url} disabled={editing} />
							</div>
							{#if isPluginProvider(editingChannel.provider_type)}
								<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
									<label for="ed-plugin" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Plugin Manifest</label>
									<textarea id="ed-plugin" class="min-h-64 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={PLUGIN_MANIFEST_EXAMPLE} bind:value={editPluginManifestInput} disabled={editing}></textarea>
								</div>
							{/if}
							<div class="flex items-center gap-2">
								<input type="checkbox" id="ed-enabled" bind:checked={editForm.enabled} disabled={editing} class="w-4 h-4 rounded border-zinc-300 dark:border-zinc-600" />
								<label for="ed-enabled" class="text-sm text-zinc-700 dark:text-zinc-300">启用</label>
							</div>
						</div>
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">限速 & 超时</p>
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
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">模型</p>
						<div class="flex gap-2 items-end">
							<div class="flex-1">
								<Input placeholder="gpt-4o, gpt-4o-mini" bind:value={editModelsInput} disabled={editing} />
							</div>
							<Button variant="outline" size="sm" type="button" disabled={editing || !!probingId} onclick={() => handleProbe(editingChannel!)}>
								<span class="flex items-center gap-1"><Radar size={14} /> Probe</span>
							</Button>
						</div>
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">标签</p>
						<Input placeholder="production, us-east" bind:value={editTagsInput} disabled={editing} />
					</div>

					{#if editError}
						<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-lg px-3 py-2">{editError}</p>
					{/if}
					<div class="flex gap-2 justify-end pt-4 border-t border-zinc-200 dark:border-zinc-800">
						<Button variant="outline" type="button" onclick={() => (editingChannel = null)}>取消</Button>
						<Button type="submit" disabled={editing}>{editing ? '保存中...' : '保存'}</Button>
					</div>
				</form>
			</div>
		</div>
	</ModalFrame>
{/if}

<!-- Main content -->
<PageShell
	title="渠道管理"
	description={isPlatformAdmin ? `管理上游 LLM 服务商连接 · 共 ${total} 个渠道` : '只读视图 · 编辑需平台管理员权限'}
	icon={Cable}
	max="full"
	class="flex h-full flex-col"
>
	{#snippet actions()}
		{#if isPlatformAdmin}
			<Button size="sm" variant="outline" onclick={handleBatchTest} disabled={batchTesting || loading}>
				<Zap size={14} />
				{batchTesting ? batchProgress : '批量测试'}
			</Button>
			<Button size="sm" onclick={() => (showCreate = true)}>
				<Plus size={14} /> 新建
			</Button>
		{/if}
	{/snippet}

	<!-- Search & Filters -->
	{#if isPlatformAdmin}
		<DataToolbar class="mb-6" searchClass="max-w-none" badgesVisible={selectedIds.size > 0}>
			{#snippet query()}
				<Search size={16} class="absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
				<Input
					id="channels-search"
					placeholder="搜索 code / 名称..."
					bind:value={search}
					oninput={onSearchInput}
					class="pl-9"
				/>
			{/snippet}

			{#snippet controls()}
				<div class="w-[180px]">
					<ProviderSelect bind:value={filterProvider} options={FILTER_PROVIDER_OPTIONS} placeholder="全部 Provider" />
				</div>
				<FilterPills bind:value={filterStatus} options={STATUS_OPTIONS} />
				<FilterPills bind:value={filterHealth} options={HEALTH_OPTIONS} />
			{/snippet}

			{#snippet badges()}
				<Badge class="bg-zinc-900 text-white ring-zinc-900 dark:bg-zinc-100 dark:text-zinc-900 dark:ring-zinc-100">
					已选 {selectedIds.size} 项
				</Badge>
				<div class="flex flex-wrap items-center gap-2">
					<Button size="sm" variant="outline" onclick={() => (batchAction = 'enable')}>启用</Button>
					<Button size="sm" variant="outline" onclick={() => (batchAction = 'disable')}>禁用</Button>
					<Button size="sm" variant="destructive" onclick={() => (batchAction = 'delete')}>删除</Button>
					<Button size="sm" variant="ghost" onclick={() => (selectedIds = new Set())}>取消</Button>
				</div>
			{/snippet}
		</DataToolbar>
	{/if}

	<!-- Loading -->
	{#if loading}
		<div class="flex items-center justify-center py-20">
			<div class="w-8 h-8 rounded-full border-2 border-zinc-200 dark:border-zinc-700 border-t-zinc-900 dark:border-t-zinc-100 animate-spin"></div>
		</div>
	{:else if error}
		<Alert variant="danger" class="p-8 text-center">
			<XCircle size={32} class="text-red-400 mx-auto mb-3" />
			<p>{error}</p>
		</Alert>
	{:else if channels.length === 0}
		<!-- Empty state -->
		<div class="flex flex-col items-center justify-center py-20 text-center">
			<div class="w-16 h-16 rounded-2xl bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center mb-5">
				<Cable size={28} class="text-zinc-400" />
			</div>
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">暂无渠道</h3>
			<p class="text-sm text-zinc-500 dark:text-zinc-400 max-w-sm mb-6">
				渠道是连接上游 LLM 服务商的桥梁。创建一个渠道开始路由 API 请求。
			</p>
			{#if isPlatformAdmin}
				<Button onclick={() => (showCreate = true)}>
					<Plus size={14} /> 创建第一个 Channel
				</Button>
			{/if}
			<p class="text-xs text-zinc-400 dark:text-zinc-500 mt-8">
				快捷键: <kbd class="px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 font-mono">j</kbd> / <kbd class="px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 font-mono">k</kbd> 导航 · <kbd class="px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 font-mono">Enter</kbd> 展开 · <kbd class="px-1.5 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 font-mono">e</kbd> 编辑
			</p>
		</div>
	{:else}
		<!-- Table -->
		<div class="flex min-h-0 flex-1 flex-col">
			<DataTable class="min-h-0 flex-1" bodyClass="divide-y-0">
				{#snippet head()}
					<tr>
						{#if isPlatformAdmin}
							<th class="px-4 py-3.5 w-10">
								<input
									type="checkbox"
									checked={selectAll}
									onchange={selectAllChange}
									class="w-3.5 h-3.5 rounded border-zinc-300 dark:border-zinc-600"
								/>
							</th>
						{/if}
						<th class={cn(dataTemplate.th, 'py-3.5 cursor-pointer select-none')} onclick={() => onSort('code')}>
							<span class="inline-flex items-center gap-1">
								Channel
								{#if sortBy === 'code'}
									{#if sortDir === 'asc'}
										<ArrowUp size={12} class={sortIconClass('code')} />
									{:else}
										<ArrowDown size={12} class={sortIconClass('code')} />
									{/if}
								{:else}
									<ArrowUpDown size={12} class={sortIconClass('code')} />
								{/if}
							</span>
						</th>
						<th class={cn(dataTemplate.th, 'py-3.5 cursor-pointer select-none')} onclick={() => onSort('provider_type')}>
							<span class="inline-flex items-center gap-1">
								Provider
								{#if sortBy === 'provider_type'}
									{#if sortDir === 'asc'}
										<ArrowUp size={12} class={sortIconClass('provider_type')} />
									{:else}
										<ArrowDown size={12} class={sortIconClass('provider_type')} />
									{/if}
								{:else}
									<ArrowUpDown size={12} class={sortIconClass('provider_type')} />
								{/if}
							</span>
						</th>
						<th class={cn(dataTemplate.th, 'py-3.5 text-center')}>状态</th>
						<th class={cn(dataTemplate.th, 'py-3.5 text-center')}>健康</th>
						<th class={cn(dataTemplate.th, 'py-3.5')}>模型</th>
						<th class={cn(dataTemplate.th, 'py-3.5 text-right')}>响应</th>
						{#if isPlatformAdmin}
							<th class="px-4 py-3.5 w-12"></th>
						{/if}
					</tr>
				{/snippet}

				{#each channels as ch, idx}
					{@const testRes = testResults[ch.id]}
					{@const isTesting = testingIds.has(ch.id)}
					{@const isExpanded = expandedId === ch.id}
					{@const isFocused = focusedIdx === idx}
					<!-- Main row -->
					<tr
						class={cn(
							'border-b border-zinc-50 dark:border-zinc-800/50',
							dataTemplate.rowInteractive,
							isFocused && 'bg-zinc-50 dark:bg-zinc-800/70'
						)}
						onclick={() => (expandedId = isExpanded ? null : ch.id)}
					>
						{#if isPlatformAdmin}
							<td class="px-4 py-4" onclick={(e: MouseEvent) => e.stopPropagation()}>
								<input
									type="checkbox"
									checked={selectedIds.has(ch.id)}
									onchange={() => channelSelectChange(ch.id)}
									class="w-3.5 h-3.5 rounded border-zinc-300 dark:border-zinc-600"
								/>
							</td>
						{/if}
						<!-- Channel -->
						<td class="px-4 py-4">
							<div class="flex items-center gap-3">
								<div class="w-8 h-8 rounded-lg bg-white dark:bg-zinc-800 border border-zinc-100 dark:border-zinc-700 flex items-center justify-center shrink-0">
									<img src="/providers/{ch.provider_type}.svg" alt="" class="w-5 h-5" />
								</div>
								<div class="min-w-0">
									<p class="font-medium text-zinc-900 dark:text-zinc-100 truncate">{ch.code}</p>
									{#if ch.name && ch.name !== ch.code}
										<p class="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5 truncate">{ch.name}</p>
									{/if}
								</div>
							</div>
						</td>
						<!-- Provider -->
						<td class="px-4 py-4">
							<span class="inline-flex items-center gap-1.5 px-2 py-1 rounded-md bg-zinc-50 dark:bg-zinc-800 text-xs font-mono text-zinc-600 dark:text-zinc-400">
								{ch.provider_type}
							</span>
						</td>
						<!-- Status -->
						<td class="px-4 py-4 text-center" onclick={(e: MouseEvent) => e.stopPropagation()}>
							{#if isPlatformAdmin}
								<button
									type="button"
									onclick={() => handleToggleEnabled(ch)}
									class="relative inline-flex h-5 w-9 items-center rounded-full transition-colors {ch.status === 'active' ? 'bg-green-500' : 'bg-zinc-300 dark:bg-zinc-600'}"
									title={ch.status === 'active' ? '点击禁用' : '点击启用'}
								>
									<span class="inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow-sm transition-transform {ch.status === 'active' ? 'translate-x-4.5' : 'translate-x-0.5'}"></span>
								</button>
							{:else}
								<span class="inline-block px-2 py-0.5 rounded-full text-xs font-medium {ch.status === 'active' ? 'bg-green-50 dark:bg-green-500/10 text-green-700 dark:text-green-400' : 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400'}">{ch.status}</span>
							{/if}
						</td>
						<!-- Health -->
						<td class="px-4 py-4 text-center">
							<span class="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs font-medium {healthBadgeCls(ch.health)}">
								<span class="w-1.5 h-1.5 rounded-full {healthDot(ch.health)}"></span>
								{ch.health}
							</span>
						</td>
						<!-- Models -->
						<td class="px-4 py-4 max-w-[200px]">
							{#if ch.supported_models && ch.supported_models.length > 0}
								<div class="flex flex-wrap gap-1">
									{#each ch.supported_models.slice(0, 2) as m}
										<span class="inline-block px-2 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300 rounded-md text-xs font-mono truncate max-w-[100px]" title={m}>{m}</span>
									{/each}
									{#if ch.supported_models.length > 2}
										<span class="inline-block px-2 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400 rounded-md text-xs">+{ch.supported_models.length - 2}</span>
									{/if}
								</div>
							{:else}
								<span class="text-xs text-zinc-400">—</span>
							{/if}
						</td>
						<!-- Response -->
						<td class="px-4 py-4 text-right">
							{#if isTesting}
								<div class="w-4 h-4 rounded-full border-2 border-zinc-200 dark:border-zinc-600 border-t-zinc-900 dark:border-t-zinc-100 animate-spin ml-auto"></div>
							{:else if testRes}
								{#if testRes.success}
									<span class="text-xs font-mono font-medium text-green-600 dark:text-green-400">{testRes.response_time_ms}ms</span>
								{:else}
									<span class="text-xs text-red-500 dark:text-red-400" title={testRes.error ?? undefined}>fail</span>
								{/if}
							{:else if ch.balance != null}
								<span class="text-xs text-zinc-500 dark:text-zinc-400 font-mono">${ch.balance.toFixed(2)}</span>
							{:else}
								<span class="text-xs text-zinc-300 dark:text-zinc-600">—</span>
							{/if}
						</td>
						<!-- Actions -->
						{#if isPlatformAdmin}
							<td class="px-4 py-4" onclick={(e: MouseEvent) => e.stopPropagation()}>
								<DropdownMenu items={getMenuItems(ch)} />
							</td>
						{/if}
					</tr>
					<!-- Expanded detail row -->
					{#if isExpanded}
						<tr class="bg-zinc-50/50 dark:bg-zinc-800/20">
							<td colspan="100" class="px-6 py-5 animate-expand">
								<div class="grid grid-cols-2 md:grid-cols-4 gap-6">
									<div>
										<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">Base URL</p>
										<p class="text-xs font-mono text-zinc-700 dark:text-zinc-300 break-all">{ch.base_url}</p>
									</div>
									<div>
										<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">RPM / TPM</p>
										<p class="text-xs font-mono text-zinc-700 dark:text-zinc-300">{fmtLimit(ch.rpm_limit)} / {fmtLimit(ch.tpm_limit)}</p>
									</div>
									<div>
										<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">超时 / 重试</p>
										<p class="text-xs font-mono text-zinc-700 dark:text-zinc-300">{ch.timeout_ms ?? 60000}ms / {ch.max_retries ?? 2}x</p>
									</div>
									<div>
										<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1">创建时间</p>
										<p class="text-xs text-zinc-700 dark:text-zinc-300">{fmtDate(ch.created_at)}</p>
									</div>
								</div>
								{#if ch.tags && ch.tags.length > 0}
									<div class="mt-4">
										<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1.5">标签</p>
										<div class="flex flex-wrap gap-1.5">
											{#each ch.tags as tag}
												<span class="px-2 py-0.5 bg-zinc-200/60 dark:bg-zinc-700 text-zinc-700 dark:text-zinc-300 rounded-md text-xs">{tag}</span>
											{/each}
										</div>
									</div>
								{/if}
								{#if ch.supported_models && ch.supported_models.length > 2}
									<div class="mt-4">
										<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1.5">全部模型 ({ch.supported_models.length})</p>
										<div class="flex flex-wrap gap-1.5">
											{#each ch.supported_models as m}
												<span class="px-2 py-0.5 bg-zinc-200/60 dark:bg-zinc-700 text-zinc-700 dark:text-zinc-300 rounded-md text-xs font-mono">{m}</span>
											{/each}
										</div>
									</div>
								{/if}
								{#if ch.last_error}
									<div class="mt-4 p-3 rounded-lg bg-red-50 dark:bg-red-900/10 border border-red-200/50 dark:border-red-800/30">
										<p class="text-[10px] font-semibold uppercase tracking-widest text-red-500 dark:text-red-400 mb-1">最近错误</p>
										<p class="text-xs text-red-700 dark:text-red-400 font-mono">{ch.last_error}</p>
										<p class="text-[10px] text-red-400 dark:text-red-500 mt-1">{fmtDate(ch.last_error_at)}</p>
									</div>
								{/if}
							</td>
						</tr>
					{/if}
				{/each}
			</DataTable>

			<!-- Pagination -->
			{#if totalPages > 1}
				<div class={dataTemplate.pagination}>
					<p class="text-xs text-zinc-500 dark:text-zinc-400">
						{(page - 1) * pageSize + 1}–{Math.min(page * pageSize, total)} / {total}
					</p>
					<div class="flex items-center gap-1">
						<button
							type="button"
							disabled={page <= 1}
							onclick={() => goPage(page - 1)}
							class="p-2 rounded-md text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
						>
							<ChevronLeft size={16} />
						</button>
						{#each pageNumbers(page, totalPages) as p}
							{#if p === '...'}
								<span class="w-8 h-8 flex items-center justify-center text-xs text-zinc-400">...</span>
							{:else}
								<button
									type="button"
									onclick={() => goPage(p as number)}
									class="w-8 h-8 rounded-md text-xs font-medium transition-colors {p === page
										? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900'
										: 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
								>{p}</button>
							{/if}
						{/each}
						<button
							type="button"
							disabled={page >= totalPages}
							onclick={() => goPage(page + 1)}
							class="p-2 rounded-md text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
						>
							<ChevronRight size={16} />
						</button>
					</div>
				</div>
			{/if}
		</div>
		<!-- Keyboard hint -->
		<p class="text-[10px] text-zinc-400 dark:text-zinc-600 mt-2 text-center shrink-0">
			<kbd class="px-1 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 font-mono">j</kbd>/<kbd class="px-1 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 font-mono">k</kbd> 导航
			<kbd class="px-1 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 font-mono ml-2">Enter</kbd> 展开
			<kbd class="px-1 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 font-mono ml-2">e</kbd> 编辑
			<kbd class="px-1 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 font-mono ml-2">t</kbd> 测试
			<kbd class="px-1 py-0.5 rounded bg-zinc-100 dark:bg-zinc-800 font-mono ml-2">Esc</kbd> 关闭
		</p>
	{/if}
</PageShell>
