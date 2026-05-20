import type { PricingRule } from '$lib/api';

export interface PricingUsagePreviewInput {
	prompt_tokens: number;
	completion_tokens: number;
	cached_tokens: number;
	reasoning_tokens: number;
	audio_input_tokens: number;
	audio_output_tokens: number;
	image_input_tokens: number;
	image_output_tokens: number;
	images_generated: number;
	audio_minutes: number;
	tts_characters: number;
	video_seconds: number;
	search_count: number;
	is_batch: boolean;
	context_length: number;
	image_quality?: string | null;
	image_size?: string | null;
	cache_ttl?: string | null;
	region?: string | null;
	deployment_type?: string | null;
}

export interface PricingPreviewLineItem {
	rule: PricingRule;
	quantity: number;
	costMicros: number;
	kind: 'charge' | 'multiplier';
	beforeMicros?: number;
	afterMicros?: number;
}

export interface PricingPreviewResult {
	baseMicros: number;
	costMicros: number;
	lineItems: PricingPreviewLineItem[];
}

export const DEFAULT_PRICING_USAGE_PREVIEW: PricingUsagePreviewInput = {
	prompt_tokens: 1000,
	completion_tokens: 500,
	cached_tokens: 0,
	reasoning_tokens: 0,
	audio_input_tokens: 0,
	audio_output_tokens: 0,
	image_input_tokens: 0,
	image_output_tokens: 0,
	images_generated: 1,
	audio_minutes: 1,
	tts_characters: 1000,
	video_seconds: 30,
	search_count: 1,
	is_batch: false,
	context_length: 8000,
	image_quality: 'hd',
	image_size: '1024x1024',
	cache_ttl: null,
	region: 'us-east-1',
	deployment_type: null
};

export function pricingRuleMatchesPreview(
	rule: PricingRule,
	model: string,
	channelId: string | null,
	at: Date = new Date()
): boolean {
	const modelName = model.trim();
	if (!modelName) return false;
	if (rule.model !== modelName && rule.model !== '*') return false;
	if (rule.channel_id && rule.channel_id !== channelId) return false;

	const effectiveFrom = Date.parse(rule.effective_from);
	const effectiveUntil = rule.effective_until ? Date.parse(rule.effective_until) : Number.POSITIVE_INFINITY;
	const timestamp = at.getTime();
	if (Number.isFinite(effectiveFrom) && timestamp < effectiveFrom) return false;
	if (Number.isFinite(effectiveUntil) && timestamp >= effectiveUntil) return false;
	return true;
}

export function selectPricingPreviewRules(
	rules: PricingRule[],
	model: string,
	channelId: string | null,
	at: Date = new Date()
): PricingRule[] {
	const matching = rules.filter((rule) => pricingRuleMatchesPreview(rule, model, channelId, at));
	const byDimension = new Map<string, PricingRule>();

	for (const rule of matching.sort((a, b) => compareRulePriority(a, b, channelId))) {
		if (!byDimension.has(rule.dimension)) {
			byDimension.set(rule.dimension, rule);
		}
	}

	return Array.from(byDimension.values()).sort((a, b) => a.dimension.localeCompare(b.dimension));
}

export function pricingRuleQuantity(rule: Pick<PricingRule, 'dimension' | 'conditions'>, ctx: PricingUsagePreviewInput): number {
	switch (rule.dimension) {
		case 'input_tokens':
			return Math.max(0, safeNumber(ctx.prompt_tokens) - safeNumber(ctx.cached_tokens));
		case 'output_tokens':
			return safeNumber(ctx.completion_tokens);
		case 'cached_input_tokens':
			return safeNumber(ctx.cached_tokens);
		case 'cache_write_tokens':
			return truthyText(ctx.cache_ttl) ? safeNumber(ctx.cached_tokens) : 0;
		case 'reasoning_tokens':
			return safeNumber(ctx.reasoning_tokens);
		case 'audio_input_tokens':
			return safeNumber(ctx.audio_input_tokens);
		case 'audio_output_tokens':
			return safeNumber(ctx.audio_output_tokens);
		case 'image_input_tokens':
			return safeNumber(ctx.image_input_tokens);
		case 'image_output_tokens':
			return safeNumber(ctx.image_output_tokens);
		case 'per_image':
			return conditionsMatch(rule.conditions, ctx) ? safeNumber(ctx.images_generated) : 0;
		case 'per_minute_audio':
			return safeNumber(ctx.audio_minutes);
		case 'per_character_tts':
			return safeNumber(ctx.tts_characters);
		case 'per_second_video':
			return safeNumber(ctx.video_seconds);
		case 'per_search':
			return safeNumber(ctx.search_count);
		case 'per_request':
			return 1;
		default:
			return 0;
	}
}

export function computePricingPreviewMicros(ctx: PricingUsagePreviewInput, rules: PricingRule[]): number {
	return computePricingPreview(ctx, rules).costMicros;
}

export function computePricingPreview(ctx: PricingUsagePreviewInput, rules: PricingRule[]): PricingPreviewResult {
	let totalUsd = 0;
	const lineItems: PricingPreviewLineItem[] = [];

	for (const rule of rules) {
		const quantity = pricingRuleQuantity(rule, ctx);
		if (quantity === 0) continue;

		const costUsd = costUsdForRule(rule, quantity);
		totalUsd += costUsd;
		lineItems.push({
			rule,
			quantity,
			costMicros: usdToMicros(costUsd),
			kind: 'charge'
		});
	}

	const baseMicros = usdToMicros(totalUsd);

	for (const rule of rules) {
		if (!multiplierApplies(rule, ctx)) continue;
		const beforeMicros = usdToMicros(totalUsd);
		totalUsd *= safeNumber(rule.rate);
		const afterMicros = usdToMicros(totalUsd);
		lineItems.push({
			rule,
			quantity: 1,
			costMicros: afterMicros - beforeMicros,
			kind: 'multiplier',
			beforeMicros,
			afterMicros
		});
	}

	return {
		baseMicros,
		costMicros: usdToMicros(totalUsd),
		lineItems
	};
}

export function formatMicrosUsd(micros: number): string {
	if (!Number.isFinite(micros)) return '$0.000000';
	const sign = micros < 0 ? '-' : '';
	const usd = Math.abs(micros) / 1_000_000;
	if (usd >= 1) return `${sign}$${usd.toFixed(4)}`;
	return `${sign}$${usd.toFixed(6)}`;
}

export function formatPreviewQuantity(quantity: number, unit: string): string {
	const suffix = unit === 'multiplier' ? 'x' : '';
	if (!Number.isFinite(quantity)) return `0${suffix}`;
	if (Number.isInteger(quantity)) return `${quantity.toLocaleString()}${suffix}`;
	return `${quantity.toLocaleString(undefined, { maximumFractionDigits: 4 })}${suffix}`;
}

function compareRulePriority(a: PricingRule, b: PricingRule, channelId: string | null): number {
	const aChannelRank = a.channel_id === channelId ? 0 : 1;
	const bChannelRank = b.channel_id === channelId ? 0 : 1;
	if (aChannelRank !== bChannelRank) return aChannelRank - bChannelRank;
	if (a.priority !== b.priority) return b.priority - a.priority;

	const aTime = Date.parse(a.effective_from);
	const bTime = Date.parse(b.effective_from);
	if (Number.isFinite(aTime) && Number.isFinite(bTime) && aTime !== bTime) {
		return bTime - aTime;
	}
	return a.id.localeCompare(b.id);
}

function costUsdForRule(rule: Pick<PricingRule, 'unit' | 'rate'>, quantity: number): number {
	const rate = safeNumber(rule.rate);
	switch (rule.unit) {
		case 'per_million_tokens':
		case 'per_million_characters':
			return (quantity * rate) / 1_000_000;
		case 'per_image':
		case 'per_minute':
		case 'per_second':
		case 'per_character':
		case 'per_search':
		case 'per_request':
			return quantity * rate;
		default:
			return (quantity * rate) / 1_000_000;
	}
}

function multiplierApplies(rule: PricingRule, ctx: PricingUsagePreviewInput): boolean {
	if (rule.dimension === 'batch_multiplier') return ctx.is_batch;
	if (rule.dimension === 'region_multiplier') return truthyText(ctx.region) && conditionsMatch(rule.conditions, ctx);
	return false;
}

function conditionsMatch(conditions: Record<string, any> | null | undefined, ctx: PricingUsagePreviewInput): boolean {
	if (!conditions || typeof conditions !== 'object' || Array.isArray(conditions)) return true;
	const entries = Object.entries(conditions);
	if (entries.length === 0) return true;

	for (const [key, value] of entries) {
		let matches = true;
		switch (key) {
			case 'quality':
				matches = optionalText(ctx.image_quality) === optionalStringValue(value);
				break;
			case 'size':
				matches = optionalText(ctx.image_size) === optionalStringValue(value);
				break;
			case 'cache_ttl':
				matches = optionalText(ctx.cache_ttl) === optionalStringValue(value);
				break;
			case 'region':
				matches = optionalText(ctx.region) === optionalStringValue(value);
				break;
			case 'deployment_type':
				matches = optionalText(ctx.deployment_type) === optionalStringValue(value);
				break;
			case 'batch':
				matches = ctx.is_batch === (typeof value === 'boolean' ? value : false);
				break;
			case 'context_above':
				matches = safeNumber(ctx.context_length) > safeNumber(value);
				break;
			default:
				matches = true;
		}
		if (!matches) return false;
	}

	return true;
}

function usdToMicros(usd: number): number {
	const micros = usd * 1_000_000;
	if (!Number.isFinite(micros)) return 0;
	return Math.round(micros);
}

function safeNumber(value: unknown): number {
	const parsed = typeof value === 'number' ? value : Number(value);
	if (!Number.isFinite(parsed)) return 0;
	return parsed;
}

function truthyText(value: string | null | undefined): boolean {
	return typeof value === 'string' && value.length > 0;
}

function optionalText(value: string | null | undefined): string | undefined {
	if (typeof value !== 'string' || value.length === 0) return undefined;
	return value;
}

function optionalStringValue(value: unknown): string | undefined {
	return typeof value === 'string' ? value : undefined;
}
