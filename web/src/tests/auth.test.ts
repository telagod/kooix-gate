import { describe, it, expect, beforeEach, vi } from 'vitest';

import { get } from 'svelte/store';
import {
	saveTokens,
	getAccessToken,
	getRefreshToken,
	clearTokens,
	initAuth,
	isLoggedIn,
	currentUser
} from '$lib/auth';

describe('auth token helpers', () => {
	beforeEach(() => {
		localStorage.clear();
		isLoggedIn.set(false);
		currentUser.set(null);
	});

	it('saveTokens stores access and refresh tokens in localStorage', () => {
		saveTokens('access-123', 'refresh-456');
		expect(localStorage.getItem('kooix_access_token')).toBe('access-123');
		expect(localStorage.getItem('kooix_refresh_token')).toBe('refresh-456');
	});

	it('saveTokens sets isLoggedIn to true', () => {
		saveTokens('a', 'r');
		expect(get(isLoggedIn)).toBe(true);
	});

	it('getAccessToken returns stored token', () => {
		localStorage.setItem('kooix_access_token', 'tok-abc');
		expect(getAccessToken()).toBe('tok-abc');
	});

	it('getAccessToken returns null when empty', () => {
		expect(getAccessToken()).toBeNull();
	});

	it('getRefreshToken returns stored token', () => {
		localStorage.setItem('kooix_refresh_token', 'ref-xyz');
		expect(getRefreshToken()).toBe('ref-xyz');
	});

	it('clearTokens removes tokens and resets stores', () => {
		saveTokens('a', 'r');
		currentUser.set({ id: '1', email: 'x@y.com', display_name: null });
		clearTokens();
		expect(localStorage.getItem('kooix_access_token')).toBeNull();
		expect(localStorage.getItem('kooix_refresh_token')).toBeNull();
		expect(get(isLoggedIn)).toBe(false);
		expect(get(currentUser)).toBeNull();
	});

	it('initAuth sets isLoggedIn based on token presence', () => {
		localStorage.setItem('kooix_access_token', 'exists');
		initAuth();
		expect(get(isLoggedIn)).toBe(true);
	});

	it('initAuth sets isLoggedIn false when no token', () => {
		initAuth();
		expect(get(isLoggedIn)).toBe(false);
	});
});
