import { beforeEach, describe, expect, it } from 'vitest';
import {
	clearTableState,
	currentPageFromOffset,
	hiddenColumnsFromVisible,
	loadTableState,
	nextSortDir,
	normalizePageSize,
	normalizeSortBy,
	normalizeSortDir,
	saveTableState,
	toggleColumnVisibility,
	visibleColumnsFromHidden
} from '$lib/table-state';
import type { TableColumn } from '$lib/table-state';

const columns: TableColumn[] = [
	{ id: 'ts', label: '时间', required: true },
	{ id: 'actor', label: '操作者' },
	{ id: 'action', label: '动作', required: true },
	{ id: 'outcome', label: '结果' }
];

describe('table-state helpers', () => {
	beforeEach(() => {
		localStorage.clear();
	});

	it('normalizes pagination and sort values', () => {
		expect(normalizePageSize(50, [25, 50, 100], 25)).toBe(50);
		expect(normalizePageSize(500, [25, 50, 100], 25)).toBe(25);
		expect(currentPageFromOffset(100, 50)).toBe(3);
		expect(normalizeSortBy('action', ['ts', 'action'], 'ts')).toBe('action');
		expect(normalizeSortBy('unknown', ['ts', 'action'], 'ts')).toBe('ts');
		expect(normalizeSortDir('asc')).toBe('asc');
		expect(normalizeSortDir('sideways', 'desc')).toBe('desc');
		expect(nextSortDir('ts', 'desc', 'ts')).toBe('asc');
		expect(nextSortDir('ts', 'desc', 'action')).toBe('asc');
	});

	it('keeps required columns visible and prevents hiding the last optional column', () => {
		let hidden = hiddenColumnsFromVisible(columns, ['ts', 'action', 'outcome']);
		expect(hidden).toEqual(['actor']);
		expect(visibleColumnsFromHidden(columns, hidden).map((column) => column.id)).toEqual(['ts', 'action', 'outcome']);

		hidden = toggleColumnVisibility(columns, hidden, 'actor');
		expect(hidden).toEqual([]);
		hidden = toggleColumnVisibility(columns, hidden, 'actor');
		expect(hidden).toEqual(['actor']);
		hidden = toggleColumnVisibility(columns, hidden, 'outcome');
		expect(hidden).toEqual(['actor']);
		expect(toggleColumnVisibility(columns, hidden, 'ts')).toEqual(['actor']);
	});

	it('persists saved table filters and column visibility', () => {
		saveTableState('unit', {
			pageSize: 100,
			sortBy: 'action',
			sortDir: 'asc',
			visibleColumns: ['ts', 'action', 'outcome'],
			filters: { status: 'denied' }
		});

		const saved = loadTableState('unit', {
			pageSize: 25,
			sortBy: 'ts',
			sortDir: 'desc',
			visibleColumns: columns.map((column) => column.id),
			filters: { status: '' }
		});

		expect(saved).toEqual({
			pageSize: 100,
			sortBy: 'action',
			sortDir: 'asc',
			visibleColumns: ['ts', 'action', 'outcome'],
			filters: { status: 'denied' }
		});

		clearTableState('unit');
		expect(localStorage.getItem('kooix:table-state:unit')).toBeNull();
	});
});
