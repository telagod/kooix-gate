import { writable, readable } from 'svelte/store';

export const page = readable({
	url: new URL('http://localhost:3000/orgs'),
	params: {},
	route: { id: '' },
	status: 200,
	error: null,
	data: {},
	form: null
});

export const navigating = readable(null);
export const updated = { check: async () => false, subscribe: readable(false).subscribe };
