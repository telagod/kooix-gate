<script lang="ts">
	import { fly } from 'svelte/transition';
	import { toasts, removeToast } from '$lib/stores/toast';

	const colorMap = {
		success: 'bg-green-500 text-white',
		error: 'bg-red-500 text-white',
		info: 'bg-zinc-600 text-white'
	};
</script>

<div class="fixed top-4 right-4 z-50 flex flex-col gap-2 pointer-events-none">
	{#each $toasts as toast (toast.id)}
		<div
			class="pointer-events-auto flex items-center justify-between gap-4 px-4 py-3 rounded-lg shadow-lg min-w-[260px] max-w-sm {colorMap[toast.type]}"
			in:fly={{ x: 40, duration: 220 }}
			out:fly={{ x: 40, duration: 180 }}
		>
			<span class="text-sm">{toast.message}</span>
			<button
				class="shrink-0 opacity-70 hover:opacity-100 transition-opacity text-lg leading-none"
				onclick={() => removeToast(toast.id)}
				aria-label="关闭"
			>×</button>
		</div>
	{/each}
</div>
