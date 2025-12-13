import { describe, test } from 'node:test';
import assert from 'node:assert';
import { TileCoord } from '../index.js';

describe('TileCoord', () => {
  describe('constructor', () => {
    test('should create TileCoord with valid coordinates', () => {
      const coord = new TileCoord(5, 17, 10);
      assert.ok(coord, 'TileCoord should be created');
    });

    test('should create TileCoord at zoom 0', () => {
      const coord = new TileCoord(0, 0, 0);
      assert.ok(coord, 'TileCoord should be created at zoom 0');
    });

    test('should create TileCoord at max zoom', () => {
      const coord = new TileCoord(20, 524288, 524288);
      assert.ok(coord, 'TileCoord should be created at high zoom');
    });

    test('should throw error for x >= 2^z', () => {
      assert.throws(
        () => new TileCoord(5, 32, 10),
        'Should throw error for x >= 2^z'
      );
    });

    test('should throw error for y >= 2^z', () => {
      assert.throws(
        () => new TileCoord(5, 10, 32),
        'Should throw error for y >= 2^z'
      );
    });

    test('should handle high zoom levels', () => {
      // Zoom levels up to 30+ should work fine
      const coord = new TileCoord(20, 0, 0);
      assert.strictEqual(coord.z, 20, 'Should handle high zoom levels');
    });

    test('should throw error for negative coordinates', () => {
      assert.throws(
        () => new TileCoord(5, -1, 10),
        'Should throw error for negative x'
      );
    });
  });

  describe('getters', () => {
    test('should return correct z value', () => {
      const coord = new TileCoord(5, 17, 10);
      assert.strictEqual(coord.z, 5, 'z should be 5');
    });

    test('should return correct x value', () => {
      const coord = new TileCoord(5, 17, 10);
      assert.strictEqual(coord.x, 17, 'x should be 17');
    });

    test('should return correct y value', () => {
      const coord = new TileCoord(5, 17, 10);
      assert.strictEqual(coord.y, 10, 'y should be 10');
    });

    test('should handle zero coordinates', () => {
      const coord = new TileCoord(0, 0, 0);
      assert.strictEqual(coord.z, 0, 'z should be 0');
      assert.strictEqual(coord.x, 0, 'x should be 0');
      assert.strictEqual(coord.y, 0, 'y should be 0');
    });
  });

  describe('toGeo()', () => {
    test('should convert tile (0,0,0) to geo coordinates', () => {
      const coord = new TileCoord(0, 0, 0);
      const [lon, lat] = coord.toGeo();

      assert.ok(typeof lon === 'number', 'longitude should be a number');
      assert.ok(typeof lat === 'number', 'latitude should be a number');
      assert.ok(lon >= -180 && lon <= 180, 'longitude should be in valid range');
      assert.ok(lat >= -90 && lat <= 90, 'latitude should be in valid range');
    });

    test('should convert Berlin tile to approximate geo coordinates', () => {
      // Berlin is at approximately z=5, x=17, y=10
      const coord = new TileCoord(5, 17, 10);
      const [lon, lat] = coord.toGeo();

      // Berlin is at approximately 13.4°E, 52.5°N
      // Tile coordinates are for the northwest corner of the tile
      assert.ok(lon > 0 && lon < 30, 'longitude should be in Eastern Europe');
      assert.ok(lat > 40 && lat < 70, 'latitude should be in Northern Europe');
    });

    test('should return array of two numbers', () => {
      const coord = new TileCoord(5, 17, 10);
      const result = coord.toGeo();

      assert.ok(Array.isArray(result), 'should return an array');
      assert.strictEqual(result.length, 2, 'should return array of length 2');
      assert.ok(typeof result[0] === 'number', 'first element should be a number');
      assert.ok(typeof result[1] === 'number', 'second element should be a number');
    });

    test('should handle edge tiles', () => {
      const coord = new TileCoord(1, 0, 0); // Northwest tile at zoom 1
      const [lon, lat] = coord.toGeo();

      assert.strictEqual(lon, -180, 'northwest lon should be -180');
      assert.ok(lat > 0, 'northwest lat should be positive');
    });
  });

  describe('fromGeo()', () => {
    test('should create TileCoord from geo coordinates', () => {
      const coord = TileCoord.fromGeo(0, 0, 0); // Equator, Prime Meridian at zoom 0
      assert.ok(coord, 'TileCoord should be created');
      assert.strictEqual(coord.z, 0, 'z should be 0');
    });

    test('should create TileCoord from Berlin coordinates', () => {
      // Berlin: 13.4°E, 52.5°N at zoom 5
      const coord = TileCoord.fromGeo(13.4, 52.5, 5);
      assert.strictEqual(coord.z, 5, 'z should be 5');

      // Should be tile 17,10 or very close
      assert.ok(coord.x >= 16 && coord.x <= 18, 'x should be around 17');
      assert.ok(coord.y >= 9 && coord.y <= 11, 'y should be around 10');
    });

    test('should handle extreme coordinates', () => {
      const coord1 = TileCoord.fromGeo(-180, 85, 5);
      const coord2 = TileCoord.fromGeo(180, -85, 5);

      assert.ok(coord1, 'should handle western extreme');
      assert.ok(coord2, 'should handle eastern extreme');
    });

    test('should throw error for invalid longitude', () => {
      assert.throws(
        () => TileCoord.fromGeo(181, 0, 5),
        'Should throw error for lon > 180'
      );

      assert.throws(
        () => TileCoord.fromGeo(-181, 0, 5),
        'Should throw error for lon < -180'
      );
    });

    test('should throw error for invalid latitude', () => {
      assert.throws(
        () => TileCoord.fromGeo(0, 91, 5),
        'Should throw error for lat > 90'
      );

      assert.throws(
        () => TileCoord.fromGeo(0, -91, 5),
        'Should throw error for lat < -90'
      );
    });

    test('should work at different zoom levels', () => {
      const coord0 = TileCoord.fromGeo(0, 0, 0);
      const coord5 = TileCoord.fromGeo(0, 0, 5);
      const coord10 = TileCoord.fromGeo(0, 0, 10);

      assert.strictEqual(coord0.z, 0, 'zoom 0 should work');
      assert.strictEqual(coord5.z, 5, 'zoom 5 should work');
      assert.strictEqual(coord10.z, 10, 'zoom 10 should work');
    });
  });

  describe('round-trip conversion', () => {
    test('should maintain zoom level in round-trip', () => {
      const original = new TileCoord(5, 17, 10);
      const [lon, lat] = original.toGeo();
      const roundTrip = TileCoord.fromGeo(lon, lat, 5);

      assert.strictEqual(roundTrip.z, original.z, 'zoom should be preserved');
      assert.strictEqual(roundTrip.x, original.x, 'x should be preserved');
      assert.strictEqual(roundTrip.y, original.y, 'y should be preserved');
    });

    test('should handle multiple round-trips', () => {
      const coords = [
        new TileCoord(0, 0, 0),
        new TileCoord(5, 17, 10),
        new TileCoord(10, 512, 384),
      ];

      coords.forEach((original) => {
        const [lon, lat] = original.toGeo();
        const roundTrip = TileCoord.fromGeo(lon, lat, original.z);

        assert.strictEqual(roundTrip.x, original.x, `x should match for ${original.z},${original.x},${original.y}`);
        assert.strictEqual(roundTrip.y, original.y, `y should match for ${original.z},${original.x},${original.y}`);
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
      assert.ok(lonDiff < 0.5, 'longitude should be close');
      assert.ok(latDiff < 0.5, 'latitude should be close');
    });
  });

  describe('edge cases', () => {
    test('should handle antimeridian (180°)', () => {
      const coord = TileCoord.fromGeo(180, 0, 5);
      assert.ok(coord, 'should handle 180° longitude');
    });

    test('should handle dateline (-180°)', () => {
      const coord = TileCoord.fromGeo(-180, 0, 5);
      assert.ok(coord, 'should handle -180° longitude');
    });

    test('should handle poles (approximately)', () => {
      // Web Mercator doesn't extend to exact poles, but should handle close to them
      const north = TileCoord.fromGeo(0, 85, 5);
      const south = TileCoord.fromGeo(0, -85, 5);

      assert.ok(north, 'should handle northern extreme');
      assert.ok(south, 'should handle southern extreme');
    });

    test('should handle all corners at zoom 1', () => {
      const coords = [
        TileCoord.fromGeo(-180, 85, 1),   // NW
        TileCoord.fromGeo(180, 85, 1),    // NE
        TileCoord.fromGeo(-180, -85, 1),  // SW
        TileCoord.fromGeo(180, -85, 1),   // SE
      ];

      coords.forEach((coord, i) => {
        assert.ok(coord, `corner ${i} should be valid`);
        assert.ok(coord.x >= 0 && coord.x < 2, `corner ${i} x should be 0 or 1`);
        assert.ok(coord.y >= 0 && coord.y < 2, `corner ${i} y should be 0 or 1`);
      });
    });
  });
});
