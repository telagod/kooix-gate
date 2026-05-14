<script lang="ts">
	import { onMount } from 'svelte';
	import { getMe, chatCompletionStream, listModels } from '$lib/api.js';
	import type { MeResult, ModelInfo } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import Card from '$lib/components/ui/Card.svelte';
	import { Send, Square, RotateCcw, Bot, User as UserIcon } from 'lucide-svelte';

	let me = $state<MeResult | null>(null);
	let currentOrg = $derived(me?.current_org ?? me?.orgs?.[0] ?? null);
	let model = $state('gpt-4o-mini');
	let availableModels = $state<ModelInfo[]>([]);
	let input = $state('');
	let messages = $state<{ role: string; content: string }[]>([]);
	let streaming = $state(false);
	let abortCtrl = $state<AbortController | null>(null);
	let error = $state('');
	let chatEl: HTMLElement | undefined = $state();

	const FALLBACK_MODELS = ['gpt-4o-mini', 'gpt-4o', 'gpt-4', 'gpt-3.5-turbo', 'claude-sonnet-4-20250514', 'claude-haiku-4-20250414'];

	onMount(async () => {
		try { me = await getMe(); } catch {}
		try {
			availableModels = await listModels();
			if (availableModels.length > 0) {
				model = availableModels[0].id;
			}
		} catch {}
	});

	let modelOptions = $derived(
		availableModels.length > 0
			? availableModels.map(m => m.id)
			: FALLBACK_MODELS
	);

	onMount(async () => {
		try { me = await getMe(); } catch {}
	});

	function scrollBottom() {
		if (chatEl) chatEl.scrollTop = chatEl.scrollHeight;
	}

	async function send() {
		if (!input.trim() || streaming) return;
		error = '';
		const userMsg = input.trim();
		input = '';
		messages = [...messages, { role: 'user', content: userMsg }];
		messages = [...messages, { role: 'assistant', content: '' }];
		streaming = true;
		setTimeout(scrollBottom, 10);

		abortCtrl = chatCompletionStream(
			currentOrg ?? '',
			model,
			messages.slice(0, -1),
			(chunk) => {
				const last = messages[messages.length - 1];
				messages = [...messages.slice(0, -1), { ...last, content: last.content + chunk }];
				scrollBottom();
			},
			() => { streaming = false; },
			(err) => { error = err; streaming = false; }
		);
	}

	function stop() {
		abortCtrl?.abort();
		streaming = false;
	}

	function reset() {
		messages = [];
		error = '';
		input = '';
	}

	function handleKeydown(e: KeyboardEvent) {
		if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault();
			send();
		}
	}
</script>

<div class="flex flex-col h-full">
	<!-- Header -->
	<div class="flex items-center justify-between px-6 py-3 border-b border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
		<div class="flex items-center gap-3">
			<h1 class="text-lg font-bold text-zinc-900 dark:text-zinc-100">Playground</h1>
			<select bind:value={model} class="text-sm border border-zinc-300 dark:border-zinc-600 rounded-md px-2 py-1 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100">
				{#each modelOptions as m}
					<option value={m}>{m}</option>
				{/each}
			</select>
		</div>
		<Button variant="ghost" size="sm" onclick={reset}>
			<RotateCcw size={14} />
		</Button>
	</div>

	<!-- Messages -->
	<div bind:this={chatEl} class="flex-1 overflow-y-auto px-6 py-4 space-y-4">
		{#if messages.length === 0}
			<div class="flex flex-col items-center justify-center h-full text-zinc-400 dark:text-zinc-500">
				<Bot size={48} class="mb-3 opacity-30" />
				<p class="text-sm">选择模型，输入消息开始对话</p>
				<p class="text-xs mt-1">支持 Shift+Enter 换行</p>
			</div>
		{:else}
			{#each messages as msg, i}
				<div class="flex gap-3 {msg.role === 'user' ? 'justify-end' : ''}">
					{#if msg.role === 'assistant'}
						<div class="w-7 h-7 rounded-full bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center shrink-0">
							<Bot size={14} class="text-zinc-500" />
						</div>
					{/if}
					<div class="max-w-[80%] rounded-lg px-4 py-2.5 text-sm whitespace-pre-wrap {
						msg.role === 'user'
							? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900'
							: 'bg-zinc-100 dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100'
					}">
						{msg.content || (streaming && i === messages.length - 1 ? '...' : '')}
					</div>
					{#if msg.role === 'user'}
						<div class="w-7 h-7 rounded-full bg-zinc-900 dark:bg-zinc-100 flex items-center justify-center shrink-0">
							<UserIcon size={14} class="text-white dark:text-zinc-900" />
						</div>
					{/if}
				</div>
			{/each}
		{/if}
	</div>

	{#if error}
		<div class="mx-6 mb-2 px-3 py-2 rounded-md bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-xs">
			{error}
		</div>
	{/if}

	<!-- Input -->
	<div class="border-t border-zinc-200 dark:border-zinc-700 px-6 py-3 bg-white dark:bg-zinc-900">
		<div class="flex gap-2 items-end">
			<textarea
				bind:value={input}
				onkeydown={handleKeydown}
				placeholder="输入消息..."
				rows={1}
				class="flex-1 resize-none rounded-lg border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-800 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-400"
			></textarea>
			{#if streaming}
				<Button variant="destructive" size="sm" onclick={stop}>
					<Square size={14} />
				</Button>
			{:else}
				<Button size="sm" onclick={send} disabled={!input.trim()}>
					<Send size={14} />
				</Button>
			{/if}
		</div>
	</div>
</div>
