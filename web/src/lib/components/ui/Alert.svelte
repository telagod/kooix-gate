<script lang="ts">
	import type { Snippet } from 'svelte';
	import { alertClass, type AlertVariant } from '$lib/design';

	let {
		variant = 'info',
		class: className = '',
		children
	}: {
		variant?: AlertVariant;
		class?: string;
		children?: Snippet;
	} = $props();
</script>

<div class={alertClass({ variant, class: className })}>
	{@render children?.()}
</div>
