// 0.4.132（followup B2 step 4）：channels page dialog manager 类型 + helper。
// 之前 modal 协调散在 page.svelte，多个 *Open 布尔互斥逻辑容易写错。
// 抽集中类型让 page 写得更紧凑、防多 modal 同时开。

export type ChannelsDialogKind =
	| 'create'
	| 'edit'
	| 'delete'
	| 'batch-delete'
	| 'probe'
	| 'replay';

export interface ChannelsDialogState {
	open: ChannelsDialogKind | null;
}

export function noDialog(): ChannelsDialogState {
	return { open: null };
}

export function openDialog(
	state: ChannelsDialogState,
	kind: ChannelsDialogKind
): void {
	state.open = kind;
}

export function closeDialog(state: ChannelsDialogState): void {
	state.open = null;
}

export function isDialogOpen(
	state: ChannelsDialogState,
	kind: ChannelsDialogKind
): boolean {
	return state.open === kind;
}
