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
	import { clsx } from 'clsx';
	import {
		Play, Square, Plus, Trash2, ChevronDown, Type,
		ImagePlus, Upload, Bot, Palette, Volume2, Mic, Eye,
		FolderPlus, PanelLeftClose, PanelLeft, RotateCcw,
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
	let showSidebar = $state(true);
	let showNodeMenu = $state(false);
	let nodeMenuPos = $state({ x: 0, y: 0 });

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
		{ label: 'AI 模型', kinds: ['llmChat', 'imageGen', 'tts', 'stt'] as FlowNodeKind[] },
		{ label: '输出', kinds: ['preview'] as FlowNodeKind[] },
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
			position: { x: x ?? 200 + Math.random() * 200, y: y ?? 100 + Math.random() * 200 },
			data: { kind, status: 'idle', params: {}, output: undefined, error: undefined },
		};
		nodes = [...nodes, newNode];
		showNodeMenu = false;
		syncToActive();
	}

	function switchWorkflow(id: string) {
		syncToActive();
		activeId = id;
		saveActiveId(id);
		syncFromActive();
	}

	function newWorkflow() {
		syncToActive();
		const w = createWorkflow();
		workflows = [w, ...workflows];
		activeId = w.id;
		saveWorkflows(workflows);
		saveActiveId(w.id);
		syncFromActive();
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
	function onEdgesChange() { syncToActive(); }

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

<svelte:window onclick={() => { if (showNodeMenu) showNodeMenu = false; }} />

<div class="flex h-full overflow-hidden bg-white dark:bg-zinc-950">
	<!-- Sidebar: workflow list -->
	{#if showSidebar}
		<div class="w-56 shrink-0 border-r border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-900 flex flex-col">
			<div class="p-3 border-b border-zinc-200 dark:border-zinc-700">
				<button onclick={newWorkflow}
					class="w-full flex items-center justify-center gap-1.5 px-3 py-2 rounded-lg text-sm font-medium bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 hover:bg-zinc-800 dark:hover:bg-zinc-200 transition-colors">
					<FolderPlus size={14} />
					新工作流
				</button>
			</div>
			<div class="flex-1 overflow-y-auto">
				{#each workflows as w}
					<div role="button" tabindex="0"
						onclick={() => switchWorkflow(w.id)}
						onkeydown={(e) => { if (e.key === 'Enter') switchWorkflow(w.id); }}
						class={clsx('w-full text-left px-3 py-2.5 text-sm transition-colors group flex items-center justify-between cursor-pointer',
							w.id === activeId ? 'bg-zinc-200 dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100' : 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800/50')}>
						<div class="flex-1 min-w-0">
							<p class="truncate text-sm font-medium">{w.name}</p>
							<p class="text-[10px] text-zinc-400 dark:text-zinc-500 mt-0.5">{formatDate(w.updatedAt)} · {w.nodes.length} 节点</p>
						</div>
						<button onclick={(e) => { e.stopPropagation(); deleteWorkflow(w.id); }}
							class="opacity-0 group-hover:opacity-100 p-1 rounded hover:bg-zinc-300 dark:hover:bg-zinc-700 transition-all shrink-0">
							<Trash2 size={12} class="text-zinc-400" />
						</button>
					</div>
				{/each}
			</div>
		</div>
	{/if}

	<!-- Main canvas area -->
	<div class="flex-1 flex flex-col min-w-0">
		<!-- Toolbar -->
		<div class="flex items-center justify-between px-3 py-2 border-b border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900">
			<div class="flex items-center gap-2">
				<button onclick={() => showSidebar = !showSidebar}
					class="p-1.5 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
					{#if showSidebar}<PanelLeftClose size={16} class="text-zinc-500" />{:else}<PanelLeft size={16} class="text-zinc-500" />{/if}
				</button>
				{#if active}
					<input
						type="text"
						bind:value={active.name}
						onblur={syncToActive}
						class="text-sm font-medium bg-transparent border-none text-zinc-900 dark:text-zinc-100 focus:outline-none focus:ring-0 w-40"
					/>
				{/if}
			</div>
			<div class="flex items-center gap-1.5">
				<!-- Add node dropdown -->
				<div class="relative">
					<button onclick={(e) => { e.stopPropagation(); showNodeMenu = !showNodeMenu; nodeMenuPos = { x: 0, y: 0 }; }}
						class="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg text-xs font-medium text-zinc-600 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors">
						<Plus size={14} />
						添加节点
						<ChevronDown size={12} />
					</button>
				</div>

				<button onclick={clearCanvas}
					class="p-1.5 rounded-lg hover:bg-zinc-100 dark:hover:bg-zinc-800 text-zinc-500 transition-colors" title="清空画布">
					<RotateCcw size={14} />
				</button>

				<div class="w-px h-5 bg-zinc-200 dark:bg-zinc-700"></div>

				{#if running}
					<button onclick={stopFlow}
						class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-red-500 text-white hover:bg-red-600 transition-colors">
						<Square size={12} />
						停止
					</button>
				{:else}
					<button onclick={runFlow} disabled={nodes.length === 0}
						class="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-xs font-medium bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 hover:bg-zinc-800 dark:hover:bg-zinc-200 transition-colors disabled:opacity-40 disabled:cursor-not-allowed">
						<Play size={12} />
						运行
					</button>
				{/if}
			</div>
		</div>

		<!-- Canvas -->
		<div class="flex-1 relative">
			<SvelteFlow
				bind:nodes
				bind:edges
				{nodeTypes}
				{isValidConnection}
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

			<!-- Node context menu (floating) -->
			{#if showNodeMenu}
				<div
					class="fixed z-50 w-52 rounded-xl border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-xl overflow-hidden"
					style={nodeMenuPos.x ? `left: ${nodeMenuPos.x}px; top: ${nodeMenuPos.y}px;` : 'right: 60px; top: 120px;'}
					onclick={(e) => e.stopPropagation()}
					role="menu" tabindex="-1"
				>
					{#each nodeCategories as cat}
						<div class="px-3 py-1.5 text-[10px] font-semibold text-zinc-400 dark:text-zinc-500 uppercase tracking-wider bg-zinc-50 dark:bg-zinc-800/50">
							{cat.label}
						</div>
						{#each cat.kinds as kind}
							{@const meta = NODE_CATALOG[kind]}
							<button onclick={() => addNode(kind, nodeMenuPos.x ? nodeMenuPos.x - 100 : undefined, nodeMenuPos.y ? nodeMenuPos.y - 100 : undefined)}
								class="w-full flex items-center gap-2.5 px-3 py-2 text-xs text-zinc-700 dark:text-zinc-300 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
								role="menuitem">
								<div class="w-5 h-5 rounded flex items-center justify-center" style:background-color="{meta.color}20">
									<svelte:component this={nodeIcons[kind]} size={12} style="color: {meta.color}" />
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
		--xy-background-color: transparent;
		--xy-node-border-radius: 12px;
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
	:global(.dark .svelte-flow__minimap) {
		border-color: var(--color-zinc-700);
	}
	:global(.svelte-flow__controls) {
		border-radius: 8px;
		border: 1px solid var(--color-zinc-200);
		overflow: hidden;
	}
	:global(.dark .svelte-flow__controls) {
		border-color: var(--color-zinc-700);
		background: var(--color-zinc-900);
	}
	:global(.dark .svelte-flow__controls button) {
		background: var(--color-zinc-900);
		color: var(--color-zinc-400);
		border-bottom-color: var(--color-zinc-700);
	}
	:global(.dark .svelte-flow__controls button:hover) {
		background: var(--color-zinc-800);
	}
	:global(.dark .svelte-flow__controls button svg) {
		fill: var(--color-zinc-400);
	}
	:global(.svelte-flow__edge-path) {
		stroke-width: 2;
	}
</style>
