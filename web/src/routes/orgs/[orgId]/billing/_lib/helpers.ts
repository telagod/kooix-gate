// 0.4.19：billing/+page.svelte 抽出的纯 helper（formatter / scope label / status label / variant）。
import type { BadgeVariant } from '$lib/design';

export function fmtCost(s: string): string {
	const n = parseFloat(s);
	return isNaN(n) ? s : `$${n.toFixed(4)}`;
}

export function fmtNum(n: number): string {
	return n.toLocaleString('en-US');
}

export function scopeLabel(kind: string): string {
	const m: Record<string, string> = { org: '组织', project: '项目', api_key: 'API Key' };
	return m[kind] ?? kind;
}

export function invoiceStatusLabel(status: string): string {
	const m: Record<string, string> = {
		draft: 'Draft',
		closed: 'Closed',
		exported: 'Exported',
		paid: 'Paid',
		waived: 'Waived',
	};
	return m[status] ?? status;
}

export function invoiceStatusVariant(status: string): BadgeVariant {
	const m: Record<string, BadgeVariant> = {
		draft: 'default',
		closed: 'warning',
		exported: 'admin',
		paid: 'success',
		waived: 'warning',
	};
	return m[status] ?? 'default';
}
