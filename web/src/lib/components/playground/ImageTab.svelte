<script lang="ts">
	import { generateImage } from '$lib/api.js';
	import type { ImageGenerationParams, ImageData } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import { ImagePlus, Download, Loader2, X, Settings2 } from 'lucide-svelte';
	import { clsx } from 'clsx';

	let prompt = $state('');
	let model = $state('dall-e-3');
	let size = $state('1024x1024');
	let quality = $state('standard');
	let style = $state('vivid');
	let n = $state(1);
	let loading = $state(false);
	let error = $state('');
	let results = $state<ImageData[]>([]);
	let history = $state<{ prompt: string; images: ImageData[]; ts: number }[]>([]);
	let showSettings = $state(false);

	const sizeOptions = ['256x256', '512x512', '1024x1024', '1024x1792', '1792x1024'];
	const qualityOptions = ['standard', 'hd'];
	const styleOptions = ['vivid', 'natural'];
	const modelOptions = ['dall-e-3', 'dall-e-2', 'gpt-image-1'];

	async function generate() {
		if (!prompt.trim() || loading) return;
		loading = true; error = ''; results = [];
		try {
			const params: ImageGenerationParams = { model, prompt: prompt.trim(), n, size, quality, style, response_format: 'url' };
			const resp = await generateImage(params);
			results = resp.data;
			history = [{ prompt: prompt.trim(), images: resp.data, ts: Date.now() }, ...history].slice(0, 20);
		} catch (err: any) { error = err?.message ?? '生成失败'; }
		finally { loading = false; }
	}

	function handleKeydown(e: KeyboardEvent) { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); generate(); } }

	function formatDate(ts: number): string { return new Date(ts).toLocaleString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }
</script>

<div class="flex h-full overflow-hidden">
	<div class="flex-1 flex flex-col min-w-0">
		<!-- Header -->
		<div class="flex items-center justify-between px-4 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
			<div class="flex items-center gap-2">
				<select bind:value={model} class="text-xs border border-zinc-200 dark:border-zinc-700 rounded-lg px-2 py-1.5 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-400">
					{#each modelOptions as m}<option value={m}>{m}</option>{/each}
				</select>
				<select bind:value={size} class="text-xs border border-zinc-200 dark:border-zinc-700 rounded-lg px-2 py-1.5 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-400">
					{#each sizeOptions as s}<option value={s}>{s}</option>{/each}
				</select>
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
					{#if loading}
						<div class="flex flex-col items-center justify-center py-20 text-zinc-400"><Loader2 size={28} class="animate-spin mb-3" /><p class="text-sm">生成中...</p></div>
					{:else if results.length > 0}
						<div class="grid grid-cols-1 md:grid-cols-2 gap-3 mb-6">
							{#each results as img}
								<div class="relative group rounded-xl overflow-hidden border border-zinc-200 dark:border-zinc-700">
									{#if img.url}
										<img src={img.url} alt={prompt} class="w-full aspect-square object-cover" />
										<a href={img.url} download target="_blank" rel="noopener" class="absolute top-2 right-2 p-2 rounded-lg bg-zinc-900/70 text-white opacity-0 group-hover:opacity-100 transition-opacity"><Download size={14} /></a>
									{/if}
									{#if img.revised_prompt}
										<div class="px-3 py-2 text-xs text-zinc-500 dark:text-zinc-400 border-t border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/50">{img.revised_prompt}</div>
									{/if}
								</div>
							{/each}
						</div>
					{:else if history.length === 0}
						<div class="flex flex-col items-center justify-center py-24 text-zinc-400 dark:text-zinc-500">
							<ImagePlus size={36} class="mb-3 opacity-20" />
							<p class="text-sm text-zinc-500 dark:text-zinc-400">输入描述生成图片</p>
						</div>
					{/if}

					{#if error}
						<div class="mb-4 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-xs flex items-center gap-2"><X size={12} />{error}</div>
					{/if}

					{#if history.length > 0 && results.length === 0}
						<div class="space-y-3">
							{#each history as item}
								<div class="rounded-xl border border-zinc-200 dark:border-zinc-700 overflow-hidden">
									<div class="px-3 py-2 text-xs text-zinc-500 dark:text-zinc-400 border-b border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/40">{item.prompt}</div>
									<div class="grid grid-cols-2 gap-1 p-1">
										{#each item.images as img}{#if img.url}<img src={img.url} alt="" class="w-full rounded" />{/if}{/each}
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
						<h3 class="text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider">生成设置</h3>
						<div>
							<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">质量</label>
							<select bind:value={quality} class="w-full h-8 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-xs text-zinc-900 dark:text-zinc-100">
								{#each qualityOptions as q}<option value={q}>{q}</option>{/each}
							</select>
						</div>
						<div>
							<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">风格</label>
							<select bind:value={style} class="w-full h-8 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-xs text-zinc-900 dark:text-zinc-100">
								{#each styleOptions as s}<option value={s}>{s}</option>{/each}
							</select>
						</div>
						<div>
							<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">数量</label>
							<input type="number" min="1" max="4" bind:value={n} class="w-full h-8 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-xs text-zinc-900 dark:text-zinc-100" />
						</div>
					</div>
				</div>
			{/if}
		</div>

		<!-- Input -->
		<div class="border-t border-zinc-200 dark:border-zinc-700 px-4 py-3 bg-white dark:bg-zinc-900">
			<div class="max-w-2xl mx-auto flex gap-2 items-end">
				<textarea bind:value={prompt} onkeydown={handleKeydown} placeholder="描述你想生成的图片..." rows={1}
					class="flex-1 resize-none rounded-xl border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-4 py-2.5 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 dark:placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-zinc-400 min-h-[40px] max-h-[120px]"
					oninput={(e: Event) => { const t = e.target as HTMLTextAreaElement; t.style.height = 'auto'; t.style.height = Math.min(t.scrollHeight, 120) + 'px'; }}></textarea>
				<Button size="sm" onclick={generate} disabled={!prompt.trim() || loading} class="rounded-xl h-10 w-10 p-0">
					{#if loading}<Loader2 size={14} class="animate-spin" />{:else}<ImagePlus size={14} />{/if}
				</Button>
			</div>
		</div>
	</div>
</div>
