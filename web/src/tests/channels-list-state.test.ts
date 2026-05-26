// 0.4.131：list-state 工厂 sanity tests
import { describe, expect, it } from 'vitest';
import {
	defaultChannelsListState,
	resetListPagination
} from '../routes/channels/_lib/list-state';

describe('channels list-state', () => {
	it('defaultChannelsListState 返业务默认', () => {
		const s = defaultChannelsListState();
		expect(s.page).toBe(1);
		expect(s.pageSize).toBe(20);
		expect(s.sortBy).toBe('created_at');
		expect(s.sortDir).toBe('desc');
		expect(s.filterStatus).toBe('');
	});

	it('每次调用返新对象', () => {
		const a = defaultChannelsListState();
		const b = defaultChannelsListState();
		expect(a).not.toBe(b);
	});

	it('resetListPagination 保留 pageSize 但 page 回 1', () => {
		const s = defaultChannelsListState();
		s.page = 5;
		s.pageSize = 50;
		resetListPagination(s);
		expect(s.page).toBe(1);
		expect(s.pageSize).toBe(50); // 不重置
	});
});
