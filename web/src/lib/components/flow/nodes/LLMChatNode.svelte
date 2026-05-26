<script lang="ts">
	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';
	import NodeCapabilityHint from '../NodeCapabilityHint.svelte';
	import MarkdownRenderer from '$lib/components/ui/MarkdownRenderer.svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();

	let modelOptions = ['gpt-4o-mini', 'gpt-4o', 'claude-sonnet-4-20250514', 'claude-haiku-4-20250414'];

	function updateParam(key: string, value: unknown) {
		data.params = { ...data.params, [key]: value };
	}
</script>

<BaseNode {data} {id}>
	<div class="space-y-2 nodrag nowheel">
		<NodeCapabilityHint kind="llmChat" label="chat-capable" />

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
