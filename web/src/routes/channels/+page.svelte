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
	import Pagination from '$lib/components/ui/Pagination.svelte';
	import ProbeModal from './_components/ProbeModal.svelte';
	import DeleteConfirmModal from './_components/DeleteConfirmModal.svelte';
	import BatchConfirmModal from './_components/BatchConfirmModal.svelte';
	import EditChannelDrawer from './_components/EditChannelDrawer.svelte';
	import CreateChannelDrawer from './_components/CreateChannelDrawer.svelte';
	import ChannelTable from './_components/ChannelTable.svelte';
	import {
		PROVIDER_OPTIONS as HELPER_PROVIDER_OPTIONS,
		FILTER_PROVIDER_OPTIONS as HELPER_FILTER_PROVIDER_OPTIONS,
		STATUS_OPTIONS as HELPER_STATUS_OPTIONS,
		HEALTH_OPTIONS as HELPER_HEALTH_OPTIONS,
		isPluginProvider,
		capabilityFallback,
		capabilityTitle,
		capabilityChipClass,
		pluginAuthSlotSummary,
		fmtLimit,
		fmtDate,
		healthBadgeCls,
		statusBadgeCls,
		healthDot,
	} from './_lib/helpers';
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

	const PROVIDER_OPTIONS = HELPER_PROVIDER_OPTIONS;
	const FILTER_PROVIDER_OPTIONS = HELPER_FILTER_PROVIDER_OPTIONS;
	const STATUS_OPTIONS = HELPER_STATUS_OPTIONS;
	const HEALTH_OPTIONS = HELPER_HEALTH_OPTIONS;

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

<!-- Modal: Probe result + Probing spinner -->
<ProbeModal
	{probingId}
	{probeResult}
	{probeChannelName}
	{syncingProbe}
	onClose={() => { probeResult = null; probingId = null; }}
	onSync={handleSyncModels}
/>

<!-- Modal: Delete confirm -->
{#if deletingId}
	<DeleteConfirmModal
		{deletingId}
		{channels}
		{deleting}
		bind:deleteConfirmation
		onClose={() => { deletingId = ''; deleteConfirmation = ''; }}
		onConfirm={handleDelete}
		updateConfirmation={(val) => (deleteConfirmation = val)}
	/>
{/if}

<!-- Modal: Batch confirm -->
<BatchConfirmModal
	{batchAction}
	selectedCount={selectedIds.size}
	{batchProcessing}
	onClose={() => (batchAction = null)}
	onConfirm={executeBatch}
/>

<!-- Drawer: Create -->
<CreateChannelDrawer
	bind:showCreate
	bind:createForm
	bind:pluginBuilderDraft
	bind:pluginBuilderStep
	bind:pluginManifestInput
	bind:modelsInput
	bind:tagsInput
	bind:createInitialKeyAlias
	bind:createInitialKeySecret
	bind:createAutoProbe
	bind:createReplayInput
	{pluginBuilderSuggestions}
	{createReplayOutput}
	{createReplayError}
	{createReplaying}
	{creating}
	{createError}
	{createProviderCaps}
	{createMissingCaps}
	{createGroups}
	{loadingCreateGroups}
	pluginManifestExample={PLUGIN_MANIFEST_EXAMPLE}
	privatePluginManifestExample={PRIVATE_PLUGIN_MANIFEST_EXAMPLE}
	pluginReplaySample={PLUGIN_REPLAY_SAMPLE}
	onClose={() => (showCreate = false)}
	onSubmit={handleCreate}
	onPresetChange={handleCreatePresetChange}
	onBuilderPresetChange={handleBuilderPresetChange}
	onChooseBuilderPath={chooseBuilderPath}
	onRefreshBuilderSuggestions={refreshBuilderSuggestions}
	onUpdateBuilderManifestPreview={updateBuilderManifestPreview}
	onLintManifest={lintCreatePluginManifest}
	onReplayManifest={replayCreatePluginManifest}
/>

<!-- Drawer: Edit -->
<EditChannelDrawer
	bind:editingChannel
	bind:editForm
	bind:editAuthForm
	bind:editPluginPreset
	bind:editPluginManifestInput
	bind:editReplayInput
	bind:editModelsInput
	bind:editTagsInput
	{editReplayOutput}
	{editReplayError}
	{editReplaying}
	{editing}
	{editError}
	{editProviderCaps}
	{editMissingCaps}
	{probingId}
	pluginManifestExample={PLUGIN_MANIFEST_EXAMPLE}
	privatePluginManifestExample={PRIVATE_PLUGIN_MANIFEST_EXAMPLE}
	pluginReplaySample={PLUGIN_REPLAY_SAMPLE}
	onClose={() => (editingChannel = null)}
	onSubmit={handleEdit}
	onProbe={handleProbe}
	onPresetChange={handleEditPresetChange}
	onLintManifest={lintEditPluginManifest}
	onReplayManifest={replayEditPluginManifest}
/>

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
			<ChannelTable
				{channels}
				{testResults}
				{testingIds}
				{expandedId}
				{focusedIdx}
				{isPlatformAdmin}
				{drainStatuses}
				{disablingIdleIds}
				{selectedIds}
				{selectAll}
				{sortBy}
				{sortDir}
				actions={{
					onToggleEnabled: handleToggleEnabled,
					onDisableWhenIdle: handleDisableWhenIdle,
					onRefreshDrainStatus: refreshDrainStatus,
					onSelectChannel: channelSelectChange,
					onSelectAll: selectAllChange,
					onSort,
					onToggleExpand: (id) => (expandedId = expandedId === id ? null : id),
					onFocus: (idx) => (focusedIdx = idx),
					getMenuItems,
					sortIconClass,
				}}
			/>

			<!-- Pagination -->
			<Pagination {page} {pageSize} {total} {totalPages} onGoPage={goPage} {pageNumbers} />
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
