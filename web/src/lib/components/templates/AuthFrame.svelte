<script lang="ts">
	import type { Snippet } from 'svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import { authTemplate, cn } from '$lib/design';

	let {
		title,
		description = '',
		max = 'sm',
		class: className = '',
		topRight,
		children
	}: {
		title: string;
		description?: string;
		max?: keyof typeof authTemplate.max;
		class?: string;
		topRight?: Snippet;
		children?: Snippet;
	} = $props();
</script>

<div class={authTemplate.shell}>
	{#if topRight}
		{@render topRight()}
	{/if}

	<Card class={cn(authTemplate.card, authTemplate.max[max], className)}>
		<div class={authTemplate.header}>
			<h1 class={authTemplate.title}>{title}</h1>
			{#if description}
				<p class={authTemplate.description}>{description}</p>
			{/if}
		</div>
		{@render children?.()}
	</Card>
</div>
