<script lang="ts">
	import type { Snippet } from 'svelte';
	import { badgeClass, type BadgeVariant } from '$lib/design';

	let {
		variant = 'default',
		class: className = '',
		children
	}: {
		variant?: BadgeVariant;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<span class={badgeClass({ variant, class: className })}>
	{@render children?.()}
</span>
