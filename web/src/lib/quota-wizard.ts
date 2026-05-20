import type { UpsertQuotaRequest } from '$lib/api';

export const BUDGET_DIMENSIONS = [
	'daily_budget_usd',
	'monthly_budget_usd',
	'lifetime_budget_usd'
] as const;

export type QuotaBudgetDimension = (typeof BUDGET_DIMENSIONS)[number];

export interface QuotaWizardDraft {
	scopeKind: string;
	scopeId: string;
	modelFilter: string;
	mode: 'enforce' | 'dry_run';
	rpmLimit: string;
	tpmLimit: string;
	budgetUsd: string;
	budgetDimension: string;
	estimatedTokens: string;
	estimatedCostMicros: string;
}

export interface QuotaWizardPreviewRule {
	dimension: string;
	limit: number;
	estimated: number;
	remaining: number;
	wouldDeny: boolean;
	unit: 'requests' | 'tokens' | 'micros';
}

export function normalizeQuotaModelFilter(value: string): string | undefined {
	const trimmed = value.trim();
	if (!trimmed || trimmed === '*') return undefined;
	return trimmed;
}

export function hasQuotaWizardLimit(draft: Pick<QuotaWizardDraft, 'rpmLimit' | 'tpmLimit' | 'budgetUsd'>): boolean {
	return [draft.rpmLimit, draft.tpmLimit, draft.budgetUsd].some((value) => parsePositiveNumber(value) !== null);
}

export function buildQuotaWizardRequests(draft: QuotaWizardDraft): UpsertQuotaRequest[] {
	const scope_id = draft.scopeId.trim();
	const model_filter = normalizeQuotaModelFilter(draft.modelFilter);
	const common = {
		scope_kind: draft.scopeKind,
		scope_id,
		model_filter,
		mode: draft.mode
	};
	const requests: UpsertQuotaRequest[] = [];
	const rpm = parsePositiveNumber(draft.rpmLimit);
	const tpm = parsePositiveNumber(draft.tpmLimit);
	const budget = parsePositiveNumber(draft.budgetUsd);

	if (rpm !== null) {
		requests.push({
			...common,
			dimension: 'rpm',
			limit_value: String(rpm),
			window_seconds: 60
		});
	}
	if (tpm !== null) {
		requests.push({
			...common,
			dimension: 'tpm',
			limit_value: String(tpm),
			window_seconds: 60
		});
	}
	if (budget !== null) {
		requests.push({
			...common,
			dimension: normalizeBudgetDimension(draft.budgetDimension),
			limit_value: String(budget),
			window_seconds: null
		});
	}

	return requests;
}

export function previewQuotaWizardDraft(draft: QuotaWizardDraft): QuotaWizardPreviewRule[] {
	const rpm = parsePositiveNumber(draft.rpmLimit);
	const tpm = parsePositiveNumber(draft.tpmLimit);
	const budget = parsePositiveNumber(draft.budgetUsd);
	const estimatedTokens = Math.max(0, Number(draft.estimatedTokens || 0) || 0);
	const estimatedCostMicros = Math.max(0, Number(draft.estimatedCostMicros || 0) || 0);
	const rows: QuotaWizardPreviewRule[] = [];

	if (rpm !== null) rows.push(previewRule('rpm', rpm, 1, 'requests'));
	if (tpm !== null) rows.push(previewRule('tpm', tpm, estimatedTokens, 'tokens'));
	if (budget !== null) {
		rows.push(previewRule(normalizeBudgetDimension(draft.budgetDimension), budget * 1_000_000, estimatedCostMicros, 'micros'));
	}

	return rows;
}

export function quotaWizardExplainDimension(draft: QuotaWizardDraft): string | undefined {
	const requests = buildQuotaWizardRequests(draft);
	if (requests.length === 1) return requests[0].dimension;
	return undefined;
}

function previewRule(
	dimension: string,
	limit: number,
	estimated: number,
	unit: QuotaWizardPreviewRule['unit']
): QuotaWizardPreviewRule {
	const remaining = Math.max(0, limit - estimated);
	return {
		dimension,
		limit,
		estimated,
		remaining,
		wouldDeny: estimated > limit,
		unit
	};
}

function normalizeBudgetDimension(value: string): QuotaBudgetDimension {
	return BUDGET_DIMENSIONS.includes(value as QuotaBudgetDimension)
		? (value as QuotaBudgetDimension)
		: 'monthly_budget_usd';
}

function parsePositiveNumber(value: string): number | null {
	const parsed = Number(value);
	if (!Number.isFinite(parsed) || parsed <= 0) return null;
	return parsed;
}
