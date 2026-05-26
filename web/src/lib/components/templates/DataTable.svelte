<script lang="ts" generics="T">
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
		// 0.4.85（product-review B4 step 1）：sticky head + maxHeight。
		maxHeight = '',
		stickyHead = false,
		// 0.4.133（B4 step 2）：虚拟化模式 — 传 rows + rowSnippet + rowHeight。
		// 等高 row（rowHeight 必填，单位 px）；slice 渲染 [start, end] 区段，
		// 上下用 spacer tr 撑出余高，避免渲染所有 row。
		// 不传 rows → 退到 legacy children passthrough 模式（caller 自己 each）。
		rows = undefined,
		rowSnippet = undefined,
		rowHeight = 48,
		overscan = 5,
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
		rows?: T[];
		rowSnippet?: Snippet<[T, number]>;
		rowHeight?: number;
		overscan?: number;
		head?: Snippet;
		empty?: Snippet;
		footer?: Snippet;
		children?: Snippet;
	} = $props();

	const isVirtual = $derived(!!rows && !!rowSnippet);

	let scrollContainer: HTMLDivElement | undefined = $state(undefined);
	let scrollTop = $state(0);
	let viewportHeight = $state(600);

	function onScroll() {
		if (scrollContainer) {
			scrollTop = scrollContainer.scrollTop;
			viewportHeight = scrollContainer.clientHeight;
		}
	}

	const visibleStart = $derived(
		isVirtual && rows
			? Math.max(0, Math.floor(scrollTop / rowHeight) - overscan)
			: 0
	);
	const visibleEnd = $derived(
		isVirtual && rows
			? Math.min(
					rows.length,
					Math.ceil((scrollTop + viewportHeight) / rowHeight) + overscan
				)
			: 0
	);
	const topPadHeight = $derived(visibleStart * rowHeight);
	const botPadHeight = $derived(
		rows ? Math.max(0, (rows.length - visibleEnd) * rowHeight) : 0
	);

	const wrapStyle = $derived(maxHeight ? `max-height: ${maxHeight}; overflow-y: auto;` : '');
</script>

<div
	bind:this={scrollContainer}
	onscroll={onScroll}
	class={cn(dataTemplate.tableWrap, className)}
	style={wrapStyle}
>
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
			{:else if isVirtual && rows && rowSnippet}
				<!-- 顶部 spacer -->
				{#if topPadHeight > 0}
					<tr style="height: {topPadHeight}px;" aria-hidden="true"
						><td colspan={emptyColspan}></td></tr
					>
				{/if}
				<!-- 可见 row -->
				{#each rows.slice(visibleStart, visibleEnd) as row, i (visibleStart + i)}
					<tr style="height: {rowHeight}px;">
						{@render rowSnippet(row, visibleStart + i)}
					</tr>
				{/each}
				<!-- 底部 spacer -->
				{#if botPadHeight > 0}
					<tr style="height: {botPadHeight}px;" aria-hidden="true"
						><td colspan={emptyColspan}></td></tr
					>
				{/if}
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
