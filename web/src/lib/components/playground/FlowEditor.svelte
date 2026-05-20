<script lang="ts">
	import { onMount } from 'svelte';
	import {
		SvelteFlow,
		Controls,
		MiniMap,
		Background,
		BackgroundVariant,
		type Connection,
	} from '@xyflow/svelte';
	import '@xyflow/svelte/dist/style.css';

	import { getMe } from '$lib/api.js';
	import type { MeResult } from '$lib/api.js';
	import type { FlowNode, FlowEdge, FlowNodeData, FlowNodeKind, Workflow } from '$lib/flow/types.js';
	import { NODE_CATALOG, PORT_COLORS } from '$lib/flow/types.js';
	import { canConnect, executeFlow } from '$lib/flow/engine.js';
	import { loadWorkflows, saveWorkflows, loadActiveId, saveActiveId, createWorkflow } from '$lib/flow/storage.js';
	import { isDark } from '$lib/stores/theme';
	import { clsx } from 'clsx';
	import {
		Play, Square, Plus, Trash2, ChevronDown, Type,
		ImagePlus, Upload, Bot, Palette, Volume2, Mic, Eye,
		RotateCcw, Workflow as WorkflowIcon, Sparkles, ArrowRight,
	} from 'lucide-svelte';

	import TextInputNode from '$lib/components/flow/nodes/TextInputNode.svelte';
	import ImageUploadNode from '$lib/components/flow/nodes/ImageUploadNode.svelte';
	import AudioUploadNode from '$lib/components/flow/nodes/AudioUploadNode.svelte';
	import LLMChatNode from '$lib/components/flow/nodes/LLMChatNode.svelte';
	import ImageGenNode from '$lib/components/flow/nodes/ImageGenNode.svelte';
	import TTSNode from '$lib/components/flow/nodes/TTSNode.svelte';
	import STTNode from '$lib/components/flow/nodes/STTNode.svelte';
	import PreviewNode from '$lib/components/flow/nodes/PreviewNode.svelte';

	const nodeTypes = {
		textInput: TextInputNode,
		imageUpload: ImageUploadNode,
		audioUpload: AudioUploadNode,
		llmChat: LLMChatNode,
		imageGen: ImageGenNode,
		tts: TTSNode,
		stt: STTNode,
		preview: PreviewNode,
	};

	let me = $state<MeResult | null>(null);
	let currentOrg = $derived(me?.current_org ?? me?.orgs?.[0] ?? null);

	let workflows = $state<Workflow[]>([]);
	let activeId = $state<string | null>(null);
	let active = $derived(workflows.find((w) => w.id === activeId) ?? null);

	let nodes = $state<FlowNode[]>([]);
	let edges = $state<FlowEdge[]>([]);

	let running = $state(false);
	let abortCtrl = $state<AbortController | null>(null);
	let showNodeMenu = $state(false);
	let nodeMenuPos = $state({ x: 0, y: 0 });
	let showWorkflowPicker = $state(false);
	let colorMode = $derived<'light' | 'dark'>($isDark ? 'dark' : 'light');

	const nodeIcons: Record<FlowNodeKind, any> = {
		textInput: Type,
		imageUpload: ImagePlus,
		audioUpload: Upload,
		llmChat: Bot,
		imageGen: Palette,
		tts: Volume2,
		stt: Mic,
		preview: Eye,
	};

	const nodeCategories = [
		{ label: '输入', kinds: ['textInput', 'imageUpload', 'audioUpload'] as FlowNodeKind[] },
		{ label: 'AI', kinds: ['llmChat', 'imageGen', 'tts', 'stt'] as FlowNodeKind[] },
		{ label: '输出', kinds: ['preview'] as FlowNodeKind[] },
	];

	const templates = [
		{ name: '文本对话', desc: '文本 → LLM → 预览', nodes: ['textInput', 'llmChat', 'preview'] as FlowNodeKind[] },
		{ name: '文生图', desc: '文本 → 图片生成 → 预览', nodes: ['textInput', 'imageGen', 'preview'] as FlowNodeKind[] },
		{ name: '语音转文字', desc: '音频 → STT → 预览', nodes: ['audioUpload', 'stt', 'preview'] as FlowNodeKind[] },
		{ name: '文字转语音', desc: '文本 → TTS → 预览', nodes: ['textInput', 'tts', 'preview'] as FlowNodeKind[] },
	];

	onMount(async () => {
		try { me = await getMe(); } catch {}
		workflows = loadWorkflows();
		const savedId = loadActiveId();
		if (savedId && workflows.find((w) => w.id === savedId)) {
			activeId = savedId;
		} else if (workflows.length > 0) {
			activeId = workflows[0].id;
		} else {
			const w = createWorkflow();
			workflows = [w];
			activeId = w.id;
		}
		syncFromActive();
	});

	function syncFromActive() {
		if (active) {
			nodes = structuredClone(active.nodes);
			edges = structuredClone(active.edges);
		}
	}

	function syncToActive() {
		if (!active) return;
		const idx = workflows.findIndex((w) => w.id === activeId);
		if (idx === -1) return;
		workflows[idx] = { ...workflows[idx], nodes: structuredClone(nodes), edges: structuredClone(edges), updatedAt: Date.now() };
		saveWorkflows(workflows);
		saveActiveId(activeId);
	}

	function addNode(kind: FlowNodeKind, x?: number, y?: number) {
		const newNode: FlowNode = {
			id: crypto.randomUUID(),
			type: kind,
			position: { x: x ?? 300 + Math.random() * 100, y: y ?? 150 + Math.random() * 100 },
			data: { kind, status: 'idle', params: {}, output: undefined, error: undefined },
		};
		nodes = [...nodes, newNode];
		showNodeMenu = false;
		syncToActive();
	}

	function applyTemplate(tpl: typeof templates[0]) {
		const spacing = 320;
		const startX = 80;
		const startY = 120;
		const newNodes: FlowNode[] = tpl.nodes.map((kind, i) => ({
			id: crypto.randomUUID(),
			type: kind,
			position: { x: startX + i * spacing, y: startY },
			data: { kind, status: 'idle' as const, params: {}, output: undefined, error: undefined },
		}));
		const newEdges: FlowEdge[] = [];
		for (let i = 0; i < newNodes.length - 1; i++) {
			const srcMeta = NODE_CATALOG[tpl.nodes[i]];
			const tgtMeta = NODE_CATALOG[tpl.nodes[i + 1]];
			if (srcMeta.outputs.length > 0 && tgtMeta.inputs.length > 0) {
				const srcPort = srcMeta.outputs[0];
				const tgtPort = tgtMeta.inputs.find(p => p.type === srcPort.type);
				if (tgtPort) {
					newEdges.push({
						id: `${newNodes[i].id}-${srcPort.id}-${newNodes[i + 1].id}-${tgtPort.id}`,
						source: newNodes[i].id,
						target: newNodes[i + 1].id,
						sourceHandle: srcPort.id,
						targetHandle: tgtPort.id,
						animated: true,
						style: `stroke: ${PORT_COLORS[srcPort.type]}`,
					});
				}
			}
		}
		nodes = newNodes;
		edges = newEdges;
		syncToActive();
	}

	function switchWorkflow(id: string) {
		syncToActive();
		activeId = id;
		saveActiveId(id);
		syncFromActive();
		showWorkflowPicker = false;
	}

	function newWorkflow() {
		syncToActive();
		const w = createWorkflow();
		workflows = [w, ...workflows];
		activeId = w.id;
		saveWorkflows(workflows);
		saveActiveId(w.id);
		syncFromActive();
		showWorkflowPicker = false;
	}

	function deleteWorkflow(id: string) {
		workflows = workflows.filter((w) => w.id !== id);
		if (activeId === id) {
			activeId = workflows[0]?.id ?? null;
			if (!activeId) { const w = createWorkflow(); workflows = [w]; activeId = w.id; }
		}
		saveWorkflows(workflows);
		saveActiveId(activeId);
		syncFromActive();
	}

	function clearCanvas() {
		nodes = [];
		edges = [];
		syncToActive();
	}

	const isValidConnection = (connection: Connection | FlowEdge) => {
		const src = nodes.find((n) => n.id === connection.source);
		const tgt = nodes.find((n) => n.id === connection.target);
		if (!src || !tgt || !connection.sourceHandle || !connection.targetHandle) return false;
		return canConnect(src, connection.sourceHandle, tgt, connection.targetHandle);
	};

	function onConnect(conn: Connection) {
		const newEdge: FlowEdge = {
			id: `${conn.source}-${conn.sourceHandle}-${conn.target}-${conn.targetHandle}`,
			source: conn.source!,
			target: conn.target!,
			sourceHandle: conn.sourceHandle,
			targetHandle: conn.targetHandle,
			animated: true,
			style: `stroke: ${getEdgeColor(conn.sourceHandle)}`,
		};
		edges = edges.filter(
			(e) => !(e.target === conn.target && e.targetHandle === conn.targetHandle)
		);
		edges = [...edges, newEdge];
		syncToActive();
	}

	function getEdgeColor(handleId: string | null | undefined): string {
		if (!handleId) return PORT_COLORS.text;
		if (handleId === 'image') return PORT_COLORS.image;
		if (handleId === 'audio') return PORT_COLORS.audio;
		return PORT_COLORS.text;
	}

	function onNodesChange() { syncToActive(); }

	function onNodeContextMenu({ event, node }: { event: MouseEvent; node: FlowNode }) {
		event.preventDefault();
		const nodeId = node.id;
		nodes = nodes.filter((n) => n.id !== nodeId);
		edges = edges.filter((e) => e.source !== nodeId && e.target !== nodeId);
		syncToActive();
	}

	function onPaneContextMenu({ event }: { event: MouseEvent }) {
		event.preventDefault();
		nodeMenuPos = { x: event.clientX, y: event.clientY };
		showNodeMenu = true;
	}

	function onDragStart(e: DragEvent, kind: FlowNodeKind) {
		e.dataTransfer?.setData('application/x-flow-node', kind);
		e.dataTransfer!.effectAllowed = 'move';
	}

	function onCanvasDrop(e: DragEvent) {
		e.preventDefault();
		const kind = e.dataTransfer?.getData('application/x-flow-node') as FlowNodeKind | undefined;
		if (!kind) return;
		const bounds = (e.currentTarget as HTMLElement).getBoundingClientRect();
		addNode(kind, e.clientX - bounds.left - 110, e.clientY - bounds.top - 30);
	}

	function onCanvasDragOver(e: DragEvent) {
		e.preventDefault();
		e.dataTransfer!.dropEffect = 'move';
	}

	async function runFlow() {
		if (running || nodes.length === 0) return;
		running = true;
		const ctrl = new AbortController();
		abortCtrl = ctrl;

		nodes = nodes.map((n) => ({ ...n, data: { ...n.data, status: 'idle' as const, output: undefined, error: undefined } }));

		try {
			await executeFlow(nodes, edges, currentOrg ?? '', {
				onNodeStart: (id) => {
					nodes = nodes.map((n) => n.id === id ? { ...n, data: { ...n.data, status: 'running' as const } } : n);
				},
				onNodeDone: (id, output) => {
					nodes = nodes.map((n) => n.id === id ? { ...n, data: { ...n.data, status: 'done' as const, output } } : n);
				},
				onNodeError: (id, error) => {
					nodes = nodes.map((n) => n.id === id ? { ...n, data: { ...n.data, status: 'error' as const, error } } : n);
				},
				onNodeStream: (id, chunk) => {
					nodes = nodes.map((n) => n.id === id ? { ...n, data: { ...n.data, output: chunk } } : n);
				},
			}, ctrl.signal);
		} catch (err: any) {
			if (err?.message !== '已取消') console.error('Flow execution error:', err);
		} finally {
			running = false;
			abortCtrl = null;
			syncToActive();
		}
	}

	function stopFlow() {
		abortCtrl?.abort();
		running = false;
	}

	function formatDate(ts: number): string {
		return new Date(ts).toLocaleString('zh-CN', { month: 'short', day: 'numeric', hour: '2-digit', minute: '2-digit' });
	}
</script>

<svelte:window onclick={() => { if (showNodeMenu) showNodeMenu = false; if (showWorkflowPicker) showWorkflowPicker = false; }} />

<div class="flex h-full overflow-hidden">
	<!-- Node Palette (left) -->
	<div class="w-48 shrink-0 border-r border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900 flex flex-col">
		<div class="px-3 py-2.5 border-b border-zinc-100 dark:border-zinc-800">
			<p class="text-[11px] font-semibold text-zinc-500 dark:text-zinc-400 uppercase tracking-wider">节点</p>
		</div>
		<div class="flex-1 overflow-y-auto py-2 px-2 space-y-3">
			{#each nodeCategories as cat}
				<div>
					<p class="px-2 mb-1.5 text-[10px] font-medium text-zinc-400 dark:text-zinc-500 uppercase tracking-wider">{cat.label}</p>
					<div class="space-y-0.5">
							{#each cat.kinds as kind}
								{@const meta = NODE_CATALOG[kind]}
								{@const Icon = nodeIcons[kind]}
								<button
									type="button"
									draggable="true"
									ondragstart={(e) => onDragStart(e, kind)}
									onclick={() => addNode(kind)}
									class="flex w-full items-center gap-2 px-2 py-1.5 rounded-lg text-left text-xs cursor-grab active:cursor-grabbing text-zinc-700 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors select-none"
								>
									<div class="w-5 h-5 rounded flex items-center justify-center shrink-0" style:background-color="{meta.color}15">
										<Icon size={12} style="color: {meta.color}" />
									</div>
									<span class="truncate">{meta.label}</span>
								</button>
							{/each}
						</div>
				</div>
			{/each}
		</div>
		<div class="px-2 py-2 border-t border-zinc-100 dark:border-zinc-800">
			<p class="text-[10px] text-zinc-400 dark:text-zinc-500 text-center">拖拽到画布 或 点击添加</p>
		</div>
	</div>

	<!-- Main area -->
	<div class="flex-1 flex flex-col min-w-0">
		<!-- Toolbar -->
		<div class="flex items-center justify-between px-3 py-2 border-b border-zinc-200 dark:border-zinc-800 bg-white dark:bg-zinc-900">
			<div class="flex items-center gap-2">
				<!-- Workflow picker -->
				<div class="relative">
					<button onclick={(e) => { e.stopPropagation(); showWorkflowPicker = !showWorkflowPicker; }}
						class="inline-flex items-center gap-1.5 px-2.5 py-1.5 rounded-lg text-sm font-medium text-zinc-900 dark:text-zinc-100 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
						<WorkflowIcon size={14} class="text-zinc-400" />
						{#if active}
							<input
								type="text"
								bind:value={active.name}
								onblur={syncToActive}
								onclick={(e) => e.stopPropagation()}
								class="bg-transparent border-none text-sm font-medium text-zinc-900 dark:text-zinc-100 focus:outline-none w-32"
							/>
						{/if}
						<ChevronDown size={12} class="text-zinc-400" />
					</button>
					{#if showWorkflowPicker}
						<div class="absolute top-full left-0 mt-1 w-64 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-xl overflow-hidden z-50">
							<div class="p-2 border-b border-zinc-100 dark:border-zinc-800">
								<button onclick={newWorkflow}
									class="w-full flex items-center gap-2 px-2.5 py-2 rounded-lg text-xs font-medium text-zinc-600 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
									<Plus size={12} /> 新建工作流
								</button>
							</div>
							<div class="max-h-[240px] overflow-y-auto p-1">
								{#each workflows as w}
									<div class="flex items-center group">
										<button onclick={() => switchWorkflow(w.id)}
											class={clsx('flex-1 text-left px-2.5 py-2 rounded-lg text-xs transition-colors',
												w.id === activeId ? 'bg-zinc-100 dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 font-medium' : 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-50 dark:hover:bg-zinc-800/50')}>
											<p class="truncate">{w.name}</p>
											<p class="text-[10px] text-zinc-400 dark:text-zinc-500 mt-0.5">{formatDate(w.updatedAt)} · {w.nodes.length} 节点</p>
										</button>
										<button onclick={(e) => { e.stopPropagation(); deleteWorkflow(w.id); }}
											class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-zinc-200 dark:hover:bg-zinc-700 mr-1 transition-all">
											<Trash2 size={10} class="text-zinc-400" />
										</button>
									</div>
								{/each}
							</div>
						</div>
					{/if}
				</div>
			</div>
			<div class="flex items-center gap-1.5">
				<button onclick={clearCanvas}
					class="p-1.5 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-800 text-zinc-400 hover:text-zinc-600 dark:hover:text-zinc-300 transition-colors" title="清空画布">
					<RotateCcw size={14} />
				</button>
				<div class="w-px h-5 bg-zinc-200 dark:bg-zinc-700"></div>
				{#if running}
					<button onclick={stopFlow}
						class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-red-500 text-white hover:bg-red-600 transition-colors">
						<Square size={12} /> 停止
					</button>
				{:else}
					<button onclick={runFlow} disabled={nodes.length === 0}
						class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 hover:bg-zinc-800 dark:hover:bg-zinc-200 transition-colors disabled:opacity-40 disabled:cursor-not-allowed">
						<Play size={12} /> 运行
					</button>
				{/if}
			</div>
		</div>

		<!-- Canvas -->
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="flex-1 relative" ondrop={onCanvasDrop} ondragover={onCanvasDragOver}>
			{#if nodes.length === 0}
				<!-- Empty state overlay -->
				<div class="absolute inset-0 z-10 flex items-center justify-center pointer-events-none">
					<div class="pointer-events-auto max-w-md w-full px-6">
						<div class="text-center mb-6">
							<div class="w-12 h-12 rounded-xl bg-zinc-100 dark:bg-zinc-800 flex items-center justify-center mx-auto mb-3">
								<Sparkles size={24} class="text-zinc-400 dark:text-zinc-500" />
							</div>
							<h3 class="text-sm font-semibold text-zinc-700 dark:text-zinc-300 mb-1">开始构建工作流</h3>
							<p class="text-xs text-zinc-500 dark:text-zinc-400">从左侧拖拽节点到画布，或选择一个模板快速开始</p>
						</div>
						<div class="grid grid-cols-2 gap-2">
							{#each templates as tpl}
								<button onclick={() => applyTemplate(tpl)}
									class="text-left p-3 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800/50 hover:border-zinc-300 dark:hover:border-zinc-600 hover:shadow-sm transition-all group">
									<p class="text-xs font-medium text-zinc-700 dark:text-zinc-300 mb-0.5">{tpl.name}</p>
									<p class="text-[10px] text-zinc-400 dark:text-zinc-500 flex items-center gap-1">
										{#each tpl.desc.split(' → ') as step, i}
											{#if i > 0}<ArrowRight size={8} class="shrink-0" />{/if}
											<span>{step}</span>
										{/each}
									</p>
								</button>
							{/each}
						</div>
					</div>
				</div>
			{/if}

			<SvelteFlow
				bind:nodes
				bind:edges
				{nodeTypes}
				{isValidConnection}
				{colorMode}
				onconnect={onConnect}
				onnodecontextmenu={onNodeContextMenu}
				onpanecontextmenu={onPaneContextMenu}
				onnodedragstop={onNodesChange}
				fitView
				snapGrid={[20, 20]}
				defaultEdgeOptions={{ animated: true, type: 'smoothstep' }}
				proOptions={{ hideAttribution: true }}
			>
				<Background variant={BackgroundVariant.Dots} gap={20} size={1} />
				<Controls position="bottom-left" />
				<MiniMap position="bottom-right" pannable zoomable />
			</SvelteFlow>

			<!-- Right-click context menu -->
				{#if showNodeMenu}
					<!-- svelte-ignore a11y_click_events_have_key_events -->
					<div
						class="fixed z-50 w-48 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-xl overflow-hidden"
					style="left: {nodeMenuPos.x}px; top: {nodeMenuPos.y}px;"
					onclick={(e) => e.stopPropagation()}
					role="menu" tabindex="-1"
				>
					{#each nodeCategories as cat}
						<div class="px-3 py-1 text-[10px] font-semibold text-zinc-400 dark:text-zinc-500 uppercase tracking-wider bg-zinc-50 dark:bg-zinc-800/50">
							{cat.label}
						</div>
							{#each cat.kinds as kind}
								{@const meta = NODE_CATALOG[kind]}
								{@const Icon = nodeIcons[kind]}
								<button onclick={() => addNode(kind, nodeMenuPos.x - 110, nodeMenuPos.y - 30)}
									class="w-full flex items-center gap-2 px-3 py-1.5 text-xs text-zinc-700 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
									role="menuitem">
									<div class="w-4 h-4 rounded flex items-center justify-center" style:background-color="{meta.color}15">
										<Icon size={10} style="color: {meta.color}" />
									</div>
								{meta.label}
							</button>
						{/each}
					{/each}
				</div>
			{/if}
		</div>
	</div>
</div>

<style>
	:global(.svelte-flow) {
		--xy-node-border-radius: 12px;
	}
	:global(.svelte-flow.dark) {
		--xy-background-color: var(--color-zinc-950);
		--xy-edge-stroke-default: var(--color-zinc-600);
		--xy-edge-stroke-selected-default: var(--color-zinc-400);
		--xy-connectionline-stroke-default: var(--color-zinc-500);
		--xy-background-pattern-dots-color-default: var(--color-zinc-700);
		--xy-minimap-background-color-default: var(--color-zinc-900);
		--xy-minimap-mask-background-color-default: rgba(9, 9, 11, 0.6);
		--xy-minimap-node-background-color-default: var(--color-zinc-700);
		--xy-node-color-default: var(--color-zinc-100);
		--xy-node-border-default: 1px solid var(--color-zinc-700);
		--xy-node-background-color-default: var(--color-zinc-900);
		--xy-node-boxshadow-hover-default: 0 1px 4px 1px rgba(255, 255, 255, 0.06);
		--xy-node-boxshadow-selected-default: 0 0 0 0.5px var(--color-zinc-500);
		--xy-handle-background-color-default: var(--color-zinc-400);
		--xy-handle-border-color-default: var(--color-zinc-900);
		--xy-selection-background-color-default: rgba(161, 161, 170, 0.08);
		--xy-selection-border-default: 1px dotted rgba(161, 161, 170, 0.5);
		--xy-controls-button-background-color-default: var(--color-zinc-900);
		--xy-controls-button-background-color-hover-default: var(--color-zinc-800);
		--xy-controls-button-color-default: var(--color-zinc-400);
		--xy-controls-button-color-hover-default: var(--color-zinc-200);
		--xy-controls-button-border-color-default: var(--color-zinc-700);
		--xy-controls-box-shadow-default: 0 0 2px 1px rgba(0, 0, 0, 0.3);
		--xy-edge-label-background-color-default: var(--color-zinc-900);
		--xy-edge-label-color-default: var(--color-zinc-300);
	}
	:global(.svelte-flow:not(.dark)) {
		--xy-background-color: #fafaf9;
		--xy-background-pattern-dots-color-default: #d4d4d8;
		--xy-minimap-background-color-default: white;
		--xy-controls-box-shadow-default: 0 0 2px 1px rgba(0, 0, 0, 0.06);
	}
	:global(.svelte-flow__node) {
		border: none !important;
		box-shadow: none !important;
		background: transparent !important;
	}
	:global(.svelte-flow__minimap) {
		border-radius: 8px;
		border: 1px solid var(--color-zinc-200);
		overflow: hidden;
	}
	:global(.svelte-flow.dark .svelte-flow__minimap) {
		border-color: var(--color-zinc-700);
		background: var(--color-zinc-900);
	}
	:global(.svelte-flow__controls) {
		border-radius: 8px;
		border: 1px solid var(--color-zinc-200);
		overflow: hidden;
	}
	:global(.svelte-flow.dark .svelte-flow__controls) {
		border-color: var(--color-zinc-700);
		background: var(--color-zinc-900);
	}
	:global(.svelte-flow.dark .svelte-flow__controls button) {
		background: var(--color-zinc-900);
		color: var(--color-zinc-400);
		border-bottom-color: var(--color-zinc-700);
	}
	:global(.svelte-flow.dark .svelte-flow__controls button:hover) {
		background: var(--color-zinc-800);
		color: var(--color-zinc-200);
	}
	:global(.svelte-flow.dark .svelte-flow__controls button svg) {
		fill: currentColor;
	}
	:global(.svelte-flow__edge-path) {
		stroke-width: 2;
	}
	:global(.svelte-flow__handle) {
		transition: transform 0.15s ease;
	}
	:global(.svelte-flow__handle:hover) {
		transform: scale(1.3);
	}
</style>
