<script lang="ts">
	import { Button } from '$lib/components/ui';
	import { ArrowLeft, ArrowRight } from 'lucide-svelte';

	interface Props {
		pageNum: number;
		pageDataLength: number;
		hasMore: boolean;
		loading: boolean;
		canPrev: boolean;
		onPrev: () => void;
		onNext: () => void;
	}

	let { pageNum, pageDataLength, hasMore, loading, canPrev, onPrev, onNext }: Props = $props();
</script>

<div class="flex items-center justify-between text-sm text-zinc-600 dark:text-zinc-400">
	<span class="text-xs">
		第 {pageNum} 页 · {pageDataLength} 条
		{#if loading}<span class="ml-2 text-zinc-400">加载中...</span>{/if}
	</span>
	<div class="flex items-center gap-2">
		<Button variant="outline" size="sm" onclick={onPrev} disabled={!canPrev || loading}>
			<ArrowLeft size={14} />
			<span class="ml-1">上一页</span>
		</Button>
		<Button variant="outline" size="sm" onclick={onNext} disabled={!hasMore || loading}>
			<span class="mr-1">下一页</span>
			<ArrowRight size={14} />
		</Button>
	</div>
</div>
