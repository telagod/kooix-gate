<script lang="ts">
	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';
	import NodeCapabilityHint from '../NodeCapabilityHint.svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();

	function updateParam(key: string, value: unknown) {
		data.params = { ...data.params, [key]: value };
	}
</script>

<BaseNode {data} {id}>
	<div class="space-y-2 nodrag nowheel">
		<NodeCapabilityHint kind="stt" label="audio-capable" />
		<select
			value={(data.params.model as string) ?? 'whisper-1'}
			onchange={(e) => updateParam('model', (e.target as HTMLSelectElement).value)}
			class="w-full text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-2 py-1 text-zinc-900 dark:text-zinc-100"
		>
			<option value="whisper-1">whisper-1</option>
		</select>
		<input
			type="text"
			value={(data.params.language as string) ?? ''}
			oninput={(e) => updateParam('language', (e.target as HTMLInputElement).value)}
			placeholder="语言 (zh, en, ja...)"
			class="w-full text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-2 py-1 text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 focus:outline-none focus:ring-1 focus:ring-zinc-400"
		/>
	</div>

	{#if data.output}
		<div class="mt-2 max-h-[100px] overflow-y-auto rounded-md bg-zinc-50 dark:bg-zinc-800 p-2 text-xs text-zinc-700 dark:text-zinc-300 whitespace-pre-wrap nowheel">
			{data.output}
		</div>
	{/if}
</BaseNode>
