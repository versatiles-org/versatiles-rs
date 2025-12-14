import { afterAll } from 'vitest';
import { convertTiles, probeTiles, ContainerReader } from '../index.js';
import path from 'path';
import fs from 'fs';

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const PMTILES_PATH = path.join(TESTDATA_DIR, 'berlin.pmtiles');
const OUTPUT_DIR = __dirname;

describe('Standalone Functions', () => {
	describe('probeTiles()', () => {
		it('should probe MBTiles file with shallow depth', async () => {
			const result = await probeTiles(MBTILES_PATH, 'shallow');

			expect(result).toBeDefined();
			expect(typeof result.sourceName).toBe('string');
			expect(typeof result.containerName).toBe('string');
			expect(result.sourceName.length).toBeGreaterThan(0);
			expect(result.containerName.length).toBeGreaterThan(0);
		});

		it('should probe MBTiles file with container depth', async () => {
			const result = await probeTiles(MBTILES_PATH, 'container');

			expect(result).toBeDefined();
			expect(result.tileJson).toBeDefined();
			expect(result.parameters).toBeDefined();

			const tileJson = JSON.parse(result.tileJson);
			expect(tileJson.tilejson).toBe('3.0.0');

			expect(typeof result.parameters.tileFormat).toBe('string');
			expect(typeof result.parameters.tileCompression).toBe('string');
			expect(typeof result.parameters.minZoom).toBe('number');
			expect(typeof result.parameters.maxZoom).toBe('number');
		});

		it('should probe PMTiles file', async () => {
			const result = await probeTiles(PMTILES_PATH, 'container');

			expect(result).toBeDefined();
			expect(result.containerName).toContain('pmtiles');
		});

		it('should probe without depth argument', async () => {
			const result = await probeTiles(MBTILES_PATH);
			expect(result).toBeDefined();
		});

		it('should probe with tiles depth', async () => {
			const result = await probeTiles(MBTILES_PATH, 'tiles');
			expect(result).toBeDefined();
		});

		it('should throw error for non-existent file', async () => {
			await expect(probeTiles('/nonexistent/file.mbtiles')).rejects.toThrow();
		});

		it('should throw error for invalid file format', async () => {
			await expect(probeTiles(__filename)).rejects.toThrow();
		});
	});

	describe('convertTiles()', () => {
		const OUTPUT_VERSATILES = path.join(OUTPUT_DIR, 'converted.versatiles');
		const OUTPUT_MBTILES = path.join(OUTPUT_DIR, 'converted.mbtiles');

		afterAll(() => {
			// Clean up output files
			[OUTPUT_VERSATILES, OUTPUT_MBTILES].forEach((file) => {
				if (fs.existsSync(file)) {
					fs.unlinkSync(file);
				}
			});
		});

		it('should convert MBTiles to VersaTiles format', async () => {
			await convertTiles(MBTILES_PATH, OUTPUT_VERSATILES);

			expect(fs.existsSync(OUTPUT_VERSATILES)).toBeTruthy();
			expect(fs.statSync(OUTPUT_VERSATILES).size).toBeGreaterThan(0);

			// Verify the converted file can be opened
			const reader: ContainerReader = await ContainerReader.open(OUTPUT_VERSATILES);
			expect(reader).toBeDefined();
		});

		it('should convert PMTiles to MBTiles format', async () => {
			await convertTiles(PMTILES_PATH, OUTPUT_MBTILES);

			expect(fs.existsSync(OUTPUT_MBTILES)).toBeTruthy();

			// Verify the converted file can be opened
			const reader: ContainerReader = await ContainerReader.open(OUTPUT_MBTILES);
			expect(reader).toBeDefined();
		});

		it('should convert with minZoom option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-minzoom.versatiles');

			await convertTiles(MBTILES_PATH, output, { minZoom: 6 });

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.minZoom).toBe(6);

			fs.unlinkSync(output);
		});

		it('should convert with maxZoom option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-maxzoom.versatiles');

			await convertTiles(MBTILES_PATH, output, { maxZoom: 7 });

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.maxZoom).toBe(7);

			fs.unlinkSync(output);
		});

		it('should convert with zoom range option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-range.versatiles');

			await convertTiles(MBTILES_PATH, output, {
				minZoom: 5,
				maxZoom: 7,
			});

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.minZoom).toBe(5);
			expect(params.maxZoom).toBe(7);

			fs.unlinkSync(output);
		});

		it('should convert with gzip compression', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-gzip.versatiles');

			await convertTiles(MBTILES_PATH, output, { compress: 'gzip' });

			expect(fs.existsSync(output)).toBeTruthy();

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.tileCompression).toBe('gzip');

			fs.unlinkSync(output);
		});

		it('should convert with brotli compression', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-brotli.versatiles');

			await convertTiles(MBTILES_PATH, output, { compress: 'brotli' });

			expect(fs.existsSync(output)).toBeTruthy();

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.tileCompression).toBe('brotli');

			fs.unlinkSync(output);
		});

		it('should convert with uncompressed option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-uncompressed.versatiles');

			await convertTiles(MBTILES_PATH, output, { compress: 'uncompressed' });

			expect(fs.existsSync(output)).toBeTruthy();

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.tileCompression).toBe('uncompressed');

			fs.unlinkSync(output);
		});

		it('should convert with bbox option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-bbox.versatiles');

			// Bounding box for Berlin area
			await convertTiles(MBTILES_PATH, output, {
				bbox: [13.0, 52.0, 14.0, 53.0],
			});

			expect(fs.existsSync(output)).toBeTruthy();

			const reader: ContainerReader = await ContainerReader.open(output);
			expect(reader).toBeDefined();

			fs.unlinkSync(output);
		});

		it('should convert with multiple options', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-multi.versatiles');

			await convertTiles(MBTILES_PATH, output, {
				minZoom: 5,
				maxZoom: 7,
				compress: 'gzip',
				bbox: [13.0, 52.0, 14.0, 53.0],
			});

			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.minZoom).toBe(5);
			expect(params.maxZoom).toBe(7);
			expect(params.tileCompression).toBe('gzip');

			fs.unlinkSync(output);
		});

		it('should convert with flipY option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-flipy.versatiles');

			await convertTiles(MBTILES_PATH, output, { flipY: true });

			expect(fs.existsSync(output)).toBeTruthy();

			fs.unlinkSync(output);
		});

		it('should convert with swapXy option', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-swapxy.versatiles');

			await convertTiles(MBTILES_PATH, output, { swapXy: true });

			expect(fs.existsSync(output)).toBeTruthy();

			fs.unlinkSync(output);
		});

		it('should throw error for non-existent input file', async () => {
			await expect(convertTiles('/nonexistent/file.mbtiles', OUTPUT_VERSATILES)).rejects.toThrow();
		});

		it('should throw error for invalid output path', async () => {
			await expect(convertTiles(MBTILES_PATH, '/nonexistent/directory/output.versatiles')).rejects.toThrow();
		});

		it('should handle conversion between same format', async () => {
			const output = path.join(OUTPUT_DIR, 'converted-same.mbtiles');

			await convertTiles(MBTILES_PATH, output);

			expect(fs.existsSync(output)).toBeTruthy();

			const reader: ContainerReader = await ContainerReader.open(output);
			expect(reader).toBeDefined();

			fs.unlinkSync(output);
		});
	});

	describe('integration: probe then convert', () => {
		it('should probe file and use metadata for conversion', async () => {
			const output = path.join(OUTPUT_DIR, 'integration.versatiles');

			// First, probe the file
			const probeResult = await probeTiles(MBTILES_PATH, 'container');
			expect(probeResult).toBeDefined();

			const { minZoom, maxZoom } = probeResult.parameters;

			// Use the probed zoom levels for conversion
			await convertTiles(MBTILES_PATH, output, {
				minZoom: minZoom,
				maxZoom: Math.min(maxZoom, minZoom + 2), // Limit range
			});

			expect(fs.existsSync(output)).toBeTruthy();

			// Verify the converted file
			const reader: ContainerReader = await ContainerReader.open(output);
			const params = await reader.parameters;
			expect(params.minZoom).toBe(minZoom);

			fs.unlinkSync(output);
		});
	});

	describe('edge cases', () => {
		it('should handle empty options object', async () => {
			const output = path.join(OUTPUT_DIR, 'empty-options.versatiles');

			await convertTiles(MBTILES_PATH, output, {});

			expect(fs.existsSync(output)).toBeTruthy();

			fs.unlinkSync(output);
		});

		it('should handle null options', async () => {
			const output = path.join(OUTPUT_DIR, 'null-options.versatiles');

			await convertTiles(MBTILES_PATH, output, null);

			expect(fs.existsSync(output)).toBeTruthy();

			fs.unlinkSync(output);
		});

		it('should handle undefined options', async () => {
			const output = path.join(OUTPUT_DIR, 'undefined-options.versatiles');

			await convertTiles(MBTILES_PATH, output, undefined);

			expect(fs.existsSync(output)).toBeTruthy();

			fs.unlinkSync(output);
		});

		it('should handle conversion without options parameter', async () => {
			const output = path.join(OUTPUT_DIR, 'no-options.versatiles');

			await convertTiles(MBTILES_PATH, output);

			expect(fs.existsSync(output)).toBeTruthy();

			fs.unlinkSync(output);
		});
	});
});
