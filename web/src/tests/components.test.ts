import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import { createRawSnippet } from 'svelte';
import Card from '$lib/components/ui/Card.svelte';
import Input from '$lib/components/ui/Input.svelte';
import Skeleton from '$lib/components/ui/Skeleton.svelte';
import Stat from '$lib/components/Stat.svelte';

function textSnippet(text: string) {
	return createRawSnippet(() => ({
		render: () => `<span>${text}</span>`
	}));
}

describe('Card component', () => {
	it('renders children content', () => {
		render(Card, { props: { children: textSnippet('Card body') } });
		expect(screen.getByText('Card body')).toBeInTheDocument();
	});

	it('applies custom class', () => {
		const { container } = render(Card, { props: { class: 'p-8', children: textSnippet('x') } });
		const div = container.querySelector('.p-8');
		expect(div).toBeInTheDocument();
	});

	it('has default border and shadow', () => {
		const { container } = render(Card, { props: { children: textSnippet('x') } });
		const div = container.firstElementChild;
		expect(div?.className).toContain('border');
		expect(div?.className).toContain('shadow-sm');
	});
});

describe('Input component', () => {
	it('renders input element', () => {
		render(Input, { props: { id: 'email', placeholder: 'Email' } });
		const input = screen.getByPlaceholderText('Email');
		expect(input).toBeInTheDocument();
		expect(input).toHaveAttribute('id', 'email');
	});

	it('respects disabled prop', () => {
		render(Input, { props: { disabled: true, placeholder: 'no' } });
		expect(screen.getByPlaceholderText('no')).toBeDisabled();
	});

	it('renders with type password', () => {
		render(Input, { props: { type: 'password', placeholder: 'pw' } });
		expect(screen.getByPlaceholderText('pw')).toHaveAttribute('type', 'password');
	});

	it('applies custom class', () => {
		const { container } = render(Input, { props: { class: 'w-64', placeholder: 'x' } });
		const input = container.querySelector('input');
		expect(input?.className).toContain('w-64');
	});
});

describe('Skeleton component', () => {
	it('renders a div with animate-pulse', () => {
		const { container } = render(Skeleton);
		const div = container.firstElementChild;
		expect(div?.className).toContain('animate-pulse');
		expect(div?.className).toContain('bg-zinc-200');
	});

	it('applies custom class', () => {
		const { container } = render(Skeleton, { props: { class: 'h-4 w-32' } });
		const div = container.firstElementChild;
		expect(div?.className).toContain('h-4');
		expect(div?.className).toContain('w-32');
	});
});

describe('Stat component', () => {
	it('renders title and value', () => {
		render(Stat, { props: { title: 'Cost', value: '$1.23' } });
		expect(screen.getByText('Cost')).toBeInTheDocument();
		expect(screen.getByText('$1.23')).toBeInTheDocument();
	});

	it('renders subtitle when provided', () => {
		render(Stat, { props: { title: 'T', value: '0', subtitle: 'per day' } });
		expect(screen.getByText('per day')).toBeInTheDocument();
	});

	it('does not render subtitle element when not provided', () => {
		const { container } = render(Stat, { props: { title: 'T', value: '0' } });
		const allP = container.querySelectorAll('p');
		expect(allP.length).toBe(2);
	});
});
