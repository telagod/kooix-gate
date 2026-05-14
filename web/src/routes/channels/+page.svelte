<!-- /channels — Channel 管理页面：列表 + 创建 + 编辑 + 删除 + 测试 + Probe + 余额 -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import {
		getMe,
		listChannels,
		listAdminChannels,
		createChannel,
		updateChannel,
		deleteChannel,
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
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let channels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');
	let currentOrg = $state<string | null>(null);
	let isPlatformAdmin = $state(false);

	// Per-channel test results
	let testResults = $state<Record<string, TestResponse>>({});
	let testingIds = $state<Set<string>>(new Set());

	// Per-channel balance
	let balances = $state<Record<string, BalanceResponse>>({});
	let loadingBalanceIds = $state<Set<string>>(new Set());

	// Probe modal
	let probeResult = $state<ProbeResponse | null>(null);
	let probeChannelName = $state('');
	let probingId = $state<string | null>(null);
	let syncingProbe = $state(false);

	// Batch test
	let batchTesting = $state(false);
	let batchProgress = $state('');

	// Create modal
	let showCreate = $state(false);
	let createForm = $state<CreateChannelRequest>({ code: '', provider_type: 'openai', base_url: '', supported_models: [] });
	let creating = $state(false);
	let createError = $state('');
	let modelsInput = $state('');

	// Edit modal
	let editingChannel = $state<Channel | null>(null);
	let editForm = $state<UpdateChannelRequest>({});
	let editing = $state(false);
	let editError = $state('');

	// Delete confirm
	let deletingId = $state<string | null>(null);
	let deleting = $state(false);

	// Toast
	let toast = $state('');
	let toastType = $state<'ok' | 'err'>('ok');

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
				channels = await listAdminChannels();
			} else {
				channels = await listChannels(currentOrg!);
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

	// ── Test ──────────────────────────────────────────

	async function handleTest(ch: Channel) {
		testingIds = new Set([...testingIds, ch.id]);
		try {
			const result = await testChannel(ch.id);
			testResults = { ...testResults, [ch.id]: result };
		} catch (err: any) {
			testResults = {
				...testResults,
				[ch.id]: {
					success: false,
					model: '',
					response_time_ms: 0,
					message: null,
					error: err?.message ?? '请求失败'
				}
			};
		} finally {
			testingIds = new Set([...testingIds].filter((id) => id !== ch.id));
		}
	}

	async function handleBatchTest() {
		batchTesting = true;
		const list = [...channels];
		for (let i = 0; i < list.length; i++) {
			const ch = list[i];
			batchProgress = `测试中 ${i + 1}/${list.length}: ${ch.code}`;
			testingIds = new Set([...testingIds, ch.id]);
			try {
				const result = await testChannel(ch.id);
				testResults = { ...testResults, [ch.id]: result };
			} catch (err: any) {
				testResults = {
					...testResults,
					[ch.id]: {
						success: false,
						model: '',
						response_time_ms: 0,
						message: null,
						error: err?.message ?? '请求失败'
					}
				};
			} finally {
				testingIds = new Set([...testingIds].filter((id) => id !== ch.id));
			}
		}
		batchProgress = '';
		batchTesting = false;
		showToast(`批量测试完成，共 ${list.length} 个 channel`);
	}

	// ── Probe ─────────────────────────────────────────

	async function handleProbe(ch: Channel) {
		probingId = ch.id;
		probeResult = null;
		probeChannelName = ch.name || ch.code;
		try {
			const result = await probeChannel(ch.id);
			probeResult = result;
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
			channels = channels.map((c) => (c.id === updated.id ? updated : c));
			showToast(`已同步 ${probeResult.models.length} 个模型到 ${probeChannelName}`);
			probeResult = null;
			probingId = null;
		} catch (err: any) {
			showToast(err?.message ?? '同步失败', 'err');
		} finally {
			syncingProbe = false;
		}
	}

	// ── Balance ───────────────────────────────────────

	async function handleRefreshBalance(ch: Channel) {
		loadingBalanceIds = new Set([...loadingBalanceIds, ch.id]);
		try {
			const result = await getChannelBalance(ch.id);
			balances = { ...balances, [ch.id]: result };
		} catch (err: any) {
			showToast(err?.message ?? '余额查询失败', 'err');
		} finally {
			loadingBalanceIds = new Set([...loadingBalanceIds].filter((id) => id !== ch.id));
		}
	}

	// ── Create ────────────────────────────────────────

	async function handleCreate(e: SubmitEvent) {
		e.preventDefault();
		if (!createForm.code.trim() || !createForm.base_url.trim()) return;
		creating = true;
		createError = '';
		try {
			const models = modelsInput.split(',').map((s) => s.trim()).filter(Boolean);
			const ch = await createChannel({ ...createForm, supported_models: models });
			channels = [...channels, ch];
			showCreate = false;
			createForm = { code: '', provider_type: 'openai', base_url: '', supported_models: [] };
			modelsInput = '';
			showToast('Channel 创建成功');
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	// ── Edit ──────────────────────────────────────────

	function startEdit(ch: Channel) {
		editingChannel = ch;
		editForm = { name: ch.name, base_url: ch.base_url, enabled: ch.status === 'active' };
		editError = '';
	}

	async function handleEdit(e: SubmitEvent) {
		e.preventDefault();
		if (!editingChannel) return;
		editing = true;
		editError = '';
		try {
			const updated = await updateChannel(editingChannel.id, editForm);
			channels = channels.map((c) => (c.id === updated.id ? updated : c));
			editingChannel = null;
			showToast('Channel 更新成功');
		} catch (err: any) {
			editError = err?.message ?? '更新失败';
		} finally {
			editing = false;
		}
	}

	// ── Delete ────────────────────────────────────────

	async function handleDelete() {
		if (!deletingId) return;
		deleting = true;
		try {
			await deleteChannel(deletingId);
			channels = channels.filter((c) => c.id !== deletingId);
			deletingId = null;
			showToast('Channel 已删除');
		} catch (err: any) {
			error = err?.message ?? '删除失败';
			deletingId = null;
		} finally {
			deleting = false;
		}
	}

	// ── Helpers ───────────────────────────────────────

	function statusBadge(status: string): string {
		if (status === 'active') return 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400';
		if (status === 'disabled') return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400';
		return 'bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400';
	}

	function healthBadge(health: string): string {
		if (health === 'healthy') return 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400';
		if (health === 'degraded') return 'bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400';
		if (health === 'unhealthy') return 'bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400';
		return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-400';
	}

	function fmtBalance(b: BalanceResponse): string {
		if (!b.supported) return '不支持';
		if (b.balance_usd == null) return b.message ?? '—';
		return `$${b.balance_usd.toFixed(2)}`;
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
	<div class="fixed inset-0 z-50 bg-black/50 flex items-center justify-center">
		<Card class="p-6 max-w-md w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">
				Probe 结果 — {probeChannelName}
			</h3>
			<p class="text-xs text-zinc-500 dark:text-zinc-400 mb-3 font-mono">{probeResult.provider_type}</p>
			<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-2">
				发现 {probeResult.models.length} 个模型
			</p>
			<div class="max-h-56 overflow-y-auto rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 p-2 space-y-0.5">
				{#each probeResult.models as m}
					<div class="text-xs font-mono text-zinc-700 dark:text-zinc-300 px-1 py-0.5 hover:bg-zinc-100 dark:hover:bg-zinc-700 rounded">{m}</div>
				{/each}
			</div>
			<div class="flex gap-2 justify-end mt-4">
				<Button
					variant="outline"
					type="button"
					onclick={() => {
						probeResult = null;
						probingId = null;
					}}
				>
					关闭
				</Button>
				<Button type="button" disabled={syncingProbe} onclick={handleSyncModels}>
					{syncingProbe ? '同步中...' : '同步到 Channel'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Probing spinner overlay (while waiting for probe result) -->
{#if probingId && !probeResult}
	<div class="fixed inset-0 z-50 bg-black/40 flex items-center justify-center">
		<Card class="p-6 max-w-xs w-full mx-4 flex flex-col items-center gap-3">
			<svg class="animate-spin h-8 w-8 text-zinc-500 dark:text-zinc-400" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
				<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
				<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z"></path>
			</svg>
			<p class="text-sm text-zinc-600 dark:text-zinc-400">正在 Probe {probeChannelName}...</p>
			<Button variant="outline" size="sm" onclick={() => (probingId = null)}>取消</Button>
		</Card>
	</div>
{/if}

<!-- Delete confirmation overlay -->
{#if deletingId}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center">
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-400 mb-4">此操作将禁用该 channel 并软删除，无法恢复。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (deletingId = null)} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Edit modal -->
{#if editingChannel}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">
				编辑 Channel: {editingChannel.code}
			</h3>
			<form onsubmit={handleEdit} class="space-y-3">
				<div>
					<label for="edit-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
					<Input id="edit-name" bind:value={editForm.name} disabled={editing} />
				</div>
				<div>
					<label for="edit-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL</label>
					<Input id="edit-url" bind:value={editForm.base_url} disabled={editing} />
				</div>
				<div class="flex items-center gap-2">
					<input
						type="checkbox"
						id="edit-enabled"
						bind:checked={editForm.enabled}
						disabled={editing}
						class="w-4 h-4 rounded border-zinc-300 dark:border-zinc-600"
					/>
					<label for="edit-enabled" class="text-sm text-zinc-700 dark:text-zinc-300">启用</label>
				</div>
				{#if editError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{editError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={() => (editingChannel = null)}>取消</Button>
					<Button type="submit" disabled={editing}>
						{editing ? '保存中...' : '保存'}
					</Button>
				</div>
			</form>
		</Card>
	</div>
{/if}

<div class="max-w-7xl mx-auto p-6">
	<!-- Header -->
	<div class="flex items-center justify-between mb-1">
		<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">渠道管理</h1>
		<div class="flex items-center gap-2">
			<p class="text-xs text-zinc-400 dark:text-zinc-500 font-mono">{currentOrg ?? '—'}</p>
			{#if isPlatformAdmin}
				<Button size="sm" variant="outline" onclick={handleBatchTest} disabled={batchTesting || loading}>
					{batchTesting ? batchProgress || '测试中...' : '批量测试'}
				</Button>
				<Button size="sm" onclick={() => (showCreate = !showCreate)}>
					{showCreate ? '取消' : '+ 创建 Channel'}
				</Button>
			{/if}
		</div>
	</div>
	<p class="text-sm text-zinc-500 dark:text-zinc-400 mb-6">
		{#if isPlatformAdmin}
			平台管理员可创建、编辑、测试和删除 channel。
		{:else}
			只读视图。编辑需平台管理员权限。
		{/if}
	</p>

	<!-- Create form -->
	{#if showCreate}
		<Card class="p-5 mb-6">
			<h2 class="text-base font-semibold text-zinc-900 dark:text-zinc-100 mb-4">新建 Channel</h2>
			<form onsubmit={handleCreate} class="space-y-3">
				<div class="grid grid-cols-1 md:grid-cols-2 gap-3">
					<div>
						<label for="ch-code" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Code</label>
						<Input id="ch-code" placeholder="openai-prod" bind:value={createForm.code} disabled={creating} />
					</div>
					<div>
						<label for="ch-provider" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Provider</label>
						<select
							id="ch-provider"
							bind:value={createForm.provider_type}
							disabled={creating}
							class="flex h-10 w-full rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-100"
						>
							<option value="openai">OpenAI</option>
							<option value="anthropic">Anthropic</option>
							<option value="gemini">Gemini</option>
							<option value="azure">Azure OpenAI</option>
							<option value="bedrock">AWS Bedrock</option>
							<option value="deepseek">DeepSeek</option>
							<option value="ollama">Ollama (Local)</option>
							<option value="mistral">Mistral</option>
							<option value="cohere">Cohere</option>
						</select>
					</div>
				</div>
				<div>
					<label for="ch-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL</label>
					<Input id="ch-url" placeholder="https://api.openai.com/v1" bind:value={createForm.base_url} disabled={creating} />
				</div>
				<div>
					<label for="ch-models" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">
						支持的模型 <span class="text-zinc-400 font-normal">(逗号分隔，留空=全部)</span>
					</label>
					<Input id="ch-models" placeholder="gpt-4o, gpt-4o-mini, gpt-3.5-turbo" bind:value={modelsInput} disabled={creating} />
				</div>
				{#if createError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{createError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={() => (showCreate = false)}>取消</Button>
					<Button type="submit" disabled={creating}>
						{creating ? '创建中...' : '创建'}
					</Button>
				</div>
			</form>
		</Card>
	{/if}

	<!-- Channel table -->
	{#if loading}
		<p class="text-zinc-500 dark:text-zinc-400">加载中...</p>
	{:else if error}
		<Card class="p-6">
			<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
		</Card>
	{:else if channels.length === 0}
		<Card class="p-6">
			<p class="text-zinc-500 dark:text-zinc-400 text-sm">暂无 channel。请使用上方按钮创建上游连接。</p>
		</Card>
	{:else}
		<div class="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
			<table class="w-full text-sm">
				<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
					<tr>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">Code</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">Provider</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">状态</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">健康度</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">模型</th>
						<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">响应</th>
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
						{@const isLoadingBal = loadingBalanceIds.has(ch.id)}
						<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800/50 transition-colors">
							<!-- Code + name -->
							<td class="px-4 py-3">
								<span class="font-mono text-zinc-900 dark:text-zinc-100">{ch.code}</span>
								{#if ch.name && ch.name !== ch.code}
									<div class="text-xs text-zinc-500 dark:text-zinc-400 mt-0.5">{ch.name}</div>
								{/if}
							</td>

							<!-- Provider -->
							<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400">{ch.provider_type}</td>

							<!-- Status -->
							<td class="px-4 py-3">
								<span class="inline-block px-2 py-0.5 rounded text-xs font-medium {statusBadge(ch.status)}">
									{ch.status}
								</span>
							</td>

							<!-- Health -->
							<td class="px-4 py-3">
								<span class="inline-block px-2 py-0.5 rounded text-xs font-medium {healthBadge(ch.health)}">
									{ch.health}
								</span>
							</td>

							<!-- Models column -->
							<td class="px-4 py-3 max-w-[200px]">
								{#if ch.supported_models && ch.supported_models.length > 0}
									<div class="flex flex-wrap gap-1">
										{#each ch.supported_models.slice(0, 3) as m}
											<span class="inline-block px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 rounded text-[10px] font-mono truncate max-w-[100px]" title={m}>
												{m}
											</span>
										{/each}
										{#if ch.supported_models.length > 3}
											<span class="inline-block px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-800 text-zinc-500 dark:text-zinc-500 rounded text-[10px]">
												+{ch.supported_models.length - 3}
											</span>
										{/if}
									</div>
								{:else}
									<span class="text-xs text-zinc-400 dark:text-zinc-600">—</span>
								{/if}
							</td>

							<!-- Response / test result -->
							<td class="px-4 py-3 min-w-[110px]">
								{#if isTesting}
									<span class="inline-flex items-center gap-1 text-xs text-zinc-500 dark:text-zinc-400">
										<svg class="animate-spin h-3 w-3" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
											<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
											<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z"></path>
										</svg>
										测试中
									</span>
								{:else if testRes}
									{#if testRes.success}
										<span class="inline-block px-2 py-0.5 rounded text-xs font-medium bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400">
											✓ {testRes.response_time_ms}ms
										</span>
									{:else}
										<span
											class="inline-block px-2 py-0.5 rounded text-xs font-medium bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400 max-w-[120px] truncate"
											title={testRes.error ?? undefined}
										>
											✗ {testRes.error ?? '失败'}
										</span>
									{/if}
									{#if bal}
										<div class="text-[10px] text-zinc-400 dark:text-zinc-500 mt-0.5 font-mono">{fmtBalance(bal)}</div>
									{/if}
								{:else if bal}
									<div class="text-xs text-zinc-500 dark:text-zinc-400 font-mono">{fmtBalance(bal)}</div>
								{:else}
									<span class="text-xs text-zinc-400 dark:text-zinc-600">—</span>
								{/if}
							</td>

							<!-- Actions -->
							{#if isPlatformAdmin}
								<td class="px-4 py-3 text-right">
									<div class="flex gap-1 justify-end flex-wrap">
										<!-- Test -->
										<Button
											variant="ghost"
											size="sm"
											disabled={isTesting}
											onclick={() => handleTest(ch)}
											class="text-zinc-600 dark:text-zinc-400"
										>
											{isTesting ? '...' : '测试'}
										</Button>
										<!-- Probe -->
										<Button
											variant="ghost"
											size="sm"
											disabled={probingId === ch.id}
											onclick={() => handleProbe(ch)}
											class="text-zinc-600 dark:text-zinc-400"
										>
											Probe
										</Button>
										<!-- Balance refresh -->
										<span title="刷新余额">
										<Button
											variant="ghost"
											size="sm"
											disabled={isLoadingBal}
											onclick={() => handleRefreshBalance(ch)}
											class="text-zinc-600 dark:text-zinc-400"
										>
											{#if isLoadingBal}
												<svg class="animate-spin h-3 w-3" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
													<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
													<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z"></path>
												</svg>
											{:else}
												<svg xmlns="http://www.w3.org/2000/svg" class="h-3.5 w-3.5" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
													<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8" />
													<path d="M21 3v5h-5" />
													<path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16" />
													<path d="M3 21v-5h5" />
												</svg>
											{/if}
										</Button>
										</span>
										<!-- Keys -->
										<Button variant="ghost" size="sm" onclick={() => goto(`/channels/${ch.id}`)}>Keys</Button>
										<!-- Edit -->
										<Button variant="ghost" size="sm" onclick={() => startEdit(ch)}>编辑</Button>
										<!-- Delete -->
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
	{/if}
</div>
