<script lang="ts">
	let { class: className = '' }: { class?: string } = $props();
</script>

<div class="animate-pulse bg-zinc-200 rounded {className}"></div>
