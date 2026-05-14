<script lang="ts">
	import { clsx } from 'clsx';

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
		return `/providers/${slug}.svg`;
	}
</script>

{#if mode === 'grid'}
	<div class="grid grid-cols-3 gap-2 {className}">
		{#each options as opt}
			<button
				type="button"
				{disabled}
				onclick={() => (value = opt.value)}
				class={clsx(
					'flex flex-col items-center gap-1.5 px-3 py-3 rounded-lg border text-center transition-all',
					value === opt.value
						? 'border-zinc-900 dark:border-zinc-100 bg-zinc-50 dark:bg-zinc-800 ring-1 ring-zinc-900 dark:ring-zinc-100'
						: 'border-zinc-200 dark:border-zinc-700 hover:border-zinc-400 dark:hover:border-zinc-500 bg-white dark:bg-zinc-900',
					disabled && 'opacity-50 pointer-events-none'
				)}
			>
				{#if opt.value}<img src={logoSrc(opt.value)} alt={opt.label} class="w-5 h-5 dark:invert" />{/if}
				<span class="text-xs font-medium text-zinc-900 dark:text-zinc-100">{opt.label}</span>
				{#if opt.description}
					<span class="text-[10px] text-zinc-500 dark:text-zinc-400 line-clamp-1">{opt.description}</span>
				{/if}
			</button>
		{/each}
	</div>
{:else}
	<div class="relative {className}">
		<button
			type="button"
			{disabled}
			onclick={toggle}
			class={clsx(
				'flex h-10 w-full items-center justify-between rounded-md border px-3 py-2 text-sm transition-colors',
				'border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-800',
				'text-zinc-900 dark:text-zinc-100',
				'focus:outline-none focus:ring-2 focus:ring-zinc-900 dark:focus:ring-zinc-300',
				disabled && 'opacity-50 cursor-not-allowed'
			)}
		>
			<span class="flex items-center gap-2 truncate">
				{#if selected}
					{#if selected.value}<img src={logoSrc(selected.value)} alt={selected.label} class="w-4 h-4 dark:invert" />{/if}
					<span>{selected.label}</span>
				{:else}
					<span class="text-zinc-500 dark:text-zinc-400">{placeholder}</span>
				{/if}
			</span>
			<svg class="w-4 h-4 text-zinc-400 shrink-0" viewBox="0 0 20 20" fill="currentColor">
				<path fill-rule="evenodd" d="M5.293 7.293a1 1 0 011.414 0L10 10.586l3.293-3.293a1 1 0 111.414 1.414l-4 4a1 1 0 01-1.414 0l-4-4a1 1 0 010-1.414z" clip-rule="evenodd" />
			</svg>
		</button>

		{#if open}
			<div class="absolute z-50 mt-1 w-full min-w-[200px] max-h-64 overflow-hidden rounded-lg border border-zinc-200 dark:border-zinc-700 bg-white dark:bg-zinc-900 shadow-lg">
				<div class="p-2 border-b border-zinc-100 dark:border-zinc-800">
					<input
						bind:this={inputEl}
						bind:value={search}
						onkeydown={onKeydown}
						placeholder="搜索..."
						class="w-full px-2 py-1.5 text-sm rounded-md bg-zinc-50 dark:bg-zinc-800 text-zinc-900 dark:text-zinc-100 placeholder:text-zinc-400 border-none outline-none"
					/>
				</div>
				<div class="overflow-y-auto max-h-48 py-1">
					{#if filtered.length === 0}
						<p class="px-3 py-2 text-xs text-zinc-500 dark:text-zinc-400">无匹配结果</p>
					{:else}
						{#each filtered as opt, i}
							<button
								type="button"
								onclick={() => pick(opt)}
								class={clsx(
									'flex w-full items-center gap-2.5 px-3 py-2 text-sm text-left transition-colors',
									i === highlightIdx
										? 'bg-zinc-100 dark:bg-zinc-800'
										: 'hover:bg-zinc-50 dark:hover:bg-zinc-800/50',
									value === opt.value && 'font-medium'
								)}
							>
								{#if opt.value}<img src={logoSrc(opt.value)} alt={opt.label} class="w-4 h-4 shrink-0 dark:invert" />{/if}
								<div class="flex-1 min-w-0">
									<p class="text-zinc-900 dark:text-zinc-100 truncate">{opt.label}</p>
									{#if opt.description}
										<p class="text-[10px] text-zinc-500 dark:text-zinc-400 truncate">{opt.description}</p>
									{/if}
								</div>
								{#if value === opt.value}
									<svg class="w-4 h-4 text-zinc-900 dark:text-zinc-100 shrink-0" viewBox="0 0 20 20" fill="currentColor">
										<path fill-rule="evenodd" d="M16.707 5.293a1 1 0 010 1.414l-8 8a1 1 0 01-1.414 0l-4-4a1 1 0 011.414-1.414L8 12.586l7.293-7.293a1 1 0 011.414 0z" clip-rule="evenodd" />
									</svg>
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
	<div class="fixed inset-0 z-40" onclick={() => (open = false)}></div>
{/if}
