<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cardClass, type CardPadding, type CardVariant } from '$lib/design';

	let {
		variant = 'default',
		padding = 'none',
		interactive = false,
		class: className = '',
		children
	}: {
		variant?: CardVariant;
		padding?: CardPadding;
		interactive?: boolean;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<div class={cardClass({ variant, padding, interactive, class: className })}>
	{@render children?.()}
</div>
