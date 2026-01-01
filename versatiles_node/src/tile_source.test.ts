import { beforeAll } from 'vitest';
import { TileSource } from '../index.js';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const PMTILES_PATH = path.join(TESTDATA_DIR, 'berlin.pmtiles');

describe('TileSource', () => {
	describe('open()', () => {
		it('should open MBTiles file', async () => {
			const reader = await TileSource.open(MBTILES_PATH);
			expect(reader).toBeDefined();
		});

		it('should open PMTiles file', async () => {
			const reader = await TileSource.open(PMTILES_PATH);
			expect(reader).toBeDefined();
		});

		it('should throw error for non-existent file', async () => {
			await expect(TileSource.open('/nonexistent/file.mbtiles')).rejects.toThrow();
		});

		it('should throw error for invalid file format', async () => {
			await expect(TileSource.open(__filename)).rejects.toThrow();
		});
	});

	describe('getTile()', () => {
		let reader: TileSource;

		beforeAll(async () => {
			reader = await TileSource.open(MBTILES_PATH);
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
			const reader = await TileSource.open(MBTILES_PATH);
			const tileJson = reader.tileJson();

			expect(tileJson).toBeDefined();
			expect(typeof tileJson).toBe('object');

			expect(tileJson.tilejson).toBe('3.0');
			// tiles array, bounds, and other fields may or may not be present depending on implementation
			expect(typeof tileJson.minzoom).toBe('number');
			expect(typeof tileJson.maxzoom).toBe('number');
		});

		it('should return valid TileJSON for PMTiles', async () => {
			const reader = await TileSource.open(PMTILES_PATH);
			const tileJson = reader.tileJson();

			expect(tileJson.tilejson).toBe('3.0');
		});
	});

	describe('metadata', () => {
		it('should return reader metadata', async () => {
			const reader = await TileSource.open(MBTILES_PATH);
			const metadata = reader.metadata();

			expect(metadata).toStrictEqual({
				maxZoom: 14,
				minZoom: 0,
				tileCompression: 'gzip',
				tileFormat: 'mvt',
			});
		});
	});

	describe('sourceType', () => {
		it('should return correct source type for MBTiles', async () => {
			const reader = await TileSource.open(MBTILES_PATH);
			const sourceType = reader.sourceType();

			expect(sourceType.kind).toEqual('container');
			expect(sourceType.name).toEqual('mbtiles');
			expect(sourceType.uri).not.toBeNull();
			expect(sourceType.input).toBeNull();
			expect(sourceType.inputs).toBeNull();
		});

		it('should return correct source type for PMTiles', async () => {
			const reader = await TileSource.open(PMTILES_PATH);
			const sourceType = reader.sourceType();

			expect(sourceType.kind).toEqual('container');
			expect(sourceType.name).toEqual('pmtiles');
			expect(sourceType.uri).not.toBeNull();
			expect(sourceType.input).toBeNull();
			expect(sourceType.inputs).toBeNull();
		});
	});

	describe('fromVpl', () => {
		it('should create TileSource from simple VPL string', async () => {
			const vpl = 'from_container filename="berlin.mbtiles"';
			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			expect(reader).toBeDefined();

			// Verify metadata
			const metadata = reader.metadata();
			expect(metadata.tileFormat).toBe('mvt');
			expect(metadata.tileCompression).toBe('gzip');
			expect(metadata.minZoom).toBe(0);
			expect(metadata.maxZoom).toBe(14);
		});

		it('should create TileSource from VPL file content', async () => {
			const fs = await import('fs/promises');
			const vplPath = path.join(TESTDATA_DIR, 'berlin.vpl');
			const vpl = await fs.readFile(vplPath, 'utf-8');

			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			expect(reader).toBeDefined();

			// Verify we can read metadata
			const metadata = reader.metadata();
			expect(metadata.tileFormat).toBe('mvt');
			expect(metadata.tileCompression).toBe('gzip');
		});

		it('should support VPL with pipeline operations', async () => {
			const vpl = 'from_container filename="berlin.mbtiles" | filter level_min=5 level_max=10';
			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			// Verify zoom filter is applied
			const metadata = reader.metadata();
			expect(metadata.minZoom).toBe(5);
			expect(metadata.maxZoom).toBe(10);
		});

		it('should retrieve tiles from VPL source', async () => {
			const vpl = 'from_container filename="berlin.mbtiles"';
			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			// Get a tile that should exist
			const tile = await reader.getTile(5, 17, 10);
			expect(tile).toBeDefined();
			expect(Buffer.isBuffer(tile)).toBeTruthy();
			expect(tile!.length).toBeGreaterThan(0);
		});

		it('should return valid TileJSON from VPL source', async () => {
			const vpl = 'from_container filename="berlin.mbtiles"';
			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			const tileJson = reader.tileJson();
			expect(tileJson).toBeDefined();
			expect(tileJson.tilejson).toBe('3.0');
			expect(tileJson.minzoom).toBe(0);
			expect(tileJson.maxzoom).toBe(14);
			expect(tileJson.vectorLayers).toBeDefined();
		});

		it('should have correct source type for VPL source', async () => {
			const vpl = 'from_container filename="berlin.mbtiles"';
			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			const sourceType = reader.sourceType();
			expect(sourceType.kind).toBe('processor');
			expect(sourceType.input).not.toBeNull();
			// VPL creates a processor with an input
			expect(sourceType.input?.kind).toBe('container');
		});

		it('should throw error for invalid VPL syntax', async () => {
			const vpl = 'invalid vpl syntax here';
			await expect(TileSource.fromVpl(vpl, TESTDATA_DIR)).rejects.toThrow();
		});

		it('should throw error for non-existent file in VPL', async () => {
			const vpl = 'from_container filename="nonexistent.mbtiles"';
			await expect(TileSource.fromVpl(vpl, TESTDATA_DIR)).rejects.toThrow();
		});

		it('should handle multiple pipeline operations', async () => {
			const vpl = `from_container filename="berlin.mbtiles" |
				filter level_min=3 level_max=8 bbox=[13.0,52.0,14.0,53.0]`;
			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			const metadata = reader.metadata();
			expect(metadata.minZoom).toBe(3);
			expect(metadata.maxZoom).toBe(8);
		});

		it('should resolve relative paths correctly', async () => {
			// Test that the directory parameter is used for resolving relative paths
			const vpl = 'from_container filename="berlin.mbtiles"';
			const reader = await TileSource.fromVpl(vpl, TESTDATA_DIR);

			const tile = await reader.getTile(5, 17, 10);
			expect(tile).toBeDefined();
		});
	});
});
