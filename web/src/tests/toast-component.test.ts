import { describe, it, expect, beforeEach, vi, afterEach } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import Toast from '$lib/components/Toast.svelte';
import { toasts, addToast } from '$lib/stores/toast';

describe('Toast component', () => {
	beforeEach(() => {
		toasts.set([]);
		vi.useFakeTimers();
	});

	afterEach(() => {
		vi.useRealTimers();
	});

	it('renders nothing when no toasts', () => {
		const { container } = render(Toast);
		expect(container.querySelector('.pointer-events-auto')).toBeNull();
	});

	it('renders toast message', async () => {
		render(Toast);
		addToast('Hello world', 'success');
		await vi.advanceTimersByTimeAsync(50);
		expect(screen.getByText('Hello world')).toBeInTheDocument();
	});

	it('applies success color class', async () => {
		render(Toast);
		addToast('OK', 'success');
		await vi.advanceTimersByTimeAsync(50);
		const el = screen.getByText('OK').closest('.pointer-events-auto');
		expect(el?.className).toContain('bg-green-500');
	});

	it('applies error color class', async () => {
		render(Toast);
		addToast('Fail', 'error');
		await vi.advanceTimersByTimeAsync(50);
		const el = screen.getByText('Fail').closest('.pointer-events-auto');
		expect(el?.className).toContain('bg-red-500');
	});

	it('applies info color class', async () => {
		render(Toast);
		addToast('Note', 'info');
		await vi.advanceTimersByTimeAsync(50);
		const el = screen.getByText('Note').closest('.pointer-events-auto');
		expect(el?.className).toContain('bg-zinc-600');
	});

	it('has dismiss button', async () => {
		render(Toast);
		addToast('Msg', 'info');
		await vi.advanceTimersByTimeAsync(50);
		const btn = screen.getByLabelText('关闭');
		expect(btn).toBeInTheDocument();
	});

	it('renders multiple toasts', async () => {
		render(Toast);
		addToast('First', 'info');
		addToast('Second', 'error');
		await vi.advanceTimersByTimeAsync(50);
		expect(screen.getByText('First')).toBeInTheDocument();
		expect(screen.getByText('Second')).toBeInTheDocument();
	});
});
