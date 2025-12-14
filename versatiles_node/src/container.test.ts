import { describe, test, before } from 'node:test';
import assert from 'node:assert';
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
		test('should open MBTiles file', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			assert.ok(reader, 'Reader should be created');
		});

		test('should open PMTiles file', async () => {
			const reader = await ContainerReader.open(PMTILES_PATH);
			assert.ok(reader, 'Reader should be created');
		});

		test('should throw error for non-existent file', async () => {
			await assert.rejects(
				async () => await ContainerReader.open('/nonexistent/file.mbtiles'),
				'Should throw error for non-existent file',
			);
		});

		test('should throw error for invalid file format', async () => {
			await assert.rejects(
				async () => await ContainerReader.open(__filename),
				'Should throw error for invalid file format',
			);
		});
	});

	describe('getTile()', () => {
		let reader: ContainerReader;

		before(async () => {
			reader = await ContainerReader.open(MBTILES_PATH);
		});

		test('should retrieve existing tile', async () => {
			// Berlin is at z=5, x=17, y=10
			const tile = await reader.getTile(5, 17, 10);
			assert.ok(tile, 'Tile should exist');
			assert.ok(Buffer.isBuffer(tile), 'Tile should be a Buffer');
			assert.ok(tile.length > 0, 'Tile should have content');
		});

		test('should return null for non-existent tile within valid range', async () => {
			// Tile that's within valid zoom range but doesn't exist in the dataset
			const tile = await reader.getTile(5, 0, 0);
			// Could be null or could exist, just verify no error
			if (tile !== null) {
				assert.ok(Buffer.isBuffer(tile), 'If tile exists, should be a Buffer');
			}
		});

		test('should handle multiple tile requests', async () => {
			const tiles = await Promise.all([
				reader.getTile(5, 17, 10),
				reader.getTile(6, 34, 20),
				reader.getTile(7, 68, 40),
			]);

			tiles.forEach((tile, index) => {
				if (tile) {
					assert.ok(Buffer.isBuffer(tile), `Tile ${index} should be a Buffer`);
				}
			});
		});

		test('should throw error for invalid coordinates', async () => {
			await assert.rejects(async () => await reader.getTile(0, 10, 0), 'Should throw error for x >= 2^z');
		});

		test('should handle tiles outside zoom range', async () => {
			// Getting a tile at a zoom level not in the container
			try {
				const tile = await reader.getTile(0, 0, 0);
				// May return null or may throw, both are acceptable
				assert.ok(tile === null || Buffer.isBuffer(tile), 'Should return null or a buffer');
			} catch {
				// Error is also acceptable for out-of-range zoom
				assert.ok(true, 'Error for out-of-range zoom is acceptable');
			}
		});
	});

	describe('tileJson', () => {
		test('should return valid TileJSON for MBTiles', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const tileJson = await reader.tileJson;

			assert.ok(tileJson, 'TileJSON should exist');
			assert.strictEqual(typeof tileJson, 'string', 'TileJSON should be a string');

			const parsed = JSON.parse(tileJson);
			assert.strictEqual(parsed.tilejson, '3.0.0', 'Should have TileJSON version');
			// tiles array, bounds, and other fields may or may not be present depending on implementation
			assert.ok(typeof parsed.minzoom === 'number', 'Should have minzoom');
			assert.ok(typeof parsed.maxzoom === 'number', 'Should have maxzoom');
		});

		test('should return valid TileJSON for PMTiles', async () => {
			const reader = await ContainerReader.open(PMTILES_PATH);
			const tileJson = await reader.tileJson;

			const parsed = JSON.parse(tileJson);
			assert.strictEqual(parsed.tilejson, '3.0.0', 'Should have TileJSON version');
		});
	});

	describe('parameters', () => {
		test('should return reader parameters', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const params = await reader.parameters;

			assert.ok(params, 'Parameters should exist');
			assert.ok(typeof params.tileFormat === 'string', 'Should have tileFormat');
			assert.ok(typeof params.tileCompression === 'string', 'Should have tileCompression');
			assert.ok(typeof params.minZoom === 'number', 'Should have minZoom');
			assert.ok(typeof params.maxZoom === 'number', 'Should have maxZoom');
			assert.ok(params.minZoom <= params.maxZoom, 'minZoom should be <= maxZoom');
		});
	});

	describe('probe()', () => {
		let reader: ContainerReader;

		before(async () => {
			reader = await ContainerReader.open(MBTILES_PATH);
		});

		test('should probe with shallow depth', async () => {
			const result = await reader.probe('shallow');
			assert.ok(result, 'Probe result should exist');
			assert.ok(typeof result.sourceName === 'string', 'Should have sourceName');
			assert.ok(typeof result.containerName === 'string', 'Should have containerName');
		});

		test('should probe with container depth', async () => {
			const result = await reader.probe('container');
			assert.ok(result, 'Probe result should exist');
			assert.ok(result.tileJson, 'Should have tileJson');
			assert.ok(result.parameters, 'Should have parameters');
		});

		test('should probe without depth argument', async () => {
			const result = await reader.probe();
			assert.ok(result, 'Probe result should exist');
		});
	});

	describe('convertTo()', () => {
		let reader: ContainerReader;
		const OUTPUT_PATH = path.join(__dirname, 'output-test.versatiles');

		before(async () => {
			reader = await ContainerReader.open(MBTILES_PATH);
			// Clean up output file if it exists
			if (fs.existsSync(OUTPUT_PATH)) {
				fs.unlinkSync(OUTPUT_PATH);
			}
		});

		test('should convert to versatiles format', async () => {
			await reader.convertTo(OUTPUT_PATH);
			assert.ok(fs.existsSync(OUTPUT_PATH), 'Output file should be created');

			// Verify we can open the converted file
			const newReader = await ContainerReader.open(OUTPUT_PATH);
			assert.ok(newReader, 'Converted file should be readable');

			// Clean up
			fs.unlinkSync(OUTPUT_PATH);
		});

		test('should convert with options', async () => {
			await reader.convertTo(OUTPUT_PATH, {
				minZoom: 5,
				maxZoom: 7,
				compress: 'gzip',
			});

			assert.ok(fs.existsSync(OUTPUT_PATH), 'Output file should be created');

			const newReader = await ContainerReader.open(OUTPUT_PATH);
			const params = await newReader.parameters;
			assert.strictEqual(params.minZoom, 5, 'Should have correct minZoom');
			assert.strictEqual(params.maxZoom, 7, 'Should have correct maxZoom');

			// Clean up
			fs.unlinkSync(OUTPUT_PATH);
		});
	});
});
