<script lang="ts">
	// 0.4.12：从 quotas/+page.svelte 抽出的 wizard modal。
	// 252 行 wizard 块整体抽出，state 仍在父页面，子组件渲染 + callback 回调。
	import { Alert, Badge, Button, Card, Input, Select } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import StatePanel from '$lib/components/templates/StatePanel.svelte';
	import { CheckCircle2, GitCompareArrows, Search } from 'lucide-svelte';
	import { cn, dataTemplate } from '$lib/design';
	import { shortId } from '$lib/id.js';
	import type { QuotaExplainRule, UpsertQuotaRequest } from '$lib/api.js';
	import type { QuotaWizardDraft, QuotaWizardPreviewRule } from '$lib/quota-wizard.js';

	type SelectOption = { value: string; label: string };

	interface Props {
		showWizard: boolean;
		quotaWizardStep: number;
		wizardDraft: QuotaWizardDraft;
		wizardSaving: boolean;
		wizardExplaining: boolean;
		wizardError: string;
		wizardExplainRules: QuotaExplainRule[];
		wizardRequests: UpsertQuotaRequest[];
		wizardPreviewRows: QuotaWizardPreviewRule[];
		wizardCanPreview: boolean;
		wizardCanExplain: boolean;
		scopeOptions: SelectOption[];
		modeOptions: SelectOption[];
		orgId: string;
		text: { primary: string; secondary: string; muted: string };
		formatNumber: (n: number | null | undefined) => string;
		formatWizardLimit: (row: QuotaWizardPreviewRule) => string;
		formatWizardEstimate: (row: QuotaWizardPreviewRule) => string;
		onClose: () => void;
		onUpdateWizard: <K extends keyof QuotaWizardDraft>(key: K, value: QuotaWizardDraft[K]) => void;
		onGoStep: (step: number) => void;
		onExplain: () => void | Promise<void>;
		onSave: () => void | Promise<void>;
	}

	let {
		showWizard,
		quotaWizardStep = $bindable(),
		wizardDraft,
		wizardSaving,
		wizardExplaining,
		wizardError,
		wizardExplainRules,
		wizardRequests,
		wizardPreviewRows,
		wizardCanPreview,
		wizardCanExplain,
		scopeOptions,
		modeOptions,
		orgId,
		text,
		formatNumber,
		formatWizardLimit,
		formatWizardEstimate,
		onClose,
		onUpdateWizard,
		onGoStep,
		onExplain,
		onSave,
	}: Props = $props();
</script>

{#if showWizard}
	<ModalFrame close={onClose} panelClass="w-full max-w-4xl">
		<Card class="max-h-[90vh] overflow-y-auto">
			<div class="border-b border-zinc-200 bg-zinc-50 p-5 dark:border-zinc-700 dark:bg-zinc-800/50">
				<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
					<div>
						<p class="text-xs font-semibold uppercase tracking-widest {text.muted}">Quota wizard 向导</p>
						<h3 class="mt-1 text-lg font-semibold {text.primary}">新建配额策略</h3>
						<p class="mt-1 text-sm {text.secondary}">按 scope、model filter、rpm/tpm/budget 一次生成多条 policy，并先跑 explain 预览。</p>
					</div>
					<Badge variant={wizardDraft.mode === 'dry_run' ? 'warning' : 'default'}>{wizardDraft.mode}</Badge>
				</div>
				<div class="mt-4 grid gap-2 md:grid-cols-4">
					{#each [
						{ step: 1, label: 'Scope 作用域', hint: '选择作用域' },
						{ step: 2, label: 'Model filter 模型过滤', hint: '限定模型' },
						{ step: 3, label: 'Limits 限额', hint: '输入 rpm/tpm/budget' },
						{ step: 4, label: 'Explain 预览', hint: '预览 would-deny' }
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
							onclick={() => onGoStep(item.step)}
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
									<label for="qw-scope-kind" class="mb-1 block text-sm font-medium {text.secondary}">Scope 作用域</label>
									<Select id="qw-scope-kind" value={wizardDraft.scopeKind} options={scopeOptions} onchange={(event) => onUpdateWizard('scopeKind', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-mode" class="mb-1 block text-sm font-medium {text.secondary}">Mode 模式</label>
									<Select id="qw-mode" value={wizardDraft.mode} options={modeOptions} onchange={(event) => onUpdateWizard('mode', event.currentTarget.value as QuotaWizardDraft['mode'])} />
								</div>
							</div>
							<div>
								<label for="qw-scope-id" class="mb-1 block text-sm font-medium {text.secondary}">Scope ID</label>
								<Input id="qw-scope-id" placeholder={orgId} value={wizardDraft.scopeId} oninput={(event) => onUpdateWizard('scopeId', event.currentTarget.value)} />
								<p class="mt-1 text-xs {text.muted}">Org 默认当前 Org；Project / API Key / User 填对应 UUID 或 typed ID。</p>
							</div>
						</div>
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">Scope 预览</p>
							<div class="mt-3 space-y-3 text-sm">
								<div class="flex items-center justify-between gap-3">
									<span class={text.muted}>类型</span>
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
								<Input id="qw-model-filter" placeholder="gpt-4o* / claude-3-5-sonnet / 留空=全部" value={wizardDraft.modelFilter} oninput={(event) => onUpdateWizard('modelFilter', event.currentTarget.value)} />
								<p class="mt-1 text-xs {text.muted}">支持精确模型名或简单 wildcard；留空等同全部模型。</p>
							</div>
							<div class="grid gap-2 md:grid-cols-3">
								{#each ['', 'gpt-4o*', 'claude-*'] as preset}
									<button
										type="button"
										class="rounded-lg border border-zinc-200 px-3 py-2 text-left text-xs font-mono text-zinc-700 hover:bg-zinc-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
										onclick={() => onUpdateWizard('modelFilter', preset)}
									>
										{preset || '全部模型'}
									</button>
								{/each}
							</div>
						</div>
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">过滤效果</p>
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
									<Input id="qw-rpm" type="number" min="0" placeholder="60" value={wizardDraft.rpmLimit} oninput={(event) => onUpdateWizard('rpmLimit', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-tpm" class="mb-1 block text-sm font-medium {text.secondary}">TPM</label>
									<Input id="qw-tpm" type="number" min="0" placeholder="120000" value={wizardDraft.tpmLimit} oninput={(event) => onUpdateWizard('tpmLimit', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-budget" class="mb-1 block text-sm font-medium {text.secondary}">Budget USD 预算</label>
									<Input id="qw-budget" type="number" min="0" step="0.01" placeholder="25" value={wizardDraft.budgetUsd} oninput={(event) => onUpdateWizard('budgetUsd', event.currentTarget.value)} />
								</div>
							</div>
							<div class="grid gap-3 md:grid-cols-3">
								<div>
									<label for="qw-budget-dim" class="mb-1 block text-sm font-medium {text.secondary}">Budget window 周期</label>
									<Select
										id="qw-budget-dim"
										value={wizardDraft.budgetDimension}
										options={[
											{ value: 'daily_budget_usd', label: 'Daily budget 日预算' },
											{ value: 'monthly_budget_usd', label: 'Monthly budget 月预算' },
											{ value: 'lifetime_budget_usd', label: 'Lifetime budget 终身预算' }
										]}
										onchange={(event) => onUpdateWizard('budgetDimension', event.currentTarget.value)}
									/>
								</div>
								<div>
									<label for="qw-est-tokens" class="mb-1 block text-sm font-medium {text.secondary}">Estimated tokens 预估 tokens</label>
									<Input id="qw-est-tokens" type="number" min="0" value={wizardDraft.estimatedTokens} oninput={(event) => onUpdateWizard('estimatedTokens', event.currentTarget.value)} />
								</div>
								<div>
									<label for="qw-est-cost" class="mb-1 block text-sm font-medium {text.secondary}">Estimated cost micros 预估成本 micros</label>
									<Input id="qw-est-cost" type="number" min="0" value={wizardDraft.estimatedCostMicros} oninput={(event) => onUpdateWizard('estimatedCostMicros', event.currentTarget.value)} />
								</div>
							</div>
						</div>
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">本地预览</p>
							{#if wizardPreviewRows.length === 0}
								<p class="mt-3 text-sm {text.muted}">至少输入 rpm、tpm、budget 中的一项。</p>
							{:else}
								<div class="mt-3 space-y-2">
									{#each wizardPreviewRows as row}
										<div class="rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-700 dark:bg-zinc-900">
											<div class="flex items-center justify-between gap-3">
												<span class="font-mono text-xs {text.primary}">{row.dimension}</span>
												<Badge variant={row.wouldDeny ? 'danger' : 'success'}>{row.wouldDeny ? 'would deny 拦截' : 'pass 放行'}</Badge>
											</div>
											<div class="mt-2 grid grid-cols-3 gap-2 text-[11px] {text.muted}">
												<span>限额 <b class={text.primary}>{formatWizardLimit(row)}</b></span>
												<span>预估 <b class={text.primary}>{formatWizardEstimate(row)}</b></span>
												<span>剩余 <b class={text.primary}>{formatWizardEstimate({ ...row, estimated: row.remaining })}</b></span>
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
							<p class="text-xs font-semibold uppercase tracking-wider {text.muted}">待保存请求</p>
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
								<Button variant="outline" onclick={onExplain} disabled={!wizardCanExplain || wizardExplaining}>
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
											<th class={dataTemplate.th}>维度</th>
											<th class={dataTemplate.th}>Mode 模式</th>
											<th class={cn(dataTemplate.th, 'text-right')}>已用</th>
											<th class={cn(dataTemplate.th, 'text-right')}>预估</th>
											<th class={cn(dataTemplate.th, 'text-right')}>剩余</th>
											<th class={dataTemplate.th}>结果</th>
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
												<Badge variant={rule.would_deny ? 'danger' : rule.mode === 'dry_run' ? 'warning' : 'success'}>{rule.would_deny ? 'would deny 拦截' : 'pass 放行'}</Badge>
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
							<Button onclick={() => onGoStep(quotaWizardStep + 1)} disabled={quotaWizardStep >= 3 && !wizardCanPreview}>下一步</Button>
						{/if}
						<Button variant="outline" onclick={onExplain} disabled={!wizardCanExplain || wizardExplaining}>
							{wizardExplaining ? 'Explain 中...' : 'Explain'}
						</Button>
						<Button onclick={onSave} disabled={!wizardCanPreview || wizardSaving}>
							{wizardSaving ? '保存中...' : '保存策略'}
						</Button>
						<Button variant="outline" onclick={onClose}>取消</Button>
					</div>
				</div>
			</div>
		</Card>
	</ModalFrame>
{/if}
