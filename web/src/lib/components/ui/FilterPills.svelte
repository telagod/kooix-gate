<script lang="ts">
	import { cn } from '$lib/design';

	export interface FilterOption {
		value: string;
		label: string;
	}

	let {
		value = $bindable(''),
		options,
		class: className = ''
	}: {
		value?: string;
		options: FilterOption[];
		class?: string;
	} = $props();
</script>

<div class={cn('inline-flex items-center gap-1', className)}>
	{#each options as opt}
		<button
			type="button"
			onclick={() => (value = opt.value)}
			class={cn(
				'rounded-full px-3 py-1.5 text-xs font-medium transition-all',
				value === opt.value
					? 'bg-zinc-900 text-white shadow-sm dark:bg-zinc-100 dark:text-zinc-900'
					: 'bg-zinc-100 text-zinc-600 hover:bg-zinc-200 dark:bg-zinc-800 dark:text-zinc-400 dark:hover:bg-zinc-700'
			)}
		>
			{opt.label}
		</button>
	{/each}
</div>
