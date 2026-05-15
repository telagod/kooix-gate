<script lang="ts">
	import { clsx } from 'clsx';

	let { model, metadata }: { model: string; metadata?: Record<string, unknown> | null } = $props();

	let modality = $derived(detectModality(model, metadata));

	function detectModality(m: string, meta?: Record<string, unknown> | null): { label: string; cls: string } {
		const type = (meta?.type as string) ?? '';
		if (type === 'image_gen' || m.startsWith('dall-e') || m === 'gpt-image-1') return { label: 'Image', cls: 'bg-emerald-50 text-emerald-700 dark:bg-emerald-900/30 dark:text-emerald-400' };
		if (type === 'tts' || m.startsWith('tts-')) return { label: 'TTS', cls: 'bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400' };
		if (type === 'stt' || m.startsWith('whisper')) return { label: 'STT', cls: 'bg-amber-50 text-amber-700 dark:bg-amber-900/30 dark:text-amber-400' };
		if (type === 'embedding' || m.includes('embedding')) return { label: 'Embed', cls: 'bg-violet-50 text-violet-700 dark:bg-violet-900/30 dark:text-violet-400' };
		if ((meta?.reasoning_tokens as number) > 0 || (meta?.thinking_tokens as number) > 0) return { label: 'Reason', cls: 'bg-sky-50 text-sky-700 dark:bg-sky-900/30 dark:text-sky-400' };
		return { label: 'Chat', cls: 'bg-zinc-100 text-zinc-600 dark:bg-zinc-800 dark:text-zinc-400' };
	}
</script>

<span class={clsx('inline-block px-1.5 py-px rounded text-[10px] font-medium leading-tight', modality.cls)}>
	{modality.label}
</span>
