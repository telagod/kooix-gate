<script lang="ts">
	import { onMount } from 'svelte';
	import { listPricingRules, upsertPricingRule, deletePricingRule, listAdminChannels } from '$lib/api.js';
	import type { PricingRule, Channel } from '$lib/api.js';
	import { shortId } from '$lib/id.js';
	import { Alert, Button, Card, Field, Input, Select, Textarea } from '$lib/components/ui';
	import { Calculator, CheckCircle2, DollarSign, Filter, Plus, Trash2 } from 'lucide-svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import PricingRulesTable from './_components/PricingRulesTable.svelte';
	import DeletePricingModal from './_components/DeletePricingModal.svelte';
	import PricingWizard from './_components/PricingWizard.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import { cn, dataTemplate } from '$lib/design';
	import {
		DEFAULT_PRICING_USAGE_PREVIEW,
		computePricingPreview,
		formatMicrosUsd,
		formatPreviewQuantity,
		selectPricingPreviewRules,
		type PricingPreviewResult,
		type PricingUsagePreviewInput
	} from '$lib/pricing-preview.js';

	let rules = $state<PricingRule[]>([]);
	let channels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');

	let filterModel = $state('');
	let filterChannelId = $state('');

	let showForm = $state(false);
	let pricingWizardStep = $state(1);
	let formModel = $state('');
	let formDimension = $state('input_tokens');
	let formUnit = $state('per_million_tokens');
	let formRate = $state('');
	let formChannelId = $state('');
	let formPriority = $state('0');
	let formDescription = $state('');
	let formConditions = $state('{}');
	let formSaving = $state(false);
	let formConfirmation = $state('');
	let usagePreview = $state<PricingUsagePreviewInput>({ ...DEFAULT_PRICING_USAGE_PREVIEW });

	let deletingId = $state('');
	let deleteConfirmation = $state('');

	const dimensions = [
		'input_tokens',
		'output_tokens',
		'cached_input_tokens',
		'cache_write_tokens',
		'reasoning_tokens',
		'audio_input_tokens',
		'audio_output_tokens',
		'image_input_tokens',
		'image_output_tokens',
		'per_image',
		'per_minute_audio',
		'per_character_tts',
		'per_second_video',
		'per_search',
		'per_request',
		'batch_multiplier',
		'priority_multiplier',
		'region_multiplier'
	];
	const units = [
		'per_million_tokens',
		'per_million_characters',
		'per_image',
		'per_minute',
		'per_second',
		'per_character',
		'per_search',
		'per_request',
		'multiplier'
	];
	const dimensionOptions = dimensions.map((value) => ({ value, label: value }));
	const unitOptions = units.map((value) => ({ value, label: value }));
	const wizardSteps = [
		{ step: 1, label: '模型与 Channel', hint: '选择生效范围' },
		{ step: 2, label: '计费维度', hint: '设置 dimension / unit / rate' },
		{ step: 3, label: '价格预览', hint: '核对 rule 与匹配规则' },
		{ step: 4, label: 'Usage cost 模拟', hint: '模拟一条请求成本' }
	];
	const conditionTemplates = [
		{ label: '空条件', value: '{}' },
		{ label: '缓存 TTL', value: '{\\n  \"cache_ttl\": \"ephemeral\"\\n}' },
		{ label: '图片尺寸', value: '{\\n  \"quality\": \"hd\",\\n  \"size\": \"1024x1024\"\\n}' },
		{ label: '音频场景', value: '{\\n  \"deployment_type\": \"realtime\"\\n}' },
		{ label: 'Batch 请求', value: '{\\n  \"batch\": true\\n}' },
		{ label: 'Region 区域', value: '{\\n  \"region\": \"us-east-1\"\\n}' }
	];

	let parsedConditions = $derived(parseConditions(formConditions));
	let draftRate = $derived(parseFloat(formRate));
	let draftRule = $derived<PricingRule | null>(
		formModel.trim() && Number.isFinite(draftRate)
			? {
					id: '__draft__',
					channel_id: formChannelId || null,
					model: formModel.trim(),
					dimension: formDimension,
					unit: formUnit,
					rate: draftRate,
					conditions: parsedConditions.ok ? parsedConditions.value : {},
					effective_from: new Date().toISOString(),
					effective_until: null,
					priority: Number(formPriority || 0),
					description: formDescription || null
				}
			: null
	);
	let existingPreviewRules = $derived(
		selectPricingPreviewRules(rules, formModel, formChannelId || null)
	);
	let previewRules = $derived(
		draftRule
			? selectPricingPreviewRules([...rules, draftRule], formModel, formChannelId || null)
			: existingPreviewRules
	);
	let pricingPreview = $derived<PricingPreviewResult>(computePricingPreview(usagePreview, previewRules));
	let draftQuantity = $derived(draftRule ? pricingPreview.lineItems.find((item) => item.rule.id === '__draft__')?.quantity ?? 0 : 0);
	let canGoPreview = $derived(Boolean(formModel.trim() && formDimension && formUnit && Number.isFinite(draftRate) && parsedConditions.ok));
	let selectedChannelLabel = $derived(channelName(formChannelId || null));

	onMount(async () => {
		try {
			const [r, c] = await Promise.all([
				listPricingRules(),
				listAdminChannels().catch(() => [])
			]);
			rules = r;
			channels = Array.isArray(c) ? c : (c as any)?.data ?? [];
		} catch (err: any) {
			error = err?.message ?? '加载失败';
		} finally {
			loading = false;
		}
	});

	async function reload() {
		const params: { channelId?: string; model?: string } = {};
		if (filterChannelId) params.channelId = filterChannelId;
		if (filterModel) params.model = filterModel;
		rules = await listPricingRules(params.channelId, params.model);
	}

	async function handleSave() {
		formSaving = true;
		try {
			if (!parsedConditions.ok) {
				error = `Conditions JSON 无效：${parsedConditions.error}`;
				return;
			}
			await upsertPricingRule({
				model: formModel,
				dimension: formDimension,
				unit: formUnit,
				rate: parseFloat(formRate),
				channel_id: formChannelId || null,
				conditions: parsedConditions.value,
				priority: Number(formPriority || 0),
				description: formDescription || null
			}, formConfirmation);
			showForm = false;
			resetWizard();
			await reload();
		} catch (err: any) {
			error = err?.message ?? '保存失败';
		} finally {
			formSaving = false;
		}
	}

	async function handleDelete(id: string) {
		deletingId = id;
		deleteConfirmation = '';
	}

	async function confirmDelete() {
		if (!deletingId) return;
		try {
			await deletePricingRule(deletingId, deleteConfirmation);
			rules = rules.filter(r => r.id !== deletingId);
			deletingId = '';
			deleteConfirmation = '';
		} catch (err: any) {
			error = err?.message ?? '删除失败';
		}
	}

	function openWizard() {
		showForm = true;
		pricingWizardStep = 1;
		error = '';
	}

	function resetWizard() {
		pricingWizardStep = 1;
		formModel = '';
		formDimension = 'input_tokens';
		formUnit = 'per_million_tokens';
		formRate = '';
		formChannelId = '';
		formPriority = '0';
		formDescription = '';
		formConditions = '{}';
		formConfirmation = '';
		usagePreview = { ...DEFAULT_PRICING_USAGE_PREVIEW };
	}

	function closeWizard() {
		showForm = false;
		resetWizard();
	}

	function goWizardStep(step: number) {
		if (step <= 2 || canGoPreview) {
			pricingWizardStep = step;
		}
	}

	function syncModelFromChannel() {
		if (formModel) return;
		const channel = channels.find((ch) => ch.id === formChannelId);
		const firstModel = channel?.supported_models?.[0];
		if (firstModel) formModel = firstModel;
	}

	function parseConditions(raw: string): { ok: true; value: Record<string, any> } | { ok: false; error: string; value: Record<string, any> } {
		try {
			const value = JSON.parse(raw || '{}');
			if (!value || Array.isArray(value) || typeof value !== 'object') {
				throw new Error('conditions must be a JSON object');
			}
			return { ok: true, value };
		} catch (err: any) {
			return { ok: false, error: err?.message ?? String(err), value: {} };
		}
	}

	function setUsageNumber(key: keyof PricingUsagePreviewInput, value: string) {
		const next = Number(value);
		usagePreview = {
			...usagePreview,
			[key]: Number.isFinite(next) ? Math.max(0, next) : 0
		};
	}

	function setUsageText(key: keyof PricingUsagePreviewInput, value: string) {
		usagePreview = {
			...usagePreview,
			[key]: value || null
		};
	}

	let filteredRules = $derived(rules);

	function channelName(chId: string | null): string {
		if (!chId) return 'Global';
		const ch = channels.find(c => c.id === chId);
		return ch ? ch.name || ch.code : shortId(chId);
	}

	let pricingConfirmation = $derived(`pricing:${formModel.trim()}:${formDimension}`);

	let filterChannelOptions = $derived([
		{ value: '', label: '全部渠道' },
		{ value: '__global__', label: '仅全局' },
		...channels.map((ch) => ({ value: ch.id, label: ch.name || ch.code }))
	]);

	let formChannelOptions = $derived([
		{ value: '', label: '全局' },
		...channels.map((ch) => ({ value: ch.id, label: ch.name || ch.code }))
	]);

	let modelOptions = $derived(
		Array.from(
			new Set([
				...rules.map((rule) => rule.model).filter(Boolean),
				...channels.flatMap((channel) => channel.supported_models ?? []).filter(Boolean)
			])
		).sort()
	);
</script>

<PageShell title="定价规则" description="管理模型计费规则，支持多维度定价" icon={DollarSign}>
	{#snippet actions()}
		<Button onclick={showForm ? closeWizard : openWizard}>
			<Plus size={14} class="mr-1" /> {showForm ? '收起 wizard' : '新建 Pricing wizard'}
		</Button>
	{/snippet}

	{#if error}
		<Alert variant="danger" class="mb-4">{error}</Alert>
	{/if}

	<!-- Filters -->
	<DataToolbar class="mb-4">
		<Filter size={14} class="text-zinc-400" />
		<Input id="pricing-filter-model" placeholder="按模型过滤" bind:value={filterModel} size="sm" class="w-48" />
		<Select id="pricing-filter-channel" bind:value={filterChannelId} options={filterChannelOptions} size="sm" class="w-44" />
		<Button variant="outline" size="sm" onclick={reload}>查询</Button>
	</DataToolbar>

	<!-- Pricing Wizard -->
	<PricingWizard
		{showForm}
		bind:pricingWizardStep
		bind:formModel
		bind:formDimension
		bind:formUnit
		bind:formRate
		bind:formChannelId
		bind:formPriority
		bind:formDescription
		bind:formConditions
		bind:formConfirmation
		{formSaving}
		bind:usagePreview
		{wizardSteps}
		{conditionTemplates}
		{dimensionOptions}
		{unitOptions}
		{channels}
		{parsedConditions}
		{draftRate}
		{draftRule}
		{previewRules}
		{pricingPreview}
		{draftQuantity}
		{canGoPreview}
		{selectedChannelLabel}
		{channelName}
		{formatMicrosUsd}
		{formatPreviewQuantity}
		{pricingConfirmation}
		{formChannelOptions}
		{modelOptions}
		onClose={closeWizard}
		onGoStep={goWizardStep}
		onSave={handleSave}
		onSyncModel={syncModelFromChannel}
		onSetUsageText={setUsageText}
		onSetUsageNumber={setUsageNumber}
	/>

	<!-- Rules Table -->
	<PricingRulesTable
		{loading}
		rules={filteredRules}
		{deletingId}
		{channelName}
		onDelete={handleDelete}
	/>
</PageShell>


<DeletePricingModal
	{deletingId}
	deletingRule={rules.find((rule) => rule.id === deletingId) ?? null}
	bind:deleteConfirmation
	onClose={() => { deletingId = ''; deleteConfirmation = ''; }}
	onConfirm={confirmDelete}
/>
