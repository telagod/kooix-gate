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
		// 0.4.85（product-review B4 step 1）：当表格 row 多时设置 maxHeight 让容器纵向滚，
		// 配合 stickyHead=true 让 thead 始终可见。真正的虚拟化（不渲染 off-screen row）
		// 在 step 2 用 row renderer + windowing 实现，本步先解决长表头消失体验。
		maxHeight = '',
		stickyHead = false,
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
		maxHeight?: string;
		stickyHead?: boolean;
		head?: Snippet;
		empty?: Snippet;
		footer?: Snippet;
		children?: Snippet;
	} = $props();

	const wrapStyle = $derived(maxHeight ? `max-height: ${maxHeight}; overflow-y: auto;` : '');
</script>

<div class={cn(dataTemplate.tableWrap, className)} style={wrapStyle}>
	<table class={cn(dataTemplate.table, tableClass)}>
		{#if head}
			<thead
				class={cn(
					dataTemplate.head,
					stickyHead && 'sticky top-0 z-10 bg-white dark:bg-zinc-950',
					headClass
				)}
			>
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
