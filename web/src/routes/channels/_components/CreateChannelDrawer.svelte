<script lang="ts">
	// 0.4.3 T1：从 channels/+page.svelte 抽出的新建 Channel Drawer。
	// CreateDrawer 调用面比 EditDrawer 更大（manifest builder 7 步 + capability 预览
	// + auth editor + replay）。状态仍由 +page.svelte 持有，子组件只渲染并通过
	// bind:value 把变更回弹；handlers 走 callback。
	import { Button, Field, Input, ProviderSelect } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PluginAuthEditor from '$lib/components/channels/PluginAuthEditor.svelte';
	import { X } from 'lucide-svelte';
	import {
		PLUGIN_PRESET_OPTIONS,
		CAPABILITY_LABELS,
		capabilityList,
		pluginCapabilitiesForPreset,
		pluginPresetBaseUrlSuggestion,
		providerBaseUrlSuggestion,
	} from '$lib/plugin-presets';
	import type {
		PluginAuthForm,
		PluginBuilderDraft,
		PluginResponsePathSuggestion,
		ProviderCapabilities,
		ProviderCapabilityKey,
	} from '$lib/plugin-presets';
	import type { ChannelGroup, CreateChannelRequest } from '$lib/api.js';
	import {
		PROVIDER_OPTIONS,
		isPluginProvider,
		capabilityChipClass,
		capabilityTitle,
		pluginAuthSlotSummary,
	} from '../_lib/helpers';

	interface Props {
		showCreate: boolean;
		createForm: CreateChannelRequest;
		pluginBuilderDraft: PluginBuilderDraft;
		pluginBuilderSuggestions: PluginResponsePathSuggestion[];
		pluginBuilderStep: number;
		pluginManifestInput: string;
		modelsInput: string;
		tagsInput: string;
		createInitialKeyAlias: string;
		createInitialKeySecret: string;
		createAutoProbe: boolean;
		createReplayInput: string;
		createReplayOutput: string;
		createReplayError: string;
		createReplaying: boolean;
		creating: boolean;
		createError: string;
		createProviderCaps: ProviderCapabilities | undefined;
		createMissingCaps: ProviderCapabilityKey[];
		createGroups: ChannelGroup[];
		loadingCreateGroups: boolean;
		pluginManifestExample: string;
		privatePluginManifestExample: string;
		pluginReplaySample: string;
		onClose: () => void;
		onSubmit: (e: SubmitEvent) => void | Promise<void>;
		onPresetChange: (e: Event) => void;
		onBuilderPresetChange: (e: Event) => void;
		onChooseBuilderPath: (
			kind: 'content' | 'finish' | 'prompt' | 'completion' | 'total',
			path: string,
		) => void;
		onRefreshBuilderSuggestions: () => void;
		onUpdateBuilderManifestPreview: () => void;
		onLintManifest: () => void;
		onReplayManifest: () => void | Promise<void>;
	}

	let {
		showCreate = $bindable(),
		createForm = $bindable(),
		pluginBuilderDraft = $bindable(),
		pluginBuilderSuggestions,
		pluginBuilderStep = $bindable(),
		pluginManifestInput = $bindable(),
		modelsInput = $bindable(),
		tagsInput = $bindable(),
		createInitialKeyAlias = $bindable(),
		createInitialKeySecret = $bindable(),
		createAutoProbe = $bindable(),
		createReplayInput = $bindable(),
		createReplayOutput,
		createReplayError,
		createReplaying,
		creating,
		createError,
		createProviderCaps,
		createMissingCaps,
		createGroups,
		loadingCreateGroups,
		pluginManifestExample,
		privatePluginManifestExample,
		pluginReplaySample,
		onClose,
		onSubmit,
		onPresetChange,
		onBuilderPresetChange,
		onChooseBuilderPath,
		onRefreshBuilderSuggestions,
		onUpdateBuilderManifestPreview,
		onLintManifest,
		onReplayManifest,
	}: Props = $props();

	function applyBaseUrlSuggestion() {
		const suggestion = isPluginProvider(createForm.provider_type)
			? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset)
			: providerBaseUrlSuggestion(createForm.provider_type);
		if (suggestion) createForm.base_url = suggestion;
	}

	const REQUEST_BODY_PLACEHOLDER = '{"model":"{{model}}","messages":"{{messages}}"}';
	const PROBE_PATH_PLACEHOLDER = '/health/{{model}}';
</script>

{#if showCreate}
	<!-- 0.4.187 修：items-stretch 让 drawer 撑满屏高，避免 ModalFrame 默认 items-center 把
	     drawer 居中收缩，导致内容超出后顶/底被裁切且无法滚动（魔尊截图反馈）。 -->
	<ModalFrame close={onClose} class="z-40 items-stretch justify-end bg-black/50 backdrop-blur-sm p-0 animate-backdrop">
		<div class="w-full max-w-lg bg-white dark:bg-zinc-900 h-full overflow-y-auto shadow-2xl animate-slide-in-right">
			<div class="p-6">
				<div class="flex items-center justify-between mb-6">
					<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">新建 Channel</h2>
					<button onclick={onClose} class="p-1.5 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
						<X size={18} />
					</button>
				</div>
				<form onsubmit={onSubmit} class="space-y-6">
					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">基础信息</p>
						<div class="space-y-3">
							<div>
								<label for="ch-code" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Code <span class="text-red-500">*</span></label>
								<Input id="ch-code" placeholder="openai-prod" bind:value={createForm.code} disabled={creating} />
							</div>
							<div>
								<label for="ch-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
								<Input id="ch-name" placeholder="OpenAI Production" bind:value={createForm.name} disabled={creating} />
							</div>
							<Field label="Provider" for="ch-provider" required>
								<ProviderSelect bind:value={createForm.provider_type} options={PROVIDER_OPTIONS} mode="grid" disabled={creating} />
							</Field>
							<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
								<div class="mb-2 flex items-center justify-between gap-2">
									<p class="text-xs font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400">Capability</p>
									{#if isPluginProvider(createForm.provider_type) && pluginBuilderDraft.preset}
										<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{pluginBuilderDraft.preset}</span>
									{:else}
										<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{createForm.provider_type}</span>
									{/if}
								</div>
								{#if createProviderCaps}
									<div class="flex flex-wrap gap-1.5" title={capabilityTitle(createProviderCaps)}>
										{#each capabilityList(createProviderCaps) as cap}
											<span class="rounded-md px-2 py-0.5 text-xs font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
										{/each}
									</div>
									{#if createMissingCaps.length > 0}
										<p class="mt-2 text-xs text-amber-700 dark:text-amber-400">
											未声明 {createMissingCaps.map((cap) => CAPABILITY_LABELS[cap]).join(' / ')}；这些请求不会路由到该 Channel。
										</p>
									{/if}
								{/if}
							</div>
							<div>
								<label for="ch-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL</label>
								<Input id="ch-url" placeholder="https://api.openai.com/v1" bind:value={createForm.base_url} disabled={creating} />
								{#if isPluginProvider(createForm.provider_type) ? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset) : providerBaseUrlSuggestion(createForm.provider_type)}
									<button type="button" class="mt-1 text-xs text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100" onclick={applyBaseUrlSuggestion}>
										使用建议：{isPluginProvider(createForm.provider_type) ? pluginPresetBaseUrlSuggestion(pluginBuilderDraft.preset) : providerBaseUrlSuggestion(createForm.provider_type)}
									</button>
								{/if}
							</div>
						</div>
					</div>

					{#if isPluginProvider(createForm.provider_type)}
						<div>
							<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">Plugin Manifest Builder</p>
							<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950 space-y-3">
								<div class="flex flex-wrap gap-2">
									{#each ['Preset', 'Auth', 'Request mapping', 'Response sample', 'SSE replay', 'Test connection', 'Save & bind'] as label, index}
										<button type="button" class="rounded-md px-2 py-0.5 text-xs font-medium ring-1 {pluginBuilderStep === index + 1 ? 'bg-zinc-900 text-white ring-zinc-900 dark:bg-zinc-100 dark:text-zinc-900 dark:ring-zinc-100' : 'bg-white text-zinc-600 ring-zinc-200 dark:bg-zinc-900 dark:text-zinc-300 dark:ring-zinc-700'}" onclick={() => (pluginBuilderStep = index + 1)}>
											{index + 1}. {label}
										</button>
									{/each}
								</div>

								{#if pluginBuilderStep === 1}
									<label for="ch-plugin-preset" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">选择 preset 或自定义</label>
									<select id="ch-plugin-preset" bind:value={pluginBuilderDraft.preset} onchange={onBuilderPresetChange} disabled={creating} class="w-full rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100">
										{#each PLUGIN_PRESET_OPTIONS as opt}
											<option value={opt.value}>{opt.label}</option>
										{/each}
									</select>
									<div class="flex justify-end gap-2 pt-2">
										<Button size="sm" variant="outline" type="button" onclick={() => { onUpdateBuilderManifestPreview(); pluginBuilderStep = 2; }}>下一步：配置 Auth</Button>
									</div>
								{/if}

								{#if pluginBuilderStep === 2}
									<PluginAuthEditor bind:form={pluginBuilderDraft.auth} disabled={creating} idPrefix="cb-auth" />
									<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">当前 auth 需要 slot：{pluginAuthSlotSummary(pluginBuilderDraft.auth)}。留空则继续使用已存在 channel key 或环境变量，创建流程不会保存明文。</p>
									<div class="flex justify-between gap-2 pt-2">
										<Button size="sm" variant="ghost" type="button" onclick={() => (pluginBuilderStep = 1)}>上一步</Button>
										<Button size="sm" variant="outline" type="button" onclick={() => { onUpdateBuilderManifestPreview(); pluginBuilderStep = 3; }}>下一步：Request mapping</Button>
									</div>
								{/if}

								{#if pluginBuilderStep === 3}
									<div class="space-y-2">
										<label for="cb-path" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Request path</label>
										<Input id="cb-path" placeholder="/chat/completions" bind:value={pluginBuilderDraft.request_path} disabled={creating} />
										<label for="cb-body" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Request body template (JSON)</label>
										<textarea id="cb-body" class="min-h-32 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={REQUEST_BODY_PLACEHOLDER} bind:value={pluginBuilderDraft.request_body} disabled={creating}></textarea>
									</div>
									<div class="flex justify-between gap-2 pt-2">
										<Button size="sm" variant="ghost" type="button" onclick={() => (pluginBuilderStep = 2)}>上一步</Button>
										<Button size="sm" variant="outline" type="button" onclick={() => { onUpdateBuilderManifestPreview(); pluginBuilderStep = 4; }}>下一步：Response sample</Button>
									</div>
								{/if}

								{#if pluginBuilderStep === 4}
									<label for="cb-sample" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">粘贴一份非流式 JSON 响应样本</label>
									<textarea id="cb-sample" class="min-h-40 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" bind:value={pluginBuilderDraft.response_sample} oninput={onRefreshBuilderSuggestions} disabled={creating}></textarea>
									{#if pluginBuilderSuggestions.length > 0}
										<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">点选字段映射到 content/finish/usage：</p>
										<div class="grid grid-cols-1 gap-1.5">
											{#each pluginBuilderSuggestions as s}
												<div class="flex flex-wrap items-center gap-1.5 rounded-md border border-zinc-200 bg-white p-2 text-xs dark:border-zinc-700 dark:bg-zinc-900">
													<code class="font-mono text-zinc-700 dark:text-zinc-300">{s.path}</code>
													<span class="text-zinc-400">→</span>
													<Button size="sm" variant="outline" type="button" onclick={() => onChooseBuilderPath('content', s.path)}>content</Button>
													<Button size="sm" variant="outline" type="button" onclick={() => onChooseBuilderPath('finish', s.path)}>finish</Button>
													<Button size="sm" variant="outline" type="button" onclick={() => onChooseBuilderPath('prompt', s.path)}>prompt</Button>
													<Button size="sm" variant="outline" type="button" onclick={() => onChooseBuilderPath('completion', s.path)}>completion</Button>
													<Button size="sm" variant="outline" type="button" onclick={() => onChooseBuilderPath('total', s.path)}>total</Button>
												</div>
											{/each}
										</div>
									{/if}
									<div class="flex justify-between gap-2 pt-2">
										<Button size="sm" variant="ghost" type="button" onclick={() => (pluginBuilderStep = 3)}>上一步</Button>
										<Button size="sm" variant="outline" type="button" onclick={() => { onUpdateBuilderManifestPreview(); pluginBuilderStep = 5; }}>下一步：SSE replay</Button>
									</div>
								{/if}

								{#if pluginBuilderStep === 5}
									<div class="space-y-2">
										<div class="flex items-center justify-between gap-2">
											<p class="text-sm font-medium text-zinc-800 dark:text-zinc-200">SSE replay preview</p>
											<Button size="sm" variant="outline" type="button" onclick={onReplayManifest} disabled={creating || createReplaying}>{createReplaying ? '回放中...' : 'Replay'}</Button>
										</div>
										<textarea class="min-h-36 w-full rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-950 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={pluginReplaySample} bind:value={createReplayInput} disabled={creating || createReplaying}></textarea>
										{#if createReplayError}
											<p class="rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/20 dark:text-red-400">{createReplayError}</p>
										{/if}
										{#if createReplayOutput}
											<pre class="max-h-56 overflow-auto rounded-md bg-zinc-950 p-3 text-xs text-zinc-100">{createReplayOutput}</pre>
										{/if}
									</div>
									<div class="flex justify-between gap-2 pt-2">
										<Button size="sm" variant="ghost" type="button" onclick={() => (pluginBuilderStep = 4)}>上一步</Button>
										<Button size="sm" variant="outline" type="button" onclick={() => (pluginBuilderStep = 6)}>下一步：Probe 配置</Button>
									</div>
								{/if}

								{#if pluginBuilderStep === 6}
									<div class="space-y-2">
										<label for="cb-probe-path" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Probe path</label>
										<Input id="cb-probe-path" placeholder={PROBE_PATH_PLACEHOLDER} bind:value={pluginBuilderDraft.probe_path} disabled={creating} />
										<label for="cb-probe-model" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Probe model</label>
										<Input id="cb-probe-model" placeholder="tiny-health" bind:value={pluginBuilderDraft.probe_model} disabled={creating} />
										<label for="cb-probe-status" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">success_status (逗号分隔)</label>
										<Input id="cb-probe-status" placeholder="200, 204" bind:value={pluginBuilderDraft.probe_success_status} disabled={creating} />
										<label for="cb-probe-cost" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">max_cost_micros</label>
										<Input id="cb-probe-cost" type="number" bind:value={pluginBuilderDraft.probe_max_cost_micros} disabled={creating} />
										<label for="cb-probe-body" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Probe body template (可选)</label>
										<textarea id="cb-probe-body" class="min-h-24 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" bind:value={pluginBuilderDraft.probe_body} disabled={creating}></textarea>
									</div>
									<div class="flex justify-between gap-2 pt-2">
										<Button size="sm" variant="ghost" type="button" onclick={() => (pluginBuilderStep = 5)}>上一步</Button>
										<Button size="sm" variant="outline" type="button" onclick={() => { onUpdateBuilderManifestPreview(); onLintManifest(); pluginBuilderStep = 7; }}>下一步：保存</Button>
									</div>
								{/if}

								{#if pluginBuilderStep === 7}
									<div class="space-y-2">
										<label for="cb-key-alias" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">初始 Channel Key alias</label>
										<Input id="cb-key-alias" placeholder="primary" bind:value={createInitialKeyAlias} disabled={creating} />
										<label for="cb-key-secret" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">初始 Channel Key secret (可选，留空则使用环境变量)</label>
										<Input id="cb-key-secret" type="password" placeholder="留空则跳过 channel key 创建" bind:value={createInitialKeySecret} disabled={creating} />
										<label for="cb-group" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">绑定到 Group (可选)</label>
										<select id="cb-group" bind:value={pluginBuilderDraft.target_group_id} disabled={creating || loadingCreateGroups} class="w-full rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100">
											<option value="">{loadingCreateGroups ? '加载中...' : '不绑定'}</option>
											{#each createGroups as g}
												<option value={g.id}>{g.name} · {g.strategy}</option>
											{/each}
										</select>
										<label class="flex items-center gap-2 text-xs text-zinc-600 dark:text-zinc-400">
											<input type="checkbox" bind:checked={createAutoProbe} disabled={creating} class="w-3.5 h-3.5 rounded border-zinc-300 dark:border-zinc-600" />
											保存后自动 probe + sync 模型
										</label>
									</div>
									<div class="flex justify-between gap-2 pt-2">
										<Button size="sm" variant="ghost" type="button" onclick={() => (pluginBuilderStep = 6)}>上一步</Button>
									</div>
								{/if}

								<div class="border-t border-zinc-200 dark:border-zinc-800 pt-3">
									<div class="mb-1 flex items-center justify-between gap-2">
										<label for="ch-plugin" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Manifest Preview</label>
										<Button size="sm" variant="outline" type="button" onclick={onLintManifest} disabled={creating}>本地 lint</Button>
									</div>
									<textarea id="ch-plugin" class="min-h-48 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={pluginBuilderDraft.preset ? pluginManifestExample : privatePluginManifestExample} bind:value={pluginManifestInput} disabled={creating}></textarea>
									<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">保存前会把 Auth Strategy 合并进 manifest 并本地 lint；manifest 只引用 secret slot，不写明文 secret。</p>
								</div>
							</div>
						</div>
					{/if}

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">限速 & 超时</p>
						<div class="grid grid-cols-2 gap-3">
							<div>
								<label for="ch-rpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">RPM</label>
								<Input id="ch-rpm" type="number" placeholder="无限制" bind:value={createForm.rpm_limit} disabled={creating} />
							</div>
							<div>
								<label for="ch-tpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">TPM</label>
								<Input id="ch-tpm" type="number" placeholder="无限制" bind:value={createForm.tpm_limit} disabled={creating} />
							</div>
							<div>
								<label for="ch-timeout" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">超时(ms)</label>
								<Input id="ch-timeout" type="number" bind:value={createForm.timeout_ms} disabled={creating} />
							</div>
							<div>
								<label for="ch-retries" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">重试次数</label>
								<Input id="ch-retries" type="number" bind:value={createForm.max_retries} disabled={creating} />
							</div>
						</div>
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">模型</p>
						<Input placeholder="gpt-4o, gpt-4o-mini" bind:value={modelsInput} disabled={creating} />
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">标签</p>
						<Input placeholder="production, us-east" bind:value={tagsInput} disabled={creating} />
					</div>

					{#if createError}
						<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-lg px-3 py-2">{createError}</p>
					{/if}
					<div class="flex gap-2 justify-end pt-4 border-t border-zinc-200 dark:border-zinc-800">
						<Button variant="outline" type="button" onclick={onClose}>取消</Button>
						<Button type="submit" disabled={creating}>{creating ? '创建中...' : '创建'}</Button>
					</div>
				</form>
			</div>
		</div>
	</ModalFrame>
{/if}
