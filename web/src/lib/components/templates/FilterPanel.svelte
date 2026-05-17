<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn, dataTemplate } from '$lib/design';

	let {
		open = true,
		class: className = '',
		children
	}: {
		open?: boolean;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

{#if open}
	<div class={cn(dataTemplate.filterPanel, className)}>
		{@render children?.()}
	</div>
{/if}
