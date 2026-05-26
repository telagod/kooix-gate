<script lang="ts">
	// 0.4.161（第四刀 #3 step 3）：节点 capability hint 共享组件。
	// 节点内根据 capability 矩阵显示「不支持」横幅 或 「可用 provider: N」提示。
	import { onMount } from 'svelte';
	import type { ProviderCapabilityEntry } from '$lib/api.js';
	import type { FlowNodeKind } from '$lib/flow/types.js';
	import { getProviderCapabilities } from '$lib/stores/provider-capabilities.js';
	import { isModalitySupported, supportingProviders } from '$lib/flow/capabilities.js';
	import { AlertCircle } from 'lucide-svelte';

	let { kind, label }: { kind: FlowNodeKind; label: string } = $props();

	let caps = $state<ProviderCapabilityEntry[] | null>(null);
	onMount(() => {
		getProviderCapabilities().then((rows) => { caps = rows; }).catch(() => {});
	});
	let supported = $derived(isModalitySupported(caps, kind));
	let providers = $derived(supportingProviders(caps, kind));
</script>

{#if caps && !supported}
	<div class="flex items-start gap-1.5 rounded-md bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 px-2 py-1.5">
		<AlertCircle size={12} class="text-amber-600 dark:text-amber-400 shrink-0 mt-0.5" />
		<span class="text-[10px] text-amber-700 dark:text-amber-300 leading-tight">无 {label} 可用 channel</span>
	</div>
{:else if caps && providers.length > 0}
	<div class="text-[9px] text-zinc-400 dark:text-zinc-500 font-mono truncate" title={providers.join(', ')}>
		可用 provider: {providers.length}
	</div>
{/if}
