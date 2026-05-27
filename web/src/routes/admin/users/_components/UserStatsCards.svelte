<script lang="ts">
	// admin/users/_components/UserStatsCards.svelte — 0.4.179 抽出
	// 父：admin/users/+page.svelte 424-438 行 3 张计数卡
	import { Card } from '$lib/components/ui';
	import { text } from '$lib/design';

	let { total, active, suspended }: { total: number; active: number; suspended: number } = $props();
</script>

<div class="mb-4 grid gap-3 md:grid-cols-3">
	<Card padding="md">
		<p class="text-xs uppercase tracking-wider {text.muted}">总用户</p>
		<p class="mt-1 text-2xl font-semibold {text.primary}">{total}</p>
	</Card>
	<Card padding="md" variant="success">
		<p class="text-xs uppercase tracking-wider {text.success}">Active 启用</p>
		<p class="mt-1 text-2xl font-semibold {text.primary}">{active}</p>
	</Card>
	<Card padding="md" variant={suspended > 0 ? 'danger' : 'default'}>
		<p class="text-xs uppercase tracking-wider {suspended > 0 ? text.danger : text.muted}">Suspended 停用</p>
		<p class="mt-1 text-2xl font-semibold {text.primary}">{suspended}</p>
	</Card>
</div>
