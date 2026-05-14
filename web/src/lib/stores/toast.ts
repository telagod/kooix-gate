import { writable } from 'svelte/store';

export type ToastType = 'success' | 'error' | 'info';

export interface Toast {
	id: string;
	message: string;
	type: ToastType;
	duration: number;
}

export const toasts = writable<Toast[]>([]);

export function addToast(message: string, type: ToastType = 'info', duration = 3000): void {
	const id = crypto.randomUUID();
	toasts.update((all) => [...all, { id, message, type, duration }]);
	setTimeout(() => removeToast(id), duration);
}

export function removeToast(id: string): void {
	toasts.update((all) => all.filter((t) => t.id !== id));
}
