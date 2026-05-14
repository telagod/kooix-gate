<script lang="ts">
	import { clsx } from 'clsx';

	let {
		type = 'text',
		placeholder = '',
		value = $bindable(''),
		id = '',
		disabled = false,
		class: className = '',
		autocomplete = 'off'
	}: {
		type?: string;
		placeholder?: string;
		value?: string;
		id?: string;
		disabled?: boolean;
		class?: string;
		autocomplete?: string;
	} = $props();
</script>

<input
	{type}
	{placeholder}
	{id}
	{disabled}
	{autocomplete}
	bind:value
	class={clsx(
		'flex h-10 w-full rounded-md border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-900 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 dark:placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-400 disabled:cursor-not-allowed disabled:opacity-50',
		className
	)}
/>
