<!-- Root layout: sidebar + content area -->
<script lang="ts">
	import favicon from '$lib/assets/favicon.svg';
	import { page } from '$app/stores';
	import { onMount } from 'svelte';
	import Sidebar from '$lib/components/Sidebar.svelte';
	import Toast from '$lib/components/Toast.svelte';
	import { initTheme } from '$lib/stores/theme';
	import '../app.css';

	let { children } = $props();
	let path = $derived($page.url.pathname);
	let showSidebar = $derived(path !== '/login' && path !== '/' && path !== '/setup');

	onMount(() => initTheme());
</script>

<svelte:head>
	<link rel="icon" href={favicon} />
</svelte:head>

{#if showSidebar}
	<div class="flex h-screen bg-zinc-50 dark:bg-zinc-950">
		<Sidebar />
		<main class="flex-1 overflow-y-auto">
			{@render children()}
		</main>
	</div>
{:else}
	{@render children()}
{/if}

<Toast />
