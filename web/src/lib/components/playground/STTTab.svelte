<script lang="ts">
	import { createTranscription } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import { Mic, Loader2, Upload, Copy, Check, X, Settings2 } from 'lucide-svelte';
	import { clsx } from 'clsx';

	let model = $state('whisper-1');
	let language = $state('');
	let loading = $state(false);
	let error = $state('');
	let result = $state('');
	let copied = $state(false);
	let selectedFile = $state<File | null>(null);
	let fileInputEl: HTMLInputElement | undefined = $state();
	let history = $state<{ filename: string; text: string; ts: number }[]>([]);
	let showSettings = $state(false);

	async function transcribe() {
		if (!selectedFile || loading) return;
		loading = true; error = ''; result = '';
		try {
			const resp = await createTranscription(selectedFile, model, language || undefined);
			result = resp.text;
			history = [{ filename: selectedFile.name, text: resp.text, ts: Date.now() }, ...history].slice(0, 20);
		} catch (err: any) { error = err?.message ?? '转录失败'; }
		finally { loading = false; }
	}

	function handleDrop(e: DragEvent) { e.preventDefault(); const f = e.dataTransfer?.files?.[0]; if (f) selectedFile = f; }
	function handleDragOver(e: DragEvent) { e.preventDefault(); }
	function copyResult() { navigator.clipboard.writeText(result); copied = true; setTimeout(() => { copied = false; }, 2000); }
</script>

<div class="flex h-full overflow-hidden">
	<div class="flex-1 flex flex-col min-w-0">
		<!-- Header -->
		<div class="flex items-center justify-between px-4 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
			<div class="flex items-center gap-2">
				<select bind:value={model} class="text-xs border border-zinc-200 dark:border-zinc-700 rounded-lg px-2 py-1.5 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-400">
					<option value="whisper-1">whisper-1</option>
				</select>
				{#if selectedFile}
					<span class="text-xs text-zinc-500 dark:text-zinc-400 truncate max-w-[200px]">{selectedFile.name} ({(selectedFile.size / 1024 / 1024).toFixed(1)} MB)</span>
				{/if}
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
					<!-- Upload area -->
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<div ondrop={handleDrop} ondragover={handleDragOver} onclick={() => fileInputEl?.click()}
						class="mb-4 border-2 border-dashed border-zinc-300 dark:border-zinc-600 rounded-xl p-6 text-center cursor-pointer hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors">
						<input bind:this={fileInputEl} type="file" accept="audio/*,.mp3,.mp4,.mpeg,.mpga,.m4a,.wav,.webm" class="hidden"
							onchange={(e: Event) => { const t = e.target as HTMLInputElement; if (t.files?.[0]) selectedFile = t.files[0]; }} />
						<Upload size={28} class="mx-auto mb-2 text-zinc-400" />
						{#if selectedFile}
							<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300">{selectedFile.name}</p>
						{:else}
							<p class="text-sm text-zinc-500 dark:text-zinc-400">点击或拖拽上传音频文件</p>
							<p class="text-xs text-zinc-400 dark:text-zinc-500 mt-1">mp3, wav, m4a, webm (最大 25MB)</p>
						{/if}
					</div>

					<div class="flex justify-center mb-6">
						<Button size="sm" onclick={transcribe} disabled={!selectedFile || loading} class="rounded-xl px-5">
							{#if loading}<Loader2 size={14} class="animate-spin" /><span class="ml-1.5">转录中...</span>{:else}<Mic size={14} /><span class="ml-1.5">开始转录</span>{/if}
						</Button>
					</div>

					{#if error}
						<div class="mb-4 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-xs flex items-center gap-2"><X size={12} />{error}</div>
					{/if}

					{#if result}
						<div class="mb-6 rounded-xl border border-zinc-200 dark:border-zinc-700 overflow-hidden">
							<div class="flex items-center justify-between px-3 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/60">
								<span class="text-xs text-zinc-500 dark:text-zinc-400">转录结果</span>
								<button onclick={copyResult} class="text-xs text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300 flex items-center gap-1">
									{#if copied}<Check size={12} class="text-emerald-500" /> 已复制{:else}<Copy size={12} /> 复制{/if}
								</button>
							</div>
							<div class="p-4 text-sm text-zinc-900 dark:text-zinc-100 whitespace-pre-wrap leading-relaxed bg-white dark:bg-zinc-800/30">{result}</div>
						</div>
					{/if}

					{#if history.length > 0 && !result}
						<div class="space-y-1.5">
							{#each history as item}
								<div class="px-3 py-2 rounded-lg border border-zinc-200 dark:border-zinc-700">
									<div class="text-[10px] text-zinc-400 dark:text-zinc-500 mb-1">{item.filename}</div>
									<p class="text-sm text-zinc-700 dark:text-zinc-300 line-clamp-2">{item.text}</p>
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
						<h3 class="text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider">转录设置</h3>
						<div>
							<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">语言（可选）</label>
							<input type="text" bind:value={language} placeholder="zh, en, ja..."
								class="w-full h-8 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-xs text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
							<p class="text-[10px] text-zinc-400 dark:text-zinc-500 mt-1">ISO-639-1 代码，留空自动检测</p>
						</div>
					</div>
				</div>
			{/if}
		</div>
	</div>
</div>
