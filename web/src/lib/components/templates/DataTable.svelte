<script lang="ts">
	import type { Snippet } from 'svelte';
	import { cn, dataTemplate } from '$lib/design';

	let {
		class: className = '',
		tableClass = '',
		headClass = '',
		bodyClass = '',
		footerClass = '',
		isEmpty = false,
		emptyColspan = 1,
		head,
		empty,
		footer,
		children
	}: {
		class?: string;
		tableClass?: string;
		headClass?: string;
		bodyClass?: string;
		footerClass?: string;
		isEmpty?: boolean;
		emptyColspan?: number;
		head?: Snippet;
		empty?: Snippet;
		footer?: Snippet;
		children?: Snippet;
	} = $props();
</script>

<div class={cn(dataTemplate.tableWrap, className)}>
	<table class={cn(dataTemplate.table, tableClass)}>
		{#if head}
			<thead class={cn(dataTemplate.head, headClass)}>
				{@render head()}
			</thead>
		{/if}

		<tbody class={cn(dataTemplate.body, bodyClass)}>
			{#if isEmpty && empty}
				<tr>
					<td colspan={emptyColspan} class={dataTemplate.emptyCell}>
						{@render empty()}
					</td>
				</tr>
			{:else}
				{@render children?.()}
			{/if}
		</tbody>

		{#if footer}
			<tfoot class={cn(dataTemplate.foot, footerClass)}>
				{@render footer()}
			</tfoot>
		{/if}
	</table>
</div>
