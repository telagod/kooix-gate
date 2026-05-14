<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { getAccessToken } from '$lib/auth.js';
	import { getSystemStatus } from '$lib/api.js';

	onMount(async () => {
		try {
			const status = await getSystemStatus();
			if (!status.initialized) {
				goto('/setup');
				return;
			}
		} catch {
			// 后端不可达时降级到普通登录流程
		}

		if (getAccessToken()) {
			goto('/dashboard');
		} else {
			goto('/login');
		}
	});
</script>

<div class="min-h-screen bg-zinc-50 dark:bg-zinc-950 flex items-center justify-center">
	<p class="text-zinc-400 dark:text-zinc-500 text-sm">加载中...</p>
</div>
