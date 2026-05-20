<!-- /orgs/[orgId]/billing — 月账单 + CSV 导出 + 配额告警 -->
<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import { CreditCard, Download, FileJson, RefreshCw } from 'lucide-svelte';
	import {
		getMonthlyBill,
		exportBillingCsv,
		exportBillingJson,
		getQuotaAlerts,
		getMe,
		transitionBillingInvoice
	} from '$lib/api.js';
	import type { MonthlyBill, QuotaAlert } from '$lib/api.js';
	import Alert from '$lib/components/ui/Alert.svelte';
	import Badge from '$lib/components/ui/Badge.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Select from '$lib/components/ui/Select.svelte';
	import Stat from '$lib/components/Stat.svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { cn, dataTemplate, text, type BadgeVariant } from '$lib/design';

	let orgId = $derived($page.params.orgId ?? '');

	// ── 月份选择器 ────────────────────────────────────────
	function buildMonthOptions(): { label: string; value: string }[] {
		const opts: { label: string; value: string }[] = [];
		const now = new Date();
		for (let i = 0; i < 12; i++) {
			const d = new Date(now.getFullYear(), now.getMonth() - i, 1);
			const value = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}`;
			const label = `${d.getFullYear()} 年 ${d.getMonth() + 1} 月`;
			opts.push({ label, value });
		}
		return opts;
	}

	const monthOptions = buildMonthOptions();
	let selectedMonth = $state(monthOptions[0].value);

	// ── 数据状态 ──────────────────────────────────────────
	let bill = $state<MonthlyBill | null>(null);
	let alerts = $state<QuotaAlert[]>([]);
	let loading = $state(true);
	let error = $state('');
	let exporting = $state(false);
	let exportingJson = $state(false);
	let transitioning = $state('');
	let exportError = $state('');
	let lastDigest = $state('');

	// ── 挂载：并行加载账单 + 告警 ─────────────
	onMount(async () => {
		try {
			await getMe(orgId);
		} catch {
			// 非 401 忽略（超管可能无 org 上下文），继续加载
		}
		await loadAll();
	});

	async function loadAll() {
		loading = true;
		error = '';
		try {
			const [b, a] = await Promise.all([
				getMonthlyBill(orgId, selectedMonth),
				getQuotaAlerts(orgId)
			]);
			bill = b;
			alerts = a;
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	}

	async function handleMonthChange() {
		await loadAll();
	}

	// ── CSV 导出 ──────────────────────────────────────────
	async function handleExport() {
		exporting = true;
		exportError = '';
		try {
			// 当前月首日 → 末日
			const [y, m] = selectedMonth.split('-').map(Number);
			const from = `${selectedMonth}-01`;
			const lastDay = new Date(y, m, 0).getDate();
			const to = `${selectedMonth}-${String(lastDay).padStart(2, '0')}`;

			const blob = await exportBillingCsv(orgId, from, to);
			const url = URL.createObjectURL(blob);
			const a = document.createElement('a');
			a.href = url;
			a.download = `billing-${shortId(orgId)}-${selectedMonth}.csv`;
			a.click();
			URL.revokeObjectURL(url);
			lastDigest = 'CSV 已导出；digest 见响应头 x-kooix-export-digest';
		} catch (err: any) {
			exportError = err?.message ?? '导出失败';
		} finally {
			exporting = false;
		}
	}

	async function handleExportJson() {
		exportingJson = true;
		exportError = '';
		try {
			const [y, m] = selectedMonth.split('-').map(Number);
			const from = `${selectedMonth}-01`;
			const lastDay = new Date(y, m, 0).getDate();
			const to = `${selectedMonth}-${String(lastDay).padStart(2, '0')}`;

			const payload = await exportBillingJson(orgId, from, to);
			lastDigest = `${payload.digest.alg}:${payload.digest.value}`;
			const blob = new Blob([JSON.stringify(payload, null, 2)], { type: 'application/json' });
			const url = URL.createObjectURL(blob);
			const a = document.createElement('a');
			a.href = url;
			a.download = `billing-${shortId(orgId)}-${selectedMonth}.json`;
			a.click();
			URL.revokeObjectURL(url);
		} catch (err: any) {
			exportError = err?.message ?? '导出失败';
		} finally {
			exportingJson = false;
		}
	}

	async function handleInvoiceTransition(status: 'closed' | 'exported' | 'paid' | 'waived') {
		if (!bill) return;
		transitioning = status;
		exportError = '';
		try {
			const digest = status === 'exported' ? bill.invoice.export_digest || lastDigest : undefined;
			if (status === 'exported' && !digest) {
				throw new Error('先导出 JSON，拿到 digest 后再标记 exported。');
			}
			const invoice = await transitionBillingInvoice(orgId, selectedMonth, status, digest);
			bill = { ...bill, invoice };
		} catch (err: any) {
			exportError = err?.message ?? '状态更新失败';
		} finally {
			transitioning = '';
		}
	}

	// ── 格式化工具 ────────────────────────────────────────
	function fmtCost(s: string): string {
		const n = parseFloat(s);
		return isNaN(n) ? s : `$${n.toFixed(4)}`;
	}

	function fmtNum(n: number): string {
		return n.toLocaleString('en-US');
	}

	// ── 告警分组 ──────────────────────────────────────────
	let watch = $derived(alerts.filter((a) => a.level === 'watch'));
	let approaching = $derived(alerts.filter((a) => a.level === 'approaching'));
	let exceeded = $derived(alerts.filter((a) => a.level === 'exceeded'));

	function scopeLabel(kind: string): string {
		const m: Record<string, string> = { org: '组织', project: '项目', api_key: 'API Key' };
		return m[kind] ?? kind;
	}

	function invoiceStatusLabel(status: string): string {
		const m: Record<string, string> = {
			draft: 'Draft',
			closed: 'Closed',
			exported: 'Exported',
			paid: 'Paid',
			waived: 'Waived'
		};
		return m[status] ?? status;
	}

	function invoiceStatusVariant(status: string): BadgeVariant {
		const m: Record<string, BadgeVariant> = {
			draft: 'default',
			closed: 'warning',
			exported: 'admin',
			paid: 'success',
			waived: 'warning'
		};
		return m[status] ?? 'default';
	}

	function canClose(): boolean {
		return bill?.invoice.status === 'draft';
	}

	function canExport(): boolean {
		return bill?.invoice.status === 'closed';
	}

	function canSettle(): boolean {
		return bill?.invoice.status === 'closed' || bill?.invoice.status === 'exported';
	}
</script>

<PageShell
	title="月账单"
	description="按月查看费用明细，支持 CSV / JSON digest 与 invoice 状态机。"
	eyebrow={`组织 / ${shortId(orgId)}`}
	icon={CreditCard}
	max="full"
>
	<DataToolbar class="mb-6" badgesVisible={!!lastDigest}>
		{#snippet query()}
			<div class="flex items-center gap-2">
				<span class="text-xs font-medium {text.muted}">账期</span>
				<Select
					bind:value={selectedMonth}
					options={monthOptions}
					onchange={handleMonthChange}
					disabled={loading}
					size="sm"
					class="w-44"
				/>
			</div>
		{/snippet}

		{#snippet actions()}
			<Button variant="outline" size="sm" onclick={loadAll} disabled={loading}>
				<RefreshCw size={14} class={loading ? 'animate-spin' : ''} />
				刷新
			</Button>
			<Button variant="outline" size="sm" onclick={handleExport} disabled={exporting || loading}>
				<Download size={14} />
				{exporting ? '导出中...' : '导出 CSV'}
			</Button>
			<Button variant="outline" size="sm" onclick={handleExportJson} disabled={exportingJson || loading}>
				<FileJson size={14} />
				{exportingJson ? '导出中...' : '导出 JSON'}
			</Button>
		{/snippet}

		{#snippet badges()}
			<Badge class="max-w-full break-all font-mono">导出摘要：{lastDigest}</Badge>
		{/snippet}
	</DataToolbar>

	{#if exportError}
		<Alert variant="danger" class="mb-4">{exportError}</Alert>
	{/if}

	{#if loading}
		<StatePanel
			title="正在读取账单"
			description="吾正在并行拉取本月 invoice、ledger 聚合与 quota alerts。"
			icon={RefreshCw}
			class="mb-6"
		/>
	{:else if error}
		<StatePanel title="账单加载失败" description={error} icon={CreditCard} variant="danger" class="mb-6">
			{#snippet actions()}
				<Button variant="outline" onclick={loadAll}>重试</Button>
			{/snippet}
		</StatePanel>
	{:else if bill}
		<Card padding="md" class="mb-6">
			<div class="flex flex-wrap items-start justify-between gap-4">
				<div class="min-w-0">
					<p class="mb-2 text-xs font-semibold uppercase tracking-wider {text.muted}">
						Invoice State
					</p>
					<div class="flex flex-wrap items-center gap-3">
						<Badge variant={invoiceStatusVariant(bill.invoice.status)}>
							{invoiceStatusLabel(bill.invoice.status)}
						</Badge>
						<span class="text-xs {text.muted}">
							{bill.invoice.month} · {fmtCost(bill.invoice.total_cost_usd)} · {fmtNum(bill.invoice.total_cost_micros)} micros
						</span>
					</div>
					{#if bill.invoice.export_digest}
						<p class="mt-2 break-all font-mono text-xs {text.muted}">
							export_digest: {bill.invoice.export_digest}
						</p>
					{/if}
				</div>
				<div class="flex flex-wrap items-center gap-2">
					<Button onclick={() => handleInvoiceTransition('closed')} disabled={!canClose() || !!transitioning} variant="outline" size="sm">
						{transitioning === 'closed' ? 'Closing...' : 'Close'}
					</Button>
					<Button onclick={() => handleInvoiceTransition('exported')} disabled={!canExport() || !!transitioning} variant="outline" size="sm">
						{transitioning === 'exported' ? 'Exporting...' : 'Mark Exported'}
					</Button>
					<Button onclick={() => handleInvoiceTransition('paid')} disabled={!canSettle() || !!transitioning} size="sm">
						{transitioning === 'paid' ? 'Paying...' : 'Paid'}
					</Button>
					<Button onclick={() => handleInvoiceTransition('waived')} disabled={!canSettle() || !!transitioning} variant="outline" size="sm">
						{transitioning === 'waived' ? 'Waiving...' : 'Waive'}
					</Button>
				</div>
			</div>
		</Card>

		<div class="mb-8 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
			<Stat title="总费用" value={fmtCost(bill.total_cost_usd)} subtitle="USD · {bill.month}" />
			<Stat title="总请求" value={fmtNum(bill.total_requests)} subtitle="本月请求次数" />
			<Stat title="输入 Tokens" value={fmtNum(bill.total_tokens_in)} subtitle="prompt 输入累计" />
			<Stat title="输出 Tokens" value={fmtNum(bill.total_tokens_out)} subtitle="completion 输出累计" />
		</div>

		<div class="mb-8">
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wider {text.secondary}">
				按项目分解
			</h2>
			<DataTable isEmpty={bill.breakdown_by_project.length === 0} emptyColspan={3} class="mb-0">
				{#snippet head()}
					<tr>
						<th class={dataTemplate.th}>Project ID</th>
						<th class={cn(dataTemplate.th, 'text-right')}>费用 (USD)</th>
						<th class={cn(dataTemplate.th, 'text-right')}>请求次数</th>
					</tr>
				{/snippet}

				{#snippet empty()}
					本月无项目用量记录。
				{/snippet}

				{#each bill.breakdown_by_project as row}
					<tr class={dataTemplate.row}>
						<td class={dataTemplate.tdMono}>{row.project_id}</td>
						<td class={cn(dataTemplate.tdMonoStrong, 'text-right')}>{fmtCost(row.cost_usd)}</td>
						<td class={cn(dataTemplate.tdMono, 'text-right')}>{fmtNum(row.requests)}</td>
					</tr>
				{/each}
			</DataTable>
		</div>

		<div class="mb-8">
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wider {text.secondary}">
				按模型分解
			</h2>
			<DataTable isEmpty={bill.breakdown_by_model.length === 0} emptyColspan={5} class="mb-0">
				{#snippet head()}
					<tr>
						<th class={dataTemplate.th}>模型</th>
						<th class={cn(dataTemplate.th, 'text-right')}>费用 (USD)</th>
						<th class={cn(dataTemplate.th, 'text-right')}>输入 Tokens</th>
						<th class={cn(dataTemplate.th, 'text-right')}>输出 Tokens</th>
						<th class={cn(dataTemplate.th, 'text-right')}>请求次数</th>
					</tr>
				{/snippet}

				{#snippet empty()}
					本月无模型用量记录。
				{/snippet}

				{#each bill.breakdown_by_model as row}
					<tr class={dataTemplate.row}>
						<td class={dataTemplate.tdStrong}>{row.model}</td>
						<td class={cn(dataTemplate.tdMonoStrong, 'text-right')}>{fmtCost(row.cost_usd)}</td>
						<td class={cn(dataTemplate.tdMono, 'text-right')}>{fmtNum(row.tokens_in)}</td>
						<td class={cn(dataTemplate.tdMono, 'text-right')}>{fmtNum(row.tokens_out)}</td>
						<td class={cn(dataTemplate.tdMono, 'text-right')}>{fmtNum(row.requests)}</td>
					</tr>
				{/each}
			</DataTable>
		</div>
	{/if}

	{#if !loading && alerts.length > 0}
		<div class="mb-6">
			<h2 class="mb-3 text-sm font-semibold uppercase tracking-wider {text.secondary}">
				配额告警
			</h2>

			{#if exceeded.length > 0}
				<div class="mb-4">
					<p class="mb-2 text-xs font-semibold uppercase tracking-wider {text.danger}">
						已超限 ({exceeded.length})
					</p>
					<div class="space-y-2">
						{#each exceeded as alert}
							<Card variant="danger" class="px-4 py-3">
								<div class="flex flex-wrap items-center justify-between gap-4">
									<div class="flex flex-wrap items-center gap-3">
										<Badge variant="danger">超限</Badge>
										<span class="text-sm font-medium text-red-900 dark:text-red-300">{alert.dimension}</span>
										<span class="text-xs {text.danger}">{scopeLabel(alert.scope_kind)}</span>
										<span class="font-mono text-xs {text.danger}">{shortId(alert.scope_id)}…</span>
									</div>
									<div class="flex items-center gap-4 text-xs tabular-nums {text.danger}">
										<span>当前：<strong>{alert.current_used}</strong></span>
										<span>限额：<strong>{alert.limit_value}</strong></span>
										<span class="font-bold">{alert.percent.toFixed(1)}%</span>
									</div>
								</div>
							</Card>
						{/each}
					</div>
				</div>
			{/if}

			{#if approaching.length > 0}
				<div>
					<p class="mb-2 text-xs font-semibold uppercase tracking-wider {text.warning}">
						接近限额 ({approaching.length})
					</p>
					<div class="space-y-2">
						{#each approaching as alert}
							<Card variant="warning" class="px-4 py-3">
								<div class="flex flex-wrap items-center justify-between gap-4">
									<div class="flex flex-wrap items-center gap-3">
										<Badge variant="warning">接近</Badge>
										<span class="text-sm font-medium text-amber-900 dark:text-amber-300">{alert.dimension}</span>
										<span class="text-xs {text.warning}">{scopeLabel(alert.scope_kind)}</span>
										<span class="font-mono text-xs {text.warning}">{shortId(alert.scope_id)}…</span>
									</div>
									<div class="flex items-center gap-4 text-xs tabular-nums {text.warning}">
										<span>当前：<strong>{alert.current_used}</strong></span>
										<span>限额：<strong>{alert.limit_value}</strong></span>
										<span class="font-bold">{alert.percent.toFixed(1)}%</span>
									</div>
								</div>
							</Card>
						{/each}
					</div>
				</div>
			{/if}

			{#if watch.length > 0}
				<div class="mt-4">
					<p class="mb-2 text-xs font-semibold uppercase tracking-wider {text.muted}">
						预算观察 ({watch.length})
					</p>
					<div class="space-y-2">
						{#each watch as alert}
							<Card variant="subtle" class="px-4 py-3">
								<div class="flex flex-wrap items-center justify-between gap-4">
									<div class="flex flex-wrap items-center gap-3">
										<Badge>50%</Badge>
										<span class="text-sm font-medium {text.primary}">{alert.dimension}</span>
										<span class="text-xs {text.secondary}">{scopeLabel(alert.scope_kind)}</span>
										<span class="font-mono text-xs {text.muted}">{shortId(alert.scope_id)}…</span>
									</div>
									<div class="flex items-center gap-4 text-xs tabular-nums {text.secondary}">
										<span>当前：<strong>{alert.current_used}</strong></span>
										<span>限额：<strong>{alert.limit_value}</strong></span>
										<span class="font-bold">{alert.percent.toFixed(1)}%</span>
									</div>
								</div>
							</Card>
						{/each}
					</div>
				</div>
			{/if}
		</div>
	{/if}
</PageShell>
