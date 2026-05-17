<script lang="ts">
	import { badgeClass, type BadgeVariant } from '$lib/design';

	let { model, metadata }: { model: string; metadata?: Record<string, unknown> | null } = $props();

	let modality = $derived(detectModality(model, metadata));

	function detectModality(m: string, meta?: Record<string, unknown> | null): { label: string; variant: BadgeVariant } {
		const type = (meta?.type as string) ?? '';
		if (type === 'image_gen' || m.startsWith('dall-e') || m === 'gpt-image-1') return { label: 'Image', variant: 'success' };
		if (type === 'tts' || m.startsWith('tts-')) return { label: 'TTS', variant: 'warning' };
		if (type === 'stt' || m.startsWith('whisper')) return { label: 'STT', variant: 'warning' };
		if (type === 'embedding' || m.includes('embedding')) return { label: 'Embed', variant: 'default' };
		if ((meta?.reasoning_tokens as number) > 0 || (meta?.thinking_tokens as number) > 0) return { label: 'Reason', variant: 'default' };
		return { label: 'Chat', variant: 'default' };
	}
</script>

<span class={badgeClass({ variant: modality.variant, class: 'px-1.5 py-px text-[10px] leading-tight' })}>
	{modality.label}
</span>
