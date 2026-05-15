import type { Node, Edge } from '@xyflow/svelte';

export type PortType = 'text' | 'image' | 'audio';

export const PORT_COLORS: Record<PortType, string> = {
	text: '#71717a',
	image: '#10b981',
	audio: '#f59e0b',
};

export interface PortDef {
	id: string;
	type: PortType;
	label: string;
}

export type FlowNodeKind =
	| 'textInput'
	| 'imageUpload'
	| 'audioUpload'
	| 'llmChat'
	| 'imageGen'
	| 'tts'
	| 'stt'
	| 'preview';

export interface NodeMeta {
	kind: FlowNodeKind;
	label: string;
	inputs: PortDef[];
	outputs: PortDef[];
	icon: string;
	color: string;
}

export const NODE_CATALOG: Record<FlowNodeKind, NodeMeta> = {
	textInput: {
		kind: 'textInput',
		label: '文本输入',
		inputs: [],
		outputs: [{ id: 'text', type: 'text', label: 'Text' }],
		icon: 'Type',
		color: '#71717a',
	},
	imageUpload: {
		kind: 'imageUpload',
		label: '图片上传',
		inputs: [],
		outputs: [{ id: 'image', type: 'image', label: 'Image' }],
		icon: 'ImagePlus',
		color: '#10b981',
	},
	audioUpload: {
		kind: 'audioUpload',
		label: '音频上传',
		inputs: [],
		outputs: [{ id: 'audio', type: 'audio', label: 'Audio' }],
		icon: 'Upload',
		color: '#f59e0b',
	},
	llmChat: {
		kind: 'llmChat',
		label: 'LLM Chat',
		inputs: [
			{ id: 'text', type: 'text', label: 'Prompt' },
			{ id: 'image', type: 'image', label: 'Image' },
		],
		outputs: [{ id: 'text', type: 'text', label: 'Response' }],
		icon: 'Bot',
		color: '#71717a',
	},
	imageGen: {
		kind: 'imageGen',
		label: '图片生成',
		inputs: [{ id: 'text', type: 'text', label: 'Prompt' }],
		outputs: [{ id: 'image', type: 'image', label: 'Image' }],
		icon: 'Palette',
		color: '#10b981',
	},
	tts: {
		kind: 'tts',
		label: '语音合成',
		inputs: [{ id: 'text', type: 'text', label: 'Text' }],
		outputs: [{ id: 'audio', type: 'audio', label: 'Audio' }],
		icon: 'Volume2',
		color: '#f59e0b',
	},
	stt: {
		kind: 'stt',
		label: '语音识别',
		inputs: [{ id: 'audio', type: 'audio', label: 'Audio' }],
		outputs: [{ id: 'text', type: 'text', label: 'Text' }],
		icon: 'Mic',
		color: '#71717a',
	},
	preview: {
		kind: 'preview',
		label: '预览',
		inputs: [
			{ id: 'text', type: 'text', label: 'Text' },
			{ id: 'image', type: 'image', label: 'Image' },
			{ id: 'audio', type: 'audio', label: 'Audio' },
		],
		outputs: [],
		icon: 'Eye',
		color: '#71717a',
	},
};

export type NodeStatus = 'idle' | 'running' | 'done' | 'error';

export interface FlowNodeData {
	kind: FlowNodeKind;
	status: NodeStatus;
	params: Record<string, unknown>;
	output?: unknown;
	error?: string;
	[key: string]: unknown;
}

export type FlowNode = Node<FlowNodeData>;
export type FlowEdge = Edge;

export interface Workflow {
	id: string;
	name: string;
	nodes: FlowNode[];
	edges: FlowEdge[];
	createdAt: number;
	updatedAt: number;
}
