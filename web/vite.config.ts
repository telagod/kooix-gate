import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vitest/config';
import tailwindcss from '@tailwindcss/vite';

export default defineConfig({
	plugins: [tailwindcss(), sveltekit()],
	resolve: {
		conditions: ['browser']
	},
	build: {
		rolldownOptions: {
			checks: {
				pluginTimings: false
			}
		}
	},
	test: {
		include: ['src/**/*.{test,spec}.{js,ts}'],
		environment: 'jsdom',
		setupFiles: ['src/tests/setup.ts'],
		globals: true,
		alias: {
			'$app/environment': '/src/tests/mocks/app-environment.ts',
			'$app/stores': '/src/tests/mocks/app-stores.ts',
			'$app/navigation': '/src/tests/mocks/app-navigation.ts'
		}
	}
});
