<script lang="ts">
	import { MoreHorizontal } from 'lucide-svelte';
	import { buttonClass, cn, surface } from '$lib/design';

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
	let btnEl: HTMLButtonElement | undefined = $state();
	let menuStyle = $state('');

	function toggle() {
		if (open) { open = false; return; }
		if (btnEl) {
			const rect = btnEl.getBoundingClientRect();
			const spaceBelow = window.innerHeight - rect.bottom;
			const menuH = items.length * 36 + 8;
			const top = spaceBelow < menuH ? rect.top - menuH : rect.bottom + 4;
			const left = Math.min(rect.right - 160, window.innerWidth - 170);
			menuStyle = `position: fixed; top: ${top}px; left: ${left}px;`;
		}
		open = true;
	}

	function handleClick(item: MenuItem) {
		if (item.disabled) return;
		open = false;
		item.onclick();
	}
</script>

<div class={cn('relative', className)}>
	<button
		bind:this={btnEl}
		type="button"
		onclick={toggle}
		class={buttonClass({ variant: 'ghost', size: 'icon', class: 'h-8 w-8 text-zinc-400 dark:text-zinc-500' })}
	>
		<MoreHorizontal size={16} />
	</button>

	{#if open}
		<button type="button" aria-label="关闭菜单" class="fixed inset-0 z-40 cursor-default appearance-none border-0 bg-transparent p-0" onclick={() => (open = false)}></button>
		<div class="z-50 min-w-[160px] rounded-lg border {surface.border} {surface.base} py-1 shadow-xl animate-fade-in"
			style={menuStyle}>
			{#each items as item}
				<button
					type="button"
					disabled={item.disabled}
					onclick={() => handleClick(item)}
					class={cn(
						'flex w-full items-center gap-2 px-3 py-2 text-left text-[13px] transition-colors',
						item.disabled && 'cursor-not-allowed opacity-40',
						item.danger
							? 'text-red-600 hover:bg-red-50 dark:text-red-400 dark:hover:bg-red-950/30'
							: 'text-zinc-700 hover:bg-zinc-50 dark:text-zinc-300 dark:hover:bg-zinc-800'
					)}
				>
					{#if item.icon}
						{@const Icon = item.icon}
						<Icon size={14} />
					{/if}
					{item.label}
				</button>
			{/each}
		</div>
	{/if}
</div>
