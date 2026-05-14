<script lang="ts">
	import type { Snippet } from 'svelte';
	import { clsx } from 'clsx';

	let {
		variant = 'default',
		size = 'default',
		disabled = false,
		type = 'button',
		class: className = '',
		onclick,
		children
	}: {
		variant?: 'default' | 'outline' | 'ghost' | 'destructive';
		size?: 'default' | 'sm' | 'lg';
		disabled?: boolean;
		type?: 'button' | 'submit' | 'reset';
		class?: string;
		onclick?: () => void;
		children?: Snippet;
	} = $props();

	const base =
		'inline-flex items-center justify-center rounded-md font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-zinc-900 dark:focus-visible:ring-zinc-300 disabled:pointer-events-none disabled:opacity-50';

	const variants: Record<string, string> = {
		default: 'bg-zinc-900 text-white hover:bg-zinc-800 dark:bg-zinc-100 dark:text-zinc-900 dark:hover:bg-zinc-200',
		outline: 'border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 hover:bg-zinc-50 dark:hover:bg-zinc-800 text-zinc-900 dark:text-zinc-100',
		ghost: 'hover:bg-zinc-100 dark:hover:bg-zinc-800 text-zinc-900 dark:text-zinc-100',
		destructive: 'bg-red-600 text-white hover:bg-red-700'
	};

	const sizes: Record<string, string> = {
		default: 'h-10 px-4 py-2 text-sm',
		sm: 'h-8 px-3 text-xs',
		lg: 'h-12 px-8 text-base'
	};
</script>

<button
	{type}
	{disabled}
	{onclick}
	class={clsx(base, variants[variant], sizes[size], className)}
>
	{@render children?.()}
</button>
