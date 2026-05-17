<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn, pageTemplate } from '$lib/design';

	let {
		title,
		description = '',
		eyebrow = '',
		max = 'default',
		icon = undefined,
		class: className = '',
		actions,
		children
	}: {
		title: string;
		description?: string;
		eyebrow?: string;
		max?: keyof typeof pageTemplate.max;
		icon?: any;
		class?: string;
		actions?: Snippet;
		children?: Snippet;
	} = $props();
</script>

<div class={cn(pageTemplate.shell, pageTemplate.max[max], className)}>
	<header class={pageTemplate.header}>
		<div class="flex min-w-0 items-start gap-3">
			{#if icon}
				{@const Icon = icon}
				<div class={pageTemplate.icon}>
					<Icon size={18} />
				</div>
			{/if}
			<div class={pageTemplate.titleWrap}>
				{#if eyebrow}
					<p class={pageTemplate.eyebrow}>{eyebrow}</p>
				{/if}
				<h1 class={pageTemplate.title}>{title}</h1>
				{#if description}
					<p class={pageTemplate.description}>{description}</p>
				{/if}
			</div>
		</div>

		{#if actions}
			<div class={pageTemplate.actions}>
				{@render actions()}
			</div>
		{/if}
	</header>

	{@render children?.()}
</div>
