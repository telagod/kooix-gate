import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import Button from '$lib/components/ui/Button.svelte';

function textSnippet(text: string) {
	return createRawSnippet(() => ({
		render: () => `<span>${text}</span>`
	}));
}

describe('Button component', () => {
	it('renders with default variant', () => {
		render(Button, { props: { children: textSnippet('Click me') } });
		const btn = screen.getByRole('button');
		expect(btn).toBeInTheDocument();
		expect(btn).toHaveTextContent('Click me');
		expect(btn.className).toContain('bg-zinc-900');
	});

	it('renders outline variant', () => {
		render(Button, { props: { variant: 'outline', children: textSnippet('Outline') } });
		const btn = screen.getByRole('button');
		expect(btn.className).toContain('border');
	});

	it('renders destructive variant', () => {
		render(Button, { props: { variant: 'destructive', children: textSnippet('Del') } });
		const btn = screen.getByRole('button');
		expect(btn.className).toContain('bg-red-600');
	});

	it('respects disabled prop', () => {
		render(Button, { props: { disabled: true, children: textSnippet('No') } });
		expect(screen.getByRole('button')).toBeDisabled();
	});

	it('renders sm size class', () => {
		render(Button, { props: { size: 'sm', children: textSnippet('S') } });
		expect(screen.getByRole('button').className).toContain('h-8');
	});

	it('renders lg size class', () => {
		render(Button, { props: { size: 'lg', children: textSnippet('L') } });
		expect(screen.getByRole('button').className).toContain('h-12');
	});

	it('sets submit type', () => {
		render(Button, { props: { type: 'submit', children: textSnippet('Go') } });
		expect(screen.getByRole('button')).toHaveAttribute('type', 'submit');
	});
});
