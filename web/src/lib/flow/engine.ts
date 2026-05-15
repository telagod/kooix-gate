import type { FlowNode, FlowEdge, FlowNodeData, PortType } from './types.js';
import { NODE_CATALOG } from './types.js';
import {
	chatCompletionStream,
	generateImage,
	createSpeech,
	createTranscription,
	listModels,
} from '$lib/api.js';
import type { ChatParams } from '$lib/api.js';

export function topoSort(nodes: FlowNode[], edges: FlowEdge[]): string[] {
	const adj = new Map<string, string[]>();
	const indeg = new Map<string, number>();
	for (const n of nodes) {
		adj.set(n.id, []);
		indeg.set(n.id, 0);
	}
	for (const e of edges) {
		adj.get(e.source)!.push(e.target);
		indeg.set(e.target, (indeg.get(e.target) ?? 0) + 1);
	}
	const queue: string[] = [];
	for (const [id, deg] of indeg) if (deg === 0) queue.push(id);
	const order: string[] = [];
	while (queue.length > 0) {
		const cur = queue.shift()!;
		order.push(cur);
		for (const next of adj.get(cur) ?? []) {
			const d = (indeg.get(next) ?? 1) - 1;
			indeg.set(next, d);
			if (d === 0) queue.push(next);
		}
	}
	if (order.length !== nodes.length) throw new Error('工作流存在环形依赖');
	return order;
}

export function canConnect(
	sourceNode: FlowNode,
	sourceHandleId: string,
	targetNode: FlowNode,
	targetHandleId: string
): boolean {
	const srcMeta = NODE_CATALOG[sourceNode.data.kind];
	const tgtMeta = NODE_CATALOG[targetNode.data.kind];
	const srcPort = srcMeta.outputs.find((p) => p.id === sourceHandleId);
	const tgtPort = tgtMeta.inputs.find((p) => p.id === targetHandleId);
	if (!srcPort || !tgtPort) return false;
	return srcPort.type === tgtPort.type;
}

function getInputs(
	nodeId: string,
	edges: FlowEdge[],
	outputs: Map<string, unknown>
): Record<string, unknown> {
	const inputs: Record<string, unknown> = {};
	for (const e of edges) {
		if (e.target === nodeId && e.targetHandle) {
			inputs[e.targetHandle] = outputs.get(`${e.source}:${e.sourceHandle}`);
		}
	}
	return inputs;
}

export interface RunCallbacks {
	onNodeStart: (nodeId: string) => void;
	onNodeDone: (nodeId: string, output: unknown) => void;
	onNodeError: (nodeId: string, error: string) => void;
	onNodeStream?: (nodeId: string, chunk: string) => void;
}

export async function executeFlow(
	nodes: FlowNode[],
	edges: FlowEdge[],
	orgId: string,
	callbacks: RunCallbacks,
	signal?: AbortSignal
): Promise<void> {
	const order = topoSort(nodes, edges);
	const nodeMap = new Map(nodes.map((n) => [n.id, n]));
	const portOutputs = new Map<string, unknown>();

	for (const nodeId of order) {
		if (signal?.aborted) throw new Error('已取消');
		const node = nodeMap.get(nodeId)!;
		const inputs = getInputs(nodeId, edges, portOutputs);
		callbacks.onNodeStart(nodeId);

		try {
			const output = await executeNode(node, inputs, orgId, callbacks, signal);
			const meta = NODE_CATALOG[node.data.kind];
			if (meta.outputs.length === 1) {
				portOutputs.set(`${nodeId}:${meta.outputs[0].id}`, output);
			} else if (output && typeof output === 'object') {
				const rec = output as Record<string, unknown>;
				for (const p of meta.outputs) {
					if (rec[p.id] !== undefined) portOutputs.set(`${nodeId}:${p.id}`, rec[p.id]);
				}
			}
			callbacks.onNodeDone(nodeId, output);
		} catch (err: any) {
			if (signal?.aborted) throw err;
			callbacks.onNodeError(nodeId, err?.message ?? String(err));
			throw err;
		}
	}
}

async function executeNode(
	node: FlowNode,
	inputs: Record<string, unknown>,
	orgId: string,
	callbacks: RunCallbacks,
	signal?: AbortSignal
): Promise<unknown> {
	const p = node.data.params;

	switch (node.data.kind) {
		case 'textInput':
			return (p.text as string) ?? '';

		case 'imageUpload':
			return (p.dataUrl as string) ?? '';

		case 'audioUpload':
			return (p.file as File) ?? null;

		case 'llmChat': {
			const promptText = (inputs.text as string) ?? (p.text as string) ?? '';
			const imageUrl = inputs.image as string | undefined;
			if (!promptText && !imageUrl) throw new Error('LLM 节点需要输入文本或图片');

			const messages: { role: string; content: any }[] = [];
			const systemPrompt = (p.systemPrompt as string) ?? '';
			if (systemPrompt) messages.push({ role: 'system', content: systemPrompt });

			if (imageUrl) {
				messages.push({
					role: 'user',
					content: [
						{ type: 'image_url', image_url: { url: imageUrl } },
						...(promptText ? [{ type: 'text', text: promptText }] : []),
					],
				});
			} else {
				messages.push({ role: 'user', content: promptText });
			}

			const model = (p.model as string) ?? 'gpt-4o-mini';
			return new Promise<string>((resolve, reject) => {
				let result = '';
				const ctrl = chatCompletionStream(
					orgId,
					{
						model,
						messages,
						temperature: (p.temperature as number) ?? 0.7,
						top_p: (p.topP as number) ?? 1.0,
						max_tokens: (p.maxTokens as number) ?? 4096,
					} as ChatParams,
					(chunk) => {
						result += chunk;
						callbacks.onNodeStream?.(node.id, result);
					},
					() => resolve(result),
					(err) => reject(new Error(err))
				);
				signal?.addEventListener('abort', () => ctrl.abort());
			});
		}

		case 'imageGen': {
			const prompt = (inputs.text as string) ?? (p.prompt as string) ?? '';
			if (!prompt) throw new Error('图片生成需要 prompt');
			const resp = await generateImage({
				model: (p.model as string) ?? 'dall-e-3',
				prompt,
				n: 1,
				size: (p.size as string) ?? '1024x1024',
				quality: (p.quality as string) ?? 'standard',
				style: (p.style as string) ?? 'vivid',
				response_format: 'url',
			});
			return resp.data[0]?.url ?? '';
		}

		case 'tts': {
			const text = (inputs.text as string) ?? (p.text as string) ?? '';
			if (!text) throw new Error('TTS 需要输入文本');
			const blob = await createSpeech({
				model: (p.model as string) ?? 'tts-1',
				input: text,
				voice: (p.voice as string) ?? 'alloy',
				response_format: (p.format as string) ?? 'mp3',
				speed: (p.speed as number) ?? 1.0,
			});
			return URL.createObjectURL(blob);
		}

		case 'stt': {
			const file = (inputs.audio as File) ?? (p.file as File);
			if (!file) throw new Error('STT 需要音频文件');
			const resp = await createTranscription(
				file,
				(p.model as string) ?? 'whisper-1',
				(p.language as string) || undefined
			);
			return resp.text;
		}

		case 'preview':
			return inputs;

		default:
			throw new Error(`未知节点类型: ${node.data.kind}`);
	}
}
