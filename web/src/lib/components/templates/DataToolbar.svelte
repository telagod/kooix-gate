<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn, dataTemplate } from '$lib/design';

	let {
		class: className = '',
		rowClass = '',
		searchClass = '',
		filtersClass = '',
		actionsClass = '',
		badgesClass = '',
		badgesVisible = true,
		query,
		controls,
		actions,
		badges,
		children
	}: {
		class?: string;
		rowClass?: string;
		searchClass?: string;
		filtersClass?: string;
		actionsClass?: string;
		badgesClass?: string;
		badgesVisible?: boolean;
		query?: Snippet;
		controls?: Snippet;
		actions?: Snippet;
		badges?: Snippet;
		children?: Snippet;
	} = $props();
</script>

<div class={cn(dataTemplate.toolbar, className)}>
	<div class={cn(dataTemplate.toolbarRow, rowClass)}>
		{#if query}
			<div class={cn(dataTemplate.toolbarSearch, searchClass)}>
				{@render query()}
			</div>
		{/if}

		{#if controls}
			<div class={cn(dataTemplate.toolbarFilters, filtersClass)}>
				{@render controls()}
			</div>
		{/if}

		{@render children?.()}

		{#if actions}
			<div class={cn(dataTemplate.toolbarActions, actionsClass)}>
				{@render actions()}
			</div>
		{/if}
	</div>

	{#if badges && badgesVisible}
		<div class={cn(dataTemplate.toolbarBadges, badgesClass)}>
			{@render badges()}
		</div>
	{/if}
</div>
