<script lang="ts">
	import ChatTab from '$lib/components/playground/ChatTab.svelte';
	import ImageTab from '$lib/components/playground/ImageTab.svelte';
	import TTSTab from '$lib/components/playground/TTSTab.svelte';
	import STTTab from '$lib/components/playground/STTTab.svelte';
	import { MessageSquare, ImagePlus, Volume2, Mic } from 'lucide-svelte';
	import { clsx } from 'clsx';

	type PlaygroundTab = 'chat' | 'images' | 'tts' | 'stt';
	let activeTab = $state<PlaygroundTab>('chat');

	const tabs: { id: PlaygroundTab; label: string; icon: any }[] = [
		{ id: 'chat', label: 'Chat', icon: MessageSquare },
		{ id: 'images', label: 'Images', icon: ImagePlus },
		{ id: 'tts', label: 'TTS', icon: Volume2 },
		{ id: 'stt', label: 'STT', icon: Mic },
	];
</script>

<div class="flex flex-col h-full overflow-hidden">
	<!-- Tab Bar -->
	<div class="flex items-center gap-1 px-3 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
		{#each tabs as tab}
			<button
				onclick={() => activeTab = tab.id}
				class={clsx(
					'inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-sm transition-colors',
					activeTab === tab.id
						? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 font-medium'
						: 'text-zinc-500 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'
				)}
			>
				<svelte:component this={tab.icon} size={14} />
				{tab.label}
			</button>
		{/each}
	</div>

	<!-- Content -->
	<div class="flex-1 overflow-hidden">
		{#if activeTab === 'chat'}
			<ChatTab />
		{:else if activeTab === 'images'}
			<ImageTab />
		{:else if activeTab === 'tts'}
			<TTSTab />
		{:else if activeTab === 'stt'}
			<STTTab />
		{/if}
	</div>
</div>
