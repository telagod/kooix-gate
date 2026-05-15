<script lang="ts">
	
	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';
	import { ImagePlus, X } from 'lucide-svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();
	let fileEl: HTMLInputElement | undefined = $state();

	function handleFile(e: Event) {
		const t = e.target as HTMLInputElement;
		const f = t.files?.[0];
		if (!f) return;
		const reader = new FileReader();
		reader.onload = () => { data.params = { ...data.params, dataUrl: reader.result as string }; };
		reader.readAsDataURL(f);
	}

	function clear() { data.params = { ...data.params, dataUrl: undefined }; }
</script>

<BaseNode {data} {id}>
	{#if data.params.dataUrl}
		<div class="relative group">
			<img src={data.params.dataUrl as string} alt="" class="w-full max-h-[120px] object-cover rounded-md" />
			<button onclick={clear} class="absolute top-1 right-1 p-0.5 rounded bg-zinc-900/70 text-white opacity-0 group-hover:opacity-100 transition-opacity nodrag">
				<X size={10} />
			</button>
		</div>
	{:else}
		<button onclick={() => fileEl?.click()} class="w-full py-4 border-2 border-dashed border-zinc-300 dark:border-zinc-600 rounded-md text-center hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors nodrag">
			<ImagePlus size={20} class="mx-auto mb-1 text-zinc-400" />
			<span class="text-[10px] text-zinc-400">点击上传图片</span>
		</button>
	{/if}
	<input bind:this={fileEl} type="file" accept="image/*" class="hidden" onchange={handleFile} />
</BaseNode>
