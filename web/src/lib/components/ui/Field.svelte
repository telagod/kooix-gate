<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn, text } from '$lib/design';

	let {
		label,
		for: forId = undefined,
		hint = '',
		error = '',
		required = false,
		class: className = '',
		children
	}: {
		label: string;
		for?: string;
		hint?: string;
		error?: string;
		required?: boolean;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<div class={cn('space-y-1.5', className)}>
	<label for={forId} class="block text-sm font-medium {text.secondary}">
		{label}
		{#if required}<span class="text-red-500">*</span>{/if}
	</label>
	{@render children?.()}
	{#if error}
		<p class="text-xs {text.danger}">{error}</p>
	{:else if hint}
		<p class="text-xs {text.muted}">{hint}</p>
	{/if}
</div>
