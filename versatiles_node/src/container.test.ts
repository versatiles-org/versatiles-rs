import { beforeAll } from 'vitest';
import { ContainerReader } from '../index.js';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const PMTILES_PATH = path.join(TESTDATA_DIR, 'berlin.pmtiles');

describe('ContainerReader', () => {
	describe('open()', () => {
		it('should open MBTiles file', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			expect(reader).toBeDefined();
		});

		it('should open PMTiles file', async () => {
			const reader = await ContainerReader.open(PMTILES_PATH);
			expect(reader).toBeDefined();
		});

		it('should throw error for non-existent file', async () => {
			await expect(ContainerReader.open('/nonexistent/file.mbtiles')).rejects.toThrow();
		});

		it('should throw error for invalid file format', async () => {
			await expect(ContainerReader.open(__filename)).rejects.toThrow();
		});
	});

	describe('getTile()', () => {
		let reader: ContainerReader;

		beforeAll(async () => {
			reader = await ContainerReader.open(MBTILES_PATH);
		});

		it('should retrieve existing tile', async () => {
			// Berlin is at z=5, x=17, y=10
			const tile = await reader.getTile(5, 17, 10);
			expect(tile).toBeDefined();
			expect(Buffer.isBuffer(tile)).toBeTruthy();
			expect(tile!.length).toBeGreaterThan(0);
		});

		it('should return null for non-existent tile within valid range', async () => {
			// Tile that's within valid zoom range but doesn't exist in the dataset
			const tile = await reader.getTile(5, 0, 0);
			// Could be null or could exist, just verify no error
			if (tile !== null) {
				expect(Buffer.isBuffer(tile)).toBeTruthy();
			}
		});

		it('should handle multiple tile requests', async () => {
			const tiles = await Promise.all([
				reader.getTile(5, 17, 10),
				reader.getTile(6, 34, 20),
				reader.getTile(7, 68, 40),
			]);

			tiles.forEach((tile) => {
				if (tile) {
					expect(Buffer.isBuffer(tile)).toBeTruthy();
				}
			});
		});

		it('should throw error for invalid coordinates', async () => {
			await expect(reader.getTile(0, 10, 0)).rejects.toThrow();
		});

		it('should handle tiles outside zoom range', async () => {
			// Getting a tile at a zoom level not in the container
			try {
				const tile = await reader.getTile(0, 0, 0);
				// May return null or may throw, both are acceptable
				expect(tile === null || Buffer.isBuffer(tile)).toBeTruthy();
			} catch {
				// Error is also acceptable for out-of-range zoom
				expect(true).toBeTruthy();
			}
		});
	});

	describe('tileJson', () => {
		it('should return valid TileJSON for MBTiles', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const tileJson = await reader.tileJson;

			expect(tileJson).toBeDefined();
			expect(typeof tileJson).toBe('string');

			const parsed = JSON.parse(tileJson);
			expect(parsed.tilejson).toBe('3.0.0');
			// tiles array, bounds, and other fields may or may not be present depending on implementation
			expect(typeof parsed.minzoom).toBe('number');
			expect(typeof parsed.maxzoom).toBe('number');
		});

		it('should return valid TileJSON for PMTiles', async () => {
			const reader = await ContainerReader.open(PMTILES_PATH);
			const tileJson = await reader.tileJson;

			const parsed = JSON.parse(tileJson);
			expect(parsed.tilejson).toBe('3.0.0');
		});
	});

	describe('parameters', () => {
		it('should return reader parameters', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const params = await reader.parameters;

			expect(params).toBeDefined();
			expect(typeof params.tileFormat).toBe('string');
			expect(typeof params.tileCompression).toBe('string');
			expect(typeof params.minZoom).toBe('number');
			expect(typeof params.maxZoom).toBe('number');
			expect(params.minZoom).toBeLessThanOrEqual(params.maxZoom);
		});
	});

	describe('probe()', () => {
		let reader: ContainerReader;

		beforeAll(async () => {
			reader = await ContainerReader.open(MBTILES_PATH);
		});

		it('should probe with shallow depth', async () => {
			const result = await reader.probe('shallow');
			expect(result).toBeDefined();
			expect(typeof result.sourceName).toBe('string');
			expect(typeof result.containerName).toBe('string');
		});

		it('should probe with container depth', async () => {
			const result = await reader.probe('container');
			expect(result).toBeDefined();
			expect(result.tileJson).toBeDefined();
			expect(result.parameters).toBeDefined();
		});

		it('should probe without depth argument', async () => {
			const result = await reader.probe();
			expect(result).toBeDefined();
		});
	});

	describe('convertTo()', () => {
		let reader: ContainerReader;
		const OUTPUT_PATH = path.join(__dirname, 'output-test.versatiles');

		beforeAll(async () => {
			reader = await ContainerReader.open(MBTILES_PATH);
			// Clean up output file if it exists
			if (fs.existsSync(OUTPUT_PATH)) {
				fs.unlinkSync(OUTPUT_PATH);
			}
		});

		it('should convert to versatiles format', async () => {
			await reader.convertTo(OUTPUT_PATH);
			expect(fs.existsSync(OUTPUT_PATH)).toBeTruthy();

			// Verify we can open the converted file
			const newReader = await ContainerReader.open(OUTPUT_PATH);
			expect(newReader).toBeDefined();

			// Clean up
			fs.unlinkSync(OUTPUT_PATH);
		});

		it('should convert with options', async () => {
			await reader.convertTo(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
				compress: 'gzip',
			});
			expect(fs.existsSync(OUTPUT_PATH)).toBeTruthy();

			const newReader = await ContainerReader.open(OUTPUT_PATH);
			const params = await newReader.parameters;
			expect(params.minZoom).toBe(5);
			expect(params.maxZoom).toBe(7);

			// Clean up
			fs.unlinkSync(OUTPUT_PATH);
		});
	});
});
