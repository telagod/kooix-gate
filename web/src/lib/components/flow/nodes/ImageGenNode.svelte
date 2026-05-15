<script lang="ts">
	
	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();

	let modelOptions = ['dall-e-3', 'dall-e-2', 'gpt-image-1'];
	let sizeOptions = ['256x256', '512x512', '1024x1024', '1024x1792', '1792x1024'];

	function updateParam(key: string, value: unknown) {
		data.params = { ...data.params, [key]: value };
	}
</script>

<BaseNode {data} {id}>
	<div class="space-y-2 nodrag nowheel">
		<div class="flex gap-1.5">
			<select
				value={(data.params.model as string) ?? 'dall-e-3'}
				onchange={(e) => updateParam('model', (e.target as HTMLSelectElement).value)}
				class="flex-1 text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-1.5 py-1 text-zinc-900 dark:text-zinc-100"
			>
				{#each modelOptions as m}<option value={m}>{m}</option>{/each}
			</select>
			<select
				value={(data.params.size as string) ?? '1024x1024'}
				onchange={(e) => updateParam('size', (e.target as HTMLSelectElement).value)}
				class="flex-1 text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-1.5 py-1 text-zinc-900 dark:text-zinc-100"
			>
				{#each sizeOptions as s}<option value={s}>{s}</option>{/each}
			</select>
		</div>
		<div class="flex gap-1.5">
			<select
				value={(data.params.quality as string) ?? 'standard'}
				onchange={(e) => updateParam('quality', (e.target as HTMLSelectElement).value)}
				class="flex-1 text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-1.5 py-1 text-zinc-900 dark:text-zinc-100"
			>
				<option value="standard">standard</option>
				<option value="hd">hd</option>
			</select>
			<select
				value={(data.params.style as string) ?? 'vivid'}
				onchange={(e) => updateParam('style', (e.target as HTMLSelectElement).value)}
				class="flex-1 text-[10px] rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-1.5 py-1 text-zinc-900 dark:text-zinc-100"
			>
				<option value="vivid">vivid</option>
				<option value="natural">natural</option>
			</select>
		</div>
	</div>

	{#if data.output}
		<div class="mt-2">
			<img src={data.output as string} alt="Generated" class="w-full rounded-md max-h-[180px] object-cover" />
		</div>
	{/if}
</BaseNode>
