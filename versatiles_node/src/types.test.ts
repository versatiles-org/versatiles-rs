import { TileCoord } from '../index.js';

describe('TileCoord', () => {
	describe('constructor', () => {
		test('should create TileCoord with valid coordinates', () => {
			const coord = new TileCoord(5, 17, 10);
			expect(coord).toBeDefined();
		});

		test('should create TileCoord at zoom 0', () => {
			const coord = new TileCoord(0, 0, 0);
			expect(coord).toBeDefined();
		});

		test('should create TileCoord at max zoom', () => {
			const coord = new TileCoord(20, 524288, 524288);
			expect(coord).toBeDefined();
		});

		test('should throw error for x >= 2^z', () => {
			expect(() => new TileCoord(5, 32, 10)).toThrow();
		});

		test('should throw error for y >= 2^z', () => {
			expect(() => new TileCoord(5, 10, 32)).toThrow();
		});

		test('should handle high zoom levels', () => {
			// Zoom levels up to 30+ should work fine
			const coord = new TileCoord(20, 0, 0);
			expect(coord.z).toBe(20);
		});

		test('should throw error for negative coordinates', () => {
			expect(() => new TileCoord(5, -1, 10)).toThrow();
		});
	});

	describe('getters', () => {
		test('should return correct z value', () => {
			const coord = new TileCoord(5, 17, 10);
			expect(coord.z).toBe(5);
		});

		test('should return correct x value', () => {
			const coord = new TileCoord(5, 17, 10);
			expect(coord.x).toBe(17);
		});

		test('should return correct y value', () => {
			const coord = new TileCoord(5, 17, 10);
			expect(coord.y).toBe(10);
		});

		test('should handle zero coordinates', () => {
			const coord = new TileCoord(0, 0, 0);
			expect(coord.z).toBe(0);
			expect(coord.x).toBe(0);
			expect(coord.y).toBe(0);
		});
	});

	describe('toGeo()', () => {
		test('should convert tile (0,0,0) to geo coordinates', () => {
			const coord = new TileCoord(0, 0, 0);
			const [lon, lat] = coord.toGeo();

			expect(typeof lon).toBe('number');
			expect(typeof lat).toBe('number');
			expect(lon).toBeGreaterThanOrEqual(-180);
			expect(lon).toBeLessThanOrEqual(180);
			expect(lat).toBeGreaterThanOrEqual(-90);
			expect(lat).toBeLessThanOrEqual(90);
		});

		test('should convert Berlin tile to approximate geo coordinates', () => {
			// Berlin is at approximately z=5, x=17, y=10
			const coord = new TileCoord(5, 17, 10);
			const [lon, lat] = coord.toGeo();

			// Berlin is at approximately 13.4°E, 52.5°N
			// Tile coordinates are for the northwest corner of the tile
			expect(lon).toBeGreaterThan(0);
			expect(lon).toBeLessThan(30);
			expect(lat).toBeGreaterThan(40);
			expect(lat).toBeLessThan(70);
		});

		test('should return array of two numbers', () => {
			const coord = new TileCoord(5, 17, 10);
			const result = coord.toGeo();

			expect(Array.isArray(result)).toBeTruthy();
			expect(result).toHaveLength(2);
			expect(typeof result[0]).toBe('number');
			expect(typeof result[1]).toBe('number');
		});

		test('should handle edge tiles', () => {
			const coord = new TileCoord(1, 0, 0); // Northwest tile at zoom 1
			const [lon, lat] = coord.toGeo();

			expect(lon).toBe(-180);
			expect(lat).toBeGreaterThan(0);
		});
	});

	describe('fromGeo()', () => {
		test('should create TileCoord from geo coordinates', () => {
			const coord = TileCoord.fromGeo(0, 0, 0); // Equator, Prime Meridian at zoom 0
			expect(coord).toBeDefined();
			expect(coord.z).toBe(0);
		});

		test('should create TileCoord from Berlin coordinates', () => {
			// Berlin: 13.4°E, 52.5°N at zoom 5
			const coord = TileCoord.fromGeo(13.4, 52.5, 5);
			expect(coord.z).toBe(5);

			// Should be tile 17,10 or very close
			expect(coord.x).toBeGreaterThanOrEqual(16);
			expect(coord.x).toBeLessThanOrEqual(18);
			expect(coord.y).toBeGreaterThanOrEqual(9);
			expect(coord.y).toBeLessThanOrEqual(11);
		});

		test('should handle extreme coordinates', () => {
			const coord1 = TileCoord.fromGeo(-180, 85, 5);
			const coord2 = TileCoord.fromGeo(180, -85, 5);

			expect(coord1).toBeDefined();
			expect(coord2).toBeDefined();
		});

		test('should throw error for invalid longitude', () => {
			expect(() => TileCoord.fromGeo(181, 0, 5)).toThrow();
			expect(() => TileCoord.fromGeo(-181, 0, 5)).toThrow();
		});

		test('should throw error for invalid latitude', () => {
			expect(() => TileCoord.fromGeo(0, 91, 5)).toThrow();
			expect(() => TileCoord.fromGeo(0, -91, 5)).toThrow();
		});

		test('should work at different zoom levels', () => {
			const coord0 = TileCoord.fromGeo(0, 0, 0);
			const coord5 = TileCoord.fromGeo(0, 0, 5);
			const coord10 = TileCoord.fromGeo(0, 0, 10);

			expect(coord0.z).toBe(0);
			expect(coord5.z).toBe(5);
			expect(coord10.z).toBe(10);
		});
	});

	describe('round-trip conversion', () => {
		test('should maintain zoom level in round-trip', () => {
			const original = new TileCoord(5, 17, 10);
			const [lon, lat] = original.toGeo();
			const roundTrip = TileCoord.fromGeo(lon, lat, 5);

			expect(roundTrip.z).toBe(original.z);
			expect(roundTrip.x).toBe(original.x);
			expect(roundTrip.y).toBe(original.y);
		});

		test('should handle multiple round-trips', () => {
			const coords = [new TileCoord(0, 0, 0), new TileCoord(5, 17, 10), new TileCoord(10, 512, 384)];

			coords.forEach((original) => {
				const [lon, lat] = original.toGeo();
				const roundTrip = TileCoord.fromGeo(lon, lat, original.z);

				expect(roundTrip.x).toBe(original.x);
				expect(roundTrip.y).toBe(original.y);
			});
		});

		test('should convert geo to tile and back to geo', () => {
			const originalLon = 13.4;
			const originalLat = 52.5;
			const zoom = 10;

			const coord = TileCoord.fromGeo(originalLon, originalLat, zoom);
			const [lon, lat] = coord.toGeo();

			// Should be close (within tile bounds)
			const lonDiff = Math.abs(lon - originalLon);
			const latDiff = Math.abs(lat - originalLat);

			// At zoom 10, tiles are ~0.35° wide
			expect(lonDiff).toBeLessThan(0.5);
			expect(latDiff).toBeLessThan(0.5);
		});
	});

	describe('edge cases', () => {
		test('should handle antimeridian (180°)', () => {
			const coord = TileCoord.fromGeo(180, 0, 5);
			expect(coord).toBeDefined();
		});

		test('should handle dateline (-180°)', () => {
			const coord = TileCoord.fromGeo(-180, 0, 5);
			expect(coord).toBeDefined();
		});

		test('should handle poles (approximately)', () => {
			// Web Mercator doesn't extend to exact poles, but should handle close to them
			const north = TileCoord.fromGeo(0, 85, 5);
			const south = TileCoord.fromGeo(0, -85, 5);

			expect(north).toBeDefined();
			expect(south).toBeDefined();
		});

		test('should handle all corners at zoom 1', () => {
			const coords = [
				TileCoord.fromGeo(-180, 85, 1), // NW
				TileCoord.fromGeo(180, 85, 1), // NE
				TileCoord.fromGeo(-180, -85, 1), // SW
				TileCoord.fromGeo(180, -85, 1), // SE
			];

			coords.forEach((coord) => {
				expect(coord).toBeDefined();
				expect(coord.x).toBeGreaterThanOrEqual(0);
				expect(coord.x).toBeLessThan(2);
				expect(coord.y).toBeGreaterThanOrEqual(0);
				expect(coord.y).toBeLessThan(2);
			});
		});
	});
});
