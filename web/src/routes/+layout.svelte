<!-- Root layout：login 页面不显示导航；其他页面渲染统一 NavBar -->
<script lang="ts">
	import favicon from '$lib/assets/favicon.svg';
	import { page } from '$app/stores';
	import NavBar from '$lib/components/NavBar.svelte';
	import '../app.css';

	let { children } = $props();
	let path = $derived($page.url.pathname);
	let showNav = $derived(path !== '/login' && path !== '/');
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{#if showNav}
	<div class="min-h-screen bg-zinc-50">
		<NavBar />
		{@render children()}
	</div>
{:else}
	{@render children()}
{/if}
