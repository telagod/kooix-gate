<script lang="ts">
	import type { Snippet } from 'svelte';
	import { buttonClass, type ButtonSize, type ButtonVariant } from '$lib/design';

	let {
		variant = 'default',
		size = 'default',
		disabled = false,
		type = 'button',
		class: className = '',
		onclick,
		children
	}: {
		variant?: ButtonVariant;
		size?: ButtonSize;
		disabled?: boolean;
		type?: 'button' | 'submit' | 'reset';
		class?: string;
		onclick?: () => void;
		children?: Snippet;
	} = $props();
</script>

<button
	{type}
	{disabled}
	{onclick}
	class={buttonClass({ variant, size, class: className })}
>
	{@render children?.()}
</button>
