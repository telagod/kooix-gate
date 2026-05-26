// 0.4.132：dialog-state 工厂 sanity tests
import { describe, expect, it } from 'vitest';
import {
	noDialog,
	openDialog,
	closeDialog,
	isDialogOpen
} from '../routes/channels/_lib/dialog-state';

describe('channels dialog-state', () => {
	it('noDialog 初始无打开', () => {
		const s = noDialog();
		expect(s.open).toBeNull();
		expect(isDialogOpen(s, 'create')).toBe(false);
	});

	it('openDialog 设置 + isDialogOpen 判断', () => {
		const s = noDialog();
		openDialog(s, 'edit');
		expect(s.open).toBe('edit');
		expect(isDialogOpen(s, 'edit')).toBe(true);
		expect(isDialogOpen(s, 'delete')).toBe(false);
	});

	it('openDialog 互斥：连续调用覆盖', () => {
		const s = noDialog();
		openDialog(s, 'create');
		openDialog(s, 'probe');
		expect(s.open).toBe('probe');
		expect(isDialogOpen(s, 'create')).toBe(false);
	});

	it('closeDialog 重置回 null', () => {
		const s = noDialog();
		openDialog(s, 'delete');
		closeDialog(s);
		expect(s.open).toBeNull();
	});

	it('每次 noDialog 返新对象', () => {
		const a = noDialog();
		const b = noDialog();
		expect(a).not.toBe(b);
	});
});
