import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { get } from 'svelte/store';
import { toasts, addToast, removeToast } from '$lib/stores/toast';

describe('toast store', () => {
	beforeEach(() => {
		toasts.set([]);
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	it('addToast appends a toast with correct fields', () => {
		addToast('hello', 'success', 5000);
		const all = get(toasts);
		expect(all).toHaveLength(1);
		expect(all[0].message).toBe('hello');
		expect(all[0].type).toBe('success');
		expect(all[0].duration).toBe(5000);
		expect(all[0].id).toBeTruthy();
	});

	it('addToast defaults to info type and 3000ms duration', () => {
		addToast('test');
		const all = get(toasts);
		expect(all[0].type).toBe('info');
		expect(all[0].duration).toBe(3000);
	});

	it('removeToast removes the specified toast', () => {
		addToast('a', 'success');
		addToast('b', 'error');
		const all = get(toasts);
		removeToast(all[0].id);
		const remaining = get(toasts);
		expect(remaining).toHaveLength(1);
		expect(remaining[0].message).toBe('b');
	});

	it('addToast auto-removes after duration', () => {
		addToast('ephemeral', 'info', 2000);
		expect(get(toasts)).toHaveLength(1);
		vi.advanceTimersByTime(2000);
		expect(get(toasts)).toHaveLength(0);
	});

	it('multiple toasts accumulate', () => {
		addToast('one');
		addToast('two');
		addToast('three');
		expect(get(toasts)).toHaveLength(3);
	});
});
