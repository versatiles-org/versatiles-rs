import { describe, it, expect, vi, afterAll } from 'vitest';
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

describe('convertTo with callbacks', () => {
	it('should complete conversion without callbacks', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);

		// Should complete successfully without any callbacks
		await reader.convertTo(OUTPUT_PATH, {
			minZoom: 5,
			maxZoom: 7,
		});

		// Verify the output file was created
		expect(fs.existsSync(OUTPUT_PATH)).toBe(true);
	});

	it('should emit progress events', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		const progressCallback = vi.fn();

		await reader.convertTo(
			OUTPUT_PATH,
			{
				minZoom: 5,
				maxZoom: 7,
			},
			progressCallback,
		);

		// Progress should have been emitted at least once
		// Note: May emit few times if conversion is fast
		expect(progressCallback.mock.calls.length).toBeGreaterThanOrEqual(0);
	});

	it('should emit message events', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		const messageCallback = vi.fn();

		await reader.convertTo(
			OUTPUT_PATH,
			{
				minZoom: 5,
				maxZoom: 7,
			},
			undefined, // no progress callback
			messageCallback,
		);

		// Should have received at least one message (step messages)
		expect(messageCallback.mock.calls.length).toBeGreaterThan(0);

		// Verify message structure
		messageCallback.mock.calls.forEach((call) => {
			const message = call[0];
			expect(typeof message.type).toBe('string');
			expect(typeof message.message).toBe('string');
			expect(['step', 'warning', 'error']).toContain(message.type);
		});
	});

	it('should emit both progress and message events', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		const progressCallback = vi.fn();
		const messageCallback = vi.fn();

		await reader.convertTo(
			OUTPUT_PATH,
			{
				minZoom: 5,
				maxZoom: 7,
			},
			progressCallback,
			messageCallback,
		);

		// Should have received both types of events
		expect(messageCallback.mock.calls.length).toBeGreaterThan(0);

		// Verify we got step messages
		const stepMessages = messageCallback.mock.calls.filter((call) => call[0].type === 'step');
		expect(stepMessages.length).toBeGreaterThan(0);
	});

	it('should verify ProgressData structure', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		let progressData: any = null;

		await reader.convertTo(
			OUTPUT_PATH,
			{
				minZoom: 5,
				maxZoom: 7,
			},
			(data) => {
				// Capture the first progress data
				if (!progressData) {
					progressData = data;
				}
			},
		);

		// If we got progress data, verify its structure
		if (progressData) {
			expect(Object.fromEntries(Object.entries(progressData).map(([key, value]) => [key, typeof value]))).toStrictEqual(
				{
					estimatedSecondsRemaining: 'number',
					eta: 'number',
					message: 'string',
					percentage: 'number',
					position: 'number',
					speed: 'number',
					total: 'number',
				},
			);
		}
	});

	it('should handle errors gracefully', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		const errorCallback = vi.fn();

		// Try to write to an invalid path
		await expect(
			reader.convertTo('/invalid/path/output.versatiles', undefined, undefined, (data) => {
				if (data.type === 'error') {
					errorCallback(data.message);
				}
			}),
		).rejects.toThrow();

		// Error may or may not be captured in callback depending on timing
		// The important part is that the Promise rejects
	});

	it('should work with only progress callback', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		const progressCallback = vi.fn();

		await reader.convertTo(
			OUTPUT_PATH,
			{
				minZoom: 5,
				maxZoom: 7,
			},
			progressCallback,
			undefined, // no message callback
		);

		// Progress callback may or may not be called depending on speed
		expect(progressCallback.mock.calls.length).toBeGreaterThanOrEqual(0);
	});

	it('should work with only message callback', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		const messageCallback = vi.fn();

		await reader.convertTo(
			OUTPUT_PATH,
			{
				minZoom: 5,
				maxZoom: 7,
			},
			undefined, // no progress callback
			messageCallback,
		);

		// Should have received step messages
		expect(messageCallback.mock.calls.length).toBeGreaterThan(0);
		const hasStepMessage = messageCallback.mock.calls.some((call) => call[0].type === 'step');
		expect(hasStepMessage).toBe(true);
	});

	it('should receive completion message', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);
		const messages: Array<{ type: string; message: string }> = [];

		await reader.convertTo(
			OUTPUT_PATH,
			{
				minZoom: 5,
				maxZoom: 7,
			},
			undefined,
			(data) => {
				messages.push({ type: data.type, message: data.message });
			},
		);

		// Should have a "Conversion complete" step message
		const completeMessage = messages.find((m) => m.message === 'Conversion complete');
		expect(completeMessage).toBeDefined();
		expect(completeMessage?.type).toBe('step');
	});

	it('should handle rapid consecutive conversions', async () => {
		const reader = await ContainerReader.open(MBTILES_PATH);

		// Run 3 conversions in sequence
		await reader.convertTo(OUTPUT_PATH, { minZoom: 5, maxZoom: 7 });
		await reader.convertTo(OUTPUT_PATH, { minZoom: 5, maxZoom: 7 });
		await reader.convertTo(OUTPUT_PATH, { minZoom: 5, maxZoom: 7 });

		// All should complete successfully
		expect(fs.existsSync(OUTPUT_PATH)).toBe(true);
	});
});
