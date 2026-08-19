import { describe, it, expect } from 'vitest';
import { TileSource, layerStats } from '../index.js';
import { gzipSync } from 'zlib';
import path from 'path';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');

/** A real vector tile from the Berlin fixture, decompressed by getTile(). */
async function berlinTile(): Promise<Buffer> {
	const source = await TileSource.fromPath(MBTILES_PATH);
	const tile = await source.getTile(14, 8802, 5373);
	if (!tile) throw new Error('fixture tile 14/8802/5373 is missing');
	return tile;
}

describe('layerStats', () => {
	it('should break a real tile down per layer', async () => {
		const stats = layerStats(await berlinTile());

		expect(stats.length).toBeGreaterThan(0);
		for (const layer of stats) {
			expect(typeof layer.name).toBe('string');
			expect(layer.name.length).toBeGreaterThan(0);
			expect(layer.encodedBytes).toBeGreaterThan(0);
		}
	});

	it('should compose with getTile() without a second way to do it', async () => {
		// The reason this is standalone: bytes the caller already holds work too.
		const tile = await berlinTile();
		expect(layerStats(tile)).toStrictEqual(layerStats(Buffer.from(tile)));
	});

	it('should report categories that sum to encodedBytes', async () => {
		// Documented, so a consumer can render a stacked bar without a fudge factor.
		for (const layer of layerStats(await berlinTile())) {
			expect(layer.geometryBytes + layer.tagBytes + layer.propertyBytes + layer.idBytes + layer.otherBytes).toBe(
				layer.encodedBytes,
			);
			expect(layer.propertyBytes).toBe(layer.keyBytes + layer.valueBytes);
		}
	});

	it('should expose counts alongside the byte split', async () => {
		const stats = layerStats(await berlinTile());
		const total = stats.reduce((sum, l) => sum + l.featureCount, 0);
		expect(total).toBeGreaterThan(0);
		for (const layer of stats) {
			expect(Number.isInteger(layer.featureCount)).toBe(true);
			expect(Number.isInteger(layer.vertexCount)).toBe(true);
		}
	});

	it('should say so when the tile is still compressed', async () => {
		// The mistake a caller makes when the tile came from somewhere other
		// than getTile() — a protobuf parse error would not say which it was.
		const gzipped = gzipSync(await berlinTile());
		expect(() => layerStats(gzipped)).toThrow(/gzip-compressed/);
	});

	it('should say so when given a raster tile', () => {
		const png = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);
		expect(() => layerStats(png)).toThrow(/PNG/);
	});

	it('should reject bytes that are not a vector tile', () => {
		expect(() => layerStats(Buffer.alloc(32, 0x42))).toThrow(/not an uncompressed vector tile/);
	});

	it('should treat an empty buffer as an empty tile', () => {
		// getTile() signals a missing tile with null, so empty bytes mean a
		// tile that exists and has no layers.
		expect(layerStats(Buffer.alloc(0))).toStrictEqual([]);
	});
});
