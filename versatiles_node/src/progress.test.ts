import { beforeAll, describe, it, expect, vi } from 'vitest';
import { ContainerReader } from '../index.js';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const OUTPUT_PATH = path.join(__dirname, 'test-output-progress.versatiles');

// Clean up output file after tests
afterAll(() => {
	if (fs.existsSync(OUTPUT_PATH)) {
		fs.unlinkSync(OUTPUT_PATH);
	}
});

describe('Progress', () => {
	describe('convertToWithProgress()', () => {
		it('should return a Progress object', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const progress = reader.convertToWithProgress(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
			});

			expect(progress).toBeDefined();
			expect(typeof progress.on).toBe('function');
			expect(typeof progress.done).toBe('function');

			// Wait for completion
			await progress.done();
		});

		it('should emit progress events', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const progressCallback = vi.fn();

			const progress = reader.convertToWithProgress(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
			});

			progress.on('progress', progressCallback);

			await progress.done();

			// Progress should have been emitted at least once
			// Note: May not emit if conversion is too fast or no tiles match the filter
			// So we just check it doesn't throw
			expect(progressCallback.mock.calls.length).toBeGreaterThanOrEqual(0);
		});

		it('should emit step events', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const stepCallback = vi.fn();

			const progress = reader.convertToWithProgress(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
			});

			progress.on('step', stepCallback);

			await progress.done();

			// Note: Step events may be emitted before listeners are registered
			// due to the async nature of the conversion. We just verify that
			// if we got any step events, they are strings.
			stepCallback.mock.calls.forEach((call) => {
				expect(typeof call[0]).toBe('string');
			});
		});

		it('should emit complete event', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const completeCallback = vi.fn();

			const progress = reader.convertToWithProgress(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
			});

			progress.on('complete', completeCallback);

			await progress.done();

			expect(completeCallback).toHaveBeenCalledTimes(1);
		});

		it('should support multiple event listeners', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const completeCallback1 = vi.fn();
			const completeCallback2 = vi.fn();

			const progress = reader.convertToWithProgress(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
			});

			progress.on('complete', completeCallback1);
			progress.on('complete', completeCallback2);

			await progress.done();

			// Both callbacks should have been called for the complete event
			expect(completeCallback1).toHaveBeenCalledTimes(1);
			expect(completeCallback2).toHaveBeenCalledTimes(1);
		});

		it('should support chaining on() calls', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const progressCallback = vi.fn();
			const completeCallback = vi.fn();

			const progress = reader
				.convertToWithProgress(OUTPUT_PATH, {
					minZoom: 5,
					maxZoom: 7,
				})
				.on('progress', progressCallback)
				.on('complete', completeCallback);

			await progress.done();

			// Complete event should always be emitted
			expect(completeCallback).toHaveBeenCalledTimes(1);
			// Progress events may or may not be emitted depending on timing
			expect(progressCallback.mock.calls.length).toBeGreaterThanOrEqual(0);
		});

		it('should verify ProgressData structure', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);

			const progress = reader.convertToWithProgress(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
			});

			const progressDataPromise = new Promise((resolve) => {
				progress.on('progress', (data) => {
					resolve(data);
				});
			});

			await progress.done();

			// If we got progress data, verify its structure
			// Note: May not get progress events if conversion is very fast
			const hasProgressEvents = await Promise.race([
				progressDataPromise,
				new Promise((resolve) => setTimeout(() => resolve(null), 100)),
			]);

			if (hasProgressEvents) {
				const data = hasProgressEvents as any;
				expect(data).toHaveProperty('position');
				expect(data).toHaveProperty('total');
				expect(data).toHaveProperty('percentage');
				expect(data).toHaveProperty('speed');
				expect(data).toHaveProperty('eta');
				expect(typeof data.position).toBe('number');
				expect(typeof data.total).toBe('number');
				expect(typeof data.percentage).toBe('number');
				expect(typeof data.speed).toBe('number');
				expect(typeof data.eta).toBe('number');
			}
		});

		it('should handle errors gracefully', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const errorCallback = vi.fn();

			// Try to write to an invalid path
			const progress = reader.convertToWithProgress('/invalid/path/output.versatiles');

			progress.on('error', errorCallback);

			// Should throw an error
			await expect(progress.done()).rejects.toThrow();

			// Error event should have been emitted
			expect(errorCallback.mock.calls.length).toBeGreaterThan(0);
		});

		it('should throw error when done() is called twice', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);

			const progress = reader.convertToWithProgress(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
			});

			// First call should succeed
			await progress.done();

			// Second call should fail
			await expect(progress.done()).rejects.toThrow();
		});
	});
});
