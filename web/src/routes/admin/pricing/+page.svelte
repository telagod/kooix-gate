<script lang="ts">
	import { onMount } from 'svelte';
	import { listPricingRules, upsertPricingRule, deletePricingRule, listAdminChannels } from '$lib/api.js';
	import type { PricingRule, Channel } from '$lib/api.js';
	import { shortId } from '$lib/id.js';
	import Card from '$lib/components/ui/Card.svelte';
	import Button from '$lib/components/ui/Button.svelte';
	import Input from '$lib/components/ui/Input.svelte';
	import { DollarSign, Plus, Trash2, Filter, X } from 'lucide-svelte';

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
	let formPriority = $state(0);
	let formDescription = $state('');
	let formSaving = $state(false);

	let deletingId = $state('');

	const dimensions = ['input_tokens', 'output_tokens', 'cached_input_tokens', 'reasoning_tokens', 'images_generated', 'audio_seconds_in', 'tts_characters'];
	const units = ['per_million', 'per_unit', 'per_second', 'per_character', 'per_image'];

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
				priority: formPriority,
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
</script>

<div class="px-6 py-6">
	<div class="flex items-center justify-between mb-6">
		<div>
			<h1 class="text-2xl font-bold text-zinc-900 dark:text-zinc-100">定价规则</h1>
			<p class="text-sm text-zinc-600 dark:text-zinc-300 mt-0.5">管理模型计费规则，支持多维度定价</p>
		</div>
		<Button onclick={() => { showForm = !showForm; }}>
			<Plus size={14} class="mr-1" /> 新建规则
		</Button>
	</div>

	{#if error}
		<Card class="p-4 mb-4 border-red-200 dark:border-red-800 bg-red-50 dark:bg-red-900/20">
			<p class="text-sm text-red-600 dark:text-red-400">{error}</p>
		</Card>
	{/if}

	<!-- Filters -->
	<div class="flex items-center gap-3 mb-4">
		<Filter size={14} class="text-zinc-400" />
		<Input placeholder="按模型过滤" bind:value={filterModel} class="w-48" />
		<select bind:value={filterChannelId} class="h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm text-zinc-900 dark:text-zinc-100">
			<option value="">全部渠道</option>
			<option value="__global__">仅全局</option>
			{#each channels as ch}
				<option value={ch.id}>{ch.name || ch.code}</option>
			{/each}
		</select>
		<Button variant="outline" size="sm" onclick={reload}>查询</Button>
	</div>

	<!-- Create Form -->
	{#if showForm}
		<Card class="p-4 mb-6">
			<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100 mb-3">新建定价规则</h3>
			<div class="grid grid-cols-2 md:grid-cols-4 gap-3">
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">模型</label>
					<Input placeholder="gpt-4o-mini" bind:value={formModel} />
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">维度</label>
					<select bind:value={formDimension} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm">
						{#each dimensions as d}
							<option value={d}>{d}</option>
						{/each}
					</select>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">单位</label>
					<select bind:value={formUnit} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm">
						{#each units as u}
							<option value={u}>{u}</option>
						{/each}
					</select>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">费率</label>
					<Input type="number" step="0.0001" placeholder="2.5000" bind:value={formRate} />
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">渠道 (空=全局)</label>
					<select bind:value={formChannelId} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 text-sm">
						<option value="">全局</option>
						{#each channels as ch}
							<option value={ch.id}>{ch.name || ch.code}</option>
						{/each}
					</select>
				</div>
				<div>
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">优先级</label>
					<Input type="number" bind:value={formPriority} />
				</div>
				<div class="col-span-2">
					<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">描述</label>
					<Input placeholder="可选描述" bind:value={formDescription} />
				</div>
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
		<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 overflow-hidden">
			<table class="w-full text-sm">
				<thead class="bg-zinc-50 dark:bg-zinc-800/50">
					<tr>
						<th class="px-4 py-3 text-left text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase">模型</th>
						<th class="px-4 py-3 text-left text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase">维度</th>
						<th class="px-4 py-3 text-left text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase">单位</th>
						<th class="px-4 py-3 text-right text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase">费率</th>
						<th class="px-4 py-3 text-left text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase">渠道</th>
						<th class="px-4 py-3 text-center text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase">优先级</th>
						<th class="px-4 py-3 text-left text-xs font-medium text-zinc-500 dark:text-zinc-400 uppercase">描述</th>
						<th class="px-4 py-3 w-12"></th>
					</tr>
				</thead>
				<tbody class="divide-y divide-zinc-200 dark:divide-zinc-700">
					{#each filteredRules as rule}
						<tr class="hover:bg-zinc-50 dark:hover:bg-zinc-800/30 transition-colors">
							<td class="px-4 py-3 font-mono text-xs text-zinc-900 dark:text-zinc-100">{rule.model}</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-300">{rule.dimension}</td>
							<td class="px-4 py-3 text-xs text-zinc-600 dark:text-zinc-300">{rule.unit}</td>
							<td class="px-4 py-3 text-right font-mono text-xs text-zinc-900 dark:text-zinc-100">{rule.rate.toFixed(4)}</td>
							<td class="px-4 py-3 text-xs text-zinc-500 dark:text-zinc-400">{channelName(rule.channel_id)}</td>
							<td class="px-4 py-3 text-center text-xs text-zinc-500 dark:text-zinc-400">{rule.priority}</td>
							<td class="px-4 py-3 text-xs text-zinc-500 dark:text-zinc-400 truncate max-w-[200px]">{rule.description ?? '—'}</td>
							<td class="px-4 py-3">
								<button
									onclick={() => handleDelete(rule.id)}
									disabled={deletingId === rule.id}
									class="p-1 rounded text-zinc-400 hover:text-red-600 dark:hover:text-red-400 transition-colors"
								>
									<Trash2 size={14} />
								</button>
							</td>
						</tr>
					{/each}
					{#if filteredRules.length === 0}
						<tr>
							<td colspan="8" class="px-4 py-8 text-center text-sm text-zinc-500 dark:text-zinc-400">
								<DollarSign size={24} class="mx-auto mb-2 text-zinc-300 dark:text-zinc-600" />
								暂无定价规则
							</td>
						</tr>
					{/if}
				</tbody>
			</table>
		</div>
		<p class="text-xs text-zinc-500 dark:text-zinc-400 mt-2">{filteredRules.length} 条规则</p>
	{/if}
</div>
