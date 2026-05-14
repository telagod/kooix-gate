<!-- Root layout：login 页面不显示导航；其他页面渲染统一 NavBar -->
<script lang="ts">
	import favicon from '$lib/assets/favicon.svg';
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import NavBar from '$lib/components/NavBar.svelte';
	import Toast from '$lib/components/Toast.svelte';
	import { initTheme } from '$lib/stores/theme';
	import '../app.css';

	let { children } = $props();
	let path = $derived($page.url.pathname);
	let showNav = $derived(path !== '/login' && path !== '/' && path !== '/setup');

	onMount(() => initTheme());
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{#if showNav}
	<div class="min-h-screen bg-zinc-50 dark:bg-zinc-950">
		<NavBar />
		{@render children()}
	</div>
{:else}
	{@render children()}
{/if}

<Toast />
