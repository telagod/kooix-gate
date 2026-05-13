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

<div class="rounded-lg border border-zinc-200 bg-white p-5 shadow-sm {className}">
	<p class="text-xs font-medium uppercase tracking-wider text-zinc-500">{title}</p>
	<p class="mt-2 text-3xl font-bold text-zinc-900 tabular-nums">{value}</p>
	{#if subtitle}
		<p class="mt-1 text-xs text-zinc-500">{subtitle}</p>
	{/if}
</div>
