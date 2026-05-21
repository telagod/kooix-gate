<script lang="ts">
	import { ChevronLeft, ChevronRight } from 'lucide-svelte';
	import { dataTemplate } from '$lib/design';

	interface Props {
		page: number;
		pageSize: number;
		total: number;
		totalPages: number;
		onGoPage: (p: number) => void;
		pageNumbers: (page: number, totalPages: number) => (number | string)[];
	}

	let { page, pageSize, total, totalPages, onGoPage, pageNumbers }: Props = $props();
</script>

{#if totalPages > 1}
	<div class={dataTemplate.pagination}>
		<p class="text-xs text-zinc-500 dark:text-zinc-400">
			{(page - 1) * pageSize + 1}–{Math.min(page * pageSize, total)} / {total}
		</p>
		<div class="flex items-center gap-1">
			<button
				type="button"
				disabled={page <= 1}
				onclick={() => onGoPage(page - 1)}
				class="p-2 rounded-md text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
			>
				<ChevronLeft size={16} />
			</button>
			{#each pageNumbers(page, totalPages) as p}
				{#if p === '...'}
					<span class="w-8 h-8 flex items-center justify-center text-xs text-zinc-400">...</span>
				{:else}
					<button
						type="button"
						onclick={() => onGoPage(p as number)}
						class="w-8 h-8 rounded-md text-xs font-medium transition-colors {p === page
							? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900'
							: 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
					>{p}</button>
				{/if}
			{/each}
			<button
				type="button"
				disabled={page >= totalPages}
				onclick={() => onGoPage(page + 1)}
				class="p-2 rounded-md text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800 disabled:opacity-30 disabled:cursor-not-allowed transition-colors"
			>
				<ChevronRight size={16} />
			</button>
		</div>
	</div>
{/if}
