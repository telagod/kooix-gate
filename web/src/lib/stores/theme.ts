import { browser } from '$app/environment';
import { writable } from 'svelte/store';

export type Theme = 'light' | 'dark' | 'system';

const STORAGE_KEY = 'kooix_theme';

function getInitial(): Theme {
	if (!browser) return 'system';
	const stored = localStorage.getItem(STORAGE_KEY);
	if (stored === 'light' || stored === 'dark' || stored === 'system') return stored;
	return 'system';
}

function shouldBeDark(theme: Theme): boolean {
	if (theme === 'dark') return true;
	if (theme === 'light') return false;
	if (!browser) return false;
	return window.matchMedia('(prefers-color-scheme: dark)').matches;
}

export const theme = writable<Theme>(getInitial());

function apply(t: Theme) {
	if (!browser) return;
	localStorage.setItem(STORAGE_KEY, t);
	const dark = shouldBeDark(t);
	document.documentElement.classList.toggle('dark', dark);
}

theme.subscribe(apply);

export function toggleTheme() {
	theme.update((current) => {
		if (current === 'light') return 'dark';
		if (current === 'dark') return 'system';
		return 'light';
	});
}

export function initTheme() {
	if (!browser) return;
	apply(getInitial());

	window.matchMedia('(prefers-color-scheme: dark)').addEventListener('change', () => {
		const t = getInitial();
		if (t === 'system') apply('system');
	});
}
