<script lang="ts">
	// 0.4.14：从 admin/pricing/+page.svelte 抽出的 wizard form modal。
	// 304 行 4 步 wizard 整体抽出。state 在父，子组件渲染 + callback。
	import { Alert, Button, Card, Field, Input, Select, Textarea } from '$lib/components/ui';
	import DataTable from '$lib/components/templates/DataTable.svelte';
	import { Calculator, CheckCircle2, DollarSign } from 'lucide-svelte';
	import { cn, dataTemplate } from '$lib/design';
	import type { PricingRule, Channel } from '$lib/api.js';
	import type { PricingPreviewResult, PricingUsagePreviewInput } from '$lib/pricing-preview.js';

	type SelectOption = { value: string; label: string };
	type ParseResult =
		| { ok: true; value: Record<string, any> }
		| { ok: false; error: string; value: Record<string, any> };
	type WizardStep = { step: number; label: string; hint: string };
	type ConditionTemplate = { label: string; value: string };

	interface Props {
		showForm: boolean;
		pricingWizardStep: number;
		formModel: string;
		formDimension: string;
		formUnit: string;
		formRate: string;
		formChannelId: string;
		formPriority: string;
		formDescription: string;
		formConditions: string;
		formConfirmation: string;
		formSaving: boolean;
		usagePreview: PricingUsagePreviewInput;
		wizardSteps: WizardStep[];
		conditionTemplates: ConditionTemplate[];
		dimensionOptions: SelectOption[];
		unitOptions: SelectOption[];
		channels: Channel[];
		parsedConditions: ParseResult;
		draftRate: number;
		draftRule: PricingRule | null;
		previewRules: PricingRule[];
		pricingPreview: PricingPreviewResult;
		draftQuantity: number;
		canGoPreview: boolean;
		selectedChannelLabel: string;
		channelName: (id: string | null) => string;
		formatMicrosUsd: (m: number) => string;
		formatPreviewQuantity: (q: number, unit: string) => string;
		pricingConfirmation: string;
		formChannelOptions: SelectOption[];
		modelOptions: string[];
		onClose: () => void;
		onGoStep: (step: number) => void;
		onSave: () => void | Promise<void>;
		onSyncModel: () => void;
		onSetUsageText: (key: keyof PricingUsagePreviewInput, value: string) => void;
		onSetUsageNumber: (key: keyof PricingUsagePreviewInput, value: string) => void;
	}

	let {
		showForm,
		pricingWizardStep = $bindable(),
		formModel = $bindable(),
		formDimension = $bindable(),
		formUnit = $bindable(),
		formRate = $bindable(),
		formChannelId = $bindable(),
		formPriority = $bindable(),
		formDescription = $bindable(),
		formConditions = $bindable(),
		formConfirmation = $bindable(),
		formSaving,
		usagePreview = $bindable(),
		wizardSteps,
		conditionTemplates,
		dimensionOptions,
		unitOptions,
		channels,
		parsedConditions,
		draftRate,
		draftRule,
		previewRules,
		pricingPreview,
		draftQuantity,
		canGoPreview,
		selectedChannelLabel,
		channelName,
		formatMicrosUsd,
		formatPreviewQuantity,
		pricingConfirmation,
		formChannelOptions,
		modelOptions,
		onClose,
		onGoStep,
		onSave,
		onSyncModel,
		onSetUsageText,
		onSetUsageNumber,
	}: Props = $props();
</script>

	{#if showForm}
		<Card class="mb-6 overflow-hidden">
			<div class="border-b border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/50">
				<div class="flex flex-col gap-3 lg:flex-row lg:items-start lg:justify-between">
					<div>
						<h3 class="text-sm font-semibold text-zinc-900 dark:text-zinc-100">Pricing wizard 向导</h3>
						<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">从模型选择、计费维度、价格预览到 usage cost 模拟，一次写入可复核 rule。</p>
					</div>
					<div class="flex items-center gap-2 rounded-lg border border-zinc-200 bg-white px-3 py-2 text-xs text-zinc-600 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300">
						<Calculator size={14} />
						<span class="font-mono">{formatMicrosUsd(pricingPreview.costMicros)}</span>
						<span>预估 cost</span>
					</div>
				</div>
				<div class="mt-4 grid gap-2 md:grid-cols-4">
					{#each wizardSteps as item}
						<button
							type="button"
							class={cn(
								'rounded-lg border px-3 py-2 text-left transition-colors',
								pricingWizardStep === item.step
									? 'border-zinc-900 bg-zinc-900 text-white dark:border-zinc-100 dark:bg-zinc-100 dark:text-zinc-900'
									: 'border-zinc-200 bg-white text-zinc-700 hover:bg-zinc-50 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-300 dark:hover:bg-zinc-800',
								item.step > 2 && !canGoPreview && 'cursor-not-allowed opacity-50'
							)}
							onclick={() => onGoStep(item.step)}
							disabled={item.step > 2 && !canGoPreview}
						>
							<div class="flex items-center gap-2 text-xs font-semibold">
								<span class="flex h-5 w-5 items-center justify-center rounded-full border border-current text-[10px]">{item.step}</span>
								{item.label}
							</div>
							<p class={cn('mt-1 text-[11px]', pricingWizardStep === item.step ? 'text-zinc-200 dark:text-zinc-700' : 'text-zinc-500 dark:text-zinc-400')}>{item.hint}</p>
						</button>
					{/each}
				</div>
			</div>

			<div class="p-4">
				{#if pricingWizardStep === 1}
					<div class="grid gap-4 lg:grid-cols-[1.1fr_0.9fr]">
						<div class="space-y-4">
							<div class="grid gap-3 md:grid-cols-2">
								<Field label="模型" for="pricing-form-model" required hint="可输入新模型；下方会展示已知模型 quick pick。">
									<Input id="pricing-form-model" placeholder="gpt-4o-mini" bind:value={formModel} />
								</Field>
								<Field label="Channel (空=全局)" for="pricing-form-channel" hint="Channel-specific rule 会优先覆盖 Global rule。">
									<Select id="pricing-form-channel" bind:value={formChannelId} options={formChannelOptions} onchange={onSyncModel} />
								</Field>
							</div>

							{#if modelOptions.length}
								<div>
									<p class="mb-2 text-xs font-medium text-zinc-600 dark:text-zinc-400">已知模型</p>
									<div class="flex flex-wrap gap-2">
										{#each modelOptions.slice(0, 18) as model}
											<button
												type="button"
												class="rounded-full border border-zinc-200 px-3 py-1 text-xs font-mono text-zinc-700 transition-colors hover:border-zinc-400 hover:bg-zinc-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
												onclick={() => { formModel = model; }}
											>
												{model}
											</button>
										{/each}
									</div>
								</div>
							{/if}
						</div>

						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">生效范围</p>
							<div class="mt-3 space-y-3 text-sm">
								<div class="flex items-center justify-between gap-4">
									<span class="text-zinc-500 dark:text-zinc-400">模型</span>
									<span class="font-mono text-zinc-900 dark:text-zinc-100">{formModel || '未选择'}</span>
								</div>
								<div class="flex items-center justify-between gap-4">
									<span class="text-zinc-500 dark:text-zinc-400">Channel</span>
									<span class="text-zinc-900 dark:text-zinc-100">{selectedChannelLabel}</span>
								</div>
								<div class="rounded-lg border border-zinc-200 bg-white p-3 text-xs text-zinc-500 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-400">
									规则匹配顺序遵循后端语义：同 dimension 下先 Channel-specific，再 Global，再按 priority / effective_from 取最新。
								</div>
							</div>
						</div>
					</div>
				{:else if pricingWizardStep === 2}
					<div class="grid gap-4 lg:grid-cols-[1fr_260px]">
						<div class="space-y-4">
							<div class="grid gap-3 md:grid-cols-4">
								<Field label="计费维度" for="pricing-form-dimension" required>
									<Select id="pricing-form-dimension" bind:value={formDimension} options={dimensionOptions} />
								</Field>
								<Field label="单位" for="pricing-form-unit" required>
									<Select id="pricing-form-unit" bind:value={formUnit} options={unitOptions} />
								</Field>
								<Field label="费率" for="pricing-form-rate" required hint="例如 $0.15 / 1M tokens。">
									<Input id="pricing-form-rate" type="number" step="0.0001" placeholder="0.1500" bind:value={formRate} invalid={formRate.length > 0 && !Number.isFinite(draftRate)} />
								</Field>
								<Field label="优先级" for="pricing-form-priority">
									<Input id="pricing-form-priority" type="number" bind:value={formPriority} />
								</Field>
							</div>
							<Field label="描述" for="pricing-form-description">
								<Input id="pricing-form-description" placeholder="例：OpenAI gpt-4o-mini input tokens" bind:value={formDescription} />
							</Field>
							<Field
								label="Conditions JSON"
								for="pricing-form-conditions"
								hint="支持 cache_ttl / quality / size / deployment_type / batch / region / context_above。未知字段按后端语义忽略。"
								error={parsedConditions.ok ? '' : parsedConditions.error}
							>
								<Textarea id="pricing-form-conditions" bind:value={formConditions} rows={8} class="font-mono text-xs" />
							</Field>
						</div>

						<div>
							<p class="mb-2 text-xs font-medium text-zinc-600 dark:text-zinc-400">常见条件模板</p>
							<div class="grid gap-2">
								{#each conditionTemplates as tpl}
									<button
										type="button"
										class="rounded-md border border-zinc-200 px-3 py-2 text-left text-xs text-zinc-700 hover:bg-zinc-50 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
										onclick={() => { formConditions = tpl.value; }}
									>
										{tpl.label}
									</button>
								{/each}
							</div>
						</div>
					</div>
				{:else if pricingWizardStep === 3}
					<div class="grid gap-4 lg:grid-cols-[0.9fr_1.1fr]">
						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">Rule 预览</p>
							<dl class="mt-3 grid grid-cols-2 gap-3 text-sm">
								<div>
									<dt class="text-xs text-zinc-500 dark:text-zinc-400">Model</dt>
									<dd class="mt-1 font-mono text-zinc-900 dark:text-zinc-100">{formModel || '—'}</dd>
								</div>
								<div>
									<dt class="text-xs text-zinc-500 dark:text-zinc-400">Channel</dt>
									<dd class="mt-1 text-zinc-900 dark:text-zinc-100">{selectedChannelLabel}</dd>
								</div>
								<div>
									<dt class="text-xs text-zinc-500 dark:text-zinc-400">计费维度</dt>
									<dd class="mt-1 font-mono text-zinc-900 dark:text-zinc-100">{formDimension}</dd>
								</div>
								<div>
									<dt class="text-xs text-zinc-500 dark:text-zinc-400">单位 / 费率</dt>
									<dd class="mt-1 font-mono text-zinc-900 dark:text-zinc-100">{formUnit} × {formRate || '—'}</dd>
								</div>
							</dl>
							<div class="mt-4 rounded-lg border border-zinc-200 bg-white p-3 dark:border-zinc-700 dark:bg-zinc-900">
								<p class="mb-2 text-xs font-medium text-zinc-500 dark:text-zinc-400">Conditions 条件</p>
								<pre class="overflow-auto text-xs text-zinc-700 dark:text-zinc-300">{JSON.stringify(parsedConditions.value, null, 2)}</pre>
							</div>
						</div>

						<div class="space-y-3">
							<div class="grid gap-3 sm:grid-cols-3">
								<div class="rounded-xl border border-zinc-200 p-3 dark:border-zinc-700">
									<p class="text-xs text-zinc-500 dark:text-zinc-400">当前草稿样本数量</p>
									<p class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatPreviewQuantity(draftQuantity, formUnit)}</p>
								</div>
								<div class="rounded-xl border border-zinc-200 p-3 dark:border-zinc-700">
									<p class="text-xs text-zinc-500 dark:text-zinc-400">匹配规则数</p>
									<p class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{previewRules.length}</p>
								</div>
								<div class="rounded-xl border border-zinc-200 p-3 dark:border-zinc-700">
									<p class="text-xs text-zinc-500 dark:text-zinc-400">预估 cost</p>
									<p class="mt-1 font-mono text-lg font-semibold text-zinc-900 dark:text-zinc-100">{formatMicrosUsd(pricingPreview.costMicros)}</p>
								</div>
							</div>
							<DataTable isEmpty={pricingPreview.lineItems.length === 0} emptyColspan={5}>
								{#snippet head()}
									<tr>
										<th class={dataTemplate.th}>来源</th>
										<th class={dataTemplate.th}>维度</th>
										<th class={cn(dataTemplate.th, 'text-right')}>数量</th>
										<th class={cn(dataTemplate.th, 'text-right')}>费率</th>
										<th class={cn(dataTemplate.th, 'text-right')}>成本</th>
									</tr>
								{/snippet}
								{#snippet empty()}
									<CheckCircle2 size={24} class="mx-auto mb-2 text-zinc-300 dark:text-zinc-600" />
									当前 usage 样本未命中可计费数量
								{/snippet}
								{#each pricingPreview.lineItems as item}
									<tr class={dataTemplate.row}>
										<td class={dataTemplate.td}>{item.rule.id === '__draft__' ? '草稿' : channelName(item.rule.channel_id)}</td>
										<td class={dataTemplate.tdMonoStrong}>{item.rule.dimension}</td>
										<td class={cn(dataTemplate.tdMono, 'text-right')}>{formatPreviewQuantity(item.quantity, item.rule.unit)}</td>
										<td class={cn(dataTemplate.tdMono, 'text-right')}>{item.rule.rate}</td>
										<td class={cn(dataTemplate.tdMonoStrong, 'text-right')}>{formatMicrosUsd(item.kind === 'multiplier' ? (item.afterMicros ?? 0) : item.costMicros)}</td>
									</tr>
								{/each}
							</DataTable>
						</div>
					</div>
				{:else}
					<div class="grid gap-4 lg:grid-cols-[1.1fr_0.9fr]">
						<div class="space-y-4">
							<div class="grid gap-3 md:grid-cols-3">
								<Field label="Prompt tokens 输入" for="usage-prompt">
									<Input id="usage-prompt" type="number" min="0" value={usagePreview.prompt_tokens} oninput={(event) => onSetUsageNumber('prompt_tokens', event.currentTarget.value)} />
								</Field>
								<Field label="Completion tokens 输出" for="usage-completion">
									<Input id="usage-completion" type="number" min="0" value={usagePreview.completion_tokens} oninput={(event) => onSetUsageNumber('completion_tokens', event.currentTarget.value)} />
								</Field>
								<Field label="Cached tokens 缓存" for="usage-cached">
									<Input id="usage-cached" type="number" min="0" value={usagePreview.cached_tokens} oninput={(event) => onSetUsageNumber('cached_tokens', event.currentTarget.value)} />
								</Field>
								<Field label="Reasoning tokens 推理" for="usage-reasoning">
									<Input id="usage-reasoning" type="number" min="0" value={usagePreview.reasoning_tokens} oninput={(event) => onSetUsageNumber('reasoning_tokens', event.currentTarget.value)} />
								</Field>
								<Field label="生成图片数" for="usage-images">
									<Input id="usage-images" type="number" min="0" value={usagePreview.images_generated} oninput={(event) => onSetUsageNumber('images_generated', event.currentTarget.value)} />
								</Field>
								<Field label="搜索次数" for="usage-search">
									<Input id="usage-search" type="number" min="0" value={usagePreview.search_count} oninput={(event) => onSetUsageNumber('search_count', event.currentTarget.value)} />
								</Field>
								<Field label="音频分钟数" for="usage-audio-minutes">
									<Input id="usage-audio-minutes" type="number" min="0" step="0.01" value={usagePreview.audio_minutes} oninput={(event) => onSetUsageNumber('audio_minutes', event.currentTarget.value)} />
								</Field>
								<Field label="TTS 字符数" for="usage-tts">
									<Input id="usage-tts" type="number" min="0" value={usagePreview.tts_characters} oninput={(event) => onSetUsageNumber('tts_characters', event.currentTarget.value)} />
								</Field>
								<Field label="视频秒数" for="usage-video">
									<Input id="usage-video" type="number" min="0" step="0.01" value={usagePreview.video_seconds} oninput={(event) => onSetUsageNumber('video_seconds', event.currentTarget.value)} />
								</Field>
								<Field label="图片质量" for="usage-quality">
									<Input id="usage-quality" placeholder="hd" value={usagePreview.image_quality ?? ''} oninput={(event) => onSetUsageText('image_quality', event.currentTarget.value)} />
								</Field>
								<Field label="图片尺寸" for="usage-size">
									<Input id="usage-size" placeholder="1024x1024" value={usagePreview.image_size ?? ''} oninput={(event) => onSetUsageText('image_size', event.currentTarget.value)} />
								</Field>
								<Field label="Region 区域" for="usage-region">
									<Input id="usage-region" placeholder="us-east-1" value={usagePreview.region ?? ''} oninput={(event) => onSetUsageText('region', event.currentTarget.value)} />
								</Field>
								<Field label="Cache TTL" for="usage-cache-ttl">
									<Input id="usage-cache-ttl" placeholder="ephemeral" value={usagePreview.cache_ttl ?? ''} oninput={(event) => onSetUsageText('cache_ttl', event.currentTarget.value)} />
								</Field>
								<Field label="Deployment type 部署类型" for="usage-deployment">
									<Input id="usage-deployment" placeholder="realtime" value={usagePreview.deployment_type ?? ''} oninput={(event) => onSetUsageText('deployment_type', event.currentTarget.value)} />
								</Field>
								<Field label="上下文长度" for="usage-context">
									<Input id="usage-context" type="number" min="0" value={usagePreview.context_length} oninput={(event) => onSetUsageNumber('context_length', event.currentTarget.value)} />
								</Field>
							</div>
							<label class="inline-flex items-center gap-2 rounded-lg border border-zinc-200 px-3 py-2 text-sm text-zinc-700 dark:border-zinc-700 dark:text-zinc-300">
								<input type="checkbox" bind:checked={usagePreview.is_batch} class="h-4 w-4 accent-zinc-900 dark:accent-zinc-100" />
								Batch request
							</label>
						</div>

						<div class="rounded-xl border border-zinc-200 bg-zinc-50 p-4 dark:border-zinc-700 dark:bg-zinc-800/40">
							<p class="text-xs font-semibold uppercase tracking-wider text-zinc-500 dark:text-zinc-400">Usage cost 模拟</p>
							<p class="mt-2 font-mono text-3xl font-semibold text-zinc-900 dark:text-zinc-100">{formatMicrosUsd(pricingPreview.costMicros)}</p>
							<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">{pricingPreview.costMicros.toLocaleString()} micros，含草稿 rule 与当前匹配规则。</p>
							<div class="mt-4 space-y-2">
								{#each pricingPreview.lineItems.slice(0, 6) as item}
									<div class="flex items-center justify-between gap-3 rounded-lg border border-zinc-200 bg-white px-3 py-2 text-xs dark:border-zinc-700 dark:bg-zinc-900">
										<span class="font-mono text-zinc-700 dark:text-zinc-300">{item.rule.dimension}</span>
										<span class="font-mono text-zinc-900 dark:text-zinc-100">{formatMicrosUsd(item.kind === 'multiplier' ? (item.afterMicros ?? 0) : item.costMicros)}</span>
									</div>
								{/each}
							</div>
						</div>
					</div>
				{/if}

				<div class="mt-5 flex flex-wrap items-center justify-between gap-3 border-t border-zinc-200 pt-4 dark:border-zinc-700">
					<div class="text-xs text-zinc-500 dark:text-zinc-400">
						{#if !parsedConditions.ok}
							<span class="text-red-600 dark:text-red-400">Conditions JSON 无效，暂不可保存。</span>
						{:else if !formModel.trim()}
							请选择或输入 Model。
						{:else if !Number.isFinite(draftRate)}
							请输入有效费率。
						{:else}
							已可保存；建议先看 usage cost 预览再落库。
						{/if}
					</div>
					<div class="flex gap-2">
						<Button variant="outline" onclick={() => (pricingWizardStep = Math.max(1, pricingWizardStep - 1))} disabled={pricingWizardStep === 1}>上一步</Button>
						{#if pricingWizardStep < 4}
							<Button onclick={() => onGoStep(pricingWizardStep + 1)} disabled={pricingWizardStep >= 2 && !canGoPreview}>下一步</Button>
						{/if}
						<Button onclick={onSave} disabled={formSaving || !canGoPreview || formConfirmation.trim() !== pricingConfirmation}>
							{formSaving ? '保存中...' : '保存 Pricing rule'}
						</Button>
						<Button variant="outline" onclick={onClose}>取消</Button>
					</div>
				</div>
				<div class="mt-4 rounded-lg border border-amber-200 bg-amber-50 p-3 dark:border-amber-900/60 dark:bg-amber-950/30">
					<p class="text-xs font-medium text-amber-800 dark:text-amber-300">高危操作二次确认</p>
					<p class="mt-1 text-xs text-amber-700 dark:text-amber-300">保存 Pricing rule 会影响实时计费。请输入确认短语：</p>
					<code class="mt-2 block rounded-md border border-amber-200 bg-white px-3 py-2 font-mono text-xs text-zinc-800 dark:border-amber-900/60 dark:bg-zinc-900 dark:text-zinc-200">{pricingConfirmation}</code>
					<Input id="pricing-save-confirm" bind:value={formConfirmation} placeholder={pricingConfirmation} class="mt-2 font-mono" />
				</div>
			</div>
		</Card>
	{/if}
