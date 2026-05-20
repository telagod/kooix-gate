import { describe, expect, it } from 'vitest';
import {
	buildQuotaWizardRequests,
	hasQuotaWizardLimit,
	normalizeQuotaModelFilter,
	previewQuotaWizardDraft,
	quotaWizardExplainDimension
} from '$lib/quota-wizard';
import type { QuotaWizardDraft } from '$lib/quota-wizard';

const draft: QuotaWizardDraft = {
	scopeKind: 'org',
	scopeId: 'org_019e2c1ba7d17162842207e4b24f5f98',
	modelFilter: 'gpt-4o*',
	mode: 'enforce',
	rpmLimit: '60',
	tpmLimit: '120000',
	budgetUsd: '25',
	budgetDimension: 'monthly_budget_usd',
	estimatedTokens: '2000',
	estimatedCostMicros: '15000'
};

describe('quota wizard helpers', () => {
	it('normalizes blank and wildcard model filters', () => {
		expect(normalizeQuotaModelFilter('')).toBeUndefined();
		expect(normalizeQuotaModelFilter(' * ')).toBeUndefined();
		expect(normalizeQuotaModelFilter(' gpt-4o* ')).toBe('gpt-4o*');
	});

	it('builds rpm, tpm, and budget upsert requests from one wizard draft', () => {
		const requests = buildQuotaWizardRequests(draft);
		expect(requests).toEqual([
			{
				scope_kind: 'org',
				scope_id: draft.scopeId,
				model_filter: 'gpt-4o*',
				mode: 'enforce',
				dimension: 'rpm',
				limit_value: '60',
				window_seconds: 60
			},
			{
				scope_kind: 'org',
				scope_id: draft.scopeId,
				model_filter: 'gpt-4o*',
				mode: 'enforce',
				dimension: 'tpm',
				limit_value: '120000',
				window_seconds: 60
			},
			{
				scope_kind: 'org',
				scope_id: draft.scopeId,
				model_filter: 'gpt-4o*',
				mode: 'enforce',
				dimension: 'monthly_budget_usd',
				limit_value: '25',
				window_seconds: null
			}
		]);
	});

	it('ignores empty or invalid limits', () => {
		expect(hasQuotaWizardLimit({ rpmLimit: '', tpmLimit: '0', budgetUsd: '-1' })).toBe(false);
		expect(buildQuotaWizardRequests({ ...draft, rpmLimit: '', tpmLimit: '0', budgetUsd: '-1' })).toEqual([]);
		expect(quotaWizardExplainDimension({ ...draft, tpmLimit: '', budgetUsd: '' })).toBe('rpm');
	});

	it('previews estimated usage and would-deny flags before backend explain', () => {
		const rows = previewQuotaWizardDraft({
			...draft,
			rpmLimit: '1',
			tpmLimit: '1000',
			budgetUsd: '0.01',
			estimatedTokens: '1500',
			estimatedCostMicros: '15000'
		});

		expect(rows).toEqual([
			{ dimension: 'rpm', limit: 1, estimated: 1, remaining: 0, wouldDeny: false, unit: 'requests' },
			{ dimension: 'tpm', limit: 1000, estimated: 1500, remaining: 0, wouldDeny: true, unit: 'tokens' },
			{ dimension: 'monthly_budget_usd', limit: 10000, estimated: 15000, remaining: 0, wouldDeny: true, unit: 'micros' }
		]);
	});
});
