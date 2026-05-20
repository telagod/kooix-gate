import { describe, expect, it } from 'vitest';
import {
	DEFAULT_PRICING_USAGE_PREVIEW,
	computePricingPreview,
	computePricingPreviewMicros,
	formatMicrosUsd,
	pricingRuleQuantity,
	selectPricingPreviewRules
} from '$lib/pricing-preview';
import type { PricingRule } from '$lib/api';

function rule(partial: Partial<PricingRule>): PricingRule {
	return {
		id: partial.id ?? `rule-${partial.dimension ?? 'input'}`,
		channel_id: partial.channel_id ?? null,
		model: partial.model ?? 'gpt-4o-mini',
		dimension: partial.dimension ?? 'input_tokens',
		unit: partial.unit ?? 'per_million_tokens',
		rate: partial.rate ?? 0,
		conditions: partial.conditions ?? {},
		effective_from: partial.effective_from ?? '2026-01-01T00:00:00Z',
		effective_until: partial.effective_until ?? null,
		priority: partial.priority ?? 0,
		description: partial.description ?? null
	};
}

describe('pricing preview helpers', () => {
	it('mirrors token compute_cost semantics in micros', () => {
		const micros = computePricingPreviewMicros(
			{ ...DEFAULT_PRICING_USAGE_PREVIEW, prompt_tokens: 1000, completion_tokens: 500, cached_tokens: 0 },
			[
				rule({ id: 'input', dimension: 'input_tokens', rate: 0.15 }),
				rule({ id: 'output', dimension: 'output_tokens', rate: 0.6 })
			]
		);

		expect(micros).toBe(450);
		expect(formatMicrosUsd(micros)).toBe('$0.000450');
	});

	it('subtracts cached tokens from input_tokens and prices cached_input_tokens separately', () => {
		const result = computePricingPreview(
			{ ...DEFAULT_PRICING_USAGE_PREVIEW, prompt_tokens: 1000, cached_tokens: 250, completion_tokens: 0 },
			[
				rule({ id: 'input', dimension: 'input_tokens', rate: 1 }),
				rule({ id: 'cached', dimension: 'cached_input_tokens', rate: 0.2 })
			]
		);

		expect(result.costMicros).toBe(800);
		expect(result.lineItems.map((item) => item.quantity)).toEqual([750, 250]);
	});

	it('matches image conditions before charging per image', () => {
		const imageRule = rule({
			model: 'dall-e-3',
			dimension: 'per_image',
			unit: 'per_image',
			rate: 0.08,
			conditions: { quality: 'hd', size: '1024x1024' }
		});

		expect(
			pricingRuleQuantity(imageRule, {
				...DEFAULT_PRICING_USAGE_PREVIEW,
				images_generated: 2,
				image_quality: 'hd',
				image_size: '1024x1024'
			})
		).toBe(2);
		expect(
			pricingRuleQuantity(imageRule, {
				...DEFAULT_PRICING_USAGE_PREVIEW,
				images_generated: 2,
				image_quality: 'standard',
				image_size: '1024x1024'
			})
		).toBe(0);
		expect(
			computePricingPreviewMicros(
				{
					...DEFAULT_PRICING_USAGE_PREVIEW,
					images_generated: 2,
					image_quality: 'hd',
					image_size: '1024x1024'
				},
				[imageRule]
			)
		).toBe(160000);
	});

	it('applies batch and region multipliers after base charges', () => {
		const result = computePricingPreview(
			{ ...DEFAULT_PRICING_USAGE_PREVIEW, prompt_tokens: 1000, is_batch: true, region: 'us-east-1' },
			[
				rule({ id: 'base', dimension: 'input_tokens', rate: 1 }),
				rule({ id: 'batch', dimension: 'batch_multiplier', unit: 'multiplier', rate: 0.5 }),
				rule({ id: 'region', dimension: 'region_multiplier', unit: 'multiplier', rate: 1.25, conditions: { region: 'us-east-1' } })
			]
		);

		expect(result.baseMicros).toBe(1000);
		expect(result.costMicros).toBe(625);
		expect(result.lineItems.filter((item) => item.kind === 'multiplier')).toHaveLength(2);
	});

	it('selects channel-specific and higher-priority preview rules by dimension', () => {
		const selected = selectPricingPreviewRules(
			[
				rule({ id: 'global-low', channel_id: null, dimension: 'input_tokens', rate: 1, priority: 0 }),
				rule({ id: 'global-high', channel_id: null, dimension: 'output_tokens', rate: 2, priority: 20 }),
				rule({ id: 'channel', channel_id: 'ch_1', dimension: 'input_tokens', rate: 0.5, priority: 1 }),
				rule({ id: 'other-channel', channel_id: 'ch_2', dimension: 'input_tokens', rate: 9, priority: 99 }),
				rule({ id: 'other-model', model: 'claude-3-5-sonnet', dimension: 'per_request', unit: 'per_request', rate: 1 })
			],
			'gpt-4o-mini',
			'ch_1',
			new Date('2026-05-20T00:00:00Z')
		);

		expect(selected.map((item) => item.id)).toEqual(['channel', 'global-high']);
	});
});
