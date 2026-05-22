<script lang="ts">
	// 0.4.2 T4：从 channels/+page.svelte 抽出的编辑 Channel Drawer。
	// props 表偏多（25+）但所有 state 仍由 +page.svelte 持有，
	// 子组件只渲染 + bind:value 把变更回弹。Manifest builder / SSE replay
	// 走 callback。
	import { Button, Input } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import PluginAuthEditor from '$lib/components/channels/PluginAuthEditor.svelte';
	import { X, Radar } from 'lucide-svelte';
	import {
		PLUGIN_PRESET_OPTIONS,
		CAPABILITY_LABELS,
		capabilityList,
		pluginPresetBaseUrlSuggestion,
		providerBaseUrlSuggestion,
	} from '$lib/plugin-presets';
	import type {
		PluginAuthForm,
		ProviderCapabilities,
		ProviderCapabilityKey,
	} from '$lib/plugin-presets';
	import type { Channel, UpdateChannelRequest } from '$lib/api.js';
	import {
		isPluginProvider,
		capabilityChipClass,
		capabilityTitle,
	} from '../_lib/helpers';

	interface Props {
		editingChannel: Channel | null;
		editForm: UpdateChannelRequest;
		editAuthForm: PluginAuthForm;
		editPluginPreset: string;
		editPluginManifestInput: string;
		editReplayInput: string;
		editReplayOutput: string;
		editReplayError: string;
		editReplaying: boolean;
		editModelsInput: string;
		editTagsInput: string;
		editing: boolean;
		editError: string;
		editProviderCaps: ProviderCapabilities | null;
		editMissingCaps: ProviderCapabilityKey[];
		probingId: string | null;
		pluginManifestExample: string;
		privatePluginManifestExample: string;
		pluginReplaySample: string;
		onClose: () => void;
		onSubmit: (e: SubmitEvent) => void | Promise<void>;
		onProbe: (ch: Channel) => void | Promise<void>;
		onPresetChange: (e: Event) => void;
		onLintManifest: () => void;
		onReplayManifest: () => void | Promise<void>;
	}

	let {
		editingChannel = $bindable(),
		editForm = $bindable(),
		editAuthForm = $bindable(),
		editPluginPreset = $bindable(),
		editPluginManifestInput = $bindable(),
		editReplayInput = $bindable(),
		editReplayOutput,
		editReplayError,
		editReplaying,
		editModelsInput = $bindable(),
		editTagsInput = $bindable(),
		editing,
		editError,
		editProviderCaps,
		editMissingCaps,
		probingId,
		pluginManifestExample,
		privatePluginManifestExample,
		pluginReplaySample,
		onClose,
		onSubmit,
		onProbe,
		onPresetChange,
		onLintManifest,
		onReplayManifest,
	}: Props = $props();

	function applyBaseUrlSuggestion() {
		if (!editingChannel) return;
		const suggestion = isPluginProvider(editingChannel.provider_type)
			? pluginPresetBaseUrlSuggestion(editPluginPreset)
			: providerBaseUrlSuggestion(editingChannel.provider_type);
		if (suggestion) editForm.base_url = suggestion;
	}
</script>

{#if editingChannel}
	<ModalFrame close={onClose} class="z-40 justify-end bg-black/50 backdrop-blur-sm p-0 animate-backdrop">
		<div class="w-full max-w-lg bg-white dark:bg-zinc-900 h-full overflow-y-auto shadow-2xl animate-slide-in-right">
			<div class="p-6">
				<div class="flex items-center justify-between mb-6">
					<div>
						<h2 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100">编辑 Channel</h2>
						<p class="text-xs font-mono text-zinc-500 dark:text-zinc-400 mt-0.5">{editingChannel.code}</p>
					</div>
					<button onclick={onClose} class="p-1.5 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-200 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
						<X size={18} />
					</button>
				</div>
				<form onsubmit={onSubmit} class="space-y-6">
					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">基础信息</p>
						<div class="space-y-3">
							<div>
								<label for="ed-name" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">名称</label>
								<Input id="ed-name" bind:value={editForm.name} disabled={editing} />
							</div>
							<div>
								<label for="ed-url" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">Base URL</label>
								<Input id="ed-url" bind:value={editForm.base_url} disabled={editing} />
								{#if editingChannel && (isPluginProvider(editingChannel.provider_type) ? pluginPresetBaseUrlSuggestion(editPluginPreset) : providerBaseUrlSuggestion(editingChannel.provider_type))}
									<button type="button" class="mt-1 text-xs text-zinc-500 hover:text-zinc-900 dark:text-zinc-400 dark:hover:text-zinc-100" onclick={applyBaseUrlSuggestion}>
										使用建议：{isPluginProvider(editingChannel.provider_type) ? pluginPresetBaseUrlSuggestion(editPluginPreset) : providerBaseUrlSuggestion(editingChannel.provider_type)}
									</button>
								{/if}
							</div>
							{#if editProviderCaps}
								<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
									<div class="mb-2 flex items-center justify-between gap-2">
										<p class="text-xs font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400">Capability</p>
										<span class="text-xs font-mono text-zinc-500 dark:text-zinc-400">{editingChannel.provider_type}</span>
									</div>
									<div class="flex flex-wrap gap-1.5" title={capabilityTitle(editProviderCaps)}>
										{#each capabilityList(editProviderCaps) as cap}
											<span class="rounded-md px-2 py-0.5 text-xs font-medium ring-1 {capabilityChipClass(cap)}">{CAPABILITY_LABELS[cap]}</span>
										{/each}
									</div>
									{#if editMissingCaps.length > 0}
										<p class="mt-2 text-xs text-amber-700 dark:text-amber-400">
											未声明 {editMissingCaps.map((cap) => CAPABILITY_LABELS[cap]).join(' / ')}；这些请求不会路由到该 Channel。
										</p>
									{/if}
								</div>
							{/if}
							{#if isPluginProvider(editingChannel.provider_type)}
								<div class="rounded-lg border border-zinc-200 bg-zinc-50 p-3 dark:border-zinc-800 dark:bg-zinc-950">
									<label for="ed-plugin-preset" class="mb-1 block text-sm font-medium text-zinc-700 dark:text-zinc-300">Provider 插件预设</label>
									<select id="ed-plugin-preset" bind:value={editPluginPreset} onchange={onPresetChange} disabled={editing} class="mb-3 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 text-sm text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100">
										{#each PLUGIN_PRESET_OPTIONS as opt}
											<option value={opt.value}>{opt.label}</option>
										{/each}
									</select>
									<PluginAuthEditor bind:form={editAuthForm} disabled={editing} idPrefix="ed-auth" />
									<div class="mb-1 flex items-center justify-between gap-2">
										<label for="ed-plugin" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300">Plugin Manifest</label>
										<Button size="sm" variant="outline" type="button" onclick={onLintManifest} disabled={editing}>本地 lint</Button>
									</div>
									<textarea id="ed-plugin" class="min-h-64 w-full rounded-md border border-zinc-200 bg-white px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-900 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={editPluginPreset ? pluginManifestExample : privatePluginManifestExample} bind:value={editPluginManifestInput} disabled={editing || !!editPluginPreset}></textarea>
									<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">保存前会把 Auth Strategy 合并进 manifest 并本地 lint；manifest 只引用 secret slot，不写明文 secret。</p>
									<div class="mt-4 rounded-md border border-zinc-200 bg-white p-3 dark:border-zinc-800 dark:bg-zinc-900">
										<div class="mb-2 flex items-center justify-between gap-2">
											<div>
												<p class="text-sm font-medium text-zinc-800 dark:text-zinc-200">SSE replay preview</p>
												<p class="text-xs text-zinc-500 dark:text-zinc-400">粘贴 raw SSE，预览归一后的 OpenAI-compatible chunks。</p>
											</div>
											<Button size="sm" variant="outline" type="button" onclick={onReplayManifest} disabled={editing || editReplaying}>{editReplaying ? '回放中...' : 'Replay'}</Button>
										</div>
										<textarea class="min-h-36 w-full rounded-md border border-zinc-200 bg-zinc-50 px-3 py-2 font-mono text-xs text-zinc-900 outline-none focus:ring-2 focus:ring-zinc-900 dark:border-zinc-700 dark:bg-zinc-950 dark:text-zinc-100 dark:focus:ring-zinc-100" placeholder={pluginReplaySample} bind:value={editReplayInput} disabled={editing || editReplaying}></textarea>
										{#if editReplayError}
											<p class="mt-2 rounded-md bg-red-50 px-2 py-1 text-xs text-red-600 dark:bg-red-900/20 dark:text-red-400">{editReplayError}</p>
										{/if}
										{#if editReplayOutput}
											<pre class="mt-2 max-h-56 overflow-auto rounded-md bg-zinc-950 p-3 text-xs text-zinc-100">{editReplayOutput}</pre>
										{/if}
									</div>
								</div>
							{/if}
							<div class="flex items-center gap-2">
								<input type="checkbox" id="ed-enabled" bind:checked={editForm.enabled} disabled={editing} class="w-4 h-4 rounded border-zinc-300 dark:border-zinc-600" />
								<label for="ed-enabled" class="text-sm text-zinc-700 dark:text-zinc-300">启用</label>
							</div>
						</div>
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">限速 & 超时</p>
						<div class="grid grid-cols-2 gap-3">
							<div>
								<label for="ed-rpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">RPM</label>
								<Input id="ed-rpm" type="number" placeholder="无限制" bind:value={editForm.rpm_limit} disabled={editing} />
							</div>
							<div>
								<label for="ed-tpm" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">TPM</label>
								<Input id="ed-tpm" type="number" placeholder="无限制" bind:value={editForm.tpm_limit} disabled={editing} />
							</div>
							<div>
								<label for="ed-timeout" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">超时(ms)</label>
								<Input id="ed-timeout" type="number" bind:value={editForm.timeout_ms} disabled={editing} />
							</div>
							<div>
								<label for="ed-retries" class="block text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-1">重试次数</label>
								<Input id="ed-retries" type="number" bind:value={editForm.max_retries} disabled={editing} />
							</div>
						</div>
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">模型</p>
						<div class="flex gap-2 items-end">
							<div class="flex-1">
								<Input placeholder="gpt-4o, gpt-4o-mini" bind:value={editModelsInput} disabled={editing} />
							</div>
							<Button variant="outline" size="sm" type="button" disabled={editing || !!probingId} onclick={() => editingChannel && onProbe(editingChannel)}>
								<span class="flex items-center gap-1"><Radar size={14} /> Probe</span>
							</Button>
						</div>
					</div>

					<div>
						<p class="text-[11px] font-semibold uppercase tracking-widest text-zinc-500 dark:text-zinc-400 mb-3">标签</p>
						<Input placeholder="production, us-east" bind:value={editTagsInput} disabled={editing} />
					</div>

					{#if editError}
						<p class="text-sm text-red-600 dark:text-red-400 bg-red-50 dark:bg-red-900/20 rounded-lg px-3 py-2">{editError}</p>
					{/if}
					<div class="flex gap-2 justify-end pt-4 border-t border-zinc-200 dark:border-zinc-800">
						<Button variant="outline" type="button" onclick={onClose}>取消</Button>
						<Button type="submit" disabled={editing}>{editing ? '保存中...' : '保存'}</Button>
					</div>
				</form>
			</div>
		</div>
	</ModalFrame>
{/if}
