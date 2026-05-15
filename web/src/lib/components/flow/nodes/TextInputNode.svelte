<script lang="ts">
	
	import type { FlowNodeData } from '$lib/flow/types.js';
	import BaseNode from '../BaseNode.svelte';

	let { data, id }: { data: FlowNodeData; id: string } = $props();

	function handleInput(e: Event) {
		const t = e.target as HTMLTextAreaElement;
		data.params = { ...data.params, text: t.value };
	}
</script>

<BaseNode {data} {id}>
	<textarea
		value={(data.params.text as string) ?? ''}
		oninput={handleInput}
		placeholder="输入文本..."
		rows={3}
		class="w-full text-xs rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-2 py-1.5 text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 resize-none focus:outline-none focus:ring-1 focus:ring-zinc-400 nodrag nowheel"
	></textarea>
</BaseNode>
