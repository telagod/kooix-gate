<script lang="ts">
	import { onMount } from 'svelte';
	import { Sparkles } from 'lucide-svelte';

	type FlowEditorComponent = typeof import('$lib/components/playground/FlowEditor.svelte').default;

	let FlowEditor = $state<FlowEditorComponent | null>(null);
	let loadError = $state<string | null>(null);

	onMount(async () => {
		try {
			FlowEditor = (await import('$lib/components/playground/FlowEditor.svelte')).default;
		} catch (error) {
			loadError = error instanceof Error ? error.message : '未知错误';
		}
	});
</script>

{#if FlowEditor}
	<FlowEditor />
{:else if loadError}
	<div class="flex h-full items-center justify-center bg-zinc-50 px-6 dark:bg-zinc-950">
		<div class="max-w-md rounded-2xl border border-red-200 bg-white p-6 text-center shadow-sm dark:border-red-900/60 dark:bg-zinc-900">
			<p class="text-sm font-semibold text-red-600 dark:text-red-300">Playground 加载失败</p>
			<p class="mt-2 text-xs text-zinc-500 dark:text-zinc-400">{loadError}</p>
		</div>
	</div>
{:else}
	<div class="flex h-full items-center justify-center bg-zinc-50 px-6 dark:bg-zinc-950">
		<div class="rounded-2xl border border-zinc-200 bg-white px-8 py-7 text-center shadow-sm dark:border-zinc-800 dark:bg-zinc-900">
			<div class="mx-auto mb-4 flex h-12 w-12 items-center justify-center rounded-xl bg-zinc-100 dark:bg-zinc-800">
				<Sparkles size={22} class="text-zinc-400 dark:text-zinc-500" />
			</div>
			<p class="text-sm font-semibold text-zinc-800 dark:text-zinc-100">正在加载 Flow editor</p>
			<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">仅在进入 Playground 时加载 @xyflow 与节点编辑器。</p>
		</div>
	</div>
{/if}
