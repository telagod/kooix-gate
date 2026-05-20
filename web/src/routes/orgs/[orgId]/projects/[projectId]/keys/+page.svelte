<!-- /orgs/[orgId]/projects/[projectId]/keys — API Key 管理 -->
<script lang="ts">
	import { shortId, rawId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { listKeys, createKey, revokeKey } from '$lib/api.js';
	import type { ApiKey, CreateKeyResponse } from '$lib/api.js';
	import Alert from '$lib/components/ui/Alert.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Field from '$lib/components/ui/Field.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate, text } from '$lib/design';
	import { ArrowLeft, Check, Copy, KeyRound, Plus, RefreshCw, Trash2, X } from 'lucide-svelte';

	let orgId = $derived($page.params.orgId ?? '');
	let projectId = $derived($page.params.projectId ?? '');

	let keys = $state<ApiKey[]>([]);
	let loading = $state(true);
	let error = $state('');

	// Create
	let showCreate = $state(false);
	let newName = $state('');
	let creating = $state(false);
	let createError = $state('');
	let createdKey = $state<CreateKeyResponse | null>(null);
	let copied = $state(false);

	// Revoke confirm
	let revokingId = $state<string | null>(null);
	let revoking = $state(false);

	// Toast
	let toast = $state('');

	onMount(async () => {
		await loadKeys();
	});

	async function loadKeys() {
		loading = true;
		error = '';
		try {
			keys = await listKeys(orgId, projectId);
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	function showToast(msg: string) {
		toast = msg;
		setTimeout(() => (toast = ''), 3000);
	}

	async function handleCreate(e: SubmitEvent) {
		e.preventDefault();
		if (!newName.trim()) return;
		creating = true;
		createError = '';
		try {
			const result = await createKey(orgId, projectId, newName.trim());
			createdKey = result;
			// Refresh list
			keys = await listKeys(orgId, projectId);
			newName = '';
		} catch (err: any) {
			createError = err?.message ?? '创建失败';
		} finally {
			creating = false;
		}
	}

	async function copyKey() {
		if (!createdKey) return;
		try {
			await navigator.clipboard.writeText(createdKey.plaintext);
			copied = true;
			setTimeout(() => (copied = false), 2000);
		} catch {
			// fallback
		}
	}

	function dismissCreated() {
		createdKey = null;
		showCreate = false;
		showToast('API Key 已创建');
	}

	async function handleRevoke() {
		if (!revokingId) return;
		revoking = true;
		try {
			await revokeKey(orgId, projectId, revokingId);
			keys = await listKeys(orgId, projectId);
			revokingId = null;
			showToast('API Key 已撤销');
		} catch (err: any) {
			error = err?.message ?? '撤销失败';
			revokingId = null;
		} finally {
			revoking = false;
		}
	}

	function formatDate(s: string | null): string {
		if (!s) return '—';
		try {
			return new Date(s).toLocaleDateString('zh-CN', {
				year: 'numeric',
				month: '2-digit',
				day: '2-digit',
				hour: '2-digit',
				minute: '2-digit'
			});
		} catch {
			return s;
		}
	}

	function keyStatusVariant(key: ApiKey): 'success' | 'danger' {
		return key.revoked ? 'danger' : 'success';
	}
</script>

<!-- Toast -->
{#if toast}
	<div class="fixed right-4 top-4 z-50 rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white shadow-lg dark:bg-zinc-100 dark:text-zinc-900">
		{toast}
	</div>
{/if}

<!-- Revoke confirmation -->
{#if revokingId}
	<ModalFrame close={() => (revokingId = null)} label="取消撤销 API Key" panelClass="w-full max-w-sm">
		<Card padding="lg">
			<div class="mb-4 flex items-start gap-3">
				<div class="rounded-lg bg-red-50 p-2 text-red-600 dark:bg-red-900/20 dark:text-red-400">
					<Trash2 size={18} />
				</div>
				<div>
					<h3 class="text-lg font-semibold {text.primary}">确认撤销</h3>
					<p class="mt-1 text-sm {text.secondary}">撤销后此 API Key 将立即失效，使用该 Key 的所有请求都会被拒绝。</p>
				</div>
			</div>
			<div class="flex justify-end gap-2">
				<Button variant="outline" onclick={() => (revokingId = null)} disabled={revoking}>取消</Button>
				<Button variant="destructive" onclick={handleRevoke} disabled={revoking}>
					<Trash2 size={14} />
					{revoking ? '撤销中...' : '确认撤销'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}

<PageShell
	title="API Keys 凭据"
	description={`Project: ${shortId(projectId)} · 明文 Key 仅在创建时显示一次。`}
	eyebrow={`组织 / ${shortId(orgId)} / 项目`}
	icon={KeyRound}
	max="wide"
>
	{#snippet actions()}
		<Button variant="outline" size="sm" onclick={() => goto(`/orgs/${rawId(orgId)}/projects/${rawId(projectId)}`)}>
			<ArrowLeft size={14} />
			项目设置
		</Button>
		<Button variant="outline" size="sm" onclick={loadKeys} disabled={loading}>
			<RefreshCw size={14} class={loading ? 'animate-spin' : ''} />
			刷新
		</Button>
		<Button size="sm" onclick={() => { showCreate = !showCreate; createdKey = null; }}>
			{#if showCreate}
				<X size={14} />
				取消
			{:else}
				<Plus size={14} />
				创建 Key
			{/if}
		</Button>
	{/snippet}

	<Alert variant="warning" class="mb-6">
		API Key 明文仅在创建时显示一次，请立即复制保存；撤销会立即阻断后续请求。
	</Alert>

	{#if showCreate && !createdKey}
		<Card padding="md" class="mb-6">
			<div class="mb-4">
				<p class="text-base font-semibold {text.primary}">创建新 Key</p>
				<p class="text-xs {text.muted}">建议使用用途 + 环境命名，例如 production-backend。</p>
			</div>
			<form onsubmit={handleCreate} class="space-y-4">
				<Field label="名称" for="key-name" required>
					<Input id="key-name" placeholder="production-backend" bind:value={newName} disabled={creating} />
				</Field>
				{#if createError}
					<Alert variant="danger">{createError}</Alert>
				{/if}
				<div class="flex justify-end gap-2">
					<Button variant="outline" type="button" onclick={() => (showCreate = false)}>取消</Button>
					<Button type="submit" disabled={creating || !newName.trim()}>
						<Plus size={14} />
						{creating ? '创建中...' : '创建'}
					</Button>
				</div>
			</form>
		</Card>
	{/if}

	{#if createdKey}
		<Card padding="md" variant="success" class="mb-6">
			<div class="mb-3 flex items-start gap-3">
				<div class="rounded-lg bg-green-100 p-2 text-green-700 dark:bg-green-900/30 dark:text-green-400">
					<Check size={18} />
				</div>
				<div>
					<h2 class="text-base font-semibold text-green-900 dark:text-green-300">Key 已创建</h2>
					<p class="mt-1 text-sm text-green-800 dark:text-green-400">请立即复制以下密钥，此页面关闭后将无法再次查看。</p>
				</div>
			</div>
			<div class="flex flex-col gap-2 md:flex-row md:items-center">
				<code class="flex-1 select-all break-all rounded-md border border-green-300 bg-white px-3 py-2 font-mono text-sm text-zinc-900 dark:border-green-700 dark:bg-zinc-900 dark:text-zinc-100">
					{createdKey.plaintext}
				</code>
				<Button size="sm" onclick={copyKey}>
					<Copy size={14} />
					{copied ? '已复制' : '复制'}
				</Button>
			</div>
			<p class="mt-2 text-xs text-green-700 dark:text-green-400">Name 名称: {createdKey.name} | Prefix 前缀: {createdKey.prefix}</p>
			<div class="mt-3 flex justify-end">
				<Button variant="outline" size="sm" onclick={dismissCreated}>我已保存，关闭</Button>
			</div>
		</Card>
	{/if}

	{#if loading}
		<StatePanel title="正在读取 API Keys 凭据" description="吾正在拉取当前 Project 的 Key 列表。" icon={KeyRound} />
	{:else if error}
		<StatePanel title="API Keys 凭据加载失败" description={error} icon={KeyRound} variant="danger">
			{#snippet actions()}
				<Button variant="outline" onclick={loadKeys}>重试</Button>
			{/snippet}
		</StatePanel>
	{:else}
		<DataTable isEmpty={keys.length === 0} emptyColspan={6}>
			{#snippet head()}
				<tr>
					<th class={dataTemplate.th}>名称</th>
					<th class={dataTemplate.th}>Key 前缀</th>
					<th class={dataTemplate.th}>创建时间</th>
					<th class={dataTemplate.th}>最后使用</th>
					<th class={dataTemplate.th}>状态</th>
					<th class="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">操作</th>
				</tr>
			{/snippet}

			{#snippet empty()}
				<div class="flex flex-col items-center">
					<KeyRound size={34} class="mb-3 text-zinc-300 dark:text-zinc-600" />
					<p class="text-sm font-medium {text.secondary}">暂无 API Key</p>
					<p class="mt-1 text-xs {text.muted}">点击右上角「创建 Key」生成 Project 凭据。</p>
				</div>
			{/snippet}

			{#each keys as key}
				<tr class={cn(dataTemplate.row, key.revoked && 'opacity-50')}>
					<td class={dataTemplate.tdStrong}>{key.name}</td>
					<td class={dataTemplate.tdMono}>
						{key.prefix ? `${key.prefix}...${key.last4}` : '—'}
					</td>
					<td class={cn(dataTemplate.td, 'text-xs')}>{formatDate(key.created_at)}</td>
					<td class={cn(dataTemplate.td, 'text-xs')}>{formatDate(key.last_used_at)}</td>
					<td class={dataTemplate.td}>
						<Badge variant={keyStatusVariant(key)}>{key.revoked ? '已撤销' : '活跃'}</Badge>
					</td>
					<td class="px-4 py-3 text-right">
						{#if !key.revoked}
							<Button variant="ghost" size="sm" onclick={() => (revokingId = key.id)} class={text.danger}>
								<Trash2 size={14} />
								撤销
							</Button>
						{:else}
							<span class="text-xs {text.muted}">—</span>
						{/if}
					</td>
				</tr>
			{/each}
		</DataTable>
	{/if}
</PageShell>
