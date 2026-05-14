import { browser } from '$app/environment';
import { writable } from 'svelte/store';

const TOKEN_KEY = 'kooix_access_token';
const REFRESH_KEY = 'kooix_refresh_token';

export const isLoggedIn = writable(false);
export const currentUser = writable<{
	id: string;
	email: string;
	display_name: string | null;
} | null>(null);

export function saveTokens(accessToken: string, refreshToken: string): void {
	if (!browser) return;
	localStorage.setItem(TOKEN_KEY, accessToken);
	localStorage.setItem(REFRESH_KEY, refreshToken);
	isLoggedIn.set(true);
}

export function getAccessToken(): string | null {
	if (!browser) return null;
	return localStorage.getItem(TOKEN_KEY);
}

export function getRefreshToken(): string | null {
	if (!browser) return null;
	return localStorage.getItem(REFRESH_KEY);
}

export function clearTokens(): void {
	if (!browser) return;
	localStorage.removeItem(TOKEN_KEY);
	localStorage.removeItem(REFRESH_KEY);
	isLoggedIn.set(false);
	currentUser.set(null);
}

export function initAuth(): void {
	if (!browser) return;
	isLoggedIn.set(!!localStorage.getItem(TOKEN_KEY));
}
