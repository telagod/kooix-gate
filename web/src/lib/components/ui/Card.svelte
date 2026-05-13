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

<div class={clsx('rounded-lg border border-zinc-200 bg-white shadow-sm', className)}>
	{@render children?.()}
</div>
