export type TableSortDir = 'asc' | 'desc';

export interface TableColumn {
	id: string;
	label: string;
	required?: boolean;
}

export interface TableStateSnapshot<TFilters extends Record<string, unknown> = Record<string, unknown>> {
	pageSize: number;
	sortBy: string;
	sortDir: TableSortDir;
	visibleColumns: string[];
	filters: TFilters;
}

const STORAGE_PREFIX = 'kooix:table-state:';

function storageKey(key: string) {
	return `${STORAGE_PREFIX}${key}`;
}

function browserStorage(): Storage | null {
	if (typeof localStorage === 'undefined') return null;
	return localStorage;
}

export function normalizePageSize(value: number, allowed: readonly number[], fallback: number): number {
	return allowed.includes(value) ? value : fallback;
}

export function currentPageFromOffset(offset: number, pageSize: number): number {
	return Math.floor(Math.max(0, offset) / Math.max(1, pageSize)) + 1;
}

export function pageOffset(page: number, pageSize: number): number {
	return (Math.max(1, page) - 1) * Math.max(1, pageSize);
}

export function nextSortDir(currentBy: string, currentDir: TableSortDir, nextBy: string, initial: TableSortDir = 'asc'): TableSortDir {
	if (currentBy !== nextBy) return initial;
	return currentDir === 'asc' ? 'desc' : 'asc';
}

export function normalizeSortBy(value: string, allowed: readonly string[], fallback: string): string {
	return allowed.includes(value) ? value : fallback;
}

export function normalizeSortDir(value: string | null | undefined, fallback: TableSortDir = 'desc'): TableSortDir {
	return value === 'asc' || value === 'desc' ? value : fallback;
}

export function normalizeVisibleColumns(columns: readonly TableColumn[], visibleIds: readonly string[]): string[] {
	const known = new Set(columns.map((column) => column.id));
	const required = columns.filter((column) => column.required).map((column) => column.id);
	const requested = visibleIds.filter((id) => known.has(id));
	const merged = [...new Set([...required, ...requested])];
	return merged.length > 0 ? merged : columns.map((column) => column.id);
}

export function hiddenColumnsFromVisible(columns: readonly TableColumn[], visibleIds: readonly string[]): string[] {
	const visible = new Set(normalizeVisibleColumns(columns, visibleIds));
	return columns.filter((column) => !visible.has(column.id)).map((column) => column.id);
}

export function visibleColumnsFromHidden(columns: readonly TableColumn[], hiddenIds: readonly string[]): TableColumn[] {
	const hidden = new Set(hiddenIds);
	return columns.filter((column) => column.required || !hidden.has(column.id));
}

export function isColumnVisible(columns: readonly TableColumn[], id: string): boolean {
	return columns.some((column) => column.id === id);
}

export function toggleColumnVisibility(
	columns: readonly TableColumn[],
	hiddenIds: readonly string[],
	id: string
): string[] {
	const target = columns.find((column) => column.id === id);
	if (!target || target.required) return [...hiddenIds];

	const hidden = new Set(hiddenIds);
	if (hidden.has(id)) {
		hidden.delete(id);
	} else {
		const visibleCount = visibleColumnsFromHidden(columns, [...hidden]).filter((column) => !column.required).length;
		if (visibleCount <= 1) return [...hiddenIds];
		hidden.add(id);
	}
	return [...hidden].filter((hiddenId) => columns.some((column) => column.id === hiddenId));
}

export function saveTableState<TFilters extends Record<string, unknown>>(
	key: string,
	state: TableStateSnapshot<TFilters>
): void {
	const storage = browserStorage();
	if (!storage) return;
	storage.setItem(storageKey(key), JSON.stringify(state));
}

export function loadTableState<TFilters extends Record<string, unknown>>(
	key: string,
	defaults: TableStateSnapshot<TFilters>
): TableStateSnapshot<TFilters> {
	const storage = browserStorage();
	if (!storage) return defaults;

	const raw = storage.getItem(storageKey(key));
	if (!raw) return defaults;

	try {
		const parsed = JSON.parse(raw) as Partial<TableStateSnapshot<TFilters>>;
		return {
			pageSize: typeof parsed.pageSize === 'number' ? parsed.pageSize : defaults.pageSize,
			sortBy: typeof parsed.sortBy === 'string' ? parsed.sortBy : defaults.sortBy,
			sortDir: normalizeSortDir(parsed.sortDir, defaults.sortDir),
			visibleColumns: Array.isArray(parsed.visibleColumns)
				? parsed.visibleColumns.filter((id): id is string => typeof id === 'string')
				: defaults.visibleColumns,
			filters: typeof parsed.filters === 'object' && parsed.filters !== null ? (parsed.filters as TFilters) : defaults.filters
		};
	} catch {
		return defaults;
	}
}

export function clearTableState(key: string): void {
	const storage = browserStorage();
	if (!storage) return;
	storage.removeItem(storageKey(key));
}
