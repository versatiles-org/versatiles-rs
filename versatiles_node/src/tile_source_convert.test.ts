import { describe, it, expect, beforeEach, afterEach } from 'vitest';
import { TileSource } from '../index.js';
import { tmpdir } from 'os';
import path from 'path';
import fs from 'fs';

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');

describe('TileSource.convertTo()', () => {
	let outputPath: string;

	beforeEach(() => {
		outputPath = path.join(tmpdir(), `test-convert-${Date.now()}.versatiles`);
	});

	afterEach(() => {
		if (fs.existsSync(outputPath)) {
			fs.unlinkSync(outputPath);
		}
	});

	it('should convert MBTiles to VersaTiles format', async () => {
		const source = await TileSource.open(MBTILES_PATH);
		await source.convertTo(outputPath);

		expect(fs.existsSync(outputPath)).toBeTruthy();
		const stats = fs.statSync(outputPath);
		expect(stats.size).toBeGreaterThan(0);
	});

	it('should convert with zoom filter options', async () => {
		const source = await TileSource.open(MBTILES_PATH);
		await source.convertTo(outputPath, {
			minZoom: 5,
			maxZoom: 10,
			compress: 'gzip',
		});

		expect(fs.existsSync(outputPath)).toBeTruthy();
	});

	it('should convert with bbox filter', async () => {
		const source = await TileSource.open(MBTILES_PATH);
		await source.convertTo(outputPath, {
			bbox: [13.0, 52.0, 14.0, 53.0], // Berlin area
			bboxBorder: 1,
		});

		expect(fs.existsSync(outputPath)).toBeTruthy();
	});

	it('should receive progress updates', async () => {
		const source = await TileSource.open(MBTILES_PATH);
		const progressUpdates: number[] = [];

		await source.convertTo(
			outputPath,
			null,
			(progress) => {
				progressUpdates.push(progress.percentage);
			},
			null,
		);

		expect(progressUpdates.length).toBeGreaterThan(0);
		expect(progressUpdates[progressUpdates.length - 1]).toBeCloseTo(100, 0);
	});

	it('should receive message updates', async () => {
		const source = await TileSource.open(MBTILES_PATH);
		const messages: Array<{ type: string; message: string }> = [];

		await source.convertTo(outputPath, null, null, (data) => {
			messages.push(data);
		});

		expect(messages.length).toBeGreaterThan(0);
		// Messages can be 'step', 'warning', or 'error'
		const validTypes = messages.every((m) => ['step', 'warning', 'error'].includes(m.type));
		expect(validTypes).toBeTruthy();
	});

	it('should convert VPL sources successfully', async () => {
		const vpl = 'from_container filename="berlin.mbtiles" | filter level_min=5 level_max=10';
		const source = await TileSource.fromVpl(vpl, TESTDATA_DIR);

		await source.convertTo(outputPath);

		expect(fs.existsSync(outputPath)).toBeTruthy();
		const stats = fs.statSync(outputPath);
		expect(stats.size).toBeGreaterThan(0);
	});
});
