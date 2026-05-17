<script module lang="ts">
	export interface SelectOption {
		value: string | null;
		label: string;
	}
</script>

<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { HTMLSelectAttributes } from 'svelte/elements';
	import { controlClass, type ControlSize } from '$lib/design';

	let {
		value = $bindable(''),
		options = [],
		id = '',
		disabled = false,
		class: className = '',
		size = 'default',
		invalid = false,
		onchange = undefined,
		children
	}: {
		value?: string | null;
		options?: SelectOption[];
		id?: string;
		disabled?: boolean;
		class?: string;
		size?: ControlSize;
		invalid?: boolean;
		onchange?: HTMLSelectAttributes['onchange'];
		children?: Snippet;
	} = $props();
</script>

<select
	{id}
	{disabled}
	{onchange}
	bind:value
	class={controlClass({ size, invalid, class: className })}
>
	{#if children}
		{@render children()}
	{:else}
		{#each options as opt}
			<option value={opt.value}>{opt.label}</option>
		{/each}
	{/if}
</select>
