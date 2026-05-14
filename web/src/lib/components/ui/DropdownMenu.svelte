<script lang="ts">
	import { clsx } from 'clsx';
	import { MoreHorizontal } from 'lucide-svelte';

	export interface MenuItem {
		label: string;
		icon?: any;
		danger?: boolean;
		disabled?: boolean;
		onclick: () => void;
	}

	let {
		items,
		class: className = ''
	}: {
		items: MenuItem[];
		class?: string;
	} = $props();

	let open = $state(false);

	function handleClick(item: MenuItem) {
		if (item.disabled) return;
		open = false;
		item.onclick();
	}
</script>

<div class="relative {className}">
	<button
		type="button"
		onclick={() => (open = !open)}
		class="p-1.5 rounded-md text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
	>
		<MoreHorizontal size={16} />
	</button>

	{#if open}
		<div class="absolute right-0 z-50 mt-1 min-w-[160px] rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-lg py-1 animate-fade-in">
			{#each items as item}
				<button
					type="button"
					disabled={item.disabled}
					onclick={() => handleClick(item)}
					class={clsx(
						'flex w-full items-center gap-2 px-3 py-2 text-[13px] text-left transition-colors',
						item.disabled && 'opacity-40 cursor-not-allowed',
						item.danger
							? 'text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-950/30'
							: 'text-zinc-700 dark:text-zinc-300 hover:bg-zinc-50 dark:hover:bg-zinc-800'
					)}
				>
					{#if item.icon}
						<svelte:component this={item.icon} size={14} />
					{/if}
					{item.label}
				</button>
			{/each}
		</div>
		<div class="fixed inset-0 z-40" onclick={() => (open = false)}></div>
	{/if}
</div>
