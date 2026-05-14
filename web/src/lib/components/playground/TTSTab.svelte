<script lang="ts">
	import { createSpeech } from '$lib/api.js';
	import type { AudioSpeechParams } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import { Volume2, Loader2, Play, Download, X } from 'lucide-svelte';

	let text = $state('');
	let model = $state('tts-1');
	let voice = $state('alloy');
	let speed = $state(1.0);
	let format = $state('mp3');
	let loading = $state(false);
	let error = $state('');
	let audioUrl = $state<string | null>(null);
	let audioEl: HTMLAudioElement | undefined = $state();
	let history = $state<{ text: string; url: string; voice: string; ts: number }[]>([]);

	const modelOptions = ['tts-1', 'tts-1-hd'];
	const voiceOptions = ['alloy', 'echo', 'fable', 'onyx', 'nova', 'shimmer'];
	const formatOptions = ['mp3', 'opus', 'aac', 'flac', 'wav'];

	async function speak() {
		if (!text.trim() || loading) return;
		loading = true;
		error = '';
		try {
			const params: AudioSpeechParams = {
				model, input: text.trim(), voice,
				response_format: format, speed
			};
			const blob = await createSpeech(params);
			if (audioUrl) URL.revokeObjectURL(audioUrl);
			audioUrl = URL.createObjectURL(blob);
			history = [{ text: text.trim().slice(0, 100), url: audioUrl, voice, ts: Date.now() }, ...history].slice(0, 20);
		} catch (err: any) {
			error = err?.message ?? '合成失败';
		} finally {
			loading = false;
		}
	}
</script>

<div class="flex h-full">
	<div class="flex-1 flex flex-col min-w-0">
		<div class="flex-1 overflow-y-auto px-4 py-6">
			<div class="max-w-2xl mx-auto">
				{#if audioUrl}
					<div class="mb-6 p-4 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800">
						<audio bind:this={audioEl} src={audioUrl} controls class="w-full mb-2"></audio>
						<div class="flex items-center justify-between text-xs text-zinc-500">
							<span>{voice} · {model}</span>
							<a href={audioUrl} download="speech.{format}" class="inline-flex items-center gap-1 text-zinc-600 dark:text-zinc-400 hover:text-zinc-900 dark:hover:text-zinc-100">
								<Download size={12} /> 下载
							</a>
						</div>
					</div>
				{:else if history.length === 0}
					<div class="flex flex-col items-center justify-center py-20 text-zinc-400">
						<Volume2 size={48} class="mb-4 opacity-20" />
						<p class="text-sm font-medium text-zinc-500">输入文本生成语音</p>
						<p class="text-xs mt-1 text-zinc-400">6 种声音，支持 HD 模式</p>
					</div>
				{/if}

				{#if error}
					<div class="mb-4 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-xs flex items-center gap-2">
						<X size={12} /> {error}
					</div>
				{/if}

				<!-- History -->
				{#if history.length > 0}
					<div class="space-y-2">
						{#each history as item}
							<div class="flex items-center gap-3 px-3 py-2 rounded-lg border border-zinc-200 dark:border-zinc-700 hover:bg-zinc-50 dark:hover:bg-zinc-800/50">
								<button onclick={() => { audioUrl = item.url; }} class="p-1.5 rounded-full hover:bg-zinc-200 dark:hover:bg-zinc-700">
									<Play size={12} class="text-zinc-500" />
								</button>
								<div class="flex-1 min-w-0">
									<p class="text-sm text-zinc-700 dark:text-zinc-300 truncate">{item.text}</p>
									<p class="text-[10px] text-zinc-400">{item.voice}</p>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</div>

		<!-- Input -->
		<div class="border-t border-zinc-200 dark:border-zinc-700 px-4 py-3 bg-white dark:bg-zinc-900">
			<div class="max-w-2xl mx-auto flex gap-2 items-end">
				<textarea
					bind:value={text}
					placeholder="输入要合成的文本..."
					rows={3}
					class="flex-1 resize-none rounded-xl border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-4 py-2.5 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-400 min-h-[60px] max-h-[200px]"
				></textarea>
				<Button size="sm" onclick={speak} disabled={!text.trim() || loading} class="rounded-xl h-10 px-4">
					{#if loading}
						<Loader2 size={14} class="animate-spin" />
					{:else}
						<Volume2 size={14} />
					{/if}
					<span class="ml-1.5">合成</span>
				</Button>
			</div>
		</div>
	</div>

	<!-- Settings -->
	<div class="w-56 shrink-0 border-l border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 p-4 overflow-y-auto space-y-4">
		<div>
			<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">模型</label>
			<select bind:value={model} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-sm text-zinc-900 dark:text-zinc-100">
				{#each modelOptions as m}<option value={m}>{m}</option>{/each}
			</select>
		</div>
		<div>
			<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">声音</label>
			<div class="grid grid-cols-2 gap-1.5">
				{#each voiceOptions as v}
					<button
						onclick={() => voice = v}
						class="px-2 py-1.5 rounded-lg text-xs transition-colors {voice === v ? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 font-medium' : 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-400 hover:bg-zinc-200 dark:hover:bg-zinc-700'}"
					>{v}</button>
				{/each}
			</div>
		</div>
		<div>
			<div class="flex justify-between text-xs mb-1">
				<label class="text-zinc-500 dark:text-zinc-400">速度</label>
				<span class="font-mono text-zinc-700 dark:text-zinc-300">{speed.toFixed(1)}x</span>
			</div>
			<input type="range" min="0.25" max="4.0" step="0.25" bind:value={speed} class="w-full accent-zinc-900 dark:accent-zinc-100" />
		</div>
		<div>
			<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">格式</label>
			<select bind:value={format} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-sm text-zinc-900 dark:text-zinc-100">
				{#each formatOptions as f}<option value={f}>{f}</option>{/each}
			</select>
		</div>
	</div>
</div>
