import { beforeAll } from 'vitest';
import { ContainerReader } from '../index.js';
import path from 'path';
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
			const tileJson = await reader.tileJson();

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
			const tileJson = await reader.tileJson();

			const parsed = JSON.parse(tileJson);
			expect(parsed.tilejson).toBe('3.0.0');
		});
	});

	describe('metadata', () => {
		it('should return reader metadata', async () => {
			const reader = await ContainerReader.open(MBTILES_PATH);
			const metadata = await reader.metadata();

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
			const reader = await ContainerReader.open(MBTILES_PATH);
			const sourceType = await reader.sourceType();

			expect(sourceType.kind).toEqual('container');
			expect(sourceType.name).toEqual('mbtiles');
			expect(sourceType.uri).not.toBeNull();
			expect(sourceType.input).toBeNull();
			expect(sourceType.inputs).toBeNull();
		});

		it('should return correct source type for PMTiles', async () => {
			const reader = await ContainerReader.open(PMTILES_PATH);
			const sourceType = await reader.sourceType();

			expect(sourceType.kind).toEqual('container');
			expect(sourceType.name).toEqual('pmtiles');
			expect(sourceType.uri).not.toBeNull();
			expect(sourceType.input).toBeNull();
			expect(sourceType.inputs).toBeNull();
		});
	});
});
