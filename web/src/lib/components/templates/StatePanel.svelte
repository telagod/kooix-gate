<script lang="ts">
	import type { Snippet } from 'svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import { cn, text } from '$lib/design';

	let {
		title = '',
		description = '',
		icon = undefined,
		variant = 'default',
		class: className = '',
		actions
	}: {
		title?: string;
		description?: string;
		icon?: any;
		variant?: 'default' | 'danger' | 'warning' | 'success';
		class?: string;
		actions?: Snippet;
	} = $props();

	let tone = $derived(
		variant === 'danger'
			? text.danger
			: variant === 'warning'
				? text.warning
				: variant === 'success'
					? text.success
					: text.muted
	);
</script>

<Card class={cn('p-8 text-center', className)}>
	{#if icon}
		{@const Icon = icon}
		<Icon size={28} class={cn('mx-auto mb-3', tone)} />
	{/if}
	{#if title}<p class="text-base font-medium {text.primary}">{title}</p>{/if}
	{#if description}<p class="mt-1 text-sm {variant === 'default' ? text.secondary : tone}">{description}</p>{/if}
	{#if actions}
		<div class="mt-4 flex justify-center gap-2">
			{@render actions()}
		</div>
	{/if}
</Card>
