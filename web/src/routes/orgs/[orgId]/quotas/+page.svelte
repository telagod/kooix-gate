<!-- /orgs/[orgId]/quotas — Org 配额管理 -->
<script lang="ts">
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import { AlertTriangle, CheckCircle2, Gauge, GitCompareArrows, Plus, RefreshCw, Search, Trash2 } from 'lucide-svelte';
	import { shortId } from '$lib/id.js';
	import {
		deleteQuota,
		explainQuota,
		listQuotas,
		reconcileQuotas,
		upsertQuota
	} from '$lib/api.js';
	import type { Quota, QuotaExplainRule, QuotaReconcileRow, UpsertQuotaRequest } from '$lib/api.js';
	import Alert from '$lib/components/ui/Alert.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import Select from '$lib/components/ui/Select.svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import SectionCard from '$lib/components/templates/SectionCard.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate, text } from '$lib/design';
	import {
		buildQuotaWizardRequests,
		hasQuotaWizardLimit,
		previewQuotaWizardDraft,
		quotaWizardExplainDimension,
		type QuotaWizardDraft,
		type QuotaWizardPreviewRule
	} from '$lib/quota-wizard.js';

	const scopeOptions = [
		{ value: 'org', label: 'Org' },
		{ value: 'project', label: 'Project' },
		{ value: 'api_key', label: 'API Key' },
		{ value: 'user', label: 'User' }
	];

	const dimensionOptions = [
		{ value: 'rpm', label: 'RPM · requests/min' },
		{ value: 'tpm', label: 'TPM · tokens/min' },
		{ value: 'concurrent', label: 'Concurrent · 并发' },
		{ value: 'daily_budget_usd', label: 'Daily budget · USD' },
		{ value: 'monthly_budget_usd', label: 'Monthly budget · USD' },
		{ value: 'lifetime_budget_usd', label: 'Lifetime budget · USD' },
		{ value: 'lifetime_tokens', label: 'Lifetime tokens' }
	];

	const modeOptions = [
		{ value: 'enforce', label: 'Enforce · 实际拦截' },
		{ value: 'dry_run', label: 'Dry-run · 只记录' }
	];

	let orgId = $derived($page.params.orgId ?? '');
	let quotas = $state<Quota[]>([]);
	let loading = $state(true);
	let error = $state('');
	let toast = $state('');
	let search = $state('');
	let scopeFilter = $state('all');
	let modeFilter = $state('all');

	let showForm = $state(false);
	let formScopeKind = $state('org');
	let formScopeId = $state('');
	let formDimension = $state('rpm');
	let formLimitValue = $state('');
	let formModelFilter = $state('');
	let formWindowSeconds = $state('');
	let formMode = $state<'enforce' | 'dry_run'>('enforce');
	let submitting = $state(false);
	let formError = $state('');

	let deletingId = $state<string | null>(null);
	let deleting = $state(false);

	let explainScopeKind = $state('org');
	let explainScopeId = $state('');
	let explainDimension = $state('');
	let explainModel = $state('');
	let explainTokens = $state('1000');
	let explainCostMicros = $state('10000');
	let explaining = $state(false);
	let explainError = $state('');
	let explainRules = $state<QuotaExplainRule[]>([]);

	let showWizard = $state(false);
	let quotaWizardStep = $state(1);
	let wizardDraft = $state<QuotaWizardDraft>({
		scopeKind: 'org',
		scopeId: '',
		modelFilter: '',
		mode: 'enforce',
		rpmLimit: '60',
		tpmLimit: '120000',
		budgetUsd: '',
		budgetDimension: 'monthly_budget_usd',
		estimatedTokens: '1000',
		estimatedCostMicros: '10000'
	});
	let wizardSaving = $state(false);
	let wizardExplaining = $state(false);
	let wizardError = $state('');
	let wizardExplainRules = $state<QuotaExplainRule[]>([]);

	let reconcileRows = $state<QuotaReconcileRow[]>([]);
	let reconciling = $state(false);
	let reconcileError = $state('');

	let filteredQuotas = $derived.by(() => {
		const query = search.trim().toLowerCase();
		return quotas.filter((q) => {
			const matchesQuery =
				!query ||
				q.dimension.toLowerCase().includes(query) ||
				q.scope_kind.toLowerCase().includes(query) ||
				q.scope_id.toLowerCase().includes(query) ||
				(q.model_filter ?? '').toLowerCase().includes(query) ||
				q.id.toLowerCase().includes(query);
			const matchesScope = scopeFilter === 'all' || q.scope_kind === scopeFilter;
			const matchesMode = modeFilter === 'all' || q.mode === modeFilter;
			return matchesQuery && matchesScope && matchesMode;
		});
	});

	let grouped = $derived.by(() => {
		const order = ['org', 'project', 'api_key', 'user'];
		const groups: Record<string, Quota[]> = {};
		for (const q of filteredQuotas) {
			if (!groups[q.scope_kind]) groups[q.scope_kind] = [];
			groups[q.scope_kind].push(q);
		}
		return Object.entries(groups).sort(([a], [b]) => {
			const ai = order.indexOf(a);
			const bi = order.indexOf(b);
			return (ai === -1 ? 99 : ai) - (bi === -1 ? 99 : bi);
		});
	});

	let summary = $derived.by(() => ({
		total: quotas.length,
		enforce: quotas.filter((q) => q.mode !== 'dry_run').length,
		dryRun: quotas.filter((q) => q.mode === 'dry_run').length,
		models: quotas.filter((q) => q.model_filter && q.model_filter !== '*').length
	}));
	let hasActiveFilters = $derived(search.trim() !== '' || scopeFilter !== 'all' || modeFilter !== 'all');
	let wizardRequests = $derived(buildQuotaWizardRequests(wizardDraft));
	let wizardPreviewRows = $derived<QuotaWizardPreviewRule[]>(previewQuotaWizardDraft(wizardDraft));
	let wizardCanPreview = $derived(Boolean(wizardDraft.scopeId.trim() && hasQuotaWizardLimit(wizardDraft)));
	let wizardCanExplain = $derived(wizardCanPreview && wizardRequests.length > 0);

	onMount(async () => {
		await loadQuotas();
	});

	async function loadQuotas() {
		loading = true;
		error = '';
		try {
			quotas = await listQuotas(orgId);
			if (!explainScopeId) explainScopeId = orgId;
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
		formWindowSeconds = '60';
		formMode = 'enforce';
		formError = '';
		showForm = true;
	}

	function openQuotaWizard() {
		wizardDraft = {
			scopeKind: 'org',
			scopeId: orgId,
			modelFilter: '',
			mode: 'enforce',
			rpmLimit: '60',
			tpmLimit: '120000',
			budgetUsd: '',
			budgetDimension: 'monthly_budget_usd',
			estimatedTokens: '1000',
			estimatedCostMicros: '10000'
		};
		quotaWizardStep = 1;
		wizardError = '';
		wizardExplainRules = [];
		showWizard = true;
	}

	function closeQuotaWizard() {
		showWizard = false;
		quotaWizardStep = 1;
		wizardError = '';
		wizardExplainRules = [];
	}

	function updateWizard<K extends keyof QuotaWizardDraft>(key: K, value: QuotaWizardDraft[K]) {
		wizardDraft = { ...wizardDraft, [key]: value };
		if (key === 'scopeKind' && value === 'org') {
			wizardDraft = { ...wizardDraft, scopeId: orgId };
		}
	}

	function goWizardStep(step: number) {
		if (step <= 2 || wizardCanPreview) {
			quotaWizardStep = step;
		}
	}

	function loadRuleIntoExplain(q: Quota) {
		explainScopeKind = q.scope_kind;
		explainScopeId = q.scope_id;
		explainDimension = q.dimension;
		explainModel = q.model_filter && q.model_filter !== '*' ? q.model_filter.replace('*', 'mini') : '';
		explainRules = [];
	}

	async function handleSubmit(e: SubmitEvent) {
		e.preventDefault();
		const limitNum = Number(formLimitValue);
		if (!formScopeId || !Number.isFinite(limitNum) || limitNum <= 0) {
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
				limit_value: formLimitValue,
				model_filter: formModelFilter.trim() || undefined,
				window_seconds: formWindowSeconds ? Number(formWindowSeconds) : undefined,
				mode: formMode
			};
			await upsertQuota(orgId, req);
			quotas = await listQuotas(orgId);
			showForm = false;
			showToast(formMode === 'dry_run' ? 'Dry-run 配额已保存' : 'Enforce 配额已保存');
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

	async function handleExplain(e: SubmitEvent) {
		e.preventDefault();
		if (!explainScopeId) {
			explainError = 'scope_id 必填';
			return;
		}
		explaining = true;
		explainError = '';
		try {
			const resp = await explainQuota(orgId, {
				scope_kind: explainScopeKind,
				scope_id: explainScopeId,
				dimension: explainDimension || undefined,
				model: explainModel || undefined,
				estimated_tokens: explainTokens ? Number(explainTokens) : undefined,
				estimated_cost_micros: explainCostMicros ? Number(explainCostMicros) : undefined
			});
			explainRules = resp.rules;
		} catch (err: any) {
			explainError = err?.message ?? '解释失败';
		} finally {
			explaining = false;
		}
	}

	async function handleWizardExplain() {
		if (!wizardCanExplain) {
			wizardError = 'Scope ID 与至少一个 rpm/tpm/budget 必填';
			return;
		}
		wizardExplaining = true;
		wizardError = '';
		try {
			const resp = await explainQuota(orgId, {
				scope_kind: wizardDraft.scopeKind,
				scope_id: wizardDraft.scopeId,
				dimension: quotaWizardExplainDimension(wizardDraft),
				model: wizardDraft.modelFilter.trim() || undefined,
				estimated_tokens: wizardDraft.estimatedTokens ? Number(wizardDraft.estimatedTokens) : undefined,
				estimated_cost_micros: wizardDraft.estimatedCostMicros ? Number(wizardDraft.estimatedCostMicros) : undefined
			});
			wizardExplainRules = resp.rules;
			quotaWizardStep = 4;
		} catch (err: any) {
			wizardError = err?.message ?? 'Explain 失败';
		} finally {
			wizardExplaining = false;
		}
	}

	async function handleWizardSave() {
		if (!wizardCanPreview) {
			wizardError = 'Scope ID 与至少一个 rpm/tpm/budget 必填';
			return;
		}
		wizardSaving = true;
		wizardError = '';
		try {
			for (const req of wizardRequests) {
				await upsertQuota(orgId, req);
			}
			quotas = await listQuotas(orgId);
			showToast(`Quota wizard 已保存 ${wizardRequests.length} 条策略`);
			closeQuotaWizard();
		} catch (err: any) {
			wizardError = err?.message ?? '保存失败';
		} finally {
			wizardSaving = false;
		}
	}

	async function handleReconcile() {
		reconciling = true;
		reconcileError = '';
		try {
			const resp = await reconcileQuotas(orgId);
			reconcileRows = resp.rows;
		} catch (err: any) {
			reconcileError = err?.message ?? '对账失败';
		} finally {
			reconciling = false;
		}
	}

	function dimensionLabel(dim: string): string {
		const labels: Record<string, string> = {
			rpm: 'RPM',
			tpm: 'TPM',
			concurrent: 'Concurrent',
			daily_budget_usd: 'Daily budget',
			monthly_budget_usd: 'Monthly budget',
			lifetime_budget_usd: 'Lifetime budget',
			lifetime_tokens: 'Lifetime tokens'
		};
		return labels[dim] ?? dim;
	}

	function dimensionUnit(dim: string): string {
		if (dim.endsWith('budget_usd')) return 'USD';
		if (dim.includes('tokens') || dim === 'tpm') return 'tokens';
		if (dim === 'concurrent') return 'inflight';
		return 'requests';
	}

	function scopeLabel(kind: string): string {
		const labels: Record<string, string> = {
			org: 'Org 组织级',
			project: 'Project 项目级',
			api_key: 'API Key 级',
			user: 'User 用户级'
		};
		return labels[kind] ?? kind;
	}

	function formatNumber(value: number | null | undefined): string {
		if (value === null || value === undefined) return '—';
		return new Intl.NumberFormat('en-US').format(value);
	}

	function formatWizardLimit(row: QuotaWizardPreviewRule): string {
		if (row.unit === 'micros') return `$${(row.limit / 1_000_000).toFixed(2)}`;
		return formatNumber(row.limit);
	}

	function formatWizardEstimate(row: QuotaWizardPreviewRule): string {
		if (row.unit === 'micros') return `$${(row.estimated / 1_000_000).toFixed(6)}`;
		return formatNumber(row.estimated);
	}

	function formatReset(rule: QuotaExplainRule): string {
		if (rule.reset_at) return new Date(rule.reset_at).toLocaleString();
		if (rule.retry_after_ms) return `${Math.ceil(rule.retry_after_ms / 1000)}s`;
		return '—';
	}

	function resetFilters() {
		search = '';
		scopeFilter = 'all';
		modeFilter = 'all';
	}
</script>

{#if toast}
	<div class="fixed right-4 top-4 z-50 rounded-lg bg-zinc-900 px-4 py-2 text-sm text-white shadow-lg dark:bg-zinc-100 dark:text-zinc-900">
		{toast}
	</div>
{/if}

{#if deletingId}
	<ModalFrame close={() => (deletingId = null)} panelClass="w-full max-w-sm">
		<Card padding="lg">
			<div class="mb-4 flex items-start gap-3">
				<div class="flex h-9 w-9 items-center justify-center rounded-lg bg-red-50 text-red-600 dark:bg-red-900/20 dark:text-red-400">
					<AlertTriangle size={18} />
				</div>
				<div>
					<h3 class="text-lg font-semibold {text.primary}">确认删除配额</h3>
					<p class="mt-1 text-sm {text.secondary}">删除后该维度限制立即失效，Redis 现有计数不会自动清空。</p>
				</div>
			</div>
			<div class="flex justify-end gap-2">
				<Button variant="outline" onclick={() => (deletingId = null)} disabled={deleting}>取消</Button>
				<Button variant="destructive" onclick={handleDelete} disabled={deleting}>
					{deleting ? '删除中...' : '确认删除'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}

{#if showForm}
	<ModalFrame close={() => (showForm = false)} panelClass="w-full max-w-2xl">
		<Card padding="lg" class="max-h-[90vh] overflow-y-auto">
			<div class="mb-5 flex items-start justify-between gap-3">
				<div>
					<p class="text-xs font-semibold uppercase tracking-widest {text.muted}">Policy Rule</p>
					<h3 class="mt-1 text-lg font-semibold {text.primary}">添加配额策略</h3>
					<p class="mt-1 text-sm {text.secondary}">user × model / api_key × model 均可用 model_filter 精确收束。</p>
				</div>
				<Badge variant={formMode === 'dry_run' ? 'warning' : 'default'}>{formMode}</Badge>
			</div>

			<form onsubmit={handleSubmit} class="space-y-4">
				<div class="grid gap-3 md:grid-cols-3">
					<div>
						<label for="q-scope" class="mb-1 block text-sm font-medium {text.secondary}">作用域</label>
						<Select id="q-scope" bind:value={formScopeKind} options={scopeOptions} disabled={submitting} />
					</div>
					<div>
						<label for="q-dim" class="mb-1 block text-sm font-medium {text.secondary}">维度</label>
						<Select id="q-dim" bind:value={formDimension} options={dimensionOptions} disabled={submitting} />
					</div>
					<div>
						<label for="q-mode" class="mb-1 block text-sm font-medium {text.secondary}">模式</label>
						<Select id="q-mode" bind:value={formMode} options={modeOptions} disabled={submitting} />
					</div>
				</div>

				<div>
					<label for="q-scope-id" class="mb-1 block text-sm font-medium {text.secondary}">Scope ID</label>
					<Input id="q-scope-id" placeholder={orgId} bind:value={formScopeId} disabled={submitting} />
					<p class="mt-1 text-xs {text.muted}">Org 填当前 Org ID；Project / API Key / User 填对应 UUID 或 typed ID。</p>
				</div>

				<div class="grid gap-3 md:grid-cols-3">
					<div>
						<label for="q-limit" class="mb-1 block text-sm font-medium {text.secondary}">限额</label>
						<Input id="q-limit" type="number" step="any" min="0" bind:value={formLimitValue} disabled={submitting} />
					</div>
					<div>
						<label for="q-window" class="mb-1 block text-sm font-medium {text.secondary}">窗口秒</label>
						<Input id="q-window" type="number" min="1" placeholder="60" bind:value={formWindowSeconds} disabled={submitting} />
					</div>
					<div>
						<label for="q-model" class="mb-1 block text-sm font-medium {text.secondary}">Model filter</label>
						<Input id="q-model" placeholder="gpt-4o*" bind:value={formModelFilter} disabled={submitting} />
					</div>
				</div>

				{#if formError}
					<Alert variant="danger">{formError}</Alert>
				{/if}

				<div class="flex justify-end gap-2">
					<Button variant="outline" type="button" onclick={() => (showForm = false)}>取消</Button>
					<Button type="submit" disabled={submitting}>{submitting ? '保存中...' : '保存策略'}</Button>
				</div>
			</form>
		</Card>
	</ModalFrame>
{/if}

{#if showWizard}
	<ModalFrame close={closeQuotaWizard} panelClass="w-full max-w-4xl">
		<Card class="max-h-[90vh] overflow-y-auto">
			<div class="border-b border-zinc-200 bg-zinc-50 p-5 dark:border-zinc-700 dark:bg-zinc-800/50">
				<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
					<div>
						<p class="text-xs font-semibold uppercase tracking-widest {text.muted}">Quota wizard</p>
						<h3 class="mt-1 text-lg font-semibold {text.primary}">新建配额策略</h3>
						<p class="mt-1 text-sm {text.secondary}">按 scope、model filter、rpm/tpm/budget 一次生成多条 policy，并先跑 explain 预览。</p>
					</div>
					<Badge variant={wizardDraft.mode === 'dry_run' ? 'warning' : 'default'}>{wizardDraft.mode}</Badge>
				</div>
				<div class="mt-4 grid gap-2 md:grid-cols-4">
					{#each [
						{ step: 1, label: 'Scope', hint: '选择作用域' },
						{ step: 2, label: 'Model filter', hint: '限定模型' },
						{ step: 3, label: 'Limits', hint: '输入 rpm/tpm/budget' },
						{ step: 4, label: 'Explain', hint: '预览 would-deny' }
					] as item}
						<button
							type="button"
							class={cn(
								'rounded-lg border px-3 py-2 text-left transition-colors',
								quotaWizardStep === item.step
									? 'border-zinc-900 bg-zinc-900 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900'
									: 'border-zinc-200 bg-white text-zinc-700 hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300 dark:hover:bg-zinc-800',
								item.step > 3 && !wizardCanPreview && 'cursor-not-allowed opacity-50'
							)}
							onclick={() => goWizardStep(item.step)}
							disabled={item.step > 3 && !wizardCanPreview}
						>
							<div class="flex items-center gap-2 text-xs font-semibold">
								<span class="flex h-5 w-5 items-center justify-center rounded-full border border-current text-[10px]">{item.step}</span>
								{item.label}
							</div>
							<p class={cn('mt-1 text-[11px]', quotaWizardStep === item.step ? 'text-zinc-200 dark:text-zinc-700' : 'text-zinc-500 dark:text-zinc-400')}>{item.hint}</p>
						</button>
					{/each}
				</div>
			</div>

			<div class="p-5">
				{#if quotaWizardStep === 1}
					<div class="grid gap-4 lg:grid-cols-[1fr_320px]">
						<div class="space-y-4">
							<div class="grid gap-3 md:grid-cols-2">
								<div>
									<label for="qw-scope-kind" class="mb-1 block text-sm font-medium {text.secondary}">Scope</label>
									<Select id="qw-scope-kind" value={wizardDraft.scopeKind} options={scopeOptions} onchange={(event) => updateWizard('scopeKind', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-mode" class="mb-1 block text-sm font-medium {text.secondary}">Mode</label>
									<Select id="qw-mode" value={wizardDraft.mode} options={modeOptions} onchange={(event) => updateWizard('mode', event.currentTarget.value as QuotaWizardDraft['mode'])} />
								</div>
							</div>
							<div>
								<label for="qw-scope-id" class="mb-1 block text-sm font-medium {text.secondary}">Scope ID</label>
								<Input id="qw-scope-id" placeholder={orgId} value={wizardDraft.scopeId} oninput={(event) => updateWizard('scopeId', event.currentTarget.value)} />
								<p class="mt-1 text-xs {text.muted}">Org 默认当前 Org；Project / API Key / User 填对应 UUID 或 typed ID。</p>
							</div>
						</div>
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">Scope preview</p>
							<div class="mt-3 space-y-3 text-sm">
								<div class="flex items-center justify-between gap-3">
									<span class={text.muted}>Kind</span>
									<span class="font-mono {text.primary}">{wizardDraft.scopeKind}</span>
								</div>
								<div class="flex items-center justify-between gap-3">
									<span class={text.muted}>ID</span>
									<span class="font-mono {text.primary}">{shortId(wizardDraft.scopeId)}</span>
								</div>
								<p class="rounded-lg border border-zinc-200 bg-white p-3 text-xs {text.muted} dark:border-zinc-700 dark:bg-zinc-900">
									后端会校验 scope 是否归属当前 Org；越权 Project / API Key / User 会返回 404。
								</p>
							</div>
						</div>
					</div>
				{:else if quotaWizardStep === 2}
					<div class="grid gap-4 lg:grid-cols-[1fr_320px]">
						<div class="space-y-4">
							<div>
								<label for="qw-model-filter" class="mb-1 block text-sm font-medium {text.secondary}">Model filter</label>
								<Input id="qw-model-filter" placeholder="gpt-4o* / claude-3-5-sonnet / 留空=全部" value={wizardDraft.modelFilter} oninput={(event) => updateWizard('modelFilter', event.currentTarget.value)} />
								<p class="mt-1 text-xs {text.muted}">支持精确模型名或简单 wildcard；留空等同全部模型。</p>
							</div>
							<div class="grid gap-2 md:grid-cols-3">
								{#each ['', 'gpt-4o*', 'claude-*'] as preset}
									<button
										type="button"
										class="rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs font-mono text-zinc-700 hover:bg-zinc-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
										onclick={() => updateWizard('modelFilter', preset)}
									>
										{preset || '全部模型'}
									</button>
								{/each}
							</div>
						</div>
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">Filter effect</p>
							<p class="mt-3 text-sm {text.secondary}">
								{#if wizardDraft.modelFilter.trim()}
									只匹配 <span class="font-mono {text.primary}">{wizardDraft.modelFilter.trim()}</span>。
								{:else}
									匹配所有模型，适合作为 Org / Project 默认保护网。
								{/if}
							</p>
						</div>
					</div>
				{:else if quotaWizardStep === 3}
					<div class="grid gap-4 lg:grid-cols-[1fr_360px]">
						<div class="space-y-4">
							<div class="grid gap-3 md:grid-cols-3">
								<div>
									<label for="qw-rpm" class="mb-1 block text-sm font-medium {text.secondary}">RPM</label>
									<Input id="qw-rpm" type="number" min="0" placeholder="60" value={wizardDraft.rpmLimit} oninput={(event) => updateWizard('rpmLimit', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-tpm" class="mb-1 block text-sm font-medium {text.secondary}">TPM</label>
									<Input id="qw-tpm" type="number" min="0" placeholder="120000" value={wizardDraft.tpmLimit} oninput={(event) => updateWizard('tpmLimit', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-budget" class="mb-1 block text-sm font-medium {text.secondary}">Budget USD</label>
									<Input id="qw-budget" type="number" min="0" step="0.01" placeholder="25" value={wizardDraft.budgetUsd} oninput={(event) => updateWizard('budgetUsd', event.currentTarget.value)} />
								</div>
							</div>
							<div class="grid gap-3 md:grid-cols-3">
								<div>
									<label for="qw-budget-dim" class="mb-1 block text-sm font-medium {text.secondary}">Budget window</label>
									<Select
										id="qw-budget-dim"
										value={wizardDraft.budgetDimension}
										options={[
											{ value: 'daily_budget_usd', label: 'Daily budget' },
											{ value: 'monthly_budget_usd', label: 'Monthly budget' },
											{ value: 'lifetime_budget_usd', label: 'Lifetime budget' }
										]}
										onchange={(event) => updateWizard('budgetDimension', event.currentTarget.value)}
									/>
								</div>
								<div>
									<label for="qw-est-tokens" class="mb-1 block text-sm font-medium {text.secondary}">Estimated tokens</label>
									<Input id="qw-est-tokens" type="number" min="0" value={wizardDraft.estimatedTokens} oninput={(event) => updateWizard('estimatedTokens', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-est-cost" class="mb-1 block text-sm font-medium {text.secondary}">Estimated cost micros</label>
									<Input id="qw-est-cost" type="number" min="0" value={wizardDraft.estimatedCostMicros} oninput={(event) => updateWizard('estimatedCostMicros', event.currentTarget.value)} />
								</div>
							</div>
						</div>
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">Local preview</p>
							{#if wizardPreviewRows.length === 0}
								<p class="mt-3 text-sm {text.muted}">至少输入 rpm、tpm、budget 中的一项。</p>
							{:else}
								<div class="mt-3 space-y-2">
									{#each wizardPreviewRows as row}
										<div class="rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-700 dark:bg-zinc-900">
											<div class="flex items-center justify-between gap-3">
												<span class="font-mono text-xs {text.primary}">{row.dimension}</span>
												<Badge variant={row.wouldDeny ? 'danger' : 'success'}>{row.wouldDeny ? 'would deny' : 'pass'}</Badge>
											</div>
											<div class="mt-2 grid grid-cols-3 gap-2 text-[11px] {text.muted}">
												<span>limit <b class={text.primary}>{formatWizardLimit(row)}</b></span>
												<span>est <b class={text.primary}>{formatWizardEstimate(row)}</b></span>
												<span>left <b class={text.primary}>{formatWizardEstimate({ ...row, estimated: row.remaining })}</b></span>
											</div>
										</div>
									{/each}
								</div>
							{/if}
						</div>
					</div>
				{:else}
					<div class="grid gap-4 lg:grid-cols-[360px_1fr]">
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">Requests to save</p>
							<div class="mt-3 space-y-2">
								{#each wizardRequests as req}
									<div class="rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-700 dark:bg-zinc-900">
										<div class="font-mono text-xs {text.primary}">{req.dimension}</div>
										<p class="mt-1 text-xs {text.muted}">limit={req.limit_value} · window={req.window_seconds ?? '—'} · model={req.model_filter ?? '全部'}</p>
									</div>
								{:else}
									<p class="text-sm {text.muted}">暂无可保存策略。</p>
								{/each}
							</div>
						</div>
						<div>
							<div class="mb-3 flex flex-wrap gap-2">
								<Button variant="outline" onclick={handleWizardExplain} disabled={!wizardCanExplain || wizardExplaining}>
									<Search size={16} />
									{wizardExplaining ? 'Explain 中...' : '运行 explain 预览'}
								</Button>
							</div>
							{#if wizardExplainRules.length === 0}
								<StatePanel title="尚未运行 explain" description="Explain 会调用后端真实规则与 Redis counter，只读预览 would-deny。" icon={CheckCircle2} />
							{:else}
								<DataTable isEmpty={wizardExplainRules.length === 0} emptyColspan={6}>
									{#snippet head()}
										<tr>
											<th class={dataTemplate.th}>Dimension</th>
											<th class={dataTemplate.th}>Mode</th>
											<th class={cn(dataTemplate.th, 'text-right')}>Used</th>
											<th class={cn(dataTemplate.th, 'text-right')}>Estimated</th>
											<th class={cn(dataTemplate.th, 'text-right')}>Remaining</th>
											<th class={dataTemplate.th}>Result</th>
										</tr>
									{/snippet}
									{#each wizardExplainRules as rule}
										<tr class={dataTemplate.row}>
											<td class={dataTemplate.tdMonoStrong}>{rule.dimension}</td>
											<td class={dataTemplate.td}>{rule.mode}</td>
											<td class={cn(dataTemplate.tdMono, 'text-right')}>{formatNumber(rule.current_used)}</td>
											<td class={cn(dataTemplate.tdMono, 'text-right')}>{formatNumber(rule.estimated)}</td>
											<td class={cn(dataTemplate.tdMono, 'text-right')}>{formatNumber(rule.remaining)}</td>
											<td class={dataTemplate.td}>
												<Badge variant={rule.would_deny ? 'danger' : rule.mode === 'dry_run' ? 'warning' : 'success'}>{rule.would_deny ? 'would deny' : 'pass'}</Badge>
											</td>
										</tr>
									{/each}
								</DataTable>
							{/if}
						</div>
					</div>
				{/if}

				{#if wizardError}
					<Alert variant="danger" class="mt-4">{wizardError}</Alert>
				{/if}

				<div class="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-zinc-200 pt-4 dark:border-zinc-700">
					<div class="text-xs {text.muted}">
						将保存 {wizardRequests.length} 条策略；budget 以 USD 输入，后端 explain 以 micros 计算。
					</div>
					<div class="flex gap-2">
						<Button variant="outline" onclick={() => (quotaWizardStep = Math.max(1, quotaWizardStep - 1))} disabled={quotaWizardStep === 1}>上一步</Button>
						{#if quotaWizardStep < 4}
							<Button onclick={() => goWizardStep(quotaWizardStep + 1)} disabled={quotaWizardStep >= 3 && !wizardCanPreview}>下一步</Button>
						{/if}
						<Button variant="outline" onclick={handleWizardExplain} disabled={!wizardCanExplain || wizardExplaining}>
							{wizardExplaining ? 'Explain 中...' : 'Explain'}
						</Button>
						<Button onclick={handleWizardSave} disabled={!wizardCanPreview || wizardSaving}>
							{wizardSaving ? '保存中...' : '保存策略'}
						</Button>
						<Button variant="outline" onclick={closeQuotaWizard}>取消</Button>
					</div>
				</div>
			</div>
		</Card>
	</ModalFrame>
{/if}

<PageShell
	title="配额管理"
	description="P1.6 policy engine：精确 scope/model 策略、dry-run、explain 与 Redis/PG 对账。"
	eyebrow={`组织 / ${shortId(orgId)}`}
	max="full"
	icon={Gauge}
>
	{#snippet actions()}
		<Button variant="outline" onclick={loadQuotas} disabled={loading}>
			<RefreshCw size={16} class={loading ? 'animate-spin' : ''} />
			刷新
		</Button>
		<Button variant="outline" onclick={openQuotaWizard}>
			<CheckCircle2 size={16} />
			Quota wizard
		</Button>
		<Button onclick={openCreateForm}>
			<Plus size={16} />
			添加配额
		</Button>
	{/snippet}

	<div class="mb-6 grid gap-3 md:grid-cols-4">
		<Card padding="md"><p class="text-xs {text.muted}">Rules</p><p class="mt-2 text-2xl font-semibold {text.primary}">{summary.total}</p></Card>
		<Card padding="md"><p class="text-xs {text.muted}">Enforce</p><p class="mt-2 text-2xl font-semibold {text.primary}">{summary.enforce}</p></Card>
		<Card padding="md"><p class="text-xs {text.muted}">Dry-run</p><p class="mt-2 text-2xl font-semibold {text.warning}">{summary.dryRun}</p></Card>
		<Card padding="md"><p class="text-xs {text.muted}">Model scoped</p><p class="mt-2 text-2xl font-semibold {text.primary}">{summary.models}</p></Card>
	</div>

	<DataToolbar badgesVisible={hasActiveFilters}>
		{#snippet query()}
			<Search size={14} class="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-zinc-400" />
			<Input class="pl-9" placeholder="搜索维度 / scope / model / quota ID" bind:value={search} disabled={loading} />
		{/snippet}

		{#snippet controls()}
			<Select class="w-40" size="sm" bind:value={scopeFilter} disabled={loading} options={[{ value: 'all', label: '全部 scope' }, ...scopeOptions]} />
			<Select class="w-40" size="sm" bind:value={modeFilter} disabled={loading} options={[{ value: 'all', label: '全部 mode' }, ...modeOptions]} />
		{/snippet}

		{#snippet actions()}
			<Button variant="outline" size="sm" onclick={resetFilters} disabled={!hasActiveFilters || loading}>清除筛选</Button>
		{/snippet}

		{#snippet badges()}
			{#if search.trim()}
				<Badge>搜索：{search.trim()}</Badge>
			{/if}
			{#if scopeFilter !== 'all'}
				<Badge>Scope：{scopeFilter}</Badge>
			{/if}
			{#if modeFilter !== 'all'}
				<Badge>Mode：{modeFilter}</Badge>
			{/if}
			<Badge>显示 {filteredQuotas.length}/{quotas.length}</Badge>
		{/snippet}
	</DataToolbar>

	{#if loading}
		<StatePanel title="正在读取配额规则" description="吾正在从 control-plane 拉取 org/project/api_key/user 策略。" icon={RefreshCw} />
	{:else if error}
		<Alert variant="danger">{error}</Alert>
	{:else}
		<div class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_420px]">
			<div class="space-y-6">
				{#if quotas.length === 0}
					<StatePanel title="暂无配额规则" description="先添加一条 dry-run 策略观测 would-deny，再切 enforce。" icon={Gauge}>
						{#snippet actions()}<Button onclick={openCreateForm}>添加第一条</Button>{/snippet}
					</StatePanel>
				{:else if filteredQuotas.length === 0}
					<StatePanel title="无匹配配额规则" description="换个搜索词、scope 或 mode 筛选。" icon={Search}>
						{#snippet actions()}<Button variant="outline" onclick={resetFilters}>清除筛选</Button>{/snippet}
					</StatePanel>
				{:else}
					{#each grouped as [scopeKind, items]}
						<SectionCard title={scopeLabel(scopeKind)} description={`${items.length} 条策略`} icon={Gauge}>
							<DataTable class="mb-0">
								{#snippet head()}
									<tr>
										<th class={dataTemplate.th}>维度</th>
										<th class={dataTemplate.th}>限额</th>
										<th class={dataTemplate.th}>Scope</th>
										<th class={dataTemplate.th}>Model</th>
										<th class={dataTemplate.th}>Mode</th>
										<th class={dataTemplate.th}>窗口</th>
										<th class="px-4 py-3 text-right text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">操作</th>
									</tr>
								{/snippet}

								{#each items as q}
									<tr class={dataTemplate.row}>
										<td class={dataTemplate.tdStrong}>
											<div class="font-medium">{dimensionLabel(q.dimension)}</div>
											<div class="mt-0.5 text-[11px] {text.muted}">{dimensionUnit(q.dimension)}</div>
										</td>
										<td class={dataTemplate.tdMonoStrong}>{q.limit_value}</td>
										<td class={dataTemplate.tdMono}>{shortId(q.scope_id)}</td>
										<td class={dataTemplate.td}>{q.model_filter ?? '全部'}</td>
										<td class={dataTemplate.td}>
											<Badge variant={q.mode === 'dry_run' ? 'warning' : 'default'}>{q.mode}</Badge>
										</td>
										<td class={dataTemplate.td}>{q.window_seconds ? `${q.window_seconds}s` : '—'}</td>
										<td class="px-4 py-3 text-right">
											<div class="flex justify-end gap-1">
												<Button variant="ghost" size="sm" onclick={() => loadRuleIntoExplain(q)}>
													<Search size={14} />
													Explain
												</Button>
												<Button variant="ghost" size="sm" onclick={() => (deletingId = q.id)}>
													<Trash2 size={14} class="text-red-600 dark:text-red-400" />
												</Button>
											</div>
										</td>
									</tr>
								{/each}
							</DataTable>
						</SectionCard>
					{/each}
				{/if}
			</div>

			<div class="space-y-6">
				<SectionCard title="Quota explain" description="不写入计数，只解释当前请求会命中哪些规则。" icon={Search}>
					<form onsubmit={handleExplain} class="space-y-3">
						<div class="grid grid-cols-2 gap-3">
							<Select bind:value={explainScopeKind} options={scopeOptions} />
							<Select bind:value={explainDimension} options={[{ value: '', label: '全部维度' }, ...dimensionOptions]} />
						</div>
						<Input placeholder="scope UUID / typed ID" bind:value={explainScopeId} />
						<div class="grid grid-cols-2 gap-3">
							<Input placeholder="model，例如 gpt-4o-mini" bind:value={explainModel} />
							<Input type="number" placeholder="estimated tokens" bind:value={explainTokens} />
						</div>
						<Input type="number" placeholder="estimated cost micros" bind:value={explainCostMicros} />
						{#if explainError}<Alert variant="danger">{explainError}</Alert>{/if}
						<Button type="submit" disabled={explaining} class="w-full">
							<Search size={16} />
							{explaining ? '解释中...' : '解释命中规则'}
						</Button>
					</form>

					{#if explainRules.length > 0}
						<div class="mt-4 space-y-2">
							{#each explainRules as rule}
								<div class={cn('rounded-lg border p-3', rule.would_deny ? 'border-red-200 bg-red-50 dark:border-red-800 dark:bg-red-900/20' : 'border-zinc-200 bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-800/40')}>
									<div class="flex items-center justify-between gap-2">
										<div class="font-medium {text.primary}">{dimensionLabel(rule.dimension)}</div>
										<Badge variant={rule.would_deny ? 'danger' : rule.mode === 'dry_run' ? 'warning' : 'success'}>{rule.would_deny ? 'would deny' : rule.mode}</Badge>
									</div>
									<div class="mt-2 grid grid-cols-3 gap-2 text-xs">
										<span class={text.muted}>used <b class={text.primary}>{formatNumber(rule.current_used)}</b></span>
										<span class={text.muted}>est <b class={text.primary}>{formatNumber(rule.estimated)}</b></span>
										<span class={text.muted}>left <b class={text.primary}>{formatNumber(rule.remaining)}</b></span>
									</div>
									<p class="mt-2 font-mono text-[11px] {text.muted}">{shortId(rule.quota_id)} · reset {formatReset(rule)}</p>
								</div>
							{/each}
						</div>
					{/if}
				</SectionCard>

				<SectionCard title="Redis / PG 对账" description="counter 维度读取 Redis，budget/tokens 对齐 persisted usage。" icon={GitCompareArrows}>
					{#if reconcileError}<Alert variant="danger" class="mb-3">{reconcileError}</Alert>{/if}
					<Button variant="outline" onclick={handleReconcile} disabled={reconciling} class="w-full">
						<GitCompareArrows size={16} />
						{reconciling ? '对账中...' : '运行对账'}
					</Button>

					{#if reconcileRows.length > 0}
						<div class="mt-4 max-h-[360px] space-y-2 overflow-y-auto pr-1">
							{#each reconcileRows as row}
								<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-700 dark:bg-zinc-800/40">
									<div class="flex items-center justify-between gap-2">
										<span class="text-sm font-medium {text.primary}">{dimensionLabel(row.dimension)}</span>
										<Badge variant={row.delta && Math.abs(row.delta) > 0 ? 'warning' : 'default'}>delta {formatNumber(row.delta)}</Badge>
									</div>
									<div class="mt-2 grid grid-cols-2 gap-2 text-xs {text.secondary}">
										<span>Redis: <b>{formatNumber(row.redis_used)}</b></span>
										<span>PG: <b>{formatNumber(row.pg_used)}</b></span>
									</div>
									{#if row.note}<p class="mt-2 text-xs {text.muted}">{row.note}</p>{/if}
								</div>
							{/each}
						</div>
					{/if}
				</SectionCard>
			</div>
		</div>
	{/if}
</PageShell>
