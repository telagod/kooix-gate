<script lang="ts">
	import { onMount } from 'svelte';
	import { listPricingRules, upsertPricingRule, deletePricingRule, listAdminChannels } from '$lib/api.js';
	import type { PricingRule, Channel } from '$lib/api.js';
	import { shortId } from '$lib/id.js';
	import { Alert, Button, Card, Field, Input, Select } from '$lib/components/ui';
	import { DollarSign, Plus, Trash2, Filter } from 'lucide-svelte';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import DataToolbar from '$lib/components/templates/DataToolbar.svelte';
	import PageShell from '$lib/components/templates/PageShell.svelte';
	import { cn, dataTemplate } from '$lib/design';

	let rules = $state<PricingRule[]>([]);
	let channels = $state<Channel[]>([]);
	let loading = $state(true);
	let error = $state('');

	let filterModel = $state('');
	let filterChannelId = $state('');

	let showForm = $state(false);
	let formModel = $state('');
	let formDimension = $state('input_tokens');
	let formUnit = $state('per_million');
	let formRate = $state('');
	let formChannelId = $state('');
	let formPriority = $state('0');
	let formDescription = $state('');
	let formSaving = $state(false);

	let deletingId = $state('');

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
			await upsertPricingRule({
				model: formModel,
				dimension: formDimension,
				unit: formUnit,
				rate: parseFloat(formRate),
				channel_id: formChannelId || null,
				priority: Number(formPriority || 0),
				description: formDescription || null
			});
			showForm = false;
			formModel = ''; formRate = ''; formChannelId = ''; formDescription = '';
			await reload();
		} catch (err: any) {
			error = err?.message ?? '保存失败';
		} finally {
			formSaving = false;
		}
	}

	async function handleDelete(id: string) {
		deletingId = id;
		try {
			await deletePricingRule(id);
			rules = rules.filter(r => r.id !== id);
		} catch (err: any) {
			error = err?.message ?? '删除失败';
		} finally {
			deletingId = '';
		}
	}

	let filteredRules = $derived(rules);

	function channelName(chId: string | null): string {
		if (!chId) return 'Global';
		const ch = channels.find(c => c.id === chId);
		return ch ? ch.name || ch.code : shortId(chId);
	}

	let filterChannelOptions = $derived([
		{ value: '', label: '全部渠道' },
		{ value: '__global__', label: '仅全局' },
		...channels.map((ch) => ({ value: ch.id, label: ch.name || ch.code }))
	]);

	let formChannelOptions = $derived([
		{ value: '', label: '全局' },
		...channels.map((ch) => ({ value: ch.id, label: ch.name || ch.code }))
	]);
</script>

<PageShell title="定价规则" description="管理模型计费规则，支持多维度定价" icon={DollarSign}>
	{#snippet actions()}
		<Button onclick={() => { showForm = !showForm; }}>
			<Plus size={14} class="mr-1" /> 新建规则
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

	<!-- Create Form -->
	{#if showForm}
		<Card class="p-4 mb-6">
			<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 mb-3">新建定价规则</h3>
			<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
				<Field label="模型" for="pricing-form-model">
					<Input id="pricing-form-model" placeholder="gpt-4o-mini" bind:value={formModel} />
				</Field>
				<Field label="维度" for="pricing-form-dimension">
					<Select id="pricing-form-dimension" bind:value={formDimension} options={dimensionOptions} size="sm" />
				</Field>
				<Field label="单位" for="pricing-form-unit">
					<Select id="pricing-form-unit" bind:value={formUnit} options={unitOptions} size="sm" />
				</Field>
				<Field label="费率" for="pricing-form-rate">
					<Input id="pricing-form-rate" type="number" step="0.0001" placeholder="2.5000" bind:value={formRate} />
				</Field>
				<Field label="渠道 (空=全局)" for="pricing-form-channel">
					<Select id="pricing-form-channel" bind:value={formChannelId} options={formChannelOptions} size="sm" />
				</Field>
				<Field label="优先级" for="pricing-form-priority">
					<Input id="pricing-form-priority" type="number" bind:value={formPriority} />
				</Field>
				<Field label="描述" for="pricing-form-description" class="col-span-2">
					<Input id="pricing-form-description" placeholder="可选描述" bind:value={formDescription} />
				</Field>
			</div>
			<div class="flex gap-2 mt-4">
				<Button onclick={handleSave} disabled={formSaving || !formModel || !formRate}>
					{formSaving ? '保存中...' : '保存'}
				</Button>
				<Button variant="outline" onclick={() => { showForm = false; }}>取消</Button>
			</div>
		</Card>
	{/if}

	<!-- Rules Table -->
	{#if loading}
		<div class="space-y-2">
			{#each Array(5) as _}
				<div class="h-12 bg-zinc-200 dark:bg-zinc-700 rounded animate-pulse"></div>
			{/each}
		</div>
	{:else}
		<DataTable isEmpty={filteredRules.length === 0} emptyColspan={8}>
			{#snippet head()}
				<tr>
					<th class={dataTemplate.th}>模型</th>
					<th class={dataTemplate.th}>维度</th>
					<th class={dataTemplate.th}>单位</th>
					<th class={cn(dataTemplate.th, 'text-right')}>费率</th>
					<th class={dataTemplate.th}>渠道</th>
					<th class={cn(dataTemplate.th, 'text-center')}>优先级</th>
					<th class={dataTemplate.th}>描述</th>
					<th class="px-4 py-3 w-12"></th>
				</tr>
			{/snippet}

			{#snippet empty()}
				<DollarSign size={24} class="mx-auto mb-2 text-zinc-300 dark:text-zinc-600" />
				暂无定价规则
			{/snippet}

			{#each filteredRules as rule}
				<tr class={dataTemplate.row}>
					<td class={dataTemplate.tdMonoStrong}>{rule.model}</td>
					<td class={dataTemplate.td}>{rule.dimension}</td>
					<td class={dataTemplate.td}>{rule.unit}</td>
					<td class={cn(dataTemplate.tdMonoStrong, 'text-right')}>{rule.rate.toFixed(4)}</td>
					<td class={dataTemplate.td}>{channelName(rule.channel_id)}</td>
					<td class={cn(dataTemplate.td, 'text-center')}>{rule.priority}</td>
					<td class={cn(dataTemplate.td, 'truncate max-w-[200px]')}>{rule.description ?? '—'}</td>
					<td class={dataTemplate.td}>
						<button
							type="button"
							aria-label="删除定价规则"
							onclick={() => handleDelete(rule.id)}
							disabled={deletingId === rule.id}
							class="p-1 rounded text-zinc-400 hover:text-red-600 dark:hover:text-red-400 transition-colors disabled:pointer-events-none disabled:opacity-50"
						>
							<Trash2 size={14} />
						</button>
					</td>
				</tr>
			{/each}
		</DataTable>
		<p class="text-xs text-zinc-500 dark:text-zinc-400 mt-2">{filteredRules.length} 条规则</p>
	{/if}
</PageShell>
