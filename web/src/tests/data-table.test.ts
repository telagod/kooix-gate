// 0.4.86：验证 DataTable 0.4.85 新增的 maxHeight / stickyHead prop。
// 用 @testing-library/svelte mount 后断言 style + class。

import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/svelte';
import DataTable from '$lib/components/templates/DataTable.svelte';

describe('DataTable maxHeight + stickyHead', () => {
	it('默认无 maxHeight / 不 sticky', () => {
		const { container } = render(DataTable, {});
		const wrap = container.querySelector('div');
		expect(wrap?.getAttribute('style') ?? '').not.toContain('max-height');
		const thead = container.querySelector('thead');
		// 默认 head 不渲染（无 head snippet），但即便渲染也不应含 sticky
		if (thead) {
			expect(thead.className).not.toContain('sticky');
		}
	});

	it('设置 maxHeight 后写入 inline style', () => {
		const { container } = render(DataTable, { maxHeight: '480px' });
		const wrap = container.querySelector('div');
		expect(wrap?.getAttribute('style') ?? '').toContain('max-height: 480px');
		expect(wrap?.getAttribute('style') ?? '').toContain('overflow-y: auto');
	});

	it('stickyHead 单独不会生效（要配合 head snippet）', () => {
		// 默认 head=undefined，stickyHead 即使为 true 也不渲染 thead
		const { container } = render(DataTable, { stickyHead: true });
		expect(container.querySelector('thead')).toBeNull();
	});
});
