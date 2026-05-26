// 0.4.131（followup B2 step 3）：channels page list state 工厂。
// svelte 5 runes 不能跨文件传 reactive，所以抽工厂返 plain 初始值，
// 调用方在 page.svelte 中 wrap 进 $state 即可。
//
// 好处：default 值集中、reset 时调一处、test 验证 default 值业务正确。

export type SortDir = 'asc' | 'desc';

export interface ChannelsListState {
	page: number;
	pageSize: number;
	sortBy: string;
	sortDir: SortDir;
	filterStatus: string;
}

export function defaultChannelsListState(): ChannelsListState {
	return {
		page: 1,
		pageSize: 20,
		sortBy: 'created_at',
		sortDir: 'desc',
		filterStatus: ''
	};
}

/** reset list state（保留 pageSize 配置）— 用户点 search/filter 后调 */
export function resetListPagination(state: ChannelsListState): void {
	state.page = 1;
}
