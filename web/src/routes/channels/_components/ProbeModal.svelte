<script lang="ts">
	import { Button, Card } from '$lib/components/ui';
	import ModalFrame from '$lib/components/templates/ModalFrame.svelte';
	import type { ProbeResponse } from '$lib/api.js';

	interface Props {
		probingId: string | null;
		probeResult: ProbeResponse | null;
		probeChannelName: string;
		syncingProbe: boolean;
		onClose: () => void;
		onSync: () => void;
	}

	let { probingId, probeResult, probeChannelName, syncingProbe, onClose, onSync }: Props = $props();
</script>

{#if probingId && probeResult}
	<ModalFrame close={onClose} class="z-50 bg-black/60 backdrop-blur-sm animate-backdrop">
		<Card class="p-6 max-w-md w-full mx-4 animate-fade-in shadow-2xl">
			<h3 class="text-lg font-semibold text-zinc-900 dark:text-zinc-100 mb-1">Probe — {probeChannelName}</h3>
			<p class="text-xs text-zinc-500 dark:text-zinc-400 mb-3 font-mono">{probeResult.provider_type}</p>
			<p class="text-sm font-medium text-zinc-700 dark:text-zinc-300 mb-2">发现 {probeResult.models.length} 个模型</p>
			<div class="max-h-56 overflow-y-auto rounded-md border border-zinc-200 dark:border-zinc-700 bg-zinc-50 dark:bg-zinc-800/50 p-2 space-y-0.5">
				{#each probeResult.models as m}
					<div class="text-xs font-mono text-zinc-700 dark:text-zinc-300 px-2 py-1 hover:bg-zinc-100 dark:hover:bg-zinc-700 rounded">{m}</div>
				{/each}
			</div>
			<div class="flex gap-2 justify-end mt-4">
				<Button variant="outline" type="button" onclick={onClose}>关闭</Button>
				<Button type="button" disabled={syncingProbe} onclick={onSync}>
					{syncingProbe ? '同步中...' : '同步到 Channel'}
				</Button>
			</div>
		</Card>
	</ModalFrame>
{/if}

{#if probingId && !probeResult}
	<div class="fixed inset-0 z-50 bg-black/50 backdrop-blur-sm flex items-center justify-center animate-backdrop">
		<Card class="p-6 max-w-xs w-full mx-4 flex flex-col items-center gap-3 animate-fade-in">
			<div class="w-10 h-10 rounded-full border-2 border-zinc-200 dark:border-zinc-700 border-t-zinc-900 dark:border-t-zinc-100 animate-spin"></div>
			<p class="text-sm text-zinc-600 dark:text-zinc-300">Probe {probeChannelName}...</p>
			<Button variant="outline" size="sm" onclick={onClose}>取消</Button>
		</Card>
	</div>
{/if}
