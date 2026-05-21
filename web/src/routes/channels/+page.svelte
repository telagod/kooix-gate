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
		createChannelKey,
		updateChannel,
		deleteChannel,
		batchEnableChannels,
		batchDisableChannels,
		batchDeleteChannels,
		drainChannel,
		getChannelDrainStatus,
		disableChannelWhenIdle,
		probeChannel,
		testChannel,
		getChannelBalance,
		replayPluginSse,
		listGroups,
		addGroupBinding
	} from '$lib/api.js';
	import type {
		Channel,
		ChannelGroup,
		CreateChannelRequest,
		UpdateChannelRequest,
		TestResponse,
		ProbeResponse,
		BalanceResponse
	} from '$lib/api.js';
	import { Alert, Badge, Button, Card, Field, FilterPills, Input, ProviderSelect } from '$lib/components/ui';
	import PluginAuthEditor from '$lib/components/channels/PluginAuthEditor.svelte';
	import type { ProviderOption } from '$lib/components/ui/ProviderSelect.svelte';
	import DropdownMenu from '$lib/components/ui/DropdownMenu.svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import {
		PLUGIN_PRESET_OPTIONS,
		CAPABILITY_LABELS,
		authFormFromManifest,
		buildPluginBuilderManifest,
		capabilityList,
		defaultPluginBuilderDraft,
		defaultPluginAuthForPreset,
		manifestPreset,
		missingCapabilityList,
		pluginCapabilitiesForPreset,
		pluginPresetBaseUrlSuggestion,
		providerBaseUrlSuggestion,
		providerCapabilities,
		selectedPluginMapping,
		suggestResponsePaths
	} from '$lib/plugin-presets';
	import type { PluginAuthForm, PluginBuilderDraft, PluginResponsePathSuggestion, ProviderCapabilities, ProviderCapabilityKey } from '$lib/plugin-presets';
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
		Zap,
		CirclePause,
		ShieldCheck
	} from 'lucide-svelte';

	const PROVIDER_OPTIONS: ProviderOption[] = [
		{ value: 'openai', label: 'OpenAI', description: 'GPT-4o / o1 / o3' },
		{ value: 'anthropic', label: 'Anthropic', description: 'Claude 4 / Sonnet / Haiku' },
		{ value: 'gemini', label: 'Google Gemini', description: 'Gemini 2.5 Pro / Flash' },
		{ value: 'azure', label: 'Azure OpenAI', description: 'Azure 托管 GPT 部署' },
		{ value: 'vertex', label: 'Google Vertex AI', description: 'Vertex AI OpenAI endpoint' },
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
		{ value: 'draining', label: 'Draining' },
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
	let drainStatuses = $state<Record<string, { inflight: number; safe_to_disable: boolean }>>({});
	let drainingIds = $state<Set<string>>(new Set());
	let disablingIdleIds = $state<Set<string>>(new Set());

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
	let pluginPreset = $state('');
	let createAuthForm = $state<PluginAuthForm>(defaultPluginAuthForPreset(''));
	let lastCreatePluginPreset = $state('');
	let createReplayInput = $state('');
	let createReplayOutput = $state('');
	let createReplayError = $state('');
	let createReplaying = $state(false);
	let pluginBuilderStep = $state(1);
	let pluginBuilderDraft = $state<PluginBuilderDraft>(defaultPluginBuilderDraft(''));
	let pluginBuilderSuggestions = $state<PluginResponsePathSuggestion[]>([]);
	let createGroups = $state<ChannelGroup[]>([]);
	let loadingCreateGroups = $state(false);
	let createInitialKeyAlias = $state('primary');
	let lastSyncedCreateInitialKeyAlias = $state('primary');
	let createInitialKeySecret = $state('');
	let createAutoProbe = $state(true);

	let editingChannel = $state<Channel | null>(null);
	let editForm = $state<UpdateChannelRequest>({});
	let editing = $state(false);
	let editError = $state('');
	let editModelsInput = $state('');
	let editTagsInput = $state('');
	let editPluginManifestInput = $state('');
	let editPluginPreset = $state('');
	let editAuthForm = $state<PluginAuthForm>(defaultPluginAuthForPreset(''));
	let lastEditPluginPreset = $state('');
	let editReplayInput = $state('');
	let editReplayOutput = $state('');
	let editReplayError = $state('');
	let editReplaying = $state(false);

	let deletingId = $state<string | null>(null);
	let deleting = $state(false);
	let deleteConfirmation = $state('');

	let batchAction = $state<'enable' | 'disable' | 'delete' | null>(null);
	let batchProcessing = $state(false);

	let toast = $state('');
	let toastType = $state<'ok' | 'err'>('ok');

	let totalPages = $derived(Math.max(1, Math.ceil(total / pageSize)));
	let createProviderCaps = $derived(
		isPluginProvider(createForm.provider_type)
			? pluginCapabilitiesForPreset(pluginBuilderDraft.preset)
			: providerCapabilities(createForm.provider_type)
	);
	let createMissingCaps = $derived(missingCapabilityList(createProviderCaps, ['embeddings', 'image', 'audio', 'batch']));
	let editProviderCaps = $derived<ProviderCapabilities | null>(
		editingChannel
			? (isPluginProvider(editingChannel.provider_type)
					? pluginCapabilitiesForPreset(editPluginPreset || manifestPreset(editingChannel.model_mapping))
					: providerCapabilities(editingChannel.provider_type))
			: null
	);
	let editMissingCaps = $derived(missingCapabilityList(editProviderCaps ?? undefined, ['embeddings', 'image', 'audio', 'batch']));

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

	let prevCreateProvider = $state('openai');
	$effect(() => {
		if (createForm.provider_type !== prevCreateProvider) {
			prevCreateProvider = createForm.provider_type;
			if (!createForm.base_url) applyBaseUrlSuggestion('create');
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

	async function handleDrain(ch: Channel) {
		drainingIds = new Set([...drainingIds, ch.id]);
		try {
			const result = await drainChannel(ch.id);
			channels = channels.map(c => c.id === result.channel.id ? result.channel : c);
			drainStatuses = { ...drainStatuses, [result.channel.id]: { inflight: result.inflight, safe_to_disable: result.safe_to_disable } };
			showToast(result.safe_to_disable ? '已进入 Draining，可安全禁用' : `已进入 Draining，等待 ${result.inflight} 个 inflight`);
		} catch (err: any) {
			showToast(err?.message ?? 'Drain 失败', 'err');
		} finally {
			drainingIds = new Set([...drainingIds].filter(id => id !== ch.id));
		}
	}

	async function refreshDrainStatus(ch: Channel) {
		try {
			const result = await getChannelDrainStatus(ch.id);
			channels = channels.map(c => c.id === result.channel.id ? result.channel : c);
			drainStatuses = { ...drainStatuses, [result.channel.id]: { inflight: result.inflight, safe_to_disable: result.safe_to_disable } };
			showToast(result.safe_to_disable ? 'Inflight 已清空' : `仍有 ${result.inflight} 个 inflight`);
		} catch (err: any) {
			showToast(err?.message ?? '刷新 drain 状态失败', 'err');
		}
	}

	async function handleDisableWhenIdle(ch: Channel) {
		disablingIdleIds = new Set([...disablingIdleIds, ch.id]);
		try {
			const result = await disableChannelWhenIdle(ch.id);
			channels = channels.map(c => c.id === result.channel.id ? result.channel : c);
			drainStatuses = { ...drainStatuses, [result.channel.id]: { inflight: result.inflight, safe_to_disable: result.safe_to_disable } };
			showToast('Inflight 已清空，Channel 已禁用');
		} catch (err: any) {
			showToast(err?.message ?? '仍有 inflight，暂不能禁用', 'err');
			await refreshDrainStatus(ch);
		} finally {
			disablingIdleIds = new Set([...disablingIdleIds].filter(id => id !== ch.id));
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
    "preset": { "provider": "openai_compatible" }
  }
}`;

	const PRIVATE_PLUGIN_MANIFEST_EXAMPLE = `{
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
      "ignore_events": ["ping"],
      "done_events": ["close"],
      "content_path": "token",
      "tool_calls_path": "tool_calls",
      "finish_reason_path": "finish",
      "done": ["[DONE]", "EOF"],
      "done_path": "type",
      "done_values": ["message_stop"],
      "usage": {
        "prompt_tokens_path": "usage.input",
        "completion_tokens_path": "usage.output"
      }
    }
  }
}`;

	const PLUGIN_REPLAY_SAMPLE = `event: token
data: {"payload":{"rid":"r1","model_name":"native","speaker":"assistant"}}

data: {"payload":{"token":"he"}}

data: {"payload":{"token":"llo"}}

data: {"payload":{"finish":"done","usage":{"input":3,"output":2}}}

data: {"payload":{"type":"message_stop"}}
`;

	const RESPONSE_SAMPLE_PLACEHOLDER = `{"result":{"text":"hello"},"usage":{"input":1,"output":2}}`;
	const PROBE_BODY_PLACEHOLDER = `{"model":"{{model}}","messages":[{"role":"user","content":"Hi"}]}`;

	const PLUGIN_BUILDER_STEPS = [
		'Preset',
		'Auth',
		'Request',
		'Response',
		'SSE',
		'Test',
		'Save'
	];

	function isPluginProvider(providerType: string | undefined): boolean {
		return ['plugin', 'custom', 'http', 'http_plugin'].includes(providerType ?? '');
	}

	function capabilityFallback(providerType: string, caps: ProviderCapabilities | undefined): ProviderCapabilities {
		if (caps) return caps;
		return providerCapabilities(providerType);
	}

	function capabilityTitle(caps: ProviderCapabilities | undefined): string {
		const active = capabilityList(caps);
		return active.length > 0 ? active.map(key => CAPABILITY_LABELS[key]).join(', ') : 'No capability declared';
	}

	function capabilityChipClass(key: ProviderCapabilityKey): string {
		if (key === 'image' || key === 'audio' || key === 'batch') {
			return 'bg-amber-50 text-amber-700 ring-amber-600/20 dark:bg-amber-500/10 dark:text-amber-400 dark:ring-amber-400/20';
		}
		return 'bg-zinc-100 text-zinc-700 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-300 dark:ring-zinc-700';
	}

	function applyBaseUrlSuggestion(kind: 'create' | 'edit') {
		if (kind === 'create') {
			const suggestion = isPluginProvider(createForm.provider_type)
				? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset)
				: providerBaseUrlSuggestion(createForm.provider_type);
			if (suggestion && !createForm.base_url) createForm.base_url = suggestion;
			return;
		}
		if (!editingChannel) return;
		const suggestion = isPluginProvider(editingChannel.provider_type)
			? pluginPresetBaseUrlSuggestion(editPluginPreset)
			: providerBaseUrlSuggestion(editingChannel.provider_type);
		if (suggestion && !editForm.base_url) editForm.base_url = suggestion;
	}

	function syncBuilderToCreateForm() {
		pluginPreset = pluginBuilderDraft.preset;
		createAuthForm = pluginBuilderDraft.auth;
		lastCreatePluginPreset = pluginPreset;
		pluginManifestInput = JSON.stringify(buildPluginBuilderManifest(pluginBuilderDraft), null, 2);
		createReplayInput = pluginBuilderDraft.raw_sse;
	}

	function syncCreateFormToBuilder() {
		pluginPreset = pluginBuilderDraft.preset;
		createAuthForm = pluginBuilderDraft.auth;
		lastCreatePluginPreset = pluginPreset;
		pluginBuilderDraft = {
			...pluginBuilderDraft,
			raw_sse: createReplayInput
		};
	}

	function handleBuilderPresetChange(event: Event) {
		const preset = (event.currentTarget as HTMLSelectElement).value;
		pluginBuilderDraft = {
			...defaultPluginBuilderDraft(preset),
			target_group_id: pluginBuilderDraft.target_group_id
		};
		syncInitialKeyAliasFromAuth();
		if (pluginPresetBaseUrlSuggestion(preset)) createForm.base_url = createForm.base_url || pluginPresetBaseUrlSuggestion(preset);
		pluginBuilderStep = 2;
		syncBuilderToCreateForm();
	}

	function updateBuilderManifestPreview() {
		try {
			syncInitialKeyAliasFromAuth();
			syncBuilderToCreateForm();
			createError = '';
		} catch (err: any) {
			createError = err?.message ?? 'Builder manifest 生成失败';
		}
	}

	function refreshBuilderSuggestions() {
		try {
			pluginBuilderSuggestions = suggestResponsePaths(pluginBuilderDraft.response_sample);
			updateBuilderManifestPreview();
		} catch (err: any) {
			pluginBuilderSuggestions = [];
			createError = err?.message ?? 'Response sample 解析失败';
		}
	}

	function chooseBuilderPath(kind: 'content' | 'finish' | 'prompt' | 'completion' | 'total', path: string) {
		if (kind === 'content') pluginBuilderDraft.response_content_path = path;
		if (kind === 'finish') pluginBuilderDraft.response_finish_reason_path = path;
		if (kind === 'prompt') pluginBuilderDraft.response_prompt_tokens_path = path;
		if (kind === 'completion') pluginBuilderDraft.response_completion_tokens_path = path;
		if (kind === 'total') pluginBuilderDraft.response_total_tokens_path = path;
		updateBuilderManifestPreview();
	}

	async function loadCreateGroups() {
		if (!isPlatformAdmin || createGroups.length > 0 || loadingCreateGroups) return;
		loadingCreateGroups = true;
		try {
			createGroups = await listGroups();
		} catch (err: any) {
			showToast(err?.message ?? '加载 Group 失败', 'err');
		} finally {
			loadingCreateGroups = false;
		}
	}

	function openCreateDrawer() {
		showCreate = true;
		applyBaseUrlSuggestion('create');
		loadCreateGroups();
	}

	function syncCreateAuthPreset() {
		if (pluginPreset === lastCreatePluginPreset) return;
		createAuthForm = defaultPluginAuthForPreset(pluginPreset);
		lastCreatePluginPreset = pluginPreset;
	}

	function syncEditAuthPreset() {
		if (editPluginPreset === lastEditPluginPreset) return;
		editAuthForm = defaultPluginAuthForPreset(editPluginPreset);
		lastEditPluginPreset = editPluginPreset;
	}

	function handleCreatePresetChange(event: Event) {
		pluginPreset = (event.currentTarget as HTMLSelectElement).value;
		createAuthForm = defaultPluginAuthForPreset(pluginPreset);
		lastCreatePluginPreset = pluginPreset;
		pluginBuilderDraft = { ...pluginBuilderDraft, preset: pluginPreset, auth: createAuthForm };
		syncInitialKeyAliasFromAuth();
		applyBaseUrlSuggestion('create');
	}

	function handleEditPresetChange(event: Event) {
		editPluginPreset = (event.currentTarget as HTMLSelectElement).value;
		editAuthForm = defaultPluginAuthForPreset(editPluginPreset);
		lastEditPluginPreset = editPluginPreset;
		applyBaseUrlSuggestion('edit');
	}

	function lintCreatePluginManifest() {
		try {
			syncInitialKeyAliasFromAuth();
			syncCreateAuthPreset();
			selectedPluginMapping(pluginPreset, pluginManifestInput, createAuthForm);
			createError = '';
			showToast('Plugin manifest 本地 lint 通过');
		} catch (err: any) {
			createError = err?.message ?? 'Plugin manifest lint 失败';
		}
	}

	function lintEditPluginManifest() {
		try {
			syncEditAuthPreset();
			selectedPluginMapping(editPluginPreset, editPluginManifestInput, editAuthForm);
			editError = '';
			showToast('Plugin manifest 本地 lint 通过');
		} catch (err: any) {
			editError = err?.message ?? 'Plugin manifest lint 失败';
		}
	}

	async function replayCreatePluginManifest() {
		if (!createReplayInput.trim()) {
			createReplayInput = PLUGIN_REPLAY_SAMPLE;
		}
		createReplaying = true;
		createReplayError = '';
		createReplayOutput = '';
		try {
			syncCreateFormToBuilder();
			const manifest = pluginManifestInput.trim()
				? selectedPluginMapping(pluginPreset, pluginManifestInput, createAuthForm)
				: buildPluginBuilderManifest(pluginBuilderDraft);
			const result = await replayPluginSse({
				manifest,
				raw_sse: createReplayInput,
				base_url: createForm.base_url || 'https://example.com',
				model: createForm.supported_models?.[0] ?? modelsInput.split(',').map(s => s.trim()).find(Boolean) ?? 'replay-model'
			});
			createReplayOutput = JSON.stringify(result.chunks, null, 2);
			pluginBuilderDraft.raw_sse = createReplayInput;
			pluginBuilderStep = Math.max(pluginBuilderStep, 6);
			showToast(`SSE replay 完成：${result.chunks.length} chunks`);
		} catch (err: any) {
			createReplayError = err?.message ?? 'SSE replay 失败';
		} finally {
			createReplaying = false;
		}
	}

	async function replayEditPluginManifest() {
		if (!editReplayInput.trim()) {
			editReplayInput = PLUGIN_REPLAY_SAMPLE;
		}
		editReplaying = true;
		editReplayError = '';
		editReplayOutput = '';
		try {
			if (!editingChannel) return;
			syncEditAuthPreset();
			const manifest = selectedPluginMapping(editPluginPreset, editPluginManifestInput, editAuthForm);
			const result = await replayPluginSse({
				manifest,
				raw_sse: editReplayInput,
				base_url: editForm.base_url || editingChannel.base_url || 'https://example.com',
				model: editForm.supported_models?.[0] ?? editModelsInput.split(',').map(s => s.trim()).find(Boolean) ?? 'replay-model'
			});
			editReplayOutput = JSON.stringify(result.chunks, null, 2);
			showToast(`SSE replay 完成：${result.chunks.length} chunks`);
		} catch (err: any) {
			editReplayError = err?.message ?? 'SSE replay 失败';
		} finally {
			editReplaying = false;
		}
	}

	function pluginAuthSlotSummary(form: PluginAuthForm): string {
		switch (form.strategy) {
			case 'bearer':
			case 'api_key_header':
			case 'api_key_query':
			case 'hmac':
				return form.secret_slot.trim() || 'primary';
			case 'basic':
				return `${form.username_slot.trim() || 'username'} / ${form.password_slot.trim() || 'primary'}`;
			case 'aws_sigv4':
				return [form.aws_access_key_slot.trim() || 'primary', form.aws_secret_key_slot.trim() || 'aws_secret_key', form.aws_session_token_slot.trim()]
					.filter(Boolean)
					.join(' / ');
			case 'oauth_client_credentials':
				return `${form.oauth_client_id_slot.trim() || 'client_id'} / ${form.oauth_client_secret_slot.trim() || 'client_secret'}`;
			case 'custom_headers':
				return '按 Headers JSON 内的 {{slot}} 引用';
			case 'none':
				return '无认证 slot';
			default:
				return 'primary';
		}
	}

	function preferredCreateKeyAlias(): string {
		const form = pluginBuilderDraft.auth;
		switch (form.strategy) {
			case 'bearer':
			case 'api_key_header':
			case 'api_key_query':
			case 'hmac':
				return form.secret_slot.trim() || 'primary';
			case 'basic':
				return form.password_slot.trim() || 'primary';
			case 'aws_sigv4':
				return form.aws_secret_key_slot.trim() || 'aws_secret_key';
			case 'oauth_client_credentials':
				return form.oauth_client_secret_slot.trim() || 'client_secret';
			default:
				return createInitialKeyAlias.trim() || 'primary';
		}
	}

	function syncInitialKeyAliasFromAuth() {
		const alias = preferredCreateKeyAlias();
		if (!alias) return;
		if (!createInitialKeyAlias.trim() || createInitialKeyAlias === lastSyncedCreateInitialKeyAlias) {
			createInitialKeyAlias = alias;
			lastSyncedCreateInitialKeyAlias = alias;
		}
	}

	function resetCreateForm() {
		createForm = { code: '', provider_type: 'openai', base_url: '', supported_models: [], rpm_limit: null, tpm_limit: null, timeout_ms: 60000, max_retries: 2, tags: [], model_mapping: {} };
		modelsInput = '';
		tagsInput = '';
		pluginManifestInput = '';
		pluginPreset = '';
		createAuthForm = defaultPluginAuthForPreset('');
		lastCreatePluginPreset = '';
		createReplayInput = '';
		createReplayOutput = '';
		createReplayError = '';
		createInitialKeyAlias = 'primary';
		lastSyncedCreateInitialKeyAlias = 'primary';
		createInitialKeySecret = '';
		createAutoProbe = true;
		pluginBuilderStep = 1;
		pluginBuilderDraft = defaultPluginBuilderDraft('');
		pluginBuilderSuggestions = [];
	}

	// ── Create ───────────────────────────────────────
	async function handleCreate(e: SubmitEvent) {
		e.preventDefault();
		if (!createForm.code.trim() || !createForm.base_url.trim()) return;
		creating = true;
		createError = '';
		try {
			if (isPluginProvider(createForm.provider_type)) {
				syncCreateFormToBuilder();
				pluginManifestInput = JSON.stringify(buildPluginBuilderManifest(pluginBuilderDraft), null, 2);
				pluginPreset = pluginBuilderDraft.preset;
				createAuthForm = pluginBuilderDraft.auth;
				lastCreatePluginPreset = pluginPreset;
			} else {
				syncCreateAuthPreset();
			}
			const models = modelsInput.split(',').map(s => s.trim()).filter(Boolean);
			const tags = tagsInput.split(',').map(s => s.trim()).filter(Boolean);
			const pluginChannel = isPluginProvider(createForm.provider_type);
			const model_mapping = pluginChannel
				? selectedPluginMapping(pluginPreset, pluginManifestInput, createAuthForm)
				: createForm.model_mapping;
			const created = await createChannel({ ...createForm, supported_models: models, tags, model_mapping });
			const joinedGroup = pluginChannel && !!pluginBuilderDraft.target_group_id;
			const shouldAutoProbe = pluginChannel && createAutoProbe;
			let keyCreated = false;
			let warningToast = false;
			let keySummary = '';
			let autoProbeSummary = '';
			if (pluginChannel && createInitialKeySecret.trim()) {
				try {
					await createChannelKey(created.id, createInitialKeySecret, createInitialKeyAlias.trim() || preferredCreateKeyAlias());
					keyCreated = true;
					keySummary = '，初始 Key 已写入';
				} catch (err: any) {
					warningToast = true;
					keySummary = `，初始 Key 写入失败：${err?.message ?? 'unknown error'}`;
				}
			}
			if (pluginChannel && pluginBuilderDraft.target_group_id) {
				await addGroupBinding(pluginBuilderDraft.target_group_id, created.id, 1, 1);
			}
			if (shouldAutoProbe) {
				try {
					const result = await probeChannel(created.id);
					autoProbeSummary = `，Probe 发现 ${result.models.length} 个模型`;
					if (result.models.length > 0 && models.length === 0) {
						const updated = await updateChannel(created.id, { supported_models: result.models });
						channels = channels.map(c => c.id === updated.id ? updated : c);
						autoProbeSummary += '并已同步';
					}
				} catch (err: any) {
					warningToast = true;
					autoProbeSummary = `，${keyCreated ? '但 ' : '未填初始 Key，'}Probe 失败：${err?.message ?? 'unknown error'}`;
				}
			}
			showCreate = false;
			resetCreateForm();
			showToast(`${joinedGroup ? 'Channel 创建成功并已加入 Group' : 'Channel 创建成功'}${keySummary}${autoProbeSummary}`, warningToast ? 'err' : 'ok');
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
		editPluginPreset = isPluginProvider(ch.provider_type) ? manifestPreset(ch.model_mapping) : '';
		lastEditPluginPreset = editPluginPreset;
		editAuthForm = isPluginProvider(ch.provider_type) ? authFormFromManifest(ch.model_mapping) : defaultPluginAuthForPreset('');
		editPluginManifestInput = isPluginProvider(ch.provider_type) ? JSON.stringify(ch.model_mapping ?? {}, null, 2) : '';
		editReplayInput = '';
		editReplayOutput = '';
		editReplayError = '';
		editError = '';
	}

	async function handleEdit(e: SubmitEvent) {
		e.preventDefault();
		if (!editingChannel) return;
		editing = true;
		editError = '';
		try {
			syncEditAuthPreset();
			const models = editModelsInput.split(',').map(s => s.trim()).filter(Boolean);
			const tags = editTagsInput.split(',').map(s => s.trim()).filter(Boolean);
			const model_mapping = isPluginProvider(editingChannel.provider_type) ? selectedPluginMapping(editPluginPreset, editPluginManifestInput, editAuthForm) : editForm.model_mapping;
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
			await deleteChannel(deletingId, deleteConfirmation);
			deletingId = null;
			deleteConfirmation = '';
			showToast('Channel 已删除');
			await loadChannels();
		} catch (err: any) {
			showToast(err?.message ?? '删除失败', 'err');
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

	function statusBadgeCls(status: string): string {
		if (status === 'active') return 'bg-green-50 dark:bg-green-500/10 text-green-700 dark:text-green-400 ring-1 ring-green-600/10 dark:ring-green-400/20';
		if (status === 'draining') return 'bg-amber-50 dark:bg-amber-500/10 text-amber-700 dark:text-amber-400 ring-1 ring-amber-600/10 dark:ring-amber-400/20';
		if (status === 'disabled') return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 ring-1 ring-zinc-200 dark:ring-zinc-700';
		return 'bg-red-50 dark:bg-red-500/10 text-red-700 dark:text-red-400 ring-1 ring-red-600/10 dark:ring-red-400/20';
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
		const isDraining = drainingIds.has(ch.id);
		const isDisabling = disablingIdleIds.has(ch.id);
		const items: any[] = [
			{ label: '测试连通性', icon: Zap, disabled: isTesting, onclick: () => handleTest(ch) },
			{ label: 'Probe 模型', icon: Radar, onclick: () => handleProbe(ch) },
			{ label: 'Drain 停新请求', icon: CirclePause, disabled: ch.status === 'draining' || ch.status === 'disabled' || isDraining, onclick: () => handleDrain(ch) },
			{ label: '空闲后禁用', icon: ShieldCheck, disabled: isDisabling, onclick: () => handleDisableWhenIdle(ch) },
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
	{@const deletingChannel = channels.find((ch) => ch.id === deletingId)}
	{@const expectedDeleteConfirmation = `delete:${deletingChannel?.code ?? ''}`}
	<ModalFrame close={() => { deletingId = null; deleteConfirmation = ''; }} class="z-50 bg-black/60 backdrop-blur-sm animate-backdrop">
		<Card class="p-6 max-w-sm w-full mx-4 animate-fade-in shadow-2xl">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">此操作将禁用该 channel 并软删除，无法恢复。请输入确认短语：</p>
			<div class="mb-4 space-y-2">
				<code class="block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expectedDeleteConfirmation}</code>
				<Input id="channel-delete-confirm" bind:value={deleteConfirmation} placeholder={expectedDeleteConfirmation} disabled={deleting} class="font-mono" />
			</div>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => { deletingId = null; deleteConfirmation = ''; }} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting || deleteConfirmation.trim() !== expectedDeleteConfirmation}>
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
							<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
								<div class="mb-2 flex items-center justify-between gap-2">
									<p class="text-xs font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400">Capability</p>
									{#if isPluginProvider(createForm.provider_type) && pluginBuilderDraft.preset}
										<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{pluginBuilderDraft.preset}</span>
									{:else}
										<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{createForm.provider_type}</span>
									{/if}
								</div>
								<div class="flex flex-wrap gap-1.5" title={capabilityTitle(createProviderCaps)}>
									{#each capabilityList(createProviderCaps) as cap}
										<span class="rounded-md px-2 py-0.5 text-xs font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
									{/each}
								</div>
								{#if createMissingCaps.length > 0}
									<p class="mt-2 text-xs text-amber-700 dark:text-amber-400">
										未声明 {createMissingCaps.map(cap => CAPABILITY_LABELS[cap]).join(' / ')}；这些请求不会路由到该 Channel。
									</p>
								{/if}
							</div>
							<div>
								<label for="ch-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL <span class="text-red-500">*</span></label>
								<Input id="ch-url" placeholder={isPluginProvider(createForm.provider_type) ? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset) || 'https://api.example.com/v1' : providerBaseUrlSuggestion(createForm.provider_type) || 'https://api.openai.com/v1'} bind:value={createForm.base_url} disabled={creating} />
								{#if (isPluginProvider(createForm.provider_type) ? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset) : providerBaseUrlSuggestion(createForm.provider_type))}
									<button type="button" class="mt-1 text-xs text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100" onclick={() => { createForm.base_url = isPluginProvider(createForm.provider_type) ? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset) : providerBaseUrlSuggestion(createForm.provider_type); }}>
										使用建议：{isPluginProvider(createForm.provider_type) ? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset) : providerBaseUrlSuggestion(createForm.provider_type)}
									</button>
								{/if}
							</div>
							{#if isPluginProvider(createForm.provider_type)}
								<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
									<div class="mb-4 flex flex-wrap gap-1.5">
										{#each PLUGIN_BUILDER_STEPS as label, index}
											<button type="button" onclick={() => (pluginBuilderStep = index + 1)} class="rounded-full px-2.5 py-1 text-xs font-medium transition-colors {pluginBuilderStep === index + 1 ? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900' : 'bg-white text-zinc-600 ring-1 ring-zinc-200 hover:bg-zinc-100 dark:bg-zinc-900 dark:text-zinc-300 dark:ring-zinc-800'}">
												{index + 1}. {label}
											</button>
										{/each}
									</div>

									{#if pluginBuilderStep === 1}
										<label for="ch-plugin-preset" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">1. 选择 preset 或自定义</label>
										<select id="ch-plugin-preset" bind:value={pluginBuilderDraft.preset} onchange={handleBuilderPresetChange} disabled={creating} class="w-full rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100">
											{#each PLUGIN_PRESET_OPTIONS as opt}
												<option value={opt.value}>{opt.label}</option>
											{/each}
										</select>
										{#if pluginBuilderDraft.preset}
											<div class="mt-3 rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900">
												<p class="mb-2 text-xs font-medium text-zinc-600 dark:text-zinc-300">Preset capability defaults</p>
												<div class="flex flex-wrap gap-1.5">
													{#each capabilityList(pluginCapabilitiesForPreset(pluginBuilderDraft.preset)) as cap}
														<span class="rounded-md px-2 py-0.5 text-xs font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
													{/each}
												</div>
												<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">Base URL 建议：{pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset) || '按上游文档填写'}</p>
											</div>
										{/if}
									{:else if pluginBuilderStep === 2}
										<p class="mb-2 text-sm font-medium text-zinc-700 dark:text-zinc-300">2. 配置 auth</p>
										<PluginAuthEditor bind:form={pluginBuilderDraft.auth} disabled={creating} idPrefix="ch-builder-auth" />
										<Button class="mt-3" size="sm" variant="outline" type="button" onclick={() => { updateBuilderManifestPreview(); pluginBuilderStep = 3; }} disabled={creating}>写入 manifest</Button>
									{:else if pluginBuilderStep === 3}
										<p class="mb-2 text-sm font-medium text-zinc-700 dark:text-zinc-300">3. 配置 request mapping</p>
										<Input placeholder="/private/chat" bind:value={pluginBuilderDraft.request_path} disabled={creating || !!pluginBuilderDraft.preset} />
										<textarea class="mt-2 min-h-36 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" bind:value={pluginBuilderDraft.request_body} disabled={creating || !!pluginBuilderDraft.preset}></textarea>
										<Button class="mt-3" size="sm" variant="outline" type="button" onclick={() => { updateBuilderManifestPreview(); pluginBuilderStep = 4; }} disabled={creating}>生成 request</Button>
									{:else if pluginBuilderStep === 4}
										<p class="mb-2 text-sm font-medium text-zinc-700 dark:text-zinc-300">4. 粘贴 non-stream response sample，点选字段映射</p>
										<textarea class="min-h-32 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={RESPONSE_SAMPLE_PLACEHOLDER} bind:value={pluginBuilderDraft.response_sample} oninput={refreshBuilderSuggestions} disabled={creating}></textarea>
										<div class="mt-2 grid grid-cols-2 gap-2">
											{#each pluginBuilderSuggestions.slice(0, 8) as s}
												<div class="rounded-md border border-zinc-200 bg-white p-2 text-xs dark:border-zinc-800 dark:bg-zinc-900">
													<p class="mb-1 truncate font-mono text-zinc-700 dark:text-zinc-300">{s.path}</p>
													<div class="flex flex-wrap gap-1">
														<button type="button" class="rounded bg-zinc-100 px-1.5 py-0.5 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300" onclick={() => chooseBuilderPath('content', s.path)}>content</button>
														<button type="button" class="rounded bg-zinc-100 px-1.5 py-0.5 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300" onclick={() => chooseBuilderPath('finish', s.path)}>finish</button>
														<button type="button" class="rounded bg-zinc-100 px-1.5 py-0.5 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300" onclick={() => chooseBuilderPath('prompt', s.path)}>prompt</button>
														<button type="button" class="rounded bg-zinc-100 px-1.5 py-0.5 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300" onclick={() => chooseBuilderPath('completion', s.path)}>completion</button>
														<button type="button" class="rounded bg-zinc-100 px-1.5 py-0.5 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-300" onclick={() => chooseBuilderPath('total', s.path)}>total</button>
													</div>
												</div>
											{/each}
										</div>
										<p class="mt-2 text-xs text-zinc-500">已选：content={pluginBuilderDraft.response_content_path || 'auto'} · prompt={pluginBuilderDraft.response_prompt_tokens_path || 'auto'} · completion={pluginBuilderDraft.response_completion_tokens_path || 'auto'}</p>
										<Button class="mt-3" size="sm" variant="outline" type="button" onclick={() => { updateBuilderManifestPreview(); pluginBuilderStep = 5; }} disabled={creating}>生成 response mapping</Button>
									{:else if pluginBuilderStep === 5}
										<div class="mb-2 flex items-center justify-between gap-2">
											<div>
												<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300">5. 粘贴 raw SSE sample，预览 chunks</p>
												<p class="text-xs text-zinc-500 dark:text-zinc-400">未填时使用内置 private SSE 样例。</p>
											</div>
											<Button size="sm" variant="outline" type="button" onclick={replayCreatePluginManifest} disabled={creating || createReplaying}>{createReplaying ? '回放中...' : 'Replay'}</Button>
										</div>
										<textarea class="min-h-36 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={PLUGIN_REPLAY_SAMPLE} bind:value={createReplayInput} disabled={creating || createReplaying}></textarea>
										{#if createReplayError}<p class="mt-2 rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/20 dark:text-red-400">{createReplayError}</p>{/if}
										{#if createReplayOutput}<pre class="mt-2 max-h-56 overflow-auto rounded-md bg-zinc-950 p-3 text-xs text-zinc-100">{createReplayOutput}</pre>{/if}
									{:else if pluginBuilderStep === 6}
										<div class="mb-2 flex items-start justify-between gap-3">
											<div>
												<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300">6. 自动 Probe 参数</p>
												<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">保存时先写入初始 Key，再调用 channel probe；若未填写 key，则依赖环境变量或 none auth，Probe 失败只提示不回滚。</p>
											</div>
											<label class="flex items-center gap-2 text-xs font-medium text-zinc-600 dark:text-zinc-300">
												<input type="checkbox" bind:checked={createAutoProbe} disabled={creating} class="rounded border-zinc-300 text-zinc-900 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:focus:ring-zinc-100" />
												保存后自动 Probe
											</label>
										</div>
										<Input placeholder="/models 或 /health/chat" bind:value={pluginBuilderDraft.probe_path} disabled={creating} />
										<div class="mt-2 grid grid-cols-2 gap-2">
											<Input placeholder="probe model" bind:value={pluginBuilderDraft.probe_model} disabled={creating} />
											<Input placeholder="200,204" bind:value={pluginBuilderDraft.probe_success_status} disabled={creating} />
										</div>
										<Input class="mt-2" placeholder="max_cost_micros，空为不声明成本" bind:value={pluginBuilderDraft.probe_max_cost_micros} disabled={creating} />
										<textarea class="mt-2 min-h-24 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={PROBE_BODY_PLACEHOLDER} bind:value={pluginBuilderDraft.probe_body} disabled={creating}></textarea>
										<div class="mt-3 rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900">
											<p class="mb-2 text-xs font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400">初始 Key（可选）</p>
											<div class="grid grid-cols-[0.8fr_1.2fr] gap-2">
												<Input placeholder="key alias / secret slot" bind:value={createInitialKeyAlias} disabled={creating} />
												<Input type="password" placeholder="只在保存时加密写入，不进入 manifest" bind:value={createInitialKeySecret} disabled={creating} autocomplete="new-password" />
											</div>
											<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">当前 auth 需要 slot：{pluginAuthSlotSummary(pluginBuilderDraft.auth)}。留空则继续使用已存在 channel key 或环境变量，创建流程不会保存明文。</p>
										</div>
										<Button class="mt-3" size="sm" variant="outline" type="button" onclick={() => { updateBuilderManifestPreview(); lintCreatePluginManifest(); pluginBuilderStep = 7; }} disabled={creating}>生成 probe manifest 并 lint</Button>
									{:else}
										<p class="mb-2 text-sm font-medium text-zinc-700 dark:text-zinc-300">7. 保存 channel 并加入 group</p>
										<select bind:value={pluginBuilderDraft.target_group_id} disabled={creating || loadingCreateGroups} class="w-full rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100">
											<option value="">不自动加入 Group</option>
											{#each createGroups as group}
												<option value={group.id}>{group.name} · {group.strategy}</option>
											{/each}
										</select>
										<p class="mt-2 text-xs text-zinc-500">保存成功后会调用 addGroupBinding(group, channel, priority=1, weight=1)。若开启自动 Probe，会在创建后立即按 manifest probe 探活；发现模型且模型列表为空时会自动同步。</p>
									{/if}

									<div class="mt-4">
										<div class="mb-1 flex items-center justify-between gap-2">
											<label for="ch-plugin" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Plugin Manifest Preview</label>
											<Button size="sm" variant="outline" type="button" onclick={lintCreatePluginManifest} disabled={creating}>本地 lint</Button>
										</div>
										<textarea id="ch-plugin" class="min-h-48 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={pluginBuilderDraft.preset ? PLUGIN_MANIFEST_EXAMPLE : PRIVATE_PLUGIN_MANIFEST_EXAMPLE} bind:value={pluginManifestInput} disabled={creating}></textarea>
										<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">Builder 会把 Auth / Request / Response / Probe 合并进 manifest；manifest 只引用 secret slot，不写明文 secret。</p>
									</div>
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
								{#if editingChannel && (isPluginProvider(editingChannel.provider_type) ? pluginPresetBaseUrlSuggestion(editPluginPreset) : providerBaseUrlSuggestion(editingChannel.provider_type))}
									<button type="button" class="mt-1 text-xs text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100" onclick={() => { if (editingChannel) editForm.base_url = isPluginProvider(editingChannel.provider_type) ? pluginPresetBaseUrlSuggestion(editPluginPreset) : providerBaseUrlSuggestion(editingChannel.provider_type); }}>
										使用建议：{isPluginProvider(editingChannel.provider_type) ? pluginPresetBaseUrlSuggestion(editPluginPreset) : providerBaseUrlSuggestion(editingChannel.provider_type)}
									</button>
								{/if}
							</div>
							{#if editProviderCaps}
								<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
									<div class="mb-2 flex items-center justify-between gap-2">
										<p class="text-xs font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400">Capability</p>
										<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{editingChannel.provider_type}</span>
									</div>
									<div class="flex flex-wrap gap-1.5" title={capabilityTitle(editProviderCaps)}>
										{#each capabilityList(editProviderCaps) as cap}
											<span class="rounded-md px-2 py-0.5 text-xs font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
										{/each}
									</div>
									{#if editMissingCaps.length > 0}
										<p class="mt-2 text-xs text-amber-700 dark:text-amber-400">
											未声明 {editMissingCaps.map(cap => CAPABILITY_LABELS[cap]).join(' / ')}；这些请求不会路由到该 Channel。
										</p>
									{/if}
								</div>
							{/if}
							{#if isPluginProvider(editingChannel.provider_type)}
								<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
									<label for="ed-plugin-preset" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Provider 插件预设</label>
										<select id="ed-plugin-preset" bind:value={editPluginPreset} onchange={handleEditPresetChange} disabled={editing} class="mb-3 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100">
											{#each PLUGIN_PRESET_OPTIONS as opt}
												<option value={opt.value}>{opt.label}</option>
											{/each}
										</select>
										<PluginAuthEditor bind:form={editAuthForm} disabled={editing} idPrefix="ed-auth" />
										<div class="mb-1 flex items-center justify-between gap-2">
											<label for="ed-plugin" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Plugin Manifest</label>
											<Button size="sm" variant="outline" type="button" onclick={lintEditPluginManifest} disabled={editing}>本地 lint</Button>
										</div>
										<textarea id="ed-plugin" class="min-h-64 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={editPluginPreset ? PLUGIN_MANIFEST_EXAMPLE : PRIVATE_PLUGIN_MANIFEST_EXAMPLE} bind:value={editPluginManifestInput} disabled={editing || !!editPluginPreset}></textarea>
										<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">保存前会把 Auth Strategy 合并进 manifest 并本地 lint；manifest 只引用 secret slot，不写明文 secret。</p>
										<div class="mt-4 rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900">
											<div class="mb-2 flex items-center justify-between gap-2">
												<div>
													<p class="text-sm font-medium text-zinc-800 dark:text-zinc-200">SSE replay preview</p>
													<p class="text-xs text-zinc-500 dark:text-zinc-400">粘贴 raw SSE，预览归一后的 OpenAI-compatible chunks。</p>
												</div>
												<Button size="sm" variant="outline" type="button" onclick={replayEditPluginManifest} disabled={editing || editReplaying}>{editReplaying ? '回放中...' : 'Replay'}</Button>
											</div>
											<textarea class="min-h-36 w-full rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-950 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={PLUGIN_REPLAY_SAMPLE} bind:value={editReplayInput} disabled={editing || editReplaying}></textarea>
											{#if editReplayError}
												<p class="mt-2 rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/20 dark:text-red-400">{editReplayError}</p>
											{/if}
											{#if editReplayOutput}
												<pre class="mt-2 max-h-56 overflow-auto rounded-md bg-zinc-950 p-3 text-xs text-zinc-100">{editReplayOutput}</pre>
											{/if}
										</div>
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
			<Button size="sm" onclick={openCreateDrawer}>
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
				<Button onclick={openCreateDrawer}>
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
							<div class="space-y-1.5">
								<span class="inline-flex items-center gap-1.5 px-2 py-1 rounded-md bg-zinc-50 dark:bg-zinc-800 text-xs font-mono text-zinc-600 dark:text-zinc-400">
									{ch.provider_type}
								</span>
								<div class="flex max-w-[220px] flex-wrap gap-1" title={capabilityTitle(capabilityFallback(ch.provider_type, ch.capabilities))}>
									{#each capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)).slice(0, 4) as cap}
										<span class="rounded px-1.5 py-0.5 text-[10px] font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
									{/each}
									{#if capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)).length > 4}
										<span class="rounded px-1.5 py-0.5 text-[10px] font-medium bg-zinc-100 text-zinc-500 ring-1 ring-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:ring-zinc-700">+{capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)).length - 4}</span>
									{/if}
								</div>
							</div>
						</td>
						<!-- Status -->
						<td class="px-4 py-4 text-center" onclick={(e: MouseEvent) => e.stopPropagation()}>
							{#if isPlatformAdmin}
								<div class="flex flex-col items-center gap-1.5">
									<button
										type="button"
										onclick={() => handleToggleEnabled(ch)}
										class="relative inline-flex h-5 w-9 items-center rounded-full transition-colors {ch.status === 'active' ? 'bg-green-500' : ch.status === 'draining' ? 'bg-amber-500' : 'bg-zinc-300 dark:bg-zinc-600'}"
										title={ch.status === 'active' ? '点击禁用' : '点击启用'}
									>
										<span class="inline-block h-3.5 w-3.5 transform rounded-full bg-white shadow-sm transition-transform {ch.status === 'active' ? 'translate-x-4.5' : 'translate-x-0.5'}"></span>
									</button>
									{#if ch.status !== 'active'}
										<span class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10px] font-medium {statusBadgeCls(ch.status)}">
											{#if ch.status === 'draining'}<CirclePause size={10} />{/if}
											{ch.status}
										</span>
									{/if}
								</div>
							{:else}
								<span class="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-xs font-medium {statusBadgeCls(ch.status)}">
									{#if ch.status === 'draining'}<CirclePause size={12} />{/if}
									{ch.status}
								</span>
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
								{#if ch.status === 'draining'}
									{@const drainInfo = drainStatuses[ch.id]}
									<div class="mt-4 rounded-lg border border-amber-200/70 bg-amber-50 px-3 py-3 dark:border-amber-800/40 dark:bg-amber-900/10">
										<div class="flex flex-wrap items-center justify-between gap-3">
											<div class="flex items-start gap-2">
												<CirclePause size={16} class="mt-0.5 text-amber-600 dark:text-amber-400" />
												<div>
													<p class="text-sm font-medium text-amber-800 dark:text-amber-300">Draining：已禁止新请求</p>
													<p class="mt-1 text-xs text-amber-700 dark:text-amber-400">
														{drainInfo ? `当前 inflight=${drainInfo.inflight}，${drainInfo.safe_to_disable ? '可安全禁用' : '等待现有请求完成'}` : '点击刷新读取当前 inflight。'}
													</p>
												</div>
											</div>
											{#if isPlatformAdmin}
												<div class="flex gap-2">
													<Button variant="outline" size="sm" onclick={() => refreshDrainStatus(ch)}>刷新</Button>
													<Button size="sm" onclick={() => handleDisableWhenIdle(ch)} disabled={disablingIdleIds.has(ch.id)}>
														{disablingIdleIds.has(ch.id) ? '检查中...' : '空闲后禁用'}
													</Button>
												</div>
											{/if}
										</div>
									</div>
								{/if}
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
								<div class="mt-4">
									<p class="text-[10px] font-semibold uppercase tracking-widest text-zinc-400 dark:text-zinc-500 mb-1.5">Capabilities</p>
									<div class="flex flex-wrap gap-1.5">
										{#each capabilityList(capabilityFallback(ch.provider_type, ch.capabilities)) as cap}
											<span class="px-2 py-0.5 rounded-md text-xs font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
										{/each}
									</div>
								</div>
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
