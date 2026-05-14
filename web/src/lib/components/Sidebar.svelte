<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import { clearTokens } from '$lib/auth.js';
	import { getMe } from '$lib/api.js';
	import type { MeResult } from '$lib/api.js';
	import { theme, toggleTheme } from '$lib/stores/theme';

	import {
		LayoutDashboard,
		Building2,
		BarChart3,
		FolderOpen,
		Receipt,
		Gauge,
		Cable,
		ClipboardList,
		Users,
		MessageSquare,
		Settings,
		Sun,
		Moon,
		Monitor,
		LogOut,
		PanelLeftClose,
		PanelLeftOpen,
		Shield
	} from 'lucide-svelte';

	let currentPath = $derived($page.url.pathname);
	let me = $state<MeResult | null>(null);
	let collapsed = $state(false);

	onMount(async () => {
		try { me = await getMe(); } catch {}
	});

	let currentOrg = $derived(me?.current_org ?? me?.orgs?.[0] ?? null);
	let isAdmin = $derived(me?.is_platform_admin ?? false);

	function active(pattern: string): boolean {
		if (pattern === '/dashboard') return currentPath === '/dashboard';
		if (pattern === '/orgs') return currentPath === '/orgs' || (currentPath.startsWith('/orgs/') && !currentPath.includes('/projects') && !currentPath.includes('/billing') && !currentPath.includes('/quotas'));
		if (pattern === '/usage') return currentPath.startsWith('/usage');
		if (pattern === '/playground') return currentPath.startsWith('/playground');
		if (pattern === '/settings') return currentPath.startsWith('/settings');
		if (pattern === '/channels') return currentPath === '/channels' || currentPath.startsWith('/channels/');
		if (pattern === '/admin/users') return currentPath.startsWith('/admin/users');
		if (pattern === '/admin/audit') return currentPath.startsWith('/admin/audit');
		return currentPath.startsWith(pattern);
	}

	function linkCls(pattern: string): string {
		const base = 'flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm transition-colors mb-0.5';
		return active(pattern)
			? `${base} bg-zinc-900 text-white dark:bg-zinc-100 dark:text-zinc-900`
			: `${base} text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800`;
	}

	const iconSize = 18;
</script>

<aside class="flex flex-col h-full {collapsed ? 'w-14' : 'w-56'} bg-white dark:bg-zinc-900 border-r border-zinc-200 dark:border-zinc-700 transition-all duration-200 shrink-0">
	<!-- Header -->
	<div class="flex items-center justify-between px-3 h-14 border-b border-zinc-200 dark:border-zinc-700">
		{#if !collapsed}
			<a href="/orgs" class="text-sm font-bold text-zinc-900 dark:text-zinc-100 truncate">Kooix Gate</a>
		{/if}
		<button
			onclick={() => (collapsed = !collapsed)}
			class="p-1.5 rounded-md text-zinc-400 dark:text-zinc-500 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
			title={collapsed ? '展开侧栏' : '收起侧栏'}
		>
			{#if collapsed}
				<PanelLeftOpen size={16} />
			{:else}
				<PanelLeftClose size={16} />
			{/if}
		</button>
	</div>

	<!-- Nav -->
	<nav class="flex-1 overflow-y-auto py-3 px-2 space-y-5">
		<!-- Main -->
		<div>
			{#if !collapsed}
				<p class="px-2.5 mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500">导航</p>
			{/if}
			<a href="/dashboard" class={linkCls('/dashboard')} title={collapsed ? '总览' : ''}>
				<LayoutDashboard size={iconSize} />
				{#if !collapsed}<span class="truncate">总览</span>{/if}
			</a>
			<a href="/orgs" class={linkCls('/orgs')} title={collapsed ? '组织' : ''}>
				<Building2 size={iconSize} />
				{#if !collapsed}<span class="truncate">组织</span>{/if}
			</a>
			<a href="/usage" class={linkCls('/usage')} title={collapsed ? '用量' : ''}>
				<BarChart3 size={iconSize} />
				{#if !collapsed}<span class="truncate">用量</span>{/if}
			</a>
			<a href="/playground" class={linkCls('/playground')} title={collapsed ? 'Playground' : ''}>
				<MessageSquare size={iconSize} />
				{#if !collapsed}<span class="truncate">Playground</span>{/if}
			</a>
		</div>

		<!-- Org context -->
		{#if currentOrg}
			<div>
				{#if !collapsed}
					<p class="px-2.5 mb-1.5 text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500">当前组织</p>
				{/if}
				<a href="/orgs/{currentOrg}/projects" class={linkCls(`/orgs/${currentOrg}/projects`)} title={collapsed ? '项目' : ''}>
					<FolderOpen size={iconSize} />
					{#if !collapsed}<span class="truncate">项目</span>{/if}
				</a>
				<a href="/orgs/{currentOrg}/billing" class={linkCls(`/orgs/${currentOrg}/billing`)} title={collapsed ? '账单' : ''}>
					<Receipt size={iconSize} />
					{#if !collapsed}<span class="truncate">账单</span>{/if}
				</a>
				<a href="/orgs/{currentOrg}/quotas" class={linkCls(`/orgs/${currentOrg}/quotas`)} title={collapsed ? '配额' : ''}>
					<Gauge size={iconSize} />
					{#if !collapsed}<span class="truncate">配额</span>{/if}
				</a>
			</div>
		{/if}

		<!-- Admin -->
		{#if isAdmin}
			<div>
				{#if !collapsed}
					<div class="px-2.5 mb-1.5 flex items-center gap-1.5">
						<p class="text-[10px] font-semibold uppercase tracking-wider text-zinc-400 dark:text-zinc-500">管理员</p>
						<span class="inline-block px-1 py-0 rounded text-[9px] font-medium bg-amber-100 dark:bg-amber-900/40 text-amber-700 dark:text-amber-400">Admin</span>
					</div>
				{:else}
					<div class="flex justify-center mb-1">
						<Shield size={12} class="text-amber-500" />
					</div>
				{/if}
				<a href="/channels" class={linkCls('/channels')} title={collapsed ? '渠道管理' : ''}>
					<Cable size={iconSize} />
					{#if !collapsed}<span class="truncate">渠道管理</span>{/if}
				</a>
				<a href="/admin/users" class={linkCls('/admin/users')} title={collapsed ? '用户管理' : ''}>
					<Users size={iconSize} />
					{#if !collapsed}<span class="truncate">用户管理</span>{/if}
				</a>
				<a href="/admin/audit" class={linkCls('/admin/audit')} title={collapsed ? '审计日志' : ''}>
					<ClipboardList size={iconSize} />
					{#if !collapsed}<span class="truncate">审计日志</span>{/if}
				</a>
			</div>
		{/if}
	</nav>

	<!-- Bottom -->
	<div class="border-t border-zinc-200 dark:border-zinc-700 p-2 space-y-0.5">
		<a href="/settings" class={linkCls('/settings')} title={collapsed ? '设置' : ''}>
			<Settings size={iconSize} />
			{#if !collapsed}<span class="truncate">个人设置</span>{/if}
		</a>

		<button
			onclick={toggleTheme}
			title={collapsed ? '切换主题' : ''}
			class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
		>
			{#if $theme === 'light'}
				<Sun size={iconSize} />
				{#if !collapsed}<span class="truncate">浅色</span>{/if}
			{:else if $theme === 'dark'}
				<Moon size={iconSize} />
				{#if !collapsed}<span class="truncate">深色</span>{/if}
			{:else}
				<Monitor size={iconSize} />
				{#if !collapsed}<span class="truncate">跟随系统</span>{/if}
			{/if}
		</button>

		<button
			onclick={() => { clearTokens(); goto('/login'); }}
			title={collapsed ? '登出' : ''}
			class="w-full flex items-center gap-2.5 px-2.5 py-2 rounded-md text-sm text-zinc-600 dark:text-zinc-400 hover:bg-zinc-100 dark:hover:bg-zinc-800 transition-colors"
		>
			<LogOut size={iconSize} />
			{#if !collapsed}<span class="truncate">登出</span>{/if}
		</button>

		{#if !collapsed && me}
			<div class="px-2.5 py-1">
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
