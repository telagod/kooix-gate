<!-- /orgs/[orgId]/billing — 月账单 + CSV 导出 + 配额告警 -->
<script lang="ts">
	import { shortId } from '$lib/id.js';
	import { onMount } from 'svelte';
	import { page } from '$app/stores';
	import {
		getMonthlyBill,
		exportBillingCsv,
		exportBillingJson,
		getQuotaAlerts,
		getMe,
		transitionBillingInvoice
	} from '$lib/api.js';
	import type { MonthlyBill, QuotaAlert } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import Stat from '$lib/components/Stat.svelte';

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

<div>
	<!-- 面包屑 -->
	<div class="px-6 py-6">
		<p class="text-xs text-zinc-500 dark:text-zinc-400 mb-1">组织 / {shortId(orgId)}... / 账单</p>
		<!-- 标题行 -->
		<div class="flex items-center justify-between mb-6 gap-4 flex-wrap">
			<div>
				<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">月账单</h1>
				<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-1">按月查看费用明细，支持 CSV / JSON digest 与 invoice 状态机。</p>
			</div>

			<div class="flex items-center gap-3 flex-wrap">
				<!-- 月份选择器 -->
				<select
					bind:value={selectedMonth}
					onchange={handleMonthChange}
					disabled={loading}
					class="flex h-10 rounded-md border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300 disabled:opacity-50"
				>
					{#each monthOptions as opt}
						<option value={opt.value}>{opt.label}</option>
					{/each}
				</select>

				<!-- CSV 导出 -->
				<Button onclick={handleExport} disabled={exporting || loading} variant="outline">
					{exporting ? '导出中...' : '导出 CSV'}
				</Button>
				<Button onclick={handleExportJson} disabled={exportingJson || loading} variant="outline">
					{exportingJson ? '导出中...' : '导出 JSON'}
				</Button>
			</div>
		</div>

		{#if exportError}
			<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-md px-3 py-2 mb-4">{exportError}</p>
		{/if}
		{#if lastDigest}
			<p class="text-xs font-mono text-zinc-600 dark:text-zinc-400 bg-zinc-50 dark:bg-zinc-900 rounded-md px-3 py-2 mb-4 break-all">
				导出摘要：{lastDigest}
			</p>
		{/if}

		{#if loading}
			<p class="text-zinc-600 dark:text-zinc-300">加载中...</p>
		{:else if error}
			<Card class="p-6">
				<p class="text-red-600 dark:text-red-400 text-sm">{error}</p>
			</Card>
		{:else if bill}
			<Card class="p-4 mb-6">
				<div class="flex items-start justify-between gap-4 flex-wrap">
					<div>
						<p class="text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider mb-2">
							Invoice State
						</p>
						<div class="flex items-center gap-3 flex-wrap">
							<span class="inline-flex items-center rounded-full border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-3 py-1 text-sm font-semibold text-zinc-900 dark:text-zinc-100">
								{invoiceStatusLabel(bill.invoice.status)}
							</span>
							<span class="text-xs text-zinc-500 dark:text-zinc-400">
								{bill.invoice.month} · {fmtCost(bill.invoice.total_cost_usd)} · {fmtNum(bill.invoice.total_cost_micros)} micros
							</span>
						</div>
						{#if bill.invoice.export_digest}
							<p class="mt-2 text-xs font-mono text-zinc-600 dark:text-zinc-400 break-all">
								export_digest: {bill.invoice.export_digest}
							</p>
						{/if}
					</div>
					<div class="flex items-center gap-2 flex-wrap">
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

			<!-- ── 统计卡片 ── -->
			<div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4 mb-8">
				<Stat
					title="总费用"
					value={fmtCost(bill.total_cost_usd)}
					subtitle="USD · {bill.month}"
				/>
				<Stat
					title="总请求"
					value={fmtNum(bill.total_requests)}
					subtitle="本月请求次数"
				/>
				<Stat
					title="输入 Tokens"
					value={fmtNum(bill.total_tokens_in)}
					subtitle="prompt 输入累计"
				/>
				<Stat
					title="输出 Tokens"
					value={fmtNum(bill.total_tokens_out)}
					subtitle="completion 输出累计"
				/>
			</div>

			<!-- ── 按项目分解 ── -->
			<div class="mb-8">
				<h2 class="text-sm font-semibold text-zinc-700 dark:text-zinc-300 uppercase tracking-wider mb-3">
					按项目分解
				</h2>
				{#if bill.breakdown_by_project.length === 0}
					<Card class="p-4">
						<p class="text-sm text-zinc-600 dark:text-zinc-300">本月无项目用量记录。</p>
					</Card>
				{:else}
					<div class="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
						<table class="w-full text-sm">
							<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
								<tr>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">Project ID</th>
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">费用 (USD)</th>
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">请求次数</th>
								</tr>
							</thead>
							<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
								{#each bill.breakdown_by_project as row}
									<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800 transition-colors">
										<td class="px-4 py-3 font-mono text-xs text-zinc-600 dark:text-zinc-400">{row.project_id}</td>
										<td class="px-4 py-3 text-right font-mono text-zinc-900 dark:text-zinc-100">{fmtCost(row.cost_usd)}</td>
										<td class="px-4 py-3 text-right font-mono text-zinc-700 dark:text-zinc-300">{fmtNum(row.requests)}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>

			<!-- ── 按模型分解 ── -->
			<div class="mb-8">
				<h2 class="text-sm font-semibold text-zinc-700 dark:text-zinc-300 uppercase tracking-wider mb-3">
					按模型分解
				</h2>
				{#if bill.breakdown_by_model.length === 0}
					<Card class="p-4">
						<p class="text-sm text-zinc-600 dark:text-zinc-300">本月无模型用量记录。</p>
					</Card>
				{:else}
					<div class="overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
						<table class="w-full text-sm">
							<thead class="bg-zinc-50 dark:bg-zinc-800 border-b border-zinc-200 dark:border-zinc-700">
								<tr>
									<th class="px-4 py-3 text-left font-medium text-zinc-600 dark:text-zinc-400">模型</th>
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">费用 (USD)</th>
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">输入 Tokens</th>
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">输出 Tokens</th>
									<th class="px-4 py-3 text-right font-medium text-zinc-600 dark:text-zinc-400">请求次数</th>
								</tr>
							</thead>
							<tbody class="divide-y divide-zinc-100 dark:divide-zinc-800">
								{#each bill.breakdown_by_model as row}
									<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800 transition-colors">
										<td class="px-4 py-3 font-medium text-zinc-900 dark:text-zinc-100">{row.model}</td>
										<td class="px-4 py-3 text-right font-mono text-zinc-900 dark:text-zinc-100">{fmtCost(row.cost_usd)}</td>
										<td class="px-4 py-3 text-right font-mono text-zinc-700 dark:text-zinc-300">{fmtNum(row.tokens_in)}</td>
										<td class="px-4 py-3 text-right font-mono text-zinc-700 dark:text-zinc-300">{fmtNum(row.tokens_out)}</td>
										<td class="px-4 py-3 text-right font-mono text-zinc-700 dark:text-zinc-300">{fmtNum(row.requests)}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					</div>
				{/if}
			</div>
		{/if}

		<!-- ── 配额告警区域（独立于账单，始终尝试渲染） ── -->
		{#if !loading && alerts.length > 0}
			<div class="mb-6">
				<h2 class="text-sm font-semibold text-zinc-700 dark:text-zinc-300 uppercase tracking-wider mb-3">
					配额告警
				</h2>

				{#if exceeded.length > 0}
					<div class="mb-4">
						<p class="text-xs font-semibold text-red-600 dark:text-red-400 uppercase tracking-wider mb-2">
							已超限 ({exceeded.length})
						</p>
						<div class="space-y-2">
							{#each exceeded as alert}
								<div class="rounded-lg border border-red-200 dark:border-red-700 bg-red-50 dark:bg-red-900/20 px-4 py-3 flex items-center justify-between gap-4 flex-wrap">
									<div class="flex items-center gap-3">
										<span class="inline-block px-2 py-0.5 rounded text-xs font-semibold bg-red-100 dark:bg-red-900/30 text-red-700 dark:text-red-400">
											超限
										</span>
										<span class="text-sm font-medium text-red-900 dark:text-red-300">{alert.dimension}</span>
										<span class="text-xs text-red-600 dark:text-red-400">{scopeLabel(alert.scope_kind)}</span>
										<span class="text-xs font-mono text-red-500 dark:text-red-400">{shortId(alert.scope_id)}…</span>
									</div>
									<div class="flex items-center gap-4 text-xs text-red-700 dark:text-red-400 tabular-nums">
										<span>当前：<strong>{alert.current_used}</strong></span>
										<span>限额：<strong>{alert.limit_value}</strong></span>
										<span class="font-bold">{alert.percent.toFixed(1)}%</span>
									</div>
								</div>
							{/each}
						</div>
					</div>
				{/if}

				{#if approaching.length > 0}
					<div>
						<p class="text-xs font-semibold text-yellow-600 dark:text-yellow-400 uppercase tracking-wider mb-2">
							接近限额 ({approaching.length})
						</p>
						<div class="space-y-2">
							{#each approaching as alert}
								<div class="rounded-lg border border-yellow-200 dark:border-yellow-700 bg-yellow-50 dark:bg-amber-900/20 px-4 py-3 flex items-center justify-between gap-4 flex-wrap">
									<div class="flex items-center gap-3">
										<span class="inline-block px-2 py-0.5 rounded text-xs font-semibold bg-yellow-100 dark:bg-amber-900/30 text-yellow-700 dark:text-amber-400">
											接近
										</span>
										<span class="text-sm font-medium text-yellow-900 dark:text-amber-300">{alert.dimension}</span>
										<span class="text-xs text-yellow-600 dark:text-amber-400">{scopeLabel(alert.scope_kind)}</span>
										<span class="text-xs font-mono text-yellow-500 dark:text-amber-400">{shortId(alert.scope_id)}…</span>
									</div>
									<div class="flex items-center gap-4 text-xs text-yellow-700 dark:text-amber-400 tabular-nums">
										<span>当前：<strong>{alert.current_used}</strong></span>
										<span>限额：<strong>{alert.limit_value}</strong></span>
										<span class="font-bold">{alert.percent.toFixed(1)}%</span>
									</div>
								</div>
							{/each}
						</div>
					</div>
				{/if}

				{#if watch.length > 0}
					<div class="mt-4">
						<p class="text-xs font-semibold text-zinc-600 dark:text-zinc-400 uppercase tracking-wider mb-2">
							预算观察 ({watch.length})
						</p>
						<div class="space-y-2">
							{#each watch as alert}
								<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 px-4 py-3 flex items-center justify-between gap-4 flex-wrap">
									<div class="flex items-center gap-3">
										<span class="inline-block px-2 py-0.5 rounded text-xs font-semibold bg-zinc-100 dark:bg-zinc-800 text-zinc-700 dark:text-zinc-300">
											50%
										</span>
										<span class="text-sm font-medium text-zinc-900 dark:text-zinc-100">{alert.dimension}</span>
										<span class="text-xs text-zinc-600 dark:text-zinc-400">{scopeLabel(alert.scope_kind)}</span>
										<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{shortId(alert.scope_id)}…</span>
									</div>
									<div class="flex items-center gap-4 text-xs text-zinc-700 dark:text-zinc-300 tabular-nums">
										<span>当前：<strong>{alert.current_used}</strong></span>
										<span>限额：<strong>{alert.limit_value}</strong></span>
										<span class="font-bold">{alert.percent.toFixed(1)}%</span>
									</div>
								</div>
							{/each}
						</div>
					</div>
				{/if}
			</div>
		{/if}
	</div>
</div>
