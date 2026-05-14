<script lang="ts">
	let { class: className = '' }: { class?: string } = $props();
</script>

<div class="animate-pulse bg-zinc-200 dark:bg-zinc-700 rounded {className}"></div>
