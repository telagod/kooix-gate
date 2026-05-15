<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { getMe, chatCompletionStream, listModels } from '$lib/api.js';
	import type { MeResult, ModelInfo, ChatParams, ChatMeta, ChatMessage } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';
	import MarkdownRenderer from '$lib/components/ui/MarkdownRenderer.svelte';
	import {
		Send, Square, RotateCcw, Bot, User as UserIcon, Settings2,
		Plus, Trash2, Copy, Check, Pencil, RefreshCw, ChevronLeft,
		MessageSquare, X, Clock, Zap, ImagePlus
	} from 'lucide-svelte';
	import { clsx } from 'clsx';

	interface Msg {
		id: string;
		role: 'system' | 'user' | 'assistant';
		content: string;
		images?: string[];
		meta?: ChatMeta;
		startedAt?: number;
		finishedAt?: number;
	}

	interface Conversation {
		id: string;
		title: string;
		messages: Msg[];
		model: string;
		systemPrompt: string;
		temperature: number;
		topP: number;
		maxTokens: number;
		createdAt: number;
		updatedAt: number;
	}

	let me = $state<MeResult | null>(null);
	let currentOrg = $derived(me?.current_org ?? me?.orgs?.[0] ?? null);
	let availableModels = $state<ModelInfo[]>([]);
	let conversations = $state<Conversation[]>([]);
	let activeId = $state<string | null>(null);
	let active = $derived(conversations.find(c => c.id === activeId) ?? null);
	let streaming = $state(false);
	let abortCtrl = $state<AbortController | null>(null);
	let error = $state('');
	let input = $state('');
	let chatEl: HTMLElement | undefined = $state();
	let showSettings = $state(false);
	let showSidebar = $state(true);
	let editingMsgId = $state<string | null>(null);
	let editContent = $state('');
	let copiedId = $state<string | null>(null);
	let pendingImages = $state<string[]>([]);
	let fileInputEl: HTMLInputElement | undefined = $state();

	const FALLBACK_MODELS = ['gpt-4o-mini', 'gpt-4o', 'claude-sonnet-4-20250514', 'claude-haiku-4-20250414'];
	const STORAGE_KEY = 'kooix_playground_conversations';
	const uid = () => crypto.randomUUID();
	let modelOptions = $derived(availableModels.length > 0 ? availableModels.map(m => m.id) : FALLBACK_MODELS);

	function saveConversations() { try { localStorage.setItem(STORAGE_KEY, JSON.stringify(conversations)); } catch {} }

	function loadConversations() {
		try {
			const raw = localStorage.getItem(STORAGE_KEY);
			if (raw) { conversations = JSON.parse(raw); if (conversations.length > 0 && !activeId) activeId = conversations[0].id; }
		} catch {}
	}

	function newConversation() {
		const conv: Conversation = { id: uid(), title: '新对话', messages: [], model: modelOptions[0] ?? 'gpt-4o-mini', systemPrompt: '', temperature: 0.7, topP: 1.0, maxTokens: 4096, createdAt: Date.now(), updatedAt: Date.now() };
		conversations = [conv, ...conversations]; activeId = conv.id; input = ''; error = ''; saveConversations();
	}

	function deleteConversation(id: string) { conversations = conversations.filter(c => c.id !== id); if (activeId === id) activeId = conversations[0]?.id ?? null; saveConversations(); }
	function switchConversation(id: string) { activeId = id; input = ''; error = ''; editingMsgId = null; }

	onMount(async () => {
		try { me = await getMe(); } catch {}
		try { availableModels = await listModels(); } catch {}
		loadConversations();
		if (conversations.length === 0) newConversation();
	});

	function scrollBottom() { if (chatEl) chatEl.scrollTop = chatEl.scrollHeight; }

	function buildMessages(): ChatMessage[] {
		if (!active) return [];
		const msgs: ChatMessage[] = [];
		if (active.systemPrompt.trim()) msgs.push({ role: 'system', content: active.systemPrompt.trim() });
		for (const m of active.messages) {
			if (m.role === 'system') continue;
			if (m.images && m.images.length > 0) {
				const parts: { type: 'text' | 'image_url'; text?: string; image_url?: { url: string } }[] = [];
				for (const img of m.images) parts.push({ type: 'image_url', image_url: { url: img } });
				if (m.content) parts.push({ type: 'text', text: m.content });
				msgs.push({ role: m.role, content: parts });
			} else {
				msgs.push({ role: m.role, content: m.content });
			}
		}
		return msgs;
	}

	function doStream(params: ChatParams) {
		abortCtrl = chatCompletionStream(currentOrg ?? '', params,
			(chunk) => { if (!active) return; const last = active.messages[active.messages.length - 1]; active.messages = [...active.messages.slice(0, -1), { ...last, content: last.content + chunk }]; scrollBottom(); },
			(meta) => { if (!active) return; const last = active.messages[active.messages.length - 1]; active.messages = [...active.messages.slice(0, -1), { ...last, meta, finishedAt: Date.now() }]; streaming = false; saveConversations(); },
			(err) => { error = err; streaming = false; saveConversations(); }
		);
	}

	async function send() {
		if ((!input.trim() && pendingImages.length === 0) || streaming || !active) return;
		error = '';
		const userMsg: Msg = { id: uid(), role: 'user', content: input.trim(), images: pendingImages.length > 0 ? [...pendingImages] : undefined };
		input = ''; pendingImages = [];
		active.messages = [...active.messages, userMsg, { id: uid(), role: 'assistant', content: '', startedAt: Date.now() }];
		if (active.messages.filter(m => m.role === 'user').length === 1) active.title = userMsg.content.slice(0, 30) + (userMsg.content.length > 30 ? '...' : '');
		active.updatedAt = Date.now(); streaming = true; saveConversations(); await tick(); scrollBottom();
		doStream({ model: active.model, messages: buildMessages().slice(0, -1), temperature: active.temperature, top_p: active.topP, max_tokens: active.maxTokens });
	}

	function stop() { abortCtrl?.abort(); streaming = false; saveConversations(); }

	function regenerate() {
		if (!active || streaming || active.messages.length < 2) return;
		if (active.messages[active.messages.length - 1].role !== 'assistant') return;
		active.messages = [...active.messages.slice(0, -1), { id: uid(), role: 'assistant', content: '', startedAt: Date.now() }];
		streaming = true; error = ''; saveConversations(); tick().then(scrollBottom);
		doStream({ model: active.model, messages: buildMessages().slice(0, -1), temperature: active.temperature, top_p: active.topP, max_tokens: active.maxTokens });
	}

	function startEdit(msg: Msg) { editingMsgId = msg.id; editContent = msg.content; }
	function cancelEdit() { editingMsgId = null; editContent = ''; }
	function applyEdit(msgId: string) { if (!active) return; const idx = active.messages.findIndex(m => m.id === msgId); if (idx === -1) return; active.messages = [...active.messages.slice(0, idx), { ...active.messages[idx], content: editContent }]; active.updatedAt = Date.now(); editingMsgId = null; editContent = ''; saveConversations(); }
	function deleteMessage(msgId: string) { if (!active) return; active.messages = active.messages.filter(m => m.id !== msgId); active.updatedAt = Date.now(); saveConversations(); }
	function copyContent(msg: Msg) { navigator.clipboard.writeText(msg.content); copiedId = msg.id; setTimeout(() => { copiedId = null; }, 2000); }
	function handleKeydown(e: KeyboardEvent) { if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); } }
	function formatLatency(msg: Msg): string { if (!msg.startedAt || !msg.finishedAt) return ''; const ms = msg.finishedAt - msg.startedAt; return ms < 1000 ? `${ms}ms` : `${(ms / 1000).toFixed(1)}s`; }
	function resetConversation() { if (!active) return; active.messages = []; active.updatedAt = Date.now(); error = ''; pendingImages = []; saveConversations(); }
	function formatDate(ts: number): string { return new Date(ts).toLocaleString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' }); }

	function fileToBase64(file: File): Promise<string> { return new Promise((resolve, reject) => { const r = new FileReader(); r.onload = () => resolve(r.result as string); r.onerror = reject; r.readAsDataURL(file); }); }
	async function handleFiles(files: FileList | File[]) { for (const f of files) { if (!f.type.startsWith('image/')) continue; if (f.size > 20 * 1024 * 1024) { error = `图片 ${f.name} 超过 20MB`; continue; } pendingImages = [...pendingImages, await fileToBase64(f)]; } }
	function handlePaste(e: ClipboardEvent) { const items = e.clipboardData?.items; if (!items) return; const imgs: File[] = []; for (const it of items) { if (it.type.startsWith('image/')) { const f = it.getAsFile(); if (f) imgs.push(f); } } if (imgs.length > 0) { e.preventDefault(); handleFiles(imgs); } }
	function handleDrop(e: DragEvent) { e.preventDefault(); if (e.dataTransfer?.files) handleFiles(e.dataTransfer.files); }
	function handleDragOver(e: DragEvent) { e.preventDefault(); }
	function removePendingImage(idx: number) { pendingImages = pendingImages.filter((_, i) => i !== idx); }

	let pageNum = $derived(0);
</script>

<div class="flex h-full overflow-hidden">
	<!-- Sidebar -->
	{#if showSidebar}
		<div class="w-60 shrink-0 border-r border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 flex flex-col">
			<div class="p-3 border-b border-zinc-200 dark:border-zinc-700">
				<Button variant="default" size="sm" class="w-full" onclick={newConversation}>
					<Plus size={14} />
					<span class="ml-1.5">新对话</span>
				</Button>
			</div>
			<div class="flex-1 overflow-y-auto">
				{#each conversations as conv}
					<div role="button" tabindex="0" onclick={() => switchConversation(conv.id)} onkeydown={(e) => { if (e.key === 'Enter') switchConversation(conv.id); }}
						class={clsx('w-full text-left px-3 py-2.5 text-sm transition-colors group flex items-center justify-between cursor-pointer',
							conv.id === activeId ? 'bg-zinc-200 dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100' : 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800/50')}>
						<div class="flex-1 min-w-0">
							<p class="truncate text-sm font-medium">{conv.title}</p>
							<p class="text-[10px] text-zinc-400 dark:text-zinc-500 mt-0.5">{formatDate(conv.updatedAt)} · {conv.model.split('/').pop()}</p>
						</div>
						<button onclick={(e) => { e.stopPropagation(); deleteConversation(conv.id); }} class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-zinc-300 dark:hover:bg-zinc-700 transition-all shrink-0">
							<Trash2 size={12} class="text-zinc-400" />
						</button>
					</div>
				{/each}
			</div>
		</div>
	{/if}

	<!-- Main -->
	<div class="flex-1 flex flex-col min-w-0">
		<!-- Header -->
		<div class="flex items-center justify-between px-3 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
			<div class="flex items-center gap-2">
				<button onclick={() => showSidebar = !showSidebar} class="p-1.5 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
					{#if showSidebar}<ChevronLeft size={16} class="text-zinc-500" />{:else}<MessageSquare size={16} class="text-zinc-500" />{/if}
				</button>
				{#if active}
					<select bind:value={active.model} onchange={() => { if (active) { active.updatedAt = Date.now(); saveConversations(); }}}
						class="text-xs border border-zinc-200 dark:border-zinc-700 rounded-lg px-2 py-1.5 bg-white dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-400 max-w-[220px]">
						{#each modelOptions as m}<option value={m}>{m}</option>{/each}
					</select>
				{/if}
			</div>
			<div class="flex items-center gap-1">
				<button onclick={() => showSettings = !showSettings}
					class={clsx('p-1.5 rounded-lg transition-colors', showSettings ? 'bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900' : 'hover:bg-zinc-100 dark:hover:bg-zinc-800 text-zinc-500')}>
					<Settings2 size={14} />
				</button>
				<button onclick={resetConversation} class="p-1.5 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-800 text-zinc-500 transition-colors">
					<RotateCcw size={14} />
				</button>
			</div>
		</div>

		<div class="flex flex-1 overflow-hidden">
			<!-- Messages -->
			<div class="flex-1 flex flex-col min-w-0">
				<div bind:this={chatEl} class="flex-1 overflow-y-auto px-4 py-4">
					{#if active && active.messages.length === 0}
						<div class="flex flex-col items-center justify-center h-full text-zinc-400 dark:text-zinc-500">
							<Bot size={48} class="mb-4 opacity-20" />
							<p class="text-sm font-medium text-zinc-500 dark:text-zinc-400">选择模型，输入消息开始对话</p>
							<p class="text-xs mt-1.5 text-zinc-400 dark:text-zinc-500">Shift+Enter 换行 · Enter 发送 · 粘贴/拖拽图片</p>
						</div>
					{:else if active}
						<div class="max-w-3xl mx-auto space-y-1">
							{#each active.messages as msg, i (msg.id)}
								{#if msg.role === 'user'}
									<div class="flex gap-3 justify-end group py-2">
										<div class="flex flex-col items-end max-w-[85%]">
											{#if editingMsgId === msg.id}
												<div class="w-full min-w-[300px]">
													<textarea bind:value={editContent} rows={3} class="w-full rounded-lg border border-zinc-300 dark:border-zinc-600 bg-white dark:bg-zinc-800 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-400 resize-none"></textarea>
													<div class="flex gap-1.5 mt-1.5 justify-end">
														<Button variant="outline" size="sm" onclick={cancelEdit}>取消</Button>
														<Button variant="default" size="sm" onclick={() => applyEdit(msg.id)}>保存</Button>
													</div>
												</div>
											{:else}
												{#if msg.images && msg.images.length > 0}
													<div class="flex flex-wrap gap-1.5 mb-1.5 justify-end">
														{#each msg.images as img}<img src={img} alt="" class="max-w-[200px] max-h-[150px] rounded-lg object-cover border border-zinc-700" />{/each}
													</div>
												{/if}
												{#if msg.content}
													<div class="rounded-2xl rounded-br-md px-4 py-2.5 text-sm bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 whitespace-pre-wrap break-words">{msg.content}</div>
												{/if}
												<div class="flex gap-0.5 mt-1 opacity-0 group-hover:opacity-100 transition-opacity">
													<button onclick={() => startEdit(msg)} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-800" title="编辑"><Pencil size={12} class="text-zinc-400" /></button>
													<button onclick={() => copyContent(msg)} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-800" title="复制">{#if copiedId === msg.id}<Check size={12} class="text-emerald-500" />{:else}<Copy size={12} class="text-zinc-400" />{/if}</button>
													<button onclick={() => deleteMessage(msg.id)} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-800" title="删除"><Trash2 size={12} class="text-zinc-400" /></button>
												</div>
											{/if}
										</div>
										<div class="w-7 h-7 rounded-full bg-zinc-900 dark:bg-zinc-200 flex items-center justify-center shrink-0 mt-1"><UserIcon size={13} class="text-white dark:text-zinc-800" /></div>
									</div>
								{:else if msg.role === 'assistant'}
									<div class="flex gap-3 group py-2">
										<div class="w-7 h-7 rounded-full bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center shrink-0 mt-1 ring-1 ring-zinc-200 dark:ring-zinc-700"><Bot size={13} class="text-zinc-500" /></div>
										<div class="flex flex-col max-w-[85%] min-w-0">
											<div class="text-sm text-zinc-900 dark:text-zinc-100">
												{#if msg.content}<MarkdownRenderer content={msg.content} streaming={streaming && i === active.messages.length - 1} />{:else if streaming && i === active.messages.length - 1}<span class="inline-block w-2 h-4 bg-zinc-400 dark:bg-zinc-500 animate-pulse rounded-sm"></span>{/if}
											</div>
											<div class="flex items-center gap-3 mt-1.5 text-[11px] text-zinc-400 dark:text-zinc-500">
												{#if msg.meta?.usage}<span class="inline-flex items-center gap-0.5" title="Token"><Zap size={10} />{msg.meta.usage.prompt_tokens}/{msg.meta.usage.completion_tokens}</span>{/if}
												{#if msg.finishedAt && msg.startedAt}<span class="inline-flex items-center gap-0.5" title="耗时"><Clock size={10} />{formatLatency(msg)}</span>{/if}
												{#if msg.meta?.model}<span class="truncate max-w-[150px]" title={msg.meta.model}>{msg.meta.model}</span>{/if}
												<div class="flex gap-0.5 opacity-0 group-hover:opacity-100 transition-opacity">
													<button onclick={() => copyContent(msg)} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-800" title="复制">{#if copiedId === msg.id}<Check size={12} class="text-emerald-500" />{:else}<Copy size={12} class="text-zinc-400" />{/if}</button>
													{#if i === active.messages.length - 1 && !streaming}<button onclick={regenerate} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-800" title="重新生成"><RefreshCw size={12} class="text-zinc-400" /></button>{/if}
													<button onclick={() => deleteMessage(msg.id)} class="p-1 rounded hover:bg-zinc-100 dark:hover:bg-zinc-800" title="删除"><Trash2 size={12} class="text-zinc-400" /></button>
												</div>
											</div>
										</div>
									</div>
								{/if}
							{/each}
						</div>
					{/if}
				</div>

				{#if error}
					<div class="mx-4 mb-2 px-3 py-2 rounded-lg bg-red-50 dark:bg-red-900/20 text-red-600 dark:text-red-400 text-xs flex items-center gap-2"><X size={12} />{error}</div>
				{/if}

				<!-- Input -->
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div class="border-t border-zinc-200 dark:border-zinc-700 px-4 py-3 bg-white dark:bg-zinc-900" ondrop={handleDrop} ondragover={handleDragOver}>
					<div class="max-w-3xl mx-auto">
						{#if pendingImages.length > 0}
							<div class="flex flex-wrap gap-2 mb-2">
								{#each pendingImages as img, idx}
									<div class="relative group w-16 h-16 rounded-lg overflow-hidden border border-zinc-200 dark:border-zinc-700">
										<img src={img} alt="" class="w-full h-full object-cover" />
										<button onclick={() => removePendingImage(idx)} class="absolute top-0 right-0 p-0.5 bg-zinc-900/70 text-white rounded-bl-md opacity-0 group-hover:opacity-100 transition-opacity"><X size={10} /></button>
									</div>
								{/each}
							</div>
						{/if}
						<div class="flex gap-2 items-end">
							<button onclick={() => fileInputEl?.click()} class="shrink-0 p-2.5 rounded-xl text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors" title="上传图片"><ImagePlus size={18} /></button>
							<input bind:this={fileInputEl} type="file" accept="image/*" multiple class="hidden" onchange={(e: Event) => { const t = e.target as HTMLInputElement; if (t.files) handleFiles(t.files); t.value = ''; }} />
							<textarea bind:value={input} onkeydown={handleKeydown} onpaste={handlePaste}
								placeholder={pendingImages.length > 0 ? '添加描述...' : '输入消息... (Shift+Enter 换行)'}
								rows={1} class="flex-1 resize-none rounded-xl border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800 px-4 py-2.5 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-400 min-h-[40px] max-h-[200px]"
								oninput={(e: Event) => { const t = e.target as HTMLTextAreaElement; t.style.height = 'auto'; t.style.height = Math.min(t.scrollHeight, 200) + 'px'; }}></textarea>
							{#if streaming}
								<Button variant="destructive" size="sm" onclick={stop} class="rounded-xl h-10 w-10 p-0"><Square size={14} /></Button>
							{:else}
								<Button size="sm" onclick={send} disabled={!input.trim() && pendingImages.length === 0} class="rounded-xl h-10 w-10 p-0"><Send size={14} /></Button>
							{/if}
						</div>
					</div>
				</div>
			</div>

			<!-- Settings Panel -->
			{#if showSettings && active}
				<div class="w-64 shrink-0 border-l border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 overflow-y-auto">
					<div class="p-4 space-y-5">
						<div>
							<h3 class="text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider mb-3">参数设置</h3>
							<div class="mb-4">
								<label class="block text-xs text-zinc-500 dark:text-zinc-400 mb-1.5">System Prompt</label>
								<textarea bind:value={active.systemPrompt} onchange={() => { if (active) { active.updatedAt = Date.now(); saveConversations(); }}} placeholder="你是一个有用的助手..." rows={4}
									class="w-full rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 px-3 py-2 text-sm text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 focus:outline-none focus:ring-2 focus:ring-zinc-400 resize-none"></textarea>
							</div>
							<div class="mb-4">
								<div class="flex justify-between text-xs mb-1.5"><label class="text-zinc-500 dark:text-zinc-400">Temperature</label><span class="font-mono text-zinc-700 dark:text-zinc-300">{active.temperature.toFixed(1)}</span></div>
								<input type="range" min="0" max="2" step="0.1" bind:value={active.temperature} onchange={() => { if (active) { active.updatedAt = Date.now(); saveConversations(); }}} class="w-full accent-zinc-900 dark:accent-zinc-100" />
								<div class="flex justify-between text-[10px] text-zinc-400 mt-0.5"><span>精确</span><span>创意</span></div>
							</div>
							<div class="mb-4">
								<div class="flex justify-between text-xs mb-1.5"><label class="text-zinc-500 dark:text-zinc-400">Top P</label><span class="font-mono text-zinc-700 dark:text-zinc-300">{active.topP.toFixed(1)}</span></div>
								<input type="range" min="0" max="1" step="0.05" bind:value={active.topP} onchange={() => { if (active) { active.updatedAt = Date.now(); saveConversations(); }}} class="w-full accent-zinc-900 dark:accent-zinc-100" />
							</div>
							<div class="mb-4">
								<div class="flex justify-between text-xs mb-1.5"><label class="text-zinc-500 dark:text-zinc-400">Max Tokens</label><span class="font-mono text-zinc-700 dark:text-zinc-300">{active.maxTokens}</span></div>
								<input type="number" min="1" max="128000" step="256" bind:value={active.maxTokens} onchange={() => { if (active) { active.updatedAt = Date.now(); saveConversations(); }}}
									class="w-full h-9 px-3 rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800 text-sm text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-2 focus:ring-zinc-400" />
							</div>
						</div>
						{#if active.messages.length > 0}
							<div>
								<h3 class="text-xs font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider mb-3">对话信息</h3>
								<div class="space-y-2 text-xs">
									<div class="flex justify-between"><span class="text-zinc-400">消息数</span><span class="text-zinc-700 dark:text-zinc-300">{active.messages.length}</span></div>
									<div class="flex justify-between"><span class="text-zinc-400">创建时间</span><span class="text-zinc-700 dark:text-zinc-300">{formatDate(active.createdAt)}</span></div>
									{#if active.messages.filter(m => m.meta?.usage).length > 0}
										{@const totalIn = active.messages.reduce((s, m) => s + (m.meta?.usage?.prompt_tokens ?? 0), 0)}
										{@const totalOut = active.messages.reduce((s, m) => s + (m.meta?.usage?.completion_tokens ?? 0), 0)}
										<div class="flex justify-between"><span class="text-zinc-400">总 Tokens</span><span class="font-mono text-zinc-700 dark:text-zinc-300">{totalIn + totalOut}</span></div>
										<div class="flex justify-between"><span class="text-zinc-400">输入/输出</span><span class="font-mono text-zinc-700 dark:text-zinc-300">{totalIn}/{totalOut}</span></div>
									{/if}
								</div>
							</div>
						{/if}
					</div>
				</div>
			{/if}
		</div>
	</div>
</div>
