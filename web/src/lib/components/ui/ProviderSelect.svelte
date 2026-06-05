<script lang="ts">
	import { Check, ChevronDown } from 'lucide-svelte';
	import { cn, controlClass, surface, text } from '$lib/design';

	export interface ProviderOption {
		value: string;
		label: string;
		description?: string;
		category?: string;
	}

	let {
		value = $bindable(''),
		options,
		placeholder = '选择 Provider...',
		disabled = false,
		mode = 'select',
		class: className = ''
	}: {
		value?: string;
		options: ProviderOption[];
		placeholder?: string;
		disabled?: boolean;
		mode?: 'select' | 'grid';
		class?: string;
	} = $props();

	let open = $state(false);
	let search = $state('');
	let highlightIdx = $state(0);
	let inputEl = $state<HTMLInputElement | null>(null);

	let filtered = $derived.by(() => {
		if (!search) return options;
		const q = search.toLowerCase();
		return options.filter(o =>
			o.value.includes(q) || o.label.toLowerCase().includes(q) || (o.description ?? '').toLowerCase().includes(q)
		);
	});

	let selected = $derived(options.find(o => o.value === value));

	function toggle() {
		if (disabled) return;
		open = !open;
		if (open) {
			search = '';
			highlightIdx = 0;
			setTimeout(() => inputEl?.focus(), 10);
		}
	}

	function pick(opt: ProviderOption) {
		value = opt.value;
		open = false;
		search = '';
	}

	function onKeydown(e: KeyboardEvent) {
		if (e.key === 'ArrowDown') {
			e.preventDefault();
			highlightIdx = Math.min(highlightIdx + 1, filtered.length - 1);
		} else if (e.key === 'ArrowUp') {
			e.preventDefault();
			highlightIdx = Math.max(highlightIdx - 1, 0);
		} else if (e.key === 'Enter') {
			e.preventDefault();
			if (filtered[highlightIdx]) pick(filtered[highlightIdx]);
		} else if (e.key === 'Escape') {
			open = false;
		}
	}

	function logoSrc(slug: string): string {
		return `/providers/${slug.replaceAll('_', '-')}.svg`;
	}
</script>

{#if mode === 'grid'}
	<div class={cn('grid grid-cols-3 gap-2', className)}>
		{#each options as opt}
			<button
				type="button"
				{disabled}
				onclick={() => (value = opt.value)}
				class={cn(
					'flex flex-col items-center gap-1.5 rounded-lg border px-3 py-3 text-center transition-all',
					value === opt.value
						? 'border-zinc-900 bg-zinc-50 ring-1 ring-zinc-900 dark:border-zinc-100 dark:bg-zinc-800 dark:ring-zinc-100'
						: 'border-zinc-200 bg-white hover:border-zinc-400 dark:border-zinc-700 dark:bg-zinc-900 dark:hover:border-zinc-500',
					disabled && 'pointer-events-none opacity-50'
				)}
			>
				{#if opt.value}<img src={logoSrc(opt.value)} alt={opt.label} class="h-5 w-5" />{/if}
				<span class="text-xs font-medium {text.primary}">{opt.label}</span>
				{#if opt.description}
					<span class="line-clamp-1 text-[10px] {text.muted}">{opt.description}</span>
				{/if}
			</button>
		{/each}
	</div>
{:else}
	<div class={cn('relative', className)}>
		<button
			type="button"
			{disabled}
			onclick={toggle}
			class={cn(controlClass(), 'items-center justify-between', disabled && 'cursor-not-allowed opacity-50')}
		>
			<span class="flex items-center gap-2 truncate">
				{#if selected}
					{#if selected.value}<img src={logoSrc(selected.value)} alt={selected.label} class="h-4 w-4" />{/if}
					<span>{selected.label}</span>
				{:else}
					<span class={text.muted}>{placeholder}</span>
				{/if}
			</span>
			<ChevronDown size={16} class="shrink-0 text-zinc-400" />
		</button>

		{#if open}
			<div class="absolute z-50 mt-1 max-h-64 w-full min-w-[200px] overflow-hidden rounded-lg border {surface.border} {surface.base} shadow-lg">
				<div class="border-b p-2 {surface.borderSubtle}">
					<input
						bind:this={inputEl}
						bind:value={search}
						onkeydown={onKeydown}
						placeholder="搜索..."
						class={controlClass({ size: 'sm', class: 'border-none bg-zinc-50 dark:bg-zinc-800 focus:ring-0' })}
					/>
				</div>
				<div class="max-h-48 overflow-y-auto py-1">
					{#if filtered.length === 0}
						<p class="px-3 py-2 text-xs {text.muted}">无匹配结果</p>
					{:else}
						{#each filtered as opt, i}
							<button
								type="button"
								onclick={() => pick(opt)}
								class={cn(
									'flex w-full items-center gap-2.5 px-3 py-2 text-left text-sm transition-colors',
									i === highlightIdx ? 'bg-zinc-100 dark:bg-zinc-800' : 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50',
									value === opt.value && 'font-medium'
								)}
							>
								{#if opt.value}<img src={logoSrc(opt.value)} alt={opt.label} class="h-4 w-4 shrink-0" />{/if}
								<div class="min-w-0 flex-1">
									<p class="truncate {text.primary}">{opt.label}</p>
									{#if opt.description}
										<p class="truncate text-[10px] {text.muted}">{opt.description}</p>
									{/if}
								</div>
								{#if value === opt.value}
									<Check size={16} class="shrink-0 {text.primary}" />
								{/if}
							</button>
						{/each}
					{/if}
				</div>
			</div>
		{/if}
	</div>
{/if}

{#if open}
	<button type="button" aria-label="关闭 Provider 选择" class="fixed inset-0 z-40 cursor-default appearance-none border-0 bg-transparent p-0" onclick={() => (open = false)}></button>
{/if}
