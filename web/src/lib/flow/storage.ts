import type { Workflow, FlowNode, FlowEdge } from './types.js';

const STORAGE_KEY = 'kooix_flow_workflows';
const ACTIVE_KEY = 'kooix_flow_active';

export function loadWorkflows(): Workflow[] {
	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		return raw ? JSON.parse(raw) : [];
	} catch {
		return [];
	}
}

export function saveWorkflows(workflows: Workflow[]): void {
	try {
		const serializable = workflows.map((w) => ({
			...w,
			nodes: w.nodes.map((n) => ({
				...n,
				data: {
					...n.data,
					params: stripNonSerializable(n.data.params),
					output: undefined,
					status: 'idle',
					error: undefined,
				},
			})),
		}));
		localStorage.setItem(STORAGE_KEY, JSON.stringify(serializable));
	} catch {}
}

function stripNonSerializable(params: Record<string, unknown>): Record<string, unknown> {
	const clean: Record<string, unknown> = {};
	for (const [k, v] of Object.entries(params)) {
		if (v instanceof File || v instanceof Blob) continue;
		clean[k] = v;
	}
	return clean;
}

export function loadActiveId(): string | null {
	try {
		return localStorage.getItem(ACTIVE_KEY);
	} catch {
		return null;
	}
}

export function saveActiveId(id: string | null): void {
	try {
		if (id) localStorage.setItem(ACTIVE_KEY, id);
		else localStorage.removeItem(ACTIVE_KEY);
	} catch {}
}

export function createWorkflow(name?: string): Workflow {
	return {
		id: crypto.randomUUID(),
		name: name ?? '新工作流',
		nodes: [],
		edges: [],
		createdAt: Date.now(),
		updatedAt: Date.now(),
	};
}
