<script lang="ts">
	import type { Snippet } from 'svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import { cn, text } from '$lib/design';

	let {
		title,
		description = '',
		icon = undefined,
		class: className = '',
		bodyClass = '',
		actions,
		children
	}: {
		title?: string;
		description?: string;
		icon?: any;
		class?: string;
		bodyClass?: string;
		actions?: Snippet;
		children?: Snippet;
	} = $props();
</script>

<Card padding="md" class={className}>
	{#if title || actions}
		<div class="mb-4 flex items-start justify-between gap-3">
			<div class="flex min-w-0 items-center gap-2">
				{#if icon}
					{@const Icon = icon}
					<Icon size={16} class="text-zinc-400" />
				{/if}
				<div class="min-w-0">
					{#if title}<h2 class="text-base font-semibold {text.primary}">{title}</h2>{/if}
					{#if description}<p class="mt-0.5 text-sm {text.secondary}">{description}</p>{/if}
				</div>
			</div>
			{#if actions}
				<div class="flex shrink-0 items-center gap-2">
					{@render actions()}
				</div>
			{/if}
		</div>
	{/if}

	<div class={cn(bodyClass)}>
		{@render children?.()}
	</div>
</Card>
