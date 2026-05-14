<script lang="ts">
	let {
		title,
		value,
		subtitle = '',
		class: className = ''
	}: {
		title: string;
		value: string;
		subtitle?: string;
		class?: string;
	} = $props();
</script>

<div class="rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 p-5 shadow-sm dark:shadow-zinc-900/30 {className}">
	<p class="text-xs font-medium uppercase tracking-wider text-zinc-500 dark:text-zinc-400">{title}</p>
	<p class="mt-2 text-3xl font-bold text-zinc-900 dark:text-zinc-100 tabular-nums">{value}</p>
	{#if subtitle}
		<p class="mt-1 text-xs text-zinc-500 dark:text-zinc-400">{subtitle}</p>
	{/if}
</div>
