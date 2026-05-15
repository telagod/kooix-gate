<script lang="ts">
	
	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';
	import MarkdownRenderer from '$lib/components/ui/MarkdownRenderer.svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();

	let previewData = $derived(data.output as Record<string, unknown> | undefined);
</script>

<BaseNode {data} {id}>
	{#if previewData}
		<div class="space-y-2 max-h-[240px] overflow-y-auto nowheel">
			{#if previewData.text}
				<div class="rounded-md bg-zinc-50 dark:bg-zinc-800 p-2 text-xs text-zinc-700 dark:text-zinc-300">
					<MarkdownRenderer content={previewData.text as string} />
				</div>
			{/if}
			{#if previewData.image}
				<img src={previewData.image as string} alt="Preview" class="w-full rounded-md max-h-[150px] object-cover" />
			{/if}
			{#if previewData.audio}
				{#if typeof previewData.audio === 'string'}
					<audio src={previewData.audio} controls class="w-full h-8"></audio>
				{:else}
					<div class="text-[10px] text-zinc-400">音频文件就绪</div>
				{/if}
			{/if}
		</div>
	{:else}
		<div class="text-[10px] text-zinc-400 dark:text-zinc-500 text-center py-3">
			连接输入端口查看结果
		</div>
	{/if}
</BaseNode>
