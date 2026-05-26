<script lang="ts">

	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';
	import NodeCapabilityHint from '../NodeCapabilityHint.svelte';
	import { clsx } from 'clsx';

	let { data, id }: { data: FlowNodeData; id: string } = $props();

	let voiceOptions = ['alloy', 'echo', 'fable', 'onyx', 'nova', 'shimmer'];

	function updateParam(key: string, value: unknown) {
		data.params = { ...data.params, [key]: value };
	}
</script>

<BaseNode {data} {id}>
	<div class="space-y-2 nodrag nowheel">
		<NodeCapabilityHint kind="tts" label="audio-capable" />
		<div class="flex gap-1.5">
			<select
				value={(data.params.model as string) ?? 'tts-1'}
				onchange={(e) => updateParam('model', (e.target as HTMLSelectElement).value)}
				class="flex-1 text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-1.5 py-1 text-zinc-900 dark:text-zinc-100"
			>
				<option value="tts-1">tts-1</option>
				<option value="tts-1-hd">tts-1-hd</option>
			</select>
		</div>
		<div class="flex flex-wrap gap-0.5">
			{#each voiceOptions as v}
				<button
					onclick={() => updateParam('voice', v)}
					class={clsx('px-1.5 py-0.5 rounded text-[10px] transition-colors',
						(data.params.voice ?? 'alloy') === v
							? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 font-medium'
							: 'text-zinc-500 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800')}
				>
					{v}
				</button>
			{/each}
		</div>
		<div class="text-[10px]">
			<div class="flex justify-between text-zinc-400 mb-0.5"><span>速度</span><span class="font-mono">{((data.params.speed as number) ?? 1.0).toFixed(1)}x</span></div>
			<input type="range" min="0.25" max="4.0" step="0.25"
				value={(data.params.speed as number) ?? 1.0}
				oninput={(e) => updateParam('speed', parseFloat((e.target as HTMLInputElement).value))}
				class="w-full accent-zinc-600 h-1" />
		</div>
	</div>

	{#if data.output}
		<div class="mt-2">
			<audio src={data.output as string} controls class="w-full h-8"></audio>
		</div>
	{/if}
</BaseNode>
