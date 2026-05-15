<script lang="ts">
	import { Handle, Position } from '@xyflow/svelte';
	import type { FlowNodeData } from '$lib/flow/types.js';
	import { NODE_CATALOG, PORT_COLORS } from '$lib/flow/types.js';
	import { clsx } from 'clsx';

	let { data, id }: { data: FlowNodeData; id: string } = $props();
	let meta = $derived(NODE_CATALOG[data.kind]);

	function handleTop(index: number, total: number): string {
		if (total === 1) return '50%';
		const step = 100 / (total + 1);
		return `${step * (index + 1)}%`;
	}
</script>

<div class={clsx(
	'rounded-xl border shadow-sm min-w-[220px] transition-all',
	data.status === 'running' && 'ring-2 ring-amber-400 ring-offset-1 dark:ring-offset-zinc-900',
	data.status === 'done' && 'ring-2 ring-emerald-400 ring-offset-1 dark:ring-offset-zinc-900',
	data.status === 'error' && 'ring-2 ring-red-400 ring-offset-1 dark:ring-offset-zinc-900',
	data.status === 'idle' && 'border-zinc-200 dark:border-zinc-700',
	'bg-white dark:bg-zinc-900'
)}>
	<!-- Header -->
	<div class="flex items-center gap-2 px-3 py-2 border-b border-zinc-100 dark:border-zinc-800 rounded-t-xl"
		style:background-color="{meta.color}10">
		<div class="w-5 h-5 rounded-md flex items-center justify-center text-white text-[10px] font-bold"
			style:background-color={meta.color}>
			{meta.label[0]}
		</div>
		<span class="text-xs font-medium text-zinc-700 dark:text-zinc-300">{meta.label}</span>
		{#if data.status === 'running'}
			<div class="ml-auto w-2 h-2 rounded-full bg-amber-400 animate-pulse"></div>
		{:else if data.status === 'done'}
			<div class="ml-auto w-2 h-2 rounded-full bg-emerald-400"></div>
		{:else if data.status === 'error'}
			<div class="ml-auto w-2 h-2 rounded-full bg-red-400"></div>
		{/if}
	</div>

	<!-- Body (slot) -->
	<div class="px-3 py-2">
		<slot />
	</div>

	<!-- Error display -->
	{#if data.error}
		<div class="px-3 pb-2">
			<div class="text-[10px] text-red-500 dark:text-red-400 truncate" title={data.error}>{data.error}</div>
		</div>
	{/if}

	<!-- Input handles (left) -->
	{#each meta.inputs as port, i}
		<Handle
			type="target"
			position={Position.Left}
			id={port.id}
			style="top: {handleTop(i, meta.inputs.length)}; background: {PORT_COLORS[port.type]}; width: 10px; height: 10px; border: 2px solid white;"
		/>
	{/each}

	<!-- Output handles (right) -->
	{#each meta.outputs as port, i}
		<Handle
			type="source"
			position={Position.Right}
			id={port.id}
			style="top: {handleTop(i, meta.outputs.length)}; background: {PORT_COLORS[port.type]}; width: 10px; height: 10px; border: 2px solid white;"
		/>
	{/each}
</div>
