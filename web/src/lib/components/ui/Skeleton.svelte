<script lang="ts">
	import { cn, skeletonClass } from '$lib/design';

	let { class: className = '' }: { class?: string } = $props();
</script>

<div class={cn(skeletonClass, className)}></div>
