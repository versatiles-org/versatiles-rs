import { defineConfig } from 'vitest/config';

export default defineConfig({
	test: {
		// Test file patterns
		include: ['src/**/*.test.ts'],

		// Use globals to avoid importing describe/test/expect in each file
		globals: true,

		// Environment - Node.js for this project
		environment: 'node',

		// Coverage configuration (optional)
		coverage: {
			provider: 'v8',
			reporter: ['text', 'json', 'html'],
			exclude: ['node_modules/', 'src/**/*.test.ts', 'dist/', 'target/'],
		},

		// Timeout for async operations (increase if HTTP tests need more time)
		testTimeout: 10000,
	},
});
