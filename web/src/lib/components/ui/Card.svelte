<script lang="ts">
	import type { Snippet } from 'svelte';
	import { clsx } from 'clsx';

	let {
		class: className = '',
		children
	}: {
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<div class={clsx('rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-sm', className)}>
	{@render children?.()}
</div>
