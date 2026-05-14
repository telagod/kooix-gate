import { getAccessToken } from '$lib/auth.js';
import { redirect } from '@sveltejs/kit';
import type { LayoutLoad } from './$types';

const PUBLIC_PATHS = ['/login', '/', '/setup'];

export const load: LayoutLoad = ({ url }) => {
	if (!PUBLIC_PATHS.includes(url.pathname) && !getAccessToken()) {
		redirect(302, '/login');
	}
	return {};
};

export const ssr = false;
