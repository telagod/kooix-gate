// 0.4.9：admin/requests + usage/requests 共享 helper。
// 这些纯函数被两个页面同时引用，先抽出消除重复。

export function rangeToDate(range: string): string {
	const now = new Date();
	switch (range) {
		case '1h':
			return new Date(now.getTime() - 3600_000).toISOString();
		case '24h':
			return new Date(now.getTime() - 86400_000).toISOString();
		case '7d':
			return new Date(now.getTime() - 7 * 86400_000).toISOString();
		case '30d':
			return new Date(now.getTime() - 30 * 86400_000).toISOString();
		default:
			return new Date(now.getTime() - 86400_000).toISOString();
	}
}

export function statusBadgeCls(status: number): string {
	if (status >= 200 && status < 300)
		return 'bg-green-50 dark:bg-green-900/30 text-green-700 dark:text-green-400 ring-1 ring-green-200 dark:ring-green-800';
	if (status >= 400 && status < 500)
		return 'bg-amber-50 dark:bg-amber-900/30 text-amber-700 dark:text-amber-400 ring-1 ring-amber-200 dark:ring-amber-800';
	if (status >= 500)
		return 'bg-red-50 dark:bg-red-900/30 text-red-700 dark:text-red-400 ring-1 ring-red-200 dark:ring-red-800';
	return 'bg-zinc-100 dark:bg-zinc-800 text-zinc-600 dark:text-zinc-300';
}

export function formatRequestDate(s: string): string {
	try {
		return new Date(s).toLocaleString('zh-CN', {
			month: '2-digit',
			day: '2-digit',
			hour: '2-digit',
			minute: '2-digit',
			second: '2-digit',
		});
	} catch {
		return s;
	}
}

export function formatLatency(ms: number | null): string {
	if (ms == null) return '—';
	if (ms < 1000) return `${ms}ms`;
	return `${(ms / 1000).toFixed(1)}s`;
}

export function formatCost(n: number): string {
	if (n < 0.0001) return '$0';
	return `$${n.toFixed(4)}`;
}

export function formatTokens(n: number): string {
	if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
	if (n >= 1000) return `${(n / 1000).toFixed(1)}K`;
	return String(n);
}
