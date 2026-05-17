<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn } from '$lib/design';

	let {
		close,
		label = '关闭弹层',
		class: className = '',
		panelClass = '',
		children
	}: {
		close?: () => void;
		label?: string;
		class?: string;
		panelClass?: string;
		children?: Snippet;
	} = $props();
</script>

<div class={cn('fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4', className)}>
	<button type="button" aria-label={label} class="absolute inset-0 cursor-default" onclick={close}></button>
	<div class={cn('relative z-10', panelClass)}>
		{@render children?.()}
	</div>
</div>
