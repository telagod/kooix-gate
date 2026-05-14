<script lang="ts">
	import { clsx } from 'clsx';

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

<div class="inline-flex items-center gap-1 {className}">
	{#each options as opt}
		<button
			type="button"
			onclick={() => (value = opt.value)}
			class={clsx(
				'px-3 py-1.5 rounded-full text-xs font-medium transition-all',
				value === opt.value
					? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 shadow-sm'
					: 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-200 dark:hover:bg-zinc-700'
			)}
		>
			{opt.label}
		</button>
	{/each}
</div>
