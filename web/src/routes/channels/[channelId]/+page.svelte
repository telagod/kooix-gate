<!-- /channels/[channelId] — Channel 详情页：Overview + Keys + Models + Logs -->
<script lang="ts">
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import {
		getMe,
		listChannelKeys,
		createChannelKey,
		rotateChannelKey,
		revokeChannelKey,
		testChannel,
		probeChannel,
		updateChannel,
		listAuditLogs
	} from '$lib/api.js';
	import type { ChannelKeySummary, TestResponse, ProbeResponse, AuditLog } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let channelId = $derived($page.params.channelId ?? '');

	let activeTab = $state<'overview' | 'keys' | 'models' | 'logs'>('overview');
	let isPlatformAdmin = $state(false);
	let loading = $state(true);
	let error = $state('');

	// Channel info (fetched via stats endpoint)
	let channelStats = $state<any>(null);

	// Keys
	let keys = $state<ChannelKeySummary[]>([]);
	let keysLoading = $state(false);

	// Models
	let probeResult = $state<ProbeResponse | null>(null);
	let probing = $state(false);

	// Test
	let testResult = $state<TestResponse | null>(null);
	let testing = $state(false);

	// Logs
	let logs = $state<AuditLog[]>([]);
	let logsLoading = $state(false);

	// Key create
	let showCreateKey = $state(false);
	let createSecret = $state('');
	let createAlias = $state('');
	let creatingKey = $state(false);
	let createKeyError = $state('');

	// Key rotate
	let showRotate = $state(false);
	let rotateSecret = $state('');
	let rotateAlias = $state('');
	let rotating = $state(false);
	let rotateError = $state('');

	// Revoke
	let revokingId = $state<string | null>(null);
	let revoking = $state(false);

	// Toast
	let toast = $state('');
	let toastType = $state<'ok' | 'err'>('ok');

	function showToast(msg: string, type: 'ok' | 'err' = 'ok') {
		toast = msg;
		toastType = type;
		setTimeout(() => (toast = ''), 3500);
	}

	onMount(async () => {
		try {
			const me = await getMe();
			isPlatformAdmin = me.is_platform_admin;
		} catch (err: any) {
			error = err?.message ?? '加载身份失败';
			loading = false;
			return;
		}
		await loadStats();
		loading = false;
	});

	async function loadStats() {
		try {
			const resp = await fetch(`/api/v1/admin/channels/${channelId}/stats`);
			// Fallback: use apiFetch pattern
			const { apiFetch } = await import('$lib/api.js');
			channelStats = await (apiFetch as any)(`/v1/admin/channels/${channelId}/stats`);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		}
	}

	async function loadKeys() {
		keysLoading = true;
		try {
			keys = await listChannelKeys(channelId);
		} catch (err: any) {
			showToast(err?.message ?? '加载 Keys 失败', 'err');
		} finally {
			keysLoading = false;
		}
	}

	async function loadLogs() {
		logsLoading = true;
		try {
			// filter by resource_id = channelId
			logs = await listAuditLogs('', 50, 0);
			logs = logs.filter(l => l.resource_id === channelId || l.resource_kind === 'channel_key');
		} catch {
			logs = [];
		} finally {
			logsLoading = false;
		}
	}

	async function switchTab(tab: typeof activeTab) {
		activeTab = tab;
		if (tab === 'keys' && keys.length === 0) await loadKeys();
		if (tab === 'logs' && logs.length === 0) await loadLogs();
	}

	async function handleTest() {
		testing = true;
		try {
			testResult = await testChannel(channelId);
		} catch (err: any) {
			testResult = { success: false, model: '', response_time_ms: 0, message: null, error: err?.message ?? '失败' };
		} finally {
			testing = false;
		}
	}

	async function handleProbe() {
		probing = true;
		try {
			probeResult = await probeChannel(channelId);
		} catch (err: any) {
			showToast(err?.message ?? 'Probe 失败', 'err');
		} finally {
			probing = false;
		}
	}

	async function handleSyncModels() {
		if (!probeResult) return;
		try {
			await updateChannel(channelId, { supported_models: probeResult.models });
			showToast(`已同步 ${probeResult.models.length} 个模型`);
			await loadStats();
		} catch (err: any) {
			showToast(err?.message ?? '同步失败', 'err');
		}
	}

	// Key operations
	async function handleCreateKey(e: SubmitEvent) {
		e.preventDefault();
		if (!createSecret.trim()) return;
		creatingKey = true;
		createKeyError = '';
		try {
			await createChannelKey(channelId, createSecret.trim(), createAlias.trim() || undefined);
			showCreateKey = false;
			createSecret = '';
			createAlias = '';
			showToast('Key 创建成功');
			await loadKeys();
		} catch (err: any) {
			createKeyError = err?.message ?? '创建失败';
		} finally {
			creatingKey = false;
		}
	}

	async function handleRotate(e: SubmitEvent) {
		e.preventDefault();
		if (!rotateSecret.trim()) return;
		rotating = true;
		rotateError = '';
		try {
			await rotateChannelKey(channelId, rotateSecret.trim(), rotateAlias.trim() || undefined);
			showRotate = false;
			rotateSecret = '';
			rotateAlias = '';
			showToast('Key 轮转成功');
			await loadKeys();
		} catch (err: any) {
			rotateError = err?.message ?? '轮转失败';
		} finally {
			rotating = false;
		}
	}

	async function handleRevoke() {
		if (!revokingId) return;
		revoking = true;
		try {
			await revokeChannelKey(channelId, revokingId);
			keys = keys.filter(k => k.id !== revokingId);
			revokingId = null;
			showToast('Key 已撤销');
		} catch (err: any) {
			showToast(err?.message ?? '撤销失败', 'err');
			revokingId = null;
		} finally {
			revoking = false;
		}
	}

	function healthDot(health: string): string {
		if (health === 'healthy') return 'bg-green-500';
		if (health === 'cooling_down') return 'bg-amber-500';
		if (health === 'disabled') return 'bg-red-500';
		return 'bg-zinc-400';
	}

	function fmtDate(s: string | null | undefined): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleDateString('zh-CN', { month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' });
		} catch { return s; }
	}

	function fmtNum(n: number | null | undefined): string {
		if (n == null) return '0';
		return n.toLocaleString();
	}
</script>

<!-- Toast -->
{#if toast}
	<div class="fixed top-4 right-4 z-50 px-4 py-2 rounded-lg shadow-lg text-sm {toastType === 'err' ? 'bg-red-600 text-white' : 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900'}">
		{toast}
	</div>
{/if}

<!-- Revoke confirm -->
{#if revokingId}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) revokingId = null; }}>
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认撤销</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">撤销后此 Key 将立即失效。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (revokingId = null)} disabled={revoking}>取消</Button>
				<Button variant="destructive" onclick={handleRevoke} disabled={revoking}>
					{revoking ? '撤销中...' : '确认撤销'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Create Key modal -->
{#if showCreateKey}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) showCreateKey = false; }}>
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">添加 Key</h3>
			<div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800 dark:text-amber-300">Secret 为上游 API Key 明文，加密存储后不可查看。</p>
			</div>
			<form onsubmit={handleCreateKey} class="space-y-3">
				<div>
					<label for="ck-secret" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Secret <span class="text-red-500">*</span></label>
					<textarea id="ck-secret" bind:value={createSecret} disabled={creatingKey} rows="3" placeholder="sk-..."
						class="flex w-full rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 py-2 text-sm font-mono text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300 disabled:opacity-50 resize-none"
					></textarea>
				</div>
				<div>
					<label for="ck-alias" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">别名</label>
					<Input id="ck-alias" bind:value={createAlias} disabled={creatingKey} placeholder="prod-key-1" />
				</div>
				{#if createKeyError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{createKeyError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={() => (showCreateKey = false)}>取消</Button>
					<Button type="submit" disabled={creatingKey || !createSecret.trim()}>
						{creatingKey ? '创建中...' : '创建'}
					</Button>
				</div>
			</form>
		</Card>
	</div>
{/if}

<!-- Rotate modal -->
{#if showRotate}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center" onclick={(e: MouseEvent) => { if (e.target === e.currentTarget) showRotate = false; }}>
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">轮转 Key</h3>
			<div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800 dark:text-amber-300">将创建新 Key 并自动撤销所有旧 Key。</p>
			</div>
			<form onsubmit={handleRotate} class="space-y-3">
				<div>
					<label for="rk-secret" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">新 Secret <span class="text-red-500">*</span></label>
					<textarea id="rk-secret" bind:value={rotateSecret} disabled={rotating} rows="3" placeholder="sk-..."
						class="flex w-full rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 py-2 text-sm font-mono text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-500 dark:placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300 disabled:opacity-50 resize-none"
					></textarea>
				</div>
				<div>
					<label for="rk-alias" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">别名</label>
					<Input id="rk-alias" bind:value={rotateAlias} disabled={rotating} placeholder="prod-key-2" />
				</div>
				{#if rotateError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{rotateError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={() => (showRotate = false)}>取消</Button>
					<Button variant="destructive" type="submit" disabled={rotating || !rotateSecret.trim()}>
						{rotating ? '轮转中...' : '确认轮转'}
					</Button>
				</div>
			</form>
		</Card>
	</div>
{/if}

<div class="px-6 py-6">
	<!-- Breadcrumb -->
	<p class="text-xs text-zinc-500 dark:text-zinc-400 mb-2">
		<a href="/channels" class="hover:underline">渠道</a> / {channelId.slice(0, 8)}...
	</p>

	{#if loading}
		<div class="flex items-center justify-center py-16">
			<svg class="animate-spin h-6 w-6 text-zinc-400" xmlns="http://www.w3.org/2000/svg" fill="none" viewBox="0 0 24 24">
				<circle class="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" stroke-width="4"></circle>
				<path class="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8z"></path>
			</svg>
		</div>
	{:else if error}
		<Card class="p-6"><p class="text-red-600 dark:text-red-400 text-sm">{error}</p></Card>
	{:else if channelStats}
		<!-- Header -->
		<div class="flex items-center justify-between mb-6">
			<div>
				<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">{channelStats.channel.name || channelStats.channel.code}</h1>
				<p class="text-sm text-zinc-600 dark:text-zinc-300 font-mono mt-0.5">{channelStats.channel.code} · {channelStats.channel.provider_type}</p>
			</div>
			{#if isPlatformAdmin}
				<div class="flex gap-2">
					<Button variant="outline" size="sm" onclick={handleTest} disabled={testing}>
						{testing ? '测试中...' : '测试连通'}
					</Button>
				</div>
			{/if}
		</div>

		<!-- Test result banner -->
		{#if testResult}
			<div class="mb-4 px-4 py-2 rounded-lg text-sm {testResult.success ? 'bg-green-50 dark:bg-green-900/20 text-green-700 dark:text-green-400' : 'bg-red-50 dark:bg-red-900/20 text-red-700 dark:text-red-400'}">
				{#if testResult.success}
					✓ 连通正常 — {testResult.response_time_ms}ms (model: {testResult.model})
				{:else}
					✗ {testResult.error ?? '连接失败'}
				{/if}
			</div>
		{/if}

		<!-- Stats cards -->
		<div class="grid grid-cols-2 md:grid-cols-4 gap-4 mb-6">
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">Keys</p>
				<p class="mt-1 text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{channelStats.keys_count}</p>
				<p class="text-xs text-zinc-500 dark:text-zinc-400">{channelStats.keys_healthy} healthy</p>
			</Card>
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">总请求</p>
				<p class="mt-1 text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{fmtNum(channelStats.total_requests)}</p>
			</Card>
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">总错误</p>
				<p class="mt-1 text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{fmtNum(channelStats.total_errors)}</p>
			</Card>
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">余额</p>
				<p class="mt-1 text-2xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">
					{channelStats.channel.balance != null ? `$${channelStats.channel.balance.toFixed(2)}` : '—'}
				</p>
			</Card>
		</div>

		<!-- Tabs -->
		<div class="border-b border-zinc-200 dark:border-zinc-700 mb-6">
			<nav class="flex gap-6">
				{#each [['overview', 'Overview'], ['keys', 'Keys'], ['models', '模型'], ['logs', '日志']] as [tab, label]}
					<button
						onclick={() => switchTab(tab as typeof activeTab)}
						class="pb-3 text-sm font-medium transition-colors border-b-2 {activeTab === tab
							? 'border-zinc-900 dark:border-zinc-100 text-zinc-900 dark:text-zinc-100'
							: 'border-transparent text-zinc-500 dark:text-zinc-400 hover:text-zinc-700 dark:hover:text-zinc-300'}"
					>{label}</button>
				{/each}
			</nav>
		</div>

		<!-- Tab content -->
		{#if activeTab === 'overview'}
			<div class="grid grid-cols-1 md:grid-cols-2 gap-6">
				<Card class="p-5">
					<h3 class="text-sm font-medium text-zinc-900 dark:text-zinc-100 mb-3">基础信息</h3>
					<dl class="space-y-2 text-sm">
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">状态</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.status}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">健康度</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.health}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">Base URL</dt><dd class="text-zinc-900 dark:text-zinc-100 font-mono text-xs truncate max-w-[200px]">{channelStats.channel.base_url}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">超时</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.timeout_ms}ms</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">重试</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.max_retries} 次</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">RPM 限制</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.rpm_limit ?? '∞'}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">TPM 限制</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.tpm_limit ?? '∞'}</dd></div>
					</dl>
				</Card>
				<Card class="p-5">
					<h3 class="text-sm font-medium text-zinc-900 dark:text-zinc-100 mb-3">最近状态</h3>
					<dl class="space-y-2 text-sm">
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">更新时间</dt><dd class="text-zinc-900 dark:text-zinc-100">{fmtDate(channelStats.channel.updated_at)}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">创建时间</dt><dd class="text-zinc-900 dark:text-zinc-100">{fmtDate(channelStats.channel.created_at)}</dd></div>
						{#if channelStats.channel.last_error}
							<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">最后错误</dt><dd class="text-red-600 dark:text-red-400 text-xs truncate max-w-[200px]">{channelStats.channel.last_error}</dd></div>
							<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">错误时间</dt><dd class="text-zinc-900 dark:text-zinc-100">{fmtDate(channelStats.channel.last_error_at)}</dd></div>
						{/if}
						{#if channelStats.channel.tags && channelStats.channel.tags.length > 0}
							<div class="flex justify-between items-start"><dt class="text-zinc-500 dark:text-zinc-400">标签</dt>
								<dd class="flex flex-wrap gap-1 justify-end">
									{#each channelStats.channel.tags as tag}
										<span class="px-1.5 py-0.5 bg-zinc-100 dark:bg-zinc-700 text-zinc-600 dark:text-zinc-300 rounded text-[10px]">{tag}</span>
									{/each}
								</dd>
							</div>
						{/if}
					</dl>
				</Card>
			</div>

		{:else if activeTab === 'keys'}
			<div class="flex items-center justify-between mb-4">
				<p class="text-sm text-zinc-600 dark:text-zinc-300">{keys.length} 个 Key</p>
				{#if isPlatformAdmin}
					<div class="flex gap-2">
						<Button variant="outline" size="sm" onclick={() => (showRotate = true)}>轮转</Button>
						<Button size="sm" onclick={() => (showCreateKey = true)}>+ 添加</Button>
					</div>
				{/if}
			</div>

			{#if keysLoading}
				<p class="text-zinc-500 dark:text-zinc-400 text-sm">加载中...</p>
			{:else if keys.length === 0}
				<Card class="p-6 text-center">
					<p class="text-zinc-500 dark:text-zinc-400 text-sm">暂无 Key。</p>
				</Card>
			{:else}
				<div class="overflow-x-auto rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
					<table class="w-full text-sm">
						<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
							<tr>
								<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">Label</th>
								<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">Fingerprint</th>
								<th class="px-3 py-3 text-center font-medium text-zinc-600 dark:text-zinc-400">Health</th>
								<th class="px-3 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">Requests</th>
								<th class="px-3 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">Errors</th>
								<th class="px-3 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">Cooldown</th>
								<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">创建时间</th>
								{#if isPlatformAdmin}
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">操作</th>
								{/if}
							</tr>
						</thead>
						<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
							{#each keys as key}
								<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
									<td class="px-4 py-3 text-zinc-900 dark:text-zinc-100">{key.label ?? '—'}</td>
									<td class="px-4 py-3 font-mono text-xs text-zinc-600 dark:text-zinc-400">{key.fingerprint}</td>
									<td class="px-3 py-3 text-center">
										<div class="flex items-center justify-center gap-1.5">
											<span class="w-2 h-2 rounded-full {healthDot(key.health)}"></span>
											<span class="text-xs text-zinc-600 dark:text-zinc-400">{key.health}</span>
										</div>
									</td>
									<td class="px-3 py-3 text-right text-zinc-700 dark:text-zinc-300 font-mono text-xs tabular-nums">{fmtNum(key.total_requests)}</td>
									<td class="px-3 py-3 text-right font-mono text-xs tabular-nums {key.total_errors > 0 ? 'text-red-600 dark:text-red-400' : 'text-zinc-500 dark:text-zinc-400'}">{fmtNum(key.total_errors)}</td>
									<td class="px-3 py-3 text-right text-xs text-zinc-500 dark:text-zinc-400">{key.cooldown_until ? fmtDate(key.cooldown_until) : '—'}</td>
									<td class="px-4 py-3 text-zinc-500 dark:text-zinc-400 text-xs">{fmtDate(key.created_at)}</td>
									{#if isPlatformAdmin}
										<td class="px-4 py-3 text-right">
											<Button variant="ghost" size="sm" onclick={() => (revokingId = key.id)}>
												<span class="text-red-600 dark:text-red-400">撤销</span>
											</Button>
										</td>
									{/if}
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}

		{:else if activeTab === 'models'}
			<div class="space-y-4">
				<div class="flex items-center justify-between">
					<p class="text-sm text-zinc-600 dark:text-zinc-300">
						{channelStats.channel.supported_models?.length ?? 0} 个已配置模型
					</p>
					{#if isPlatformAdmin}
						<Button variant="outline" size="sm" onclick={handleProbe} disabled={probing}>
							{probing ? 'Probing...' : 'Probe 上游模型'}
						</Button>
					{/if}
				</div>

				{#if channelStats.channel.supported_models && channelStats.channel.supported_models.length > 0}
					<div class="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-4 gap-2">
						{#each channelStats.channel.supported_models as m}
							<div class="px-3 py-2 rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 text-xs font-mono text-zinc-700 dark:text-zinc-300 truncate" title={m}>{m}</div>
						{/each}
					</div>
				{:else}
					<Card class="p-6 text-center">
						<p class="text-zinc-500 dark:text-zinc-400 text-sm">未配置模型列表（通配所有模型）。</p>
					</Card>
				{/if}

				{#if probeResult}
					<Card class="p-5">
						<h3 class="text-sm font-medium text-zinc-900 dark:text-zinc-100 mb-3">Probe 发现 {probeResult.models.length} 个模型</h3>
						<div class="max-h-48 overflow-y-auto grid grid-cols-2 md:grid-cols-3 gap-1">
							{#each probeResult.models as m}
								<div class="text-xs font-mono text-zinc-600 dark:text-zinc-400 px-2 py-1">{m}</div>
							{/each}
						</div>
						<div class="flex gap-2 justify-end mt-4">
							<Button variant="outline" size="sm" onclick={() => (probeResult = null)}>关闭</Button>
							<Button size="sm" onclick={handleSyncModels}>同步到 Channel</Button>
						</div>
					</Card>
				{/if}

				{#if channelStats.channel.model_mapping && Object.keys(channelStats.channel.model_mapping).length > 0}
					<Card class="p-5">
						<h3 class="text-sm font-medium text-zinc-900 dark:text-zinc-100 mb-3">模型映射</h3>
						<div class="space-y-1">
							{#each Object.entries(channelStats.channel.model_mapping) as [alias, target]}
								<div class="flex items-center gap-2 text-xs">
									<span class="font-mono text-zinc-600 dark:text-zinc-400">{alias}</span>
									<span class="text-zinc-400">→</span>
									<span class="font-mono text-zinc-900 dark:text-zinc-100">{target}</span>
								</div>
							{/each}
						</div>
					</Card>
				{/if}
			</div>

		{:else if activeTab === 'logs'}
			{#if logsLoading}
				<p class="text-zinc-500 dark:text-zinc-400 text-sm">加载中...</p>
			{:else if logs.length === 0}
				<Card class="p-6 text-center">
					<p class="text-zinc-500 dark:text-zinc-400 text-sm">暂无相关日志。</p>
				</Card>
			{:else}
				<div class="overflow-x-auto rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
					<table class="w-full text-sm">
						<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
							<tr>
								<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">时间</th>
								<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">动作</th>
								<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">操作者</th>
								<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">结果</th>
							</tr>
						</thead>
						<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
							{#each logs as log}
								<tr>
									<td class="px-4 py-3 text-xs text-zinc-500 dark:text-zinc-400">{fmtDate(log.ts)}</td>
									<td class="px-4 py-3 text-zinc-900 dark:text-zinc-100 font-mono text-xs">{log.action}</td>
									<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400 text-xs">{log.actor_kind}:{log.actor_id?.slice(0, 8) ?? '—'}</td>
									<td class="px-4 py-3">
										<span class="px-2 py-0.5 rounded text-xs {log.outcome === 'success' ? 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400' : 'bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400'}">{log.outcome}</span>
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				</div>
			{/if}
		{/if}
	{/if}
</div>
