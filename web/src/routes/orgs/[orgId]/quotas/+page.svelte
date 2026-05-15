<!-- /orgs/[orgId]/quotas — Org 配额管理 -->
<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { listQuotas, upsertQuota, deleteQuota } from '$lib/api.js';
	import type { Quota, UpsertQuotaRequest } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Card from '$lib/components/ui/Card.svelte';

	let orgId = $derived($page.params.orgId ?? '');

	let quotas = $state<Quota[]>([]);
	let loading = $state(true);
	let error = $state('');

	// Create/Edit form
	let showForm = $state(false);
	let formScopeKind = $state('org');
	let formScopeId = $state('');
	let formDimension = $state('rpm');
	let formLimitValue = $state('');
	let formModelFilter = $state('');
	let formWindowSeconds = $state('');
	let submitting = $state(false);
	let formError = $state('');

	// Delete confirm
	let deletingId = $state<string | null>(null);
	let deleting = $state(false);

	// Toast
	let toast = $state('');

	// Grouped quotas
	let grouped = $derived.by(() => {
		const groups: Record<string, Quota[]> = {};
		for (const q of quotas) {
			if (!groups[q.scope_kind]) groups[q.scope_kind] = [];
			groups[q.scope_kind].push(q);
		}
		return groups;
	});

	onMount(async () => {
		await loadQuotas();
	});

	async function loadQuotas() {
		loading = true;
		error = '';
		try {
			quotas = await listQuotas(orgId);
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

	function openCreateForm() {
		formScopeKind = 'org';
		formScopeId = orgId;
		formDimension = 'rpm';
		formLimitValue = '';
		formModelFilter = '';
		formWindowSeconds = '';
		formError = '';
		showForm = true;
	}

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		const limitNum = Number(formLimitValue);
		if (!formScopeId || !limitNum || limitNum <= 0) {
			formError = 'scope_id 和 limit_value (> 0) 必填';
			return;
		}
		submitting = true;
		formError = '';
		try {
			const req: UpsertQuotaRequest = {
				scope_kind: formScopeKind,
				scope_id: formScopeId,
				dimension: formDimension,
				limit_value: limitNum,
				model_filter: formModelFilter || undefined,
				window_seconds: formWindowSeconds ? Number(formWindowSeconds) : undefined
			};
			await upsertQuota(orgId, req);
			quotas = await listQuotas(orgId);
			showForm = false;
			showToast('配额已保存');
		} catch (err: any) {
			formError = err?.message ?? '保存失败';
		} finally {
			submitting = false;
		}
	}

	async function handleDelete() {
		if (!deletingId) return;
		deleting = true;
		try {
			await deleteQuota(orgId, deletingId);
			quotas = quotas.filter((q) => q.id !== deletingId);
			deletingId = null;
			showToast('配额已删除');
		} catch (err: any) {
			error = err?.message ?? '删除失败';
			deletingId = null;
		} finally {
			deleting = false;
		}
	}

	function dimensionLabel(dim: string): string {
		const labels: Record<string, string> = {
			rpm: 'RPM (requests/min)',
			tpm: 'TPM (tokens/min)',
			concurrent: '并发数',
			daily_budget_usd: '日预算 (USD)',
			monthly_budget_usd: '月预算 (USD)',
			lifetime_tokens: '终身 Tokens'
		};
		return labels[dim] ?? dim;
	}

	function scopeLabel(kind: string): string {
		const labels: Record<string, string> = {
			org: '组织级',
			project: '项目级',
			api_key: 'API Key 级'
		};
		return labels[kind] ?? kind;
	}
</script>

<!-- Toast -->
{#if toast}
	<div class="fixed top-4 right-4 z-50 bg-zinc-900 text-white px-4 py-2 rounded-lg shadow-lg text-sm">
		{toast}
	</div>
{/if}

<!-- Delete confirmation -->
{#if deletingId}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center">
		<Card class="p-6 max-w-sm w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-2">确认删除配额</h3>
			<p class="text-sm text-zinc-600 dark:text-zinc-400 mb-4">删除后该维度限制将失效。</p>
			<div class="flex gap-2 justify-end">
				<Button variant="outline" onclick={() => (deletingId = null)} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</div>
{/if}

<!-- Create/Edit form modal -->
{#if showForm}
	<div class="fixed inset-0 z-40 bg-black/50 flex items-center justify-center">
		<Card class="p-6 max-w-lg w-full mx-4">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-4">添加配额</h3>
			<form onsubmit={handleSubmit} class="space-y-3">
				<div class="grid grid-cols-2 gap-3">
					<div>
						<label for="q-scope" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">作用域类型</label>
						<select id="q-scope" bind:value={formScopeKind} disabled={submitting}
							class="flex h-10 w-full rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300">
							<option value="org">组织</option>
							<option value="project">项目</option>
							<option value="api_key">API Key</option>
						</select>
					</div>
					<div>
						<label for="q-dim" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">维度</label>
						<select id="q-dim" bind:value={formDimension} disabled={submitting}
							class="flex h-10 w-full rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300">
							<option value="rpm">RPM</option>
							<option value="tpm">TPM</option>
							<option value="concurrent">并发数</option>
							<option value="daily_budget_usd">日预算 USD</option>
							<option value="monthly_budget_usd">月预算 USD</option>
							<option value="lifetime_tokens">终身 Tokens</option>
						</select>
					</div>
				</div>
				<div>
					<label for="q-scope-id" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Scope ID (UUID)</label>
					<Input id="q-scope-id" placeholder={orgId} bind:value={formScopeId} disabled={submitting} />
					<p class="text-xs text-zinc-500 dark:text-zinc-400 mt-1">组织级填 Org ID，项目级填 Project ID，Key 级填 API Key ID</p>
				</div>
				<div class="grid grid-cols-2 gap-3">
					<div>
						<label for="q-limit" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">限额</label>
						<Input id="q-limit" type="number" bind:value={formLimitValue} disabled={submitting} />
					</div>
					<div>
						<label for="q-window" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">窗口 (秒，可选)</label>
						<Input id="q-window" type="number" placeholder="60" bind:value={formWindowSeconds} disabled={submitting} />
					</div>
				</div>
				<div>
					<label for="q-model" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">模型过滤器 (可选)</label>
					<Input id="q-model" placeholder="gpt-4o" bind:value={formModelFilter} disabled={submitting} />
				</div>
				{#if formError}
					<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2">{formError}</p>
				{/if}
				<div class="flex gap-2 justify-end">
					<Button variant="outline" type="button" onclick={() => (showForm = false)}>取消</Button>
					<Button type="submit" disabled={submitting}>
						{submitting ? '保存中...' : '保存'}
					</Button>
				</div>
			</form>
		</Card>
	</div>
{/if}

<div>
	<!-- 面包屑 -->
	<div class="px-6 py-6">
		<p class="text-xs text-zinc-500 dark:text-zinc-400 mb-1">组织 / {shortId(orgId)}... / 配额</p>
		<div class="flex items-center justify-between mb-6">
			<div>
				<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">配额管理</h1>
				<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-1">管理组织及其下属项目、API Key 的配额规则。</p>
			</div>
			<Button onclick={openCreateForm}>+ 添加配额</Button>
		</div>

		{#if loading}
			<p class="text-zinc-600 dark:text-zinc-300">加载中...</p>
		{:else if error}
			<Card class="p-6">
				<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
			</Card>
		{:else if quotas.length === 0}
			<Card class="p-6">
				<p class="text-zinc-600 dark:text-zinc-300 text-sm">暂无配额规则。点击上方按钮创建。</p>
			</Card>
		{:else}
			{#each Object.entries(grouped) as [scopeKind, items]}
				<div class="mb-6">
					<h2 class="text-sm font-semibold text-zinc-700 dark:text-zinc-300 uppercase tracking-wider mb-3">
						{scopeLabel(scopeKind)}
					</h2>
					<div class="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
						<table class="w-full text-sm">
							<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
								<tr>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">维度</th>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">限额</th>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">Scope ID</th>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">模型</th>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">窗口</th>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">状态</th>
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">操作</th>
								</tr>
							</thead>
							<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
								{#each items as q}
									<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800 transition-colors">
										<td class="px-4 py-3 text-zinc-900 dark:text-zinc-100 font-medium">{dimensionLabel(q.dimension)}</td>
										<td class="px-4 py-3 font-mono text-zinc-700 dark:text-zinc-300">{q.limit_value}</td>
										<td class="px-4 py-3 font-mono text-xs text-zinc-600 dark:text-zinc-300">{shortId(q.scope_id)}...</td>
										<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400">{q.model_filter ?? '全部'}</td>
										<td class="px-4 py-3 text-zinc-600 dark:text-zinc-400">
											{q.window_seconds ? `${q.window_seconds}s` : '—'}
										</td>
										<td class="px-4 py-3">
											{#if q.enabled}
												<span class="inline-block px-2 py-0.5 rounded text-xs font-medium bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400">启用</span>
											{:else}
												<span class="inline-block px-2 py-0.5 rounded text-xs font-medium bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300">禁用</span>
											{/if}
										</td>
										<td class="px-4 py-3 text-right">
											<Button variant="ghost" size="sm" onclick={() => (deletingId = q.id)}>
												<span class="text-red-600 dark:text-red-400">删除</span>
											</Button>
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				</div>
			{/each}
		{/if}
	</div>
</div>
