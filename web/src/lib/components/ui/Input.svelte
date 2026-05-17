<script lang="ts">
	import type { HTMLInputAttributes } from 'svelte/elements';
	import { controlClass, type ControlSize } from '$lib/design';

	let {
		type = 'text',
		placeholder = '',
		value = $bindable(''),
		id = '',
		disabled = false,
		class: className = '',
		autocomplete = 'off',
		step = undefined,
		min = undefined,
		max = undefined,
		name = undefined,
		size = 'default',
		invalid = false,
		onkeydown = undefined,
		oninput = undefined,
		onblur = undefined,
		onclick = undefined
	}: {
		type?: HTMLInputAttributes['type'];
		placeholder?: string;
		value?: string | number | null;
		id?: string;
		disabled?: boolean;
		class?: string;
		autocomplete?: HTMLInputAttributes['autocomplete'];
		step?: HTMLInputAttributes['step'];
		min?: HTMLInputAttributes['min'];
		max?: HTMLInputAttributes['max'];
		name?: HTMLInputAttributes['name'];
		size?: ControlSize;
		invalid?: boolean;
		onkeydown?: HTMLInputAttributes['onkeydown'];
		oninput?: HTMLInputAttributes['oninput'];
		onblur?: HTMLInputAttributes['onblur'];
		onclick?: HTMLInputAttributes['onclick'];
	} = $props();
</script>

<input
	{type}
	{placeholder}
	{id}
	{name}
	{disabled}
	{autocomplete}
	{step}
	{min}
	{max}
	{onkeydown}
	{oninput}
	{onblur}
	{onclick}
	bind:value
	class={controlClass({ size, invalid, class: className })}
/>
