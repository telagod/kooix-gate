<script lang="ts">
	import { page } from '$app/stores';
</script>

<div class="min-h-screen bg-zinc-50 dark:bg-zinc-950 flex items-center justify-center p-4">
	<div class="bg-white dark:bg-zinc-900 rounded-2xl shadow-sm border border-zinc-200 dark:border-zinc-700 p-8 max-w-md w-full text-center">
		<p class="text-5xl font-bold text-zinc-300 dark:text-zinc-600 mb-2">
			{$page.status}
		</p>
		<h1 class="text-xl font-semibold text-zinc-800 dark:text-zinc-200 mb-2">出错了</h1>
		<p class="text-sm text-zinc-500 dark:text-zinc-400 mb-6">
			{$page.error?.message ?? '发生了未知错误'}
		</p>
		<a
			href="/"
			class="inline-block px-5 py-2.5 bg-zinc-900 dark:bg-zinc-100 text-white dark:text-zinc-900 text-sm font-medium rounded-lg hover:bg-zinc-700 dark:hover:bg-zinc-200 transition-colors"
		>
			返回首页
		</a>
	</div>
</div>
