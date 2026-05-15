<script lang="ts">
	
	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';
	import { Upload, X } from 'lucide-svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();
	let fileEl: HTMLInputElement | undefined = $state();

	function handleFile(e: Event) {
		const t = e.target as HTMLInputElement;
		const f = t.files?.[0];
		if (!f) return;
		data.params = { ...data.params, file: f, fileName: f.name };
	}

	function clear() { data.params = { ...data.params, file: undefined, fileName: undefined }; }
</script>

<BaseNode {data} {id}>
	{#if data.params.fileName}
		<div class="flex items-center gap-2 px-2 py-1.5 rounded-md bg-zinc-50 dark:bg-zinc-800 border border-zinc-200 dark:border-zinc-700">
			<span class="text-xs text-zinc-600 dark:text-zinc-300 truncate flex-1">{data.params.fileName}</span>
			<button onclick={clear} class="p-0.5 rounded hover:bg-zinc-200 dark:hover:bg-zinc-700 nodrag"><X size={10} class="text-zinc-400" /></button>
		</div>
	{:else}
		<button onclick={() => fileEl?.click()} class="w-full py-4 border-2 border-dashed border-zinc-300 dark:border-zinc-600 rounded-md text-center hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors nodrag">
			<Upload size={20} class="mx-auto mb-1 text-zinc-400" />
			<span class="text-[10px] text-zinc-400">上传音频文件</span>
		</button>
	{/if}
	<input bind:this={fileEl} type="file" accept="audio/*,.mp3,.wav,.m4a,.webm,.ogg,.flac" class="hidden" onchange={handleFile} />
</BaseNode>
