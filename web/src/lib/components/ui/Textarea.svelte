<script lang="ts">
	import { textareaClass, type ControlSize } from '$lib/design';
	import type { HTMLTextareaAttributes } from 'svelte/elements';

	let {
		value = $bindable(''),
		placeholder = '',
		id = '',
		disabled = false,
		rows = 3,
		class: className = '',
		size = 'default',
		invalid = false,
		onkeydown = undefined,
		oninput = undefined
	}: {
		value?: string;
		placeholder?: string;
		id?: string;
		disabled?: boolean;
		rows?: number;
		class?: string;
		size?: ControlSize;
		invalid?: boolean;
		onkeydown?: HTMLTextareaAttributes['onkeydown'];
		oninput?: HTMLTextareaAttributes['oninput'];
	} = $props();
</script>

<textarea
	{id}
	{placeholder}
	{disabled}
	{rows}
	{onkeydown}
	{oninput}
	bind:value
	class={textareaClass({ size, invalid, class: className })}
></textarea>
