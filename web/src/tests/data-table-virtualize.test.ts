// 0.4.134：DataTable virtualize 行为测试
import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import DataTable from '$lib/components/templates/DataTable.svelte';

describe('DataTable virtualize', () => {
	it('不传 rows → legacy passthrough（children 渲染）', () => {
		const { container } = render(DataTable, {
			children: createRawSnippet(() => ({
				render: () => '<tr><td>legacy row</td></tr>'
			}))
		});
		expect(container.textContent).toContain('legacy row');
	});

	it('传 rows + rowSnippet → 虚拟化模式渲染部分 row', () => {
		const rows = Array.from({ length: 100 }, (_, i) => ({ id: i }));
		const rowSnippet = createRawSnippet((row: () => unknown) => ({
			render: () => `<td>row-${(row() as { id: number }).id}</td>`
		}));
		const { container } = render(DataTable, {
			rows,
			rowSnippet,
			rowHeight: 48,
			maxHeight: '480px'
		});
		// 视口 ~480/48 = 10 行 + overscan 5 上下 = ~15-20 行渲染（不会全 100 行）
		const renderedCells = container.querySelectorAll('td');
		expect(renderedCells.length).toBeLessThan(100);
		// 至少渲染若干行（含 spacer）
		expect(renderedCells.length).toBeGreaterThan(0);
	});

	it('rows 为空数组 → 不渲染任何 row', () => {
		const rows: Array<{ id: number }> = [];
		const rowSnippet = createRawSnippet((row: () => unknown) => ({
			render: () => `<td>row-${(row() as { id: number }).id}</td>`
		}));
		const { container } = render(DataTable, { rows, rowSnippet });
		const matchingCells = Array.from(container.querySelectorAll('td')).filter(
			(c) => c.textContent?.startsWith('row-')
		);
		expect(matchingCells.length).toBe(0);
	});
});
