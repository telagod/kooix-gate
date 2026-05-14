<script lang="ts">
	import { createTranscription } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import { Mic, Loader2, Upload, Copy, Check, X } from 'lucide-svelte';

	let model = $state('whisper-1');
	let language = $state('');
	let loading = $state(false);
	let error = $state('');
	let result = $state('');
	let copied = $state(false);
	let selectedFile = $state<File | null>(null);
	let fileInputEl: HTMLInputElement | undefined = $state();
	let history = $state<{ filename: string; text: string; ts: number }[]>([]);

	async function transcribe() {
		if (!selectedFile || loading) return;
		loading = true;
		error = '';
		result = '';
		try {
			const resp = await createTranscription(selectedFile, model, language || undefined);
			result = resp.text;
			history = [{ filename: selectedFile.name, text: resp.text, ts: Date.now() }, ...history].slice(0, 20);
		} catch (err: any) {
			error = err?.message ?? '转录失败';
		} finally {
			loading = false;
		}
	}

	function handleDrop(e: DragEvent) {
		e.preventDefault();
		const file = e.dataTransfer?.files?.[0];
		if (file) selectedFile = file;
	}

	function handleDragOver(e: DragEvent) { e.preventDefault(); }

	function copyResult() {
		navigator.clipboard.writeText(result);
		copied = true;
		setTimeout(() => { copied = false; }, 2000);
	}
</script>

<div class="flex h-full">
	<div class="flex-1 flex flex-col min-w-0">
		<div class="flex-1 overflow-y-auto px-4 py-6">
			<div class="max-w-2xl mx-auto">
				<!-- Upload area -->
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div
					ondrop={handleDrop}
					ondragover={handleDragOver}
					onclick={() => fileInputEl?.click()}
					class="mb-6 border-2 border-dashed border-zinc-300 dark:border-zinc-600 rounded-xl p-8 text-center cursor-pointer hover:border-zinc-400 dark:hover:border-zinc-500 transition-colors"
				>
					<input
						bind:this={fileInputEl}
						type="file"
						accept="audio/*,.mp3,.mp4,.mpeg,.mpga,.m4a,.wav,.webm"
						class="hidden"
						onchange={(e: Event) => { const t = e.target as HTMLInputElement; if (t.files?.[0]) selectedFile = t.files[0]; }}
					/>
					<Upload size={32} class="mx-auto mb-3 text-zinc-400" />
					{#if selectedFile}
						<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300">{selectedFile.name}</p>
						<p class="text-xs text-zinc-400 mt-1">{(selectedFile.size / 1024 / 1024).toFixed(1)} MB</p>
					{:else}
						<p class="text-sm text-zinc-500">点击或拖拽上传音频文件</p>
						<p class="text-xs text-zinc-400 mt-1">支持 mp3, wav, m4a, webm 等格式（最大 25MB）</p>
					{/if}
				</div>

				<div class="flex justify-center mb-6">
					<Button size="sm" onclick={transcribe} disabled={!selectedFile || loading} class="rounded-xl px-6">
						{#if loading}
							<Loader2 size={14} class="animate-spin" />
							<span class="ml-1.5">转录中...</span>
						{:else}
							<Mic size={14} />
							<span class="ml-1.5">开始转录</span>
						{/if}
					</Button>
				</div>

				{#if error}
					<div class="mb-4 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-xs flex items-center gap-2">
						<X size={12} /> {error}
					</div>
				{/if}

				<!-- Result -->
				{#if result}
					<div class="mb-6 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 overflow-hidden">
						<div class="flex items-center justify-between px-3 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/60">
							<span class="text-xs text-zinc-500">转录结果</span>
							<button onclick={copyResult} class="text-xs text-zinc-500 hover:text-zinc-700 dark:hover:text-zinc-300 flex items-center gap-1">
								{#if copied}
									<Check size={12} class="text-emerald-500" /> 已复制
								{:else}
									<Copy size={12} /> 复制
								{/if}
							</button>
						</div>
						<div class="p-4 text-sm text-zinc-900 dark:text-zinc-100 whitespace-pre-wrap leading-relaxed">{result}</div>
					</div>
				{/if}

				<!-- History -->
				{#if history.length > 0 && !result}
					<div class="space-y-2">
						{#each history as item}
							<div class="px-3 py-2 rounded-lg border border-zinc-200 dark:border-zinc-700">
								<div class="flex items-center justify-between text-[10px] text-zinc-400 mb-1">
									<span>{item.filename}</span>
								</div>
								<p class="text-sm text-zinc-700 dark:text-zinc-300 line-clamp-2">{item.text}</p>
							</div>
						{/each}
					</div>
				{/if}
			</div>
		</div>
	</div>

	<!-- Settings -->
	<div class="w-56 shrink-0 border-l border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 p-4 overflow-y-auto space-y-4">
		<div>
			<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">模型</label>
			<select bind:value={model} class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-sm text-zinc-900 dark:text-zinc-100">
				<option value="whisper-1">whisper-1</option>
			</select>
		</div>
		<div>
			<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1">语言（可选）</label>
			<input type="text" bind:value={language} placeholder="zh, en, ja..." class="w-full h-9 px-2 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400" />
			<p class="text-[10px] text-zinc-400 mt-1">ISO-639-1 代码，留空自动检测</p>
		</div>
	</div>
</div>
