<script lang="ts">
	import { goto } from '$app/navigation';
	import { onMount } from 'svelte';
	import { getAccessToken } from '$lib/auth.js';

	onMount(() => {
		if (getAccessToken()) {
			goto('/orgs');
		} else {
			goto('/login');
		}
	});
</script>

<div class="min-h-screen bg-zinc-50 flex items-center justify-center">
	<p class="text-zinc-400 text-sm">跳转中...</p>
</div>
