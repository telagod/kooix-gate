<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import { clearTokens } from '$lib/auth.js';
	import { getMe } from '$lib/api.js';
	import type { MeResult } from '$lib/api.js';
	import Button from '$lib/components/ui/Button.svelte';

	let currentPath = $derived($page.url.pathname);
	let me = $state<MeResult | null>(null);
	let showUserMenu = $state(false);

	onMount(async () => {
		try {
			me = await getMe();
		} catch {
			// ignore, nav still works
		}
	});

	function isActive(prefix: string): boolean {
		if (prefix === '/orgs') {
			return currentPath === '/orgs' || (currentPath.startsWith('/orgs/') && !currentPath.includes('/billing'));
		}
		if (prefix === '/billing') {
			return currentPath.includes('/billing');
		}
		return currentPath.startsWith(prefix);
	}

	function handleLogout() {
		clearTokens();
		goto('/login');
	}

	interface NavLink {
		href: string;
		label: string;
		prefix: string;
		adminOnly?: boolean;
	}

	const links: NavLink[] = [
		{ href: '/orgs', label: 'Orgs', prefix: '/orgs' },
		{ href: '/usage', label: 'Usage', prefix: '/usage' },
		{ href: '/channels', label: 'Channels', prefix: '/channels' },
		{ href: '/admin/audit', label: 'Audit', prefix: '/admin', adminOnly: true }
	];

	let visibleLinks = $derived(
		me?.is_platform_admin ? links : links.filter((l) => !l.adminOnly)
	);
</script>

<nav class="bg-white border-b border-zinc-200 px-6 py-3 flex items-center justify-between">
	<div class="flex items-center gap-6">
		<a href="/orgs" class="text-lg font-bold text-zinc-900 hover:text-zinc-700">Kooix Gate</a>
		<div class="flex items-center gap-1">
			{#each visibleLinks as link}
				<a
					href={link.href}
					class="px-3 py-1.5 text-sm rounded-md transition-colors {isActive(link.prefix)
						? 'bg-zinc-900 text-white'
						: 'text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900'}"
				>
					{link.label}
				</a>
			{/each}
		</div>
	</div>

	<div class="relative">
		<button
			onclick={() => (showUserMenu = !showUserMenu)}
			class="flex items-center gap-2 px-3 py-1.5 rounded-md text-sm text-zinc-700 hover:bg-zinc-100 transition-colors"
		>
			<span class="w-6 h-6 rounded-full bg-zinc-200 flex items-center justify-center text-xs font-medium text-zinc-600">
				{me?.subject?.user_id?.slice(0, 1)?.toUpperCase() ?? '?'}
			</span>
			<span class="hidden sm:inline text-zinc-600">
				{#if me?.is_platform_admin}
					Admin
				{:else}
					User
				{/if}
			</span>
		</button>

		{#if showUserMenu}
			<!-- backdrop -->
			<button
				class="fixed inset-0 z-30"
				onclick={() => (showUserMenu = false)}
				aria-label="close menu"
			></button>
			<div class="absolute right-0 top-full mt-1 z-40 w-48 bg-white rounded-lg border border-zinc-200 shadow-lg py-1">
				{#if me?.current_org}
					<div class="px-3 py-2 border-b border-zinc-100">
						<p class="text-xs text-zinc-400">Current Org</p>
						<p class="text-sm font-mono text-zinc-700 truncate">{me.current_org}</p>
					</div>
				{/if}
				<button
					class="w-full text-left px-3 py-2 text-sm text-zinc-700 hover:bg-zinc-50 transition-colors"
					onclick={() => { showUserMenu = false; handleLogout(); }}
				>
					登出
				</button>
			</div>
		{/if}
	</div>
</nav>
