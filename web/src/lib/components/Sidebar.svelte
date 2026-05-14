<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import { clearTokens } from '$lib/auth.js';
	import { getMe } from '$lib/api.js';
	import type { MeResult } from '$lib/api.js';
	import { theme, toggleTheme } from '$lib/stores/theme';

	let currentPath = $derived($page.url.pathname);
	let me = $state<MeResult | null>(null);
	let collapsed = $state(false);

	onMount(async () => {
		try {
			me = await getMe();
		} catch {}
	});

	let currentOrg = $derived(me?.current_org ?? me?.orgs?.[0] ?? null);
	let isAdmin = $derived(me?.is_platform_admin ?? false);

	function isActive(pattern: string): boolean {
		if (pattern === '/orgs') return currentPath === '/orgs' || currentPath.startsWith('/orgs/');
		if (pattern === '/usage') return currentPath.startsWith('/usage');
		if (pattern === '/channels') return currentPath === '/channels' || currentPath.startsWith('/channels/');
		if (pattern === '/admin/audit') return currentPath.startsWith('/admin');
		return currentPath.startsWith(pattern);
	}

	function handleLogout() {
		clearTokens();
		goto('/login');
	}

	const themeLabel: Record<string, string> = { light: '☀ 浅色', dark: '🌙 深色', system: '💻 系统' };
	const themeIcon: Record<string, string> = { light: '☀', dark: '🌙', system: '💻' };

	interface NavItem {
		href: string;
		label: string;
		icon: string;
		pattern: string;
	}

	const userNav: NavItem[] = [
		{ href: '/orgs', label: '组织', icon: '🏢', pattern: '/orgs' },
		{ href: '/usage', label: '用量', icon: '📊', pattern: '/usage' },
	];

	let orgNav = $derived.by((): NavItem[] => {
		if (!currentOrg) return [];
		return [
			{ href: `/orgs/${currentOrg}/projects`, label: '项目', icon: '📁', pattern: `/orgs/${currentOrg}/projects` },
			{ href: `/orgs/${currentOrg}/billing`, label: '账单', icon: '💰', pattern: `/orgs/${currentOrg}/billing` },
			{ href: `/orgs/${currentOrg}/quotas`, label: '配额', icon: '⚙', pattern: `/orgs/${currentOrg}/quotas` },
		];
	});

	const adminNav: NavItem[] = [
		{ href: '/channels', label: '渠道管理', icon: '🔌', pattern: '/channels' },
		{ href: '/admin/audit', label: '审计日志', icon: '📋', pattern: '/admin/audit' },
	];
</script>

<aside class="flex flex-col h-full {collapsed ? 'w-16' : 'w-56'} bg-white dark:bg-zinc-900 border-r border-zinc-200 dark:border-zinc-700 transition-all duration-200 shrink-0">
	<!-- Logo -->
	<div class="flex items-center justify-between px-4 h-14 border-b border-zinc-200 dark:border-zinc-700">
		{#if !collapsed}
			<a href="/orgs" class="text-base font-bold text-zinc-900 dark:text-zinc-100 truncate">Kooix Gate</a>
		{:else}
			<a href="/orgs" class="text-base font-bold text-zinc-900 dark:text-zinc-100">K</a>
		{/if}
		<button
			onclick={() => (collapsed = !collapsed)}
			class="p-1 rounded text-zinc-400 dark:text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors text-xs"
			title={collapsed ? '展开' : '收起'}
		>
			{collapsed ? '»' : '«'}
		</button>
	</div>

	<!-- Nav sections -->
	<nav class="flex-1 overflow-y-auto py-3 px-2 space-y-4">
		<!-- User section -->
		<div>
			{#if !collapsed}
				<p class="px-2 mb-1 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500">导航</p>
			{/if}
			{#each userNav as item}
				<a
					href={item.href}
					class="flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm transition-colors mb-0.5
						{isActive(item.pattern)
							? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
							: 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
					title={collapsed ? item.label : ''}
				>
					<span class="text-base leading-none shrink-0">{item.icon}</span>
					{#if !collapsed}<span class="truncate">{item.label}</span>{/if}
				</a>
			{/each}
		</div>

		<!-- Org context section -->
		{#if orgNav.length > 0}
			<div>
				{#if !collapsed}
					<p class="px-2 mb-1 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500">
						当前组织
					</p>
				{/if}
				{#each orgNav as item}
					<a
						href={item.href}
						class="flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm transition-colors mb-0.5
							{isActive(item.pattern)
								? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
								: 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
						title={collapsed ? item.label : ''}
					>
						<span class="text-base leading-none shrink-0">{item.icon}</span>
						{#if !collapsed}<span class="truncate">{item.label}</span>{/if}
					</a>
				{/each}
			</div>
		{/if}

		<!-- Admin section -->
		{#if isAdmin}
			<div>
				{#if !collapsed}
					<div class="px-2 mb-1 flex items-center gap-1.5">
						<p class="text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500">管理员</p>
						<span class="inline-block px-1 py-0 rounded text-[9px] font-medium bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-400">Admin</span>
					</div>
				{/if}
				{#each adminNav as item}
					<a
						href={item.href}
						class="flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm transition-colors mb-0.5
							{isActive(item.pattern)
								? 'bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900'
								: 'text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800'}"
						title={collapsed ? item.label : ''}
					>
						<span class="text-base leading-none shrink-0">{item.icon}</span>
						{#if !collapsed}<span class="truncate">{item.label}</span>{/if}
					</a>
				{/each}
			</div>
		{/if}
	</nav>

	<!-- Bottom: theme + user -->
	<div class="border-t border-zinc-200 dark:border-zinc-700 p-2 space-y-1">
		<button
			onclick={toggleTheme}
			title="切换主题"
			class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
		>
			<span class="text-base leading-none shrink-0">{themeIcon[$theme]}</span>
			{#if !collapsed}<span class="truncate">{themeLabel[$theme]}</span>{/if}
		</button>

		<button
			onclick={handleLogout}
			class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
			title={collapsed ? '登出' : ''}
		>
			<span class="text-base leading-none shrink-0">🚪</span>
			{#if !collapsed}<span class="truncate">登出</span>{/if}
		</button>

		{#if !collapsed && me}
			<div class="px-2.5 py-1.5">
				<p class="text-[11px] text-zinc-400 dark:text-zinc-500 truncate">
					{#if isAdmin}
						<span class="text-amber-600 dark:text-amber-400">Admin</span> ·
					{/if}
					{me.subject?.user_id?.slice(0, 8) ?? ''}...
				</p>
			</div>
		{/if}
	</div>
</aside>
