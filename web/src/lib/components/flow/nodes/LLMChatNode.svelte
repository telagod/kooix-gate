<script lang="ts">
	import { onMount } from 'svelte';
	import type { FlowNodeData } from '$lib/flow/types.js';
	import type { ProviderCapabilityEntry } from '$lib/api.js';
	import { getProviderCapabilities } from '$lib/stores/provider-capabilities.js';
	import { isModalitySupported, supportingProviders } from '$lib/flow/capabilities.js';
	import BaseNode from '../BaseNode.svelte';
	import MarkdownRenderer from '$lib/components/ui/MarkdownRenderer.svelte';
	import { AlertCircle } from 'lucide-svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();

	let modelOptions = ['gpt-4o-mini', 'gpt-4o', 'claude-sonnet-4-20250514', 'claude-haiku-4-20250414'];

	// 0.4.160（第四刀 #3 step 2）：节点内 capability hint
	let caps = $state<ProviderCapabilityEntry[] | null>(null);
	onMount(() => {
		getProviderCapabilities().then((rows) => { caps = rows; }).catch(() => {});
	});
	let supported = $derived(isModalitySupported(caps, 'llmChat'));
	let providers = $derived(supportingProviders(caps, 'llmChat'));

	function updateParam(key: string, value: unknown) {
		data.params = { ...data.params, [key]: value };
	}
</script>

<BaseNode {data} {id}>
	<div class="space-y-2 nodrag nowheel">
		{#if caps && !supported}
			<div class="flex items-start gap-1.5 rounded-md bg-amber-50 dark:bg-amber-900/20 border border-amber-200 dark:border-amber-800 px-2 py-1.5">
				<AlertCircle size={12} class="text-amber-600 dark:text-amber-400 shrink-0 mt-0.5" />
				<span class="text-[10px] text-amber-700 dark:text-amber-300 leading-tight">无 chat-capable channel 可用</span>
			</div>
		{:else if caps && providers.length > 0}
			<div class="text-[9px] text-zinc-400 dark:text-zinc-500 font-mono truncate" title={providers.join(', ')}>
				可用 provider: {providers.length}
			</div>
		{/if}

		<select
			value={(data.params.model as string) ?? 'gpt-4o-mini'}
			onchange={(e) => updateParam('model', (e.target as HTMLSelectElement).value)}
			class="w-full text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-2 py-1 text-zinc-900 dark:text-zinc-100"
		>
			{#each modelOptions as m}<option value={m}>{m}</option>{/each}
		</select>

		<div class="flex gap-2 text-[10px]">
			<div class="flex-1">
				<div class="flex justify-between text-zinc-400 mb-0.5"><span>Temp</span><span class="font-mono">{((data.params.temperature as number) ?? 0.7).toFixed(1)}</span></div>
				<input type="range" min="0" max="2" step="0.1"
					value={(data.params.temperature as number) ?? 0.7}
					oninput={(e) => updateParam('temperature', parseFloat((e.target as HTMLInputElement).value))}
					class="w-full accent-zinc-600 h-1" />
			</div>
			<div class="flex-1">
				<div class="flex justify-between text-zinc-400 mb-0.5"><span>Top P</span><span class="font-mono">{((data.params.topP as number) ?? 1.0).toFixed(1)}</span></div>
				<input type="range" min="0" max="1" step="0.05"
					value={(data.params.topP as number) ?? 1.0}
					oninput={(e) => updateParam('topP', parseFloat((e.target as HTMLInputElement).value))}
					class="w-full accent-zinc-600 h-1" />
			</div>
		</div>

		<textarea
			value={(data.params.systemPrompt as string) ?? ''}
			oninput={(e) => updateParam('systemPrompt', (e.target as HTMLTextAreaElement).value)}
			placeholder="System prompt..."
			rows={2}
			class="w-full text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-2 py-1 text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 resize-none focus:outline-none focus:ring-1 focus:ring-zinc-400"
		></textarea>
	</div>

	{#if data.output}
		<div class="mt-2 max-h-[160px] overflow-y-auto rounded-md bg-zinc-50 dark:bg-zinc-800 p-2 text-xs text-zinc-700 dark:text-zinc-300 nowheel">
			<MarkdownRenderer content={data.output as string} streaming={data.status === 'running'} />
		</div>
	{/if}
</BaseNode>
