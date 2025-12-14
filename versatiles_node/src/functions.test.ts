import { describe, test, after } from 'node:test';
import assert from 'node:assert';
import { convertTiles, probeTiles, ContainerReader } from '../index.js';
import path from 'path';
import fs from 'fs';

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const PMTILES_PATH = path.join(TESTDATA_DIR, 'berlin.pmtiles');
const OUTPUT_DIR = __dirname;

describe('Standalone Functions', () => {
	describe('probeTiles()', () => {
		test('should probe MBTiles file with shallow depth', async () => {
			const result = await probeTiles(MBTILES_PATH, 'shallow');

			assert.ok(result, 'Result should exist');
			assert.ok(typeof result.sourceName === 'string', 'Should have sourceName');
			assert.ok(typeof result.containerName === 'string', 'Should have containerName');
			assert.ok(result.sourceName.length > 0, 'sourceName should not be empty');
			assert.ok(result.containerName.length > 0, 'containerName should not be empty');
		});

		test('should probe MBTiles file with container depth', async () => {
			const result = await probeTiles(MBTILES_PATH, 'container');

			assert.ok(result, 'Result should exist');
			assert.ok(result.tileJson, 'Should have tileJson');
			assert.ok(result.parameters, 'Should have parameters');

			const tileJson = JSON.parse(result.tileJson);
			assert.strictEqual(tileJson.tilejson, '3.0.0', 'Should have valid TileJSON');

			assert.ok(typeof result.parameters.tileFormat === 'string', 'Should have tileFormat');
			assert.ok(typeof result.parameters.tileCompression === 'string', 'Should have tileCompression');
			assert.ok(typeof result.parameters.minZoom === 'number', 'Should have minZoom');
			assert.ok(typeof result.parameters.maxZoom === 'number', 'Should have maxZoom');
		});

		test('should probe PMTiles file', async () => {
			const result = await probeTiles(PMTILES_PATH, 'container');

			assert.ok(result, 'Result should exist');
			assert.ok(result.containerName.includes('pmtiles'), 'Should identify as PMTiles');
		});

		test('should probe without depth argument', async () => {
			const result = await probeTiles(MBTILES_PATH);
			assert.ok(result, 'Result should exist without depth argument');
		});

		test('should probe with tiles depth', async () => {
			const result = await probeTiles(MBTILES_PATH, 'tiles');
			assert.ok(result, 'Result should exist with tiles depth');
		});

		test('should throw error for non-existent file', async () => {
			await assert.rejects(
				async () => await probeTiles('/nonexistent/file.mbtiles'),
				'Should throw error for non-existent file',
			);
		});

		test('should throw error for invalid file format', async () => {
			await assert.rejects(async () => await probeTiles(__filename), 'Should throw error for invalid file format');
		});
	});

	describe('convertTiles()', () => {
		const OUTPUT_VERSATILES = path.join(OUTPUT_DIR, 'converted.versatiles');
		const OUTPUT_MBTILES = path.join(OUTPUT_DIR, 'converted.mbtiles');

		after(() => {
			// Clean up output files
			[OUTPUT_VERSATILES, OUTPUT_MBTILES].forEach((file) => {
				if (fs.existsSync(file)) {
					fs.unlinkSync(file);
				}
			});
		});

		test('should convert MBTiles to VersaTiles format', async () => {
			await convertTiles(MBTILES_PATH, OUTPUT_VERSATILES);

			assert.ok(fs.existsSync(OUTPUT_VERSATILES), 'Output file should exist');
			assert.ok(fs.statSync(OUTPUT_VERSATILES).size > 0, 'Output file should not be empty');

			// Verify the converted file can be opened
			const reader: ContainerReader = await ContainerReader.open(OUTPUT_VERSATILES);
			assert.ok(reader, 'Converted file should be readable');
		});

		test('should convert PMTiles to MBTiles format', async () => {
			await convertTiles(PMTILES_PATH, OUTPUT_MBTILES);

			assert.ok(fs.existsSync(OUTPUT_MBTILES), 'Output file should exist');

			// Verify the converted file can be opened
			const reader: ContainerReader = await ContainerReader.open(OUTPUT_MBTILES);
			assert.ok(reader, 'Converted file should be readable');
		});

		test('should convert with minZoom option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-minzoom.versatiles');

			await convertTiles(MBTILES_PATH, output, { minZoom: 6 });

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.minZoom, 6, 'Should have correct minZoom');

			fs.unlinkSync(output);
		});

		test('should convert with maxZoom option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-maxzoom.versatiles');

			await convertTiles(MBTILES_PATH, output, { maxZoom: 7 });

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.maxZoom, 7, 'Should have correct maxZoom');

			fs.unlinkSync(output);
		});

		test('should convert with zoom range option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-range.versatiles');

			await convertTiles(MBTILES_PATH, output, {
				minZoom: 5,
				maxZoom: 7,
			});

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.minZoom, 5, 'Should have correct minZoom');
			assert.strictEqual(params.maxZoom, 7, 'Should have correct maxZoom');

			fs.unlinkSync(output);
		});

		test('should convert with gzip compression', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-gzip.versatiles');

			await convertTiles(MBTILES_PATH, output, { compress: 'gzip' });

			assert.ok(fs.existsSync(output), 'Output file should exist');

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.tileCompression, 'gzip', 'Should use gzip compression');

			fs.unlinkSync(output);
		});

		test('should convert with brotli compression', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-brotli.versatiles');

			await convertTiles(MBTILES_PATH, output, { compress: 'brotli' });

			assert.ok(fs.existsSync(output), 'Output file should exist');

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.tileCompression, 'brotli', 'Should use brotli compression');

			fs.unlinkSync(output);
		});

		test('should convert with uncompressed option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-uncompressed.versatiles');

			await convertTiles(MBTILES_PATH, output, { compress: 'uncompressed' });

			assert.ok(fs.existsSync(output), 'Output file should exist');

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.tileCompression, 'uncompressed', 'Should be uncompressed');

			fs.unlinkSync(output);
		});

		test('should convert with bbox option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-bbox.versatiles');

			// Bounding box for Berlin area
			await convertTiles(MBTILES_PATH, output, {
				bbox: [13.0, 52.0, 14.0, 53.0],
			});

			assert.ok(fs.existsSync(output), 'Output file should exist');

			const reader: ContainerReader = await ContainerReader.open(output);
			assert.ok(reader, 'Converted file should be readable');

			fs.unlinkSync(output);
		});

		test('should convert with multiple options', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-multi.versatiles');

			await convertTiles(MBTILES_PATH, output, {
				minZoom: 5,
				maxZoom: 7,
				compress: 'gzip',
				bbox: [13.0, 52.0, 14.0, 53.0],
			});

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.minZoom, 5, 'Should have correct minZoom');
			assert.strictEqual(params.maxZoom, 7, 'Should have correct maxZoom');
			assert.strictEqual(params.tileCompression, 'gzip', 'Should use gzip compression');

			fs.unlinkSync(output);
		});

		test('should convert with flipY option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-flipy.versatiles');

			await convertTiles(MBTILES_PATH, output, { flipY: true });

			assert.ok(fs.existsSync(output), 'Output file should exist');

			fs.unlinkSync(output);
		});

		test('should convert with swapXy option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-swapxy.versatiles');

			await convertTiles(MBTILES_PATH, output, { swapXy: true });

			assert.ok(fs.existsSync(output), 'Output file should exist');

			fs.unlinkSync(output);
		});

		test('should throw error for non-existent input file', async () => {
			await assert.rejects(
				async () => await convertTiles('/nonexistent/file.mbtiles', OUTPUT_VERSATILES),
				'Should throw error for non-existent input file',
			);
		});

		test('should throw error for invalid output path', async () => {
			await assert.rejects(
				async () => await convertTiles(MBTILES_PATH, '/nonexistent/directory/output.versatiles'),
				'Should throw error for invalid output path',
			);
		});

		test('should handle conversion between same format', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-same.mbtiles');

			await convertTiles(MBTILES_PATH, output);

			assert.ok(fs.existsSync(output), 'Output file should exist');

			const reader: ContainerReader = await ContainerReader.open(output);
			assert.ok(reader, 'Converted file should be readable');

			fs.unlinkSync(output);
		});
	});

	describe('integration: probe then convert', () => {
		test('should probe file and use metadata for conversion', async () => {
			const output = path.join(OUTPUT_DIR, 'integration.versatiles');

			// First, probe the file
			const probeResult = await probeTiles(MBTILES_PATH, 'container');
			assert.ok(probeResult, 'Probe should succeed');

			const { minZoom, maxZoom } = probeResult.parameters;

			// Use the probed zoom levels for conversion
			await convertTiles(MBTILES_PATH, output, {
				minZoom: minZoom,
				maxZoom: Math.min(maxZoom, minZoom + 2), // Limit range
			});

			assert.ok(fs.existsSync(output), 'Conversion should succeed');

			// Verify the converted file
			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			assert.strictEqual(params.minZoom, minZoom, 'Should match probed minZoom');

			fs.unlinkSync(output);
		});
	});

	describe('edge cases', () => {
		test('should handle empty options object', async () => {
			const output = path.join(OUTPUT_DIR, 'empty-options.versatiles');

			await convertTiles(MBTILES_PATH, output, {});

			assert.ok(fs.existsSync(output), 'Should work with empty options');

			fs.unlinkSync(output);
		});

		test('should handle null options', async () => {
			const output = path.join(OUTPUT_DIR, 'null-options.versatiles');

			await convertTiles(MBTILES_PATH, output, null);

			assert.ok(fs.existsSync(output), 'Should work with null options');

			fs.unlinkSync(output);
		});

		test('should handle undefined options', async () => {
			const output = path.join(OUTPUT_DIR, 'undefined-options.versatiles');

			await convertTiles(MBTILES_PATH, output, undefined);

			assert.ok(fs.existsSync(output), 'Should work with undefined options');

			fs.unlinkSync(output);
		});

		test('should handle conversion without options parameter', async () => {
			const output = path.join(OUTPUT_DIR, 'no-options.versatiles');

			await convertTiles(MBTILES_PATH, output);

			assert.ok(fs.existsSync(output), 'Should work without options parameter');

			fs.unlinkSync(output);
		});
	});
});
