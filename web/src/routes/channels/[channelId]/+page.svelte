<!-- /channels/[channelId] — Channel 详情页：Overview + Keys + Models + Logs -->
<script lang="ts">
	import { rawId, shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import {
		getMe,
		getChannelStats,
		listChannelKeys,
		createChannelKey,
		rotateChannelKey,
		revokeChannelKey,
		testChannel,
		probeChannel,
		updateChannel,
		drainChannel,
		getChannelDrainStatus,
		disableChannelWhenIdle,
		listAuditLogs
	} from '$lib/api.js';
	import type { ChannelKeySummary, ChannelStats, TestResponse, ProbeResponse, AuditLog } from '$lib/api.js';
	import { Alert, Badge, Button, Card, Field, Input, Textarea } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import SectionCard from '$lib/components/templates/SectionCard.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import { Cable, CirclePause, ListChecks, Plus, RotateCw, Server, ShieldCheck, XCircle, Zap } from 'lucide-svelte';

	let channelId = $derived($page.params.channelId ?? '');

	let activeTab = $state<'overview' | 'keys' | 'models' | 'logs'>('overview');
	let isPlatformAdmin = $state(false);
	let loading = $state(true);
	let error = $state('');

	// Channel info
	let channelStats = $state<ChannelStats | null>(null);

	// Keys
	let keys = $state<ChannelKeySummary[]>([]);
	let keysLoading = $state(false);

	// Models
	let probeResult = $state<ProbeResponse | null>(null);
	let probing = $state(false);

	// Test
	let testResult = $state<TestResponse | null>(null);
	let testing = $state(false);
	let drainInfo = $state<{ inflight: number; safe_to_disable: boolean } | null>(null);
	let draining = $state(false);
	let disablingWhenIdle = $state(false);

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
	let rotateConfirmation = $state('');
	let rotating = $state(false);
	let rotateError = $state('');

	// Revoke
	let revokingId = $state<string | null>(null);
	let revokeConfirmation = $state('');
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
			channelStats = await getChannelStats(channelId);
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

	async function handleDrain() {
		draining = true;
		try {
			const result = await drainChannel(channelId);
			channelStats = { ...(channelStats as ChannelStats), channel: result.channel };
			drainInfo = { inflight: result.inflight, safe_to_disable: result.safe_to_disable };
			showToast(result.safe_to_disable ? '已进入 Draining，可安全禁用' : `已进入 Draining，等待 ${result.inflight} 个 inflight`);
		} catch (err: any) {
			showToast(err?.message ?? 'Drain 失败', 'err');
		} finally {
			draining = false;
		}
	}

	async function refreshDrainStatus() {
		try {
			const result = await getChannelDrainStatus(channelId);
			channelStats = { ...(channelStats as ChannelStats), channel: result.channel };
			drainInfo = { inflight: result.inflight, safe_to_disable: result.safe_to_disable };
			showToast(result.safe_to_disable ? 'Inflight 已清空' : `仍有 ${result.inflight} 个 inflight`);
		} catch (err: any) {
			showToast(err?.message ?? '刷新 drain 状态失败', 'err');
		}
	}

	async function handleDisableWhenIdle() {
		disablingWhenIdle = true;
		try {
			const result = await disableChannelWhenIdle(channelId);
			channelStats = { ...(channelStats as ChannelStats), channel: result.channel };
			drainInfo = { inflight: result.inflight, safe_to_disable: result.safe_to_disable };
			showToast('Inflight 已清空，Channel 已禁用');
		} catch (err: any) {
			showToast(err?.message ?? '仍有 inflight，暂不能禁用', 'err');
			await refreshDrainStatus();
		} finally {
			disablingWhenIdle = false;
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
			await rotateChannelKey(channelId, rotateSecret.trim(), rotateAlias.trim() || undefined, rotateConfirmation);
			showRotate = false;
			rotateSecret = '';
			rotateAlias = '';
			rotateConfirmation = '';
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
			await revokeChannelKey(channelId, revokingId, revokeConfirmation);
			keys = keys.filter(k => k.id !== revokingId);
			revokingId = null;
			revokeConfirmation = '';
			showToast('Key 已撤销');
		} catch (err: any) {
			showToast(err?.message ?? '撤销失败', 'err');
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
	{@const expectedRevokeConfirmation = `revoke:${rawId(revokingId)}`}
	<ModalFrame close={() => { revokingId = null; revokeConfirmation = ''; }} class="z-40">
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认撤销</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mb-4">撤销后此 Key 将立即失效。请输入确认短语：</p>
			<div class="mb-4 space-y-2">
				<code class="block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expectedRevokeConfirmation}</code>
				<Input id="revoke-confirm" bind:value={revokeConfirmation} disabled={revoking} placeholder={expectedRevokeConfirmation} class="font-mono" />
			</div>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => { revokingId = null; revokeConfirmation = ''; }} disabled={revoking}>取消</Button>
				<Button variant="destructive" onclick={handleRevoke} disabled={revoking || revokeConfirmation.trim() !== expectedRevokeConfirmation}>
					{revoking ? '撤销中...' : '确认撤销'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}

<!-- Create Key modal -->
{#if showCreateKey}
	<ModalFrame close={() => { showCreateKey = false; }} class="z-40">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">添加 Key</h3>
			<div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800 dark:text-amber-300">Secret 为上游 API Key 明文，加密存储后不可查看。</p>
			</div>
			<form onsubmit={handleCreateKey} class="space-y-3">
				<Field label="Secret" for="ck-secret" required>
					<Textarea id="ck-secret" bind:value={createSecret} disabled={creatingKey} rows={3} placeholder="sk-..." class="font-mono resize-none" />
				</Field>
				<Field label="别名" for="ck-alias">
					<Input id="ck-alias" bind:value={createAlias} disabled={creatingKey} placeholder="prod-key-1" />
				</Field>
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
	</ModalFrame>
{/if}

<!-- Rotate modal -->
{#if showRotate}
	{@const expectedRotateConfirmation = `rotate:${channelStats?.channel.code ?? ''}`}
	<ModalFrame close={() => { showRotate = false; rotateConfirmation = ''; }} class="z-40">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">轮转 Key</h3>
			<div class="bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-700 rounded-md px-3 py-2 mb-4">
				<p class="text-xs text-amber-800 dark:text-amber-300">将创建新 Key 并自动撤销所有旧 Key。</p>
			</div>
			<form onsubmit={handleRotate} class="space-y-3">
				<Field label="新 Secret" for="rk-secret" required>
					<Textarea id="rk-secret" bind:value={rotateSecret} disabled={rotating} rows={3} placeholder="sk-..." class="font-mono resize-none" />
				</Field>
				<Field label="别名" for="rk-alias">
					<Input id="rk-alias" bind:value={rotateAlias} disabled={rotating} placeholder="prod-key-2" />
				</Field>
				<Field label="二次确认" for="rk-confirm" hint="轮转会禁用旧 healthy key；请输入下方短语。">
					<code class="mb-2 block rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-800 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-200">{expectedRotateConfirmation}</code>
					<Input id="rk-confirm" bind:value={rotateConfirmation} disabled={rotating} placeholder={expectedRotateConfirmation} class="font-mono" />
				</Field>
				{#if rotateError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{rotateError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={() => (showRotate = false)}>取消</Button>
					<Button variant="destructive" type="submit" disabled={rotating || !rotateSecret.trim() || rotateConfirmation.trim() !== expectedRotateConfirmation}>
						{rotating ? '轮转中...' : '确认轮转'}
					</Button>
				</div>
			</form>
		</Card>
	</ModalFrame>
{/if}

<PageShell
	title={channelStats ? (channelStats.channel.name || channelStats.channel.code) : 'Channel 详情'}
	description={channelStats ? `${channelStats.channel.code} · ${channelStats.channel.provider_type}` : shortId(channelId)}
	eyebrow={`渠道 / ${shortId(channelId)}...`}
	icon={Cable}
	max="full"
>
	{#snippet actions()}
		{#if channelStats && isPlatformAdmin}
			<Button variant="outline" size="sm" onclick={handleDrain} disabled={draining || channelStats.channel.status === 'draining' || channelStats.channel.status === 'disabled'}>
				<CirclePause size={14} />
				{draining ? 'Draining...' : 'Drain'}
			</Button>
			{#if channelStats.channel.status === 'draining'}
				<Button variant="outline" size="sm" onclick={handleDisableWhenIdle} disabled={disablingWhenIdle}>
					<ShieldCheck size={14} />
					{disablingWhenIdle ? '检查中...' : '空闲禁用'}
				</Button>
			{/if}
			<Button variant="outline" size="sm" onclick={handleTest} disabled={testing}>
				<Zap size={14} />
				{testing ? '测试中...' : '测试连通'}
			</Button>
		{/if}
	{/snippet}

	{#if loading}
		<div class="flex items-center justify-center py-16">
			<div class="h-6 w-6 animate-spin rounded-full border-2 border-zinc-200 border-t-zinc-900 dark:border-zinc-700 dark:border-t-zinc-100"></div>
		</div>
	{:else if error}
		<StatePanel title="加载失败" description={error} variant="danger" icon={XCircle} />
	{:else if channelStats}
		{#if testResult}
			<Alert variant={testResult.success ? 'success' : 'danger'} class="mb-4">
				{#if testResult.success}
					连通正常 — {testResult.response_time_ms}ms (model: {testResult.model})
				{:else}
					{testResult.error ?? '连接失败'}
				{/if}
			</Alert>
		{/if}

		{#if channelStats.channel.status === 'draining'}
			<Alert variant="warning" class="mb-4">
				<div class="flex flex-wrap items-center justify-between gap-3">
					<div class="flex items-start gap-2">
						<CirclePause size={16} class="mt-0.5" />
						<div>
							<p class="font-medium">Draining：新请求已停止进入该 Channel</p>
							<p class="mt-1 text-xs">
								{drainInfo ? `inflight=${drainInfo.inflight} · ${drainInfo.safe_to_disable ? '可安全禁用' : '等待现有请求完成'}` : '点击刷新读取当前 inflight。'}
							</p>
						</div>
					</div>
					<div class="flex gap-2">
						<Button variant="outline" size="sm" onclick={refreshDrainStatus}>刷新</Button>
						<Button size="sm" onclick={handleDisableWhenIdle} disabled={disablingWhenIdle}>
							{disablingWhenIdle ? '检查中...' : '空闲后禁用'}
						</Button>
					</div>
				</div>
			</Alert>
		{/if}

		<div class="mb-6 grid grid-cols-2 gap-4 md:grid-cols-4">
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">Keys 凭据</p>
				<p class="mt-1 text-2xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">{channelStats.keys_count}</p>
				<p class="text-xs text-zinc-500 dark:text-zinc-400">{channelStats.keys_healthy} healthy 健康</p>
			</Card>
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">总请求</p>
				<p class="mt-1 text-2xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">{fmtNum(channelStats.total_requests)}</p>
			</Card>
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">总错误</p>
				<p class="mt-1 text-2xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">{fmtNum(channelStats.total_errors)}</p>
			</Card>
			<Card class="p-4">
				<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">余额</p>
				<p class="mt-1 text-2xl font-bold tabular-nums text-zinc-900 dark:text-zinc-100">
					{channelStats.channel.balance != null ? `$${channelStats.channel.balance.toFixed(2)}` : '—'}
				</p>
			</Card>
		</div>

		<DataToolbar class="mb-6 border-b border-zinc-200 pb-0 dark:border-zinc-700" rowClass="gap-6">
			{#snippet controls()}
				{#each [['overview', 'Overview 总览'], ['keys', 'Keys 凭据'], ['models', '模型'], ['logs', '日志']] as [tab, label]}
					<button
						type="button"
						onclick={() => switchTab(tab as typeof activeTab)}
						class="border-b-2 pb-3 text-sm font-medium transition-colors {activeTab === tab
							? 'border-zinc-900 text-zinc-900 dark:border-zinc-100 dark:text-zinc-100'
							: 'border-transparent text-zinc-500 hover:text-zinc-700 dark:text-zinc-400 dark:hover:text-zinc-300'}"
					>{label}</button>
				{/each}
			{/snippet}
		</DataToolbar>

		{#if activeTab === 'overview'}
			<div class="grid grid-cols-1 gap-6 md:grid-cols-2">
				<SectionCard title="基础信息" icon={Server}>
					<dl class="space-y-2 text-sm">
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">状态</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.status}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">健康度</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.health}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">Base URL</dt><dd class="max-w-[200px] truncate font-mono text-xs text-zinc-900 dark:text-zinc-100">{channelStats.channel.base_url}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">超时</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.timeout_ms}ms</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">重试</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.max_retries} 次</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">RPM 限制</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.rpm_limit ?? '∞'}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">TPM 限制</dt><dd class="text-zinc-900 dark:text-zinc-100">{channelStats.channel.tpm_limit ?? '∞'}</dd></div>
					</dl>
				</SectionCard>
				<SectionCard title="最近状态" icon={ListChecks}>
					<dl class="space-y-2 text-sm">
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">更新时间</dt><dd class="text-zinc-900 dark:text-zinc-100">{fmtDate(channelStats.channel.updated_at)}</dd></div>
						<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">创建时间</dt><dd class="text-zinc-900 dark:text-zinc-100">{fmtDate(channelStats.channel.created_at)}</dd></div>
						{#if channelStats.channel.last_error}
							<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">最后错误</dt><dd class="max-w-[200px] truncate text-xs text-red-600 dark:text-red-400">{channelStats.channel.last_error}</dd></div>
							<div class="flex justify-between"><dt class="text-zinc-500 dark:text-zinc-400">错误时间</dt><dd class="text-zinc-900 dark:text-zinc-100">{fmtDate(channelStats.channel.last_error_at)}</dd></div>
						{/if}
						{#if channelStats.channel.tags && channelStats.channel.tags.length > 0}
							<div class="flex items-start justify-between"><dt class="text-zinc-500 dark:text-zinc-400">标签</dt>
								<dd class="flex flex-wrap justify-end gap-1">
									{#each channelStats.channel.tags as tag}
										<Badge class="text-[10px]">{tag}</Badge>
									{/each}
								</dd>
							</div>
						{/if}
					</dl>
				</SectionCard>
			</div>

		{:else if activeTab === 'keys'}
			<DataToolbar class="mb-4" badgesVisible={false}>
				{#snippet controls()}
					<p class="text-sm text-zinc-600 dark:text-zinc-300">{keys.length} 个 Key</p>
				{/snippet}
				{#snippet actions()}
					{#if isPlatformAdmin}
						<Button variant="outline" size="sm" onclick={() => (showRotate = true)}>
							<RotateCw size={14} /> 轮转
						</Button>
						<Button size="sm" onclick={() => (showCreateKey = true)}>
							<Plus size={14} /> 添加
						</Button>
					{/if}
				{/snippet}
			</DataToolbar>

			{#if keysLoading}
				<p class="text-sm text-zinc-500 dark:text-zinc-400">加载中...</p>
			{:else}
				<DataTable isEmpty={keys.length === 0} emptyColspan={isPlatformAdmin ? 8 : 7}>
					{#snippet head()}
						<tr>
							<th class={dataTemplate.th}>Label 标签</th>
							<th class={dataTemplate.th}>Fingerprint 指纹</th>
							<th class={cn(dataTemplate.th, 'text-center')}>Health 健康</th>
							<th class={cn(dataTemplate.th, 'text-right')}>Requests 请求</th>
							<th class={cn(dataTemplate.th, 'text-right')}>Errors 错误</th>
							<th class={cn(dataTemplate.th, 'text-right')}>Cooldown 冷却</th>
							<th class={dataTemplate.th}>创建时间</th>
							{#if isPlatformAdmin}
								<th class={cn(dataTemplate.th, 'text-right')}>操作</th>
							{/if}
						</tr>
					{/snippet}
					{#snippet empty()}
						暂无 Key。
					{/snippet}
					{#each keys as key}
						<tr class={dataTemplate.row}>
							<td class={dataTemplate.tdStrong}>{key.label ?? '—'}</td>
							<td class={dataTemplate.tdMono}>{key.fingerprint}</td>
							<td class={cn(dataTemplate.td, 'text-center')}>
								<div class="flex items-center justify-center gap-1.5">
									<span class="h-2 w-2 rounded-full {healthDot(key.health)}"></span>
									<span>{key.health}</span>
								</div>
							</td>
							<td class={cn(dataTemplate.tdMonoStrong, 'text-right tabular-nums')}>{fmtNum(key.total_requests)}</td>
							<td class="px-4 py-3 text-right font-mono text-xs tabular-nums {key.total_errors > 0 ? 'text-red-600 dark:text-red-400' : 'text-zinc-500 dark:text-zinc-400'}">{fmtNum(key.total_errors)}</td>
							<td class={cn(dataTemplate.td, 'text-right')}>{key.cooldown_until ? fmtDate(key.cooldown_until) : '—'}</td>
							<td class={dataTemplate.td}>{fmtDate(key.created_at)}</td>
							{#if isPlatformAdmin}
								<td class={cn(dataTemplate.td, 'text-right')}>
									<Button variant="ghost" size="sm" onclick={() => (revokingId = key.id)}>
										<span class="text-red-600 dark:text-red-400">撤销</span>
									</Button>
								</td>
							{/if}
						</tr>
					{/each}
				</DataTable>
			{/if}

		{:else if activeTab === 'models'}
			<div class="space-y-4">
				<DataToolbar badgesVisible={false}>
					{#snippet controls()}
						<p class="text-sm text-zinc-600 dark:text-zinc-300">
							{channelStats?.channel.supported_models?.length ?? 0} 个已配置模型
						</p>
					{/snippet}
					{#snippet actions()}
						{#if isPlatformAdmin}
							<Button variant="outline" size="sm" onclick={handleProbe} disabled={probing}>
								{probing ? 'Probing...' : 'Probe 上游模型'}
							</Button>
						{/if}
					{/snippet}
				</DataToolbar>

				{#if channelStats.channel.supported_models && channelStats.channel.supported_models.length > 0}
					<div class="grid grid-cols-2 gap-2 md:grid-cols-3 lg:grid-cols-4">
						{#each channelStats.channel.supported_models as m}
							<div class="truncate rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-700 dark:border-zinc-700 dark:bg-zinc-800 dark:text-zinc-300" title={m}>{m}</div>
						{/each}
					</div>
				{:else}
					<StatePanel title="未配置模型列表" description="通配所有模型。" />
				{/if}

				{#if probeResult}
					<SectionCard title={`Probe 发现 ${probeResult.models.length} 个模型`}>
						<div class="grid max-h-48 grid-cols-2 gap-1 overflow-y-auto md:grid-cols-3">
							{#each probeResult.models as m}
								<div class="px-2 py-1 font-mono text-xs text-zinc-600 dark:text-zinc-400">{m}</div>
							{/each}
						</div>
						<div class="mt-4 flex justify-end gap-2">
							<Button variant="outline" size="sm" onclick={() => (probeResult = null)}>关闭</Button>
							<Button size="sm" onclick={handleSyncModels}>同步到 Channel</Button>
						</div>
					</SectionCard>
				{/if}

				{#if channelStats.channel.model_mapping && Object.keys(channelStats.channel.model_mapping).length > 0}
					<SectionCard title="模型映射">
						<div class="space-y-1">
							{#each Object.entries(channelStats.channel.model_mapping) as [alias, target]}
								<div class="flex items-center gap-2 text-xs">
									<span class="font-mono text-zinc-600 dark:text-zinc-400">{alias}</span>
									<span class="text-zinc-400">→</span>
									<span class="font-mono text-zinc-900 dark:text-zinc-100">{target}</span>
								</div>
							{/each}
						</div>
					</SectionCard>
				{/if}
			</div>

		{:else if activeTab === 'logs'}
			{#if logsLoading}
				<p class="text-sm text-zinc-500 dark:text-zinc-400">加载中...</p>
			{:else}
				<DataTable isEmpty={logs.length === 0} emptyColspan={4}>
					{#snippet head()}
						<tr>
							<th class={dataTemplate.th}>时间</th>
							<th class={dataTemplate.th}>动作</th>
							<th class={dataTemplate.th}>操作者</th>
							<th class={dataTemplate.th}>结果</th>
						</tr>
					{/snippet}
					{#snippet empty()}
						暂无相关日志。
					{/snippet}
					{#each logs as log}
						<tr class={dataTemplate.row}>
							<td class={dataTemplate.td}>{fmtDate(log.ts)}</td>
							<td class={dataTemplate.tdMonoStrong}>{log.action}</td>
							<td class={dataTemplate.td}>{log.actor_kind}:{log.actor_id ? shortId(log.actor_id) : '—'}</td>
							<td class={dataTemplate.td}>
								<Badge variant={log.outcome === 'success' ? 'success' : 'danger'}>{log.outcome}</Badge>
							</td>
						</tr>
					{/each}
				</DataTable>
			{/if}
		{/if}
	{/if}
</PageShell>
