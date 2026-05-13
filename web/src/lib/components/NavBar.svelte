<!-- 顶栏导航：登录后页面通用 -->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { clearTokens } from '$lib/auth.js';
	import Button from '$lib/components/ui/Button.svelte';

	let currentPath = $derived($page.url.pathname);

	function isActive(prefix: string): boolean {
		if (prefix === '/orgs') {
			return currentPath === '/orgs' || currentPath.startsWith('/orgs/');
		}
		return currentPath.startsWith(prefix);
	}

	function handleLogout() {
		clearTokens();
		goto('/login');
	}
</script>

<nav class="bg-white border-b border-zinc-200 px-6 py-3 flex items-center justify-between">
	<div class="flex items-center gap-6">
		<a href="/orgs" class="text-lg font-bold text-zinc-900 hover:text-zinc-700">Kooix Gate</a>
		<div class="flex items-center gap-1">
			<a
				href="/orgs"
				class="px-3 py-1.5 text-sm rounded-md transition-colors {isActive('/orgs')
					? 'bg-zinc-900 text-white'
					: 'text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900'}"
			>
				Orgs
			</a>
			<a
				href="/usage"
				class="px-3 py-1.5 text-sm rounded-md transition-colors {isActive('/usage')
					? 'bg-zinc-900 text-white'
					: 'text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900'}"
			>
				Usage
			</a>
			<a
				href="/channels"
				class="px-3 py-1.5 text-sm rounded-md transition-colors {isActive('/channels')
					? 'bg-zinc-900 text-white'
					: 'text-zinc-600 hover:bg-zinc-100 hover:text-zinc-900'}"
			>
				Channels
			</a>
		</div>
	</div>
	<Button variant="ghost" size="sm" onclick={handleLogout}>登出</Button>
</nav>
