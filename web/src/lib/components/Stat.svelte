<script lang="ts">
	import { cardClass, text } from '$lib/design';

	let {
		title,
		value,
		subtitle = '',
		class: className = ''
	}: {
		title: string;
		value: string;
		subtitle?: string;
		class?: string;
	} = $props();
</script>

<div class={cardClass({ padding: 'md', class: className })}>
	<p class="text-xs font-medium uppercase tracking-wider {text.secondary}">{title}</p>
	<p class="mt-2 text-3xl font-bold tabular-nums {text.primary}">{value}</p>
	{#if subtitle}
		<p class="mt-1 text-sm {text.muted}">{subtitle}</p>
	{/if}
</div>
