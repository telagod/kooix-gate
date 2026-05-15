<script lang="ts">
	import { createSpeech } from '$lib/api.js';
	import type { AudioSpeechParams } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import { Volume2, Loader2, Play, Download, X, Settings2 } from 'lucide-svelte';
	import { clsx } from 'clsx';

	let text = $state('');
	let model = $state('tts-1');
	let voice = $state('alloy');
	let speed = $state(1.0);
	let format = $state('mp3');
	let loading = $state(false);
	let error = $state('');
	let audioUrl = $state<string | null>(null);
	let history = $state<{ text: string; url: string; voice: string; ts: number }[]>([]);
	let showSettings = $state(false);

	const voiceOptions = ['alloy', 'echo', 'fable', 'onyx', 'nova', 'shimmer'];
	const formatOptions = ['mp3', 'opus', 'aac', 'flac', 'wav'];

	async function speak() {
		if (!text.trim() || loading) return;
		loading = true; error = '';
		try {
			const params: AudioSpeechParams = { model, input: text.trim(), voice, response_format: format, speed };
			const blob = await createSpeech(params);
			if (audioUrl) URL.revokeObjectURL(audioUrl);
			audioUrl = URL.createObjectURL(blob);
			history = [{ text: text.trim().slice(0, 100), url: audioUrl, voice, ts: Date.now() }, ...history].slice(0, 20);
		} catch (err: any) { error = err?.message ?? '合成失败'; }
		finally { loading = false; }
	}
</script>

<div class="flex h-full overflow-hidden">
	<div class="flex-1 flex flex-col min-w-0">
		<!-- Header -->
		<div class="flex items-center justify-between px-4 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
			<div class="flex items-center gap-2">
				<select bind:value={model} class="text-xs border border-zinc-200 dark:border-zinc-700 rounded-lg px-2 py-1.5 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-400">
					<option value="tts-1">tts-1</option>
					<option value="tts-1-hd">tts-1-hd</option>
				</select>
				<div class="flex gap-0.5">
					{#each voiceOptions as v}
						<button onclick={() => voice = v}
							class={clsx('px-2 py-1 rounded-md text-xs transition-colors',
								voice === v ? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 font-medium' : 'text-zinc-500 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800')}>
							{v}
						</button>
					{/each}
				</div>
			</div>
			<button onclick={() => showSettings = !showSettings}
				class={clsx('p-1.5 rounded-lg transition-colors', showSettings ? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900' : 'hover:bg-zinc-100 dark:hover:bg-zinc-800 text-zinc-500')}>
				<Settings2 size={14} />
			</button>
		</div>

		<!-- Content -->
		<div class="flex flex-1 overflow-hidden">
			<div class="flex-1 overflow-y-auto px-4 py-6">
				<div class="max-w-2xl mx-auto">
					{#if audioUrl}
						<div class="mb-6 p-4 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800">
							<audio src={audioUrl} controls class="w-full mb-2"></audio>
							<div class="flex items-center justify-between text-xs text-zinc-500 dark:text-zinc-400">
								<span>{voice} · {model}</span>
								<a href={audioUrl} download="speech.{format}" class="inline-flex items-center gap-1 hover:text-zinc-900 dark:hover:text-zinc-100"><Download size={12} /> 下载</a>
							</div>
						</div>
					{:else if history.length === 0}
						<div class="flex flex-col items-center justify-center py-24 text-zinc-400 dark:text-zinc-500">
							<Volume2 size={36} class="mb-3 opacity-20" />
							<p class="text-sm text-zinc-500 dark:text-zinc-400">输入文本生成语音</p>
						</div>
					{/if}

					{#if error}
						<div class="mb-4 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-xs flex items-center gap-2"><X size={12} />{error}</div>
					{/if}

					{#if history.length > 0}
						<div class="space-y-1.5">
							{#each history as item}
								<div class="flex items-center gap-3 px-3 py-2 rounded-lg border border-zinc-200 dark:border-zinc-700 hover:bg-zinc-50 dark:hover:bg-zinc-800/50 transition-colors">
									<button onclick={() => { audioUrl = item.url; }} class="p-1.5 rounded-full hover:bg-zinc-200 dark:hover:bg-zinc-700"><Play size={12} class="text-zinc-500" /></button>
									<div class="flex-1 min-w-0">
										<p class="text-sm text-zinc-700 dark:text-zinc-300 truncate">{item.text}</p>
										<p class="text-[10px] text-zinc-400 dark:text-zinc-500">{item.voice}</p>
									</div>
								</div>
							{/each}
						</div>
					{/if}
				</div>
			</div>

			<!-- Settings Panel -->
			{#if showSettings}
				<div class="w-56 shrink-0 border-l border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 overflow-y-auto">
					<div class="p-4 space-y-4">
						<h3 class="text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider">语音设置</h3>
						<div>
							<div class="flex justify-between text-xs mb-1"><label class="text-zinc-500 dark:text-zinc-400">速度</label><span class="font-mono text-zinc-700 dark:text-zinc-300">{speed.toFixed(1)}x</span></div>
							<input type="range" min="0.25" max="4.0" step="0.25" bind:value={speed} class="w-full accent-zinc-900 dark:accent-zinc-100" />
							<div class="flex justify-between text-[10px] text-zinc-400 mt-0.5"><span>0.25x</span><span>4.0x</span></div>
						</div>
						<div>
							<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">格式</label>
							<select bind:value={format} class="w-full h-8 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-xs text-zinc-900 dark:text-zinc-100">
								{#each formatOptions as f}<option value={f}>{f}</option>{/each}
							</select>
						</div>
					</div>
				</div>
			{/if}
		</div>

		<!-- Input -->
		<div class="border-t border-zinc-200 dark:border-zinc-700 px-4 py-3 bg-white dark:bg-zinc-900">
			<div class="max-w-2xl mx-auto flex gap-2 items-end">
				<textarea bind:value={text} placeholder="输入要合成的文本..." rows={2}
					class="flex-1 resize-none rounded-xl border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-4 py-2.5 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 dark:placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-zinc-400 min-h-[50px] max-h-[200px]"
					oninput={(e: Event) => { const t = e.target as HTMLTextAreaElement; t.style.height = 'auto'; t.style.height = Math.min(t.scrollHeight, 200) + 'px'; }}></textarea>
				<Button size="sm" onclick={speak} disabled={!text.trim() || loading} class="rounded-xl h-10 w-10 p-0">
					{#if loading}<Loader2 size={14} class="animate-spin" />{:else}<Volume2 size={14} />{/if}
				</Button>
			</div>
		</div>
	</div>
</div>
