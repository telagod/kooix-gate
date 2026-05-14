import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/svelte';
import ErrorPage from '../routes/+error.svelte';

describe('+error.svelte', () => {
	it('renders status code and message', () => {
		render(ErrorPage);
		expect(screen.getByText('出错了')).toBeInTheDocument();
		expect(screen.getByText('返回首页')).toBeInTheDocument();
	});

	it('renders status number from page store', () => {
		render(ErrorPage);
		expect(screen.getByText('200')).toBeInTheDocument();
	});

	it('has a link to home', () => {
		render(ErrorPage);
		const link = screen.getByText('返回首页');
		expect(link).toHaveAttribute('href', '/');
	});
});
