#!/usr/bin/env node

/**
 * Read individual tiles and work with tile coordinates
 *
 * This example shows how to read tiles from containers,
 * work with tile coordinates, and convert between geographic
 * and tile coordinate systems.
 */

import { tmpdir } from 'os';
import { TileSource, TileCoord } from '../index.js';
import fs from 'fs/promises';
import path from 'path';
import { log } from './lib/logger.mjs';

log.title('VersaTiles Tile Reading Example');

const containerPath = new URL('../../testdata/berlin.mbtiles', import.meta.url).pathname;

{
	// Example 1: Read a single tile
	log.section('Example 1: Read a single tile');

	const reader = await TileSource.open(containerPath);

	// Get a tile at zoom 10, column 550, row 335 (Berlin area)
	const tile = await reader.getTile(10, 550, 335);

	if (!tile) throw new Error('Tile not found!');

	log.info('Size', `${tile.length} bytes`);
	log.info('Type', Buffer.isBuffer(tile) ? 'Buffer' : typeof tile);

	// Optionally save the tile to a file
	const outputPath = path.join(tmpdir(), 'tile-10-550-335.png');
	await fs.writeFile(outputPath, tile);
	log.path('Saved to', outputPath);
}

{
	// Example 2: Read multiple tiles
	log.section('Example 2: Read multiple tiles');
	const reader = await TileSource.open(containerPath);

	const tiles = [
		{ z: 5, x: 17, y: 10 },
		{ z: 5, x: 17, y: 11 },
		{ z: 6, x: 34, y: 20 },
	];

	for (const coord of tiles) {
		const tile = await reader.getTile(coord.z, coord.x, coord.y);
		const status = tile ? `${tile.length} bytes` : 'not found';
		log.tileStatus(coord, status, !!tile);
	}
}

{
	// Example 3: Coordinate conversion - Geographic to Tile
	log.section('Example 3: Convert geographic coordinates to tile coordinates');

	// Berlin coordinates
	const lon = 13.405;
	const lat = 52.52;
	const zoom = 10;

	const coord = TileCoord.fromGeo(lon, lat, zoom);

	log.info('Geographic coordinates', `${lon}°, ${lat}°`);
	log.info('Tile coordinates', `zoom=${zoom}, x=${coord.x}, y=${coord.y}`);
	// Get the tile at these coordinates

	const reader = await TileSource.open(containerPath);
	const tile = await reader.getTile(coord.z, coord.x, coord.y);

	log.info('Tile size', `${tile.length} bytes`);
}

{
	// Example 4: Coordinate conversion - Tile to Geographic
	log.section('Example 4: Convert tile coordinates to geographic coordinates');

	const coord = new TileCoord(10, 550, 335);

	log.info('Tile coordinates', `z=${coord.z}, x=${coord.x}, y=${coord.y}`);

	const [lon, lat] = coord.toGeo();
	log.info('Geographic coordinates (NW corner)', `${lon}°, ${lat}°`);

	const bbox = coord.toGeoBbox();
	log.info('Tile bounding box', '');
	log.infoIndented('West', `${bbox[0]}°`);
	log.infoIndented('South', `${bbox[1]}°`);
	log.infoIndented('East', `${bbox[2]}°`);
	log.infoIndented('North', `${bbox[3]}°`);

	const json = coord.toJson();
	log.info('JSON representation', json);
}

{
	// Example 5: Read tiles in a geographic area
	log.section('Example 5: Read all tiles in a bounding box');

	const reader = await TileSource.open(containerPath);

	// Define a small area in Berlin
	const west = 13.4;
	const south = 52.51;
	const east = 13.41;
	const north = 52.52;
	const zoom = 14;

	// Convert corners to tile coordinates
	const nw = TileCoord.fromGeo(west, north, zoom);
	const se = TileCoord.fromGeo(east, south, zoom);

	log.info('Area', `${west}°, ${south}° to ${east}°, ${north}°`);
	log.info('Tile range', `x=${nw.x}-${se.x}, y=${nw.y}-${se.y}`);

	let tileCount = 0;
	let totalSize = 0;

	// Read all tiles in the range
	for (let x = nw.x; x <= se.x; x++) {
		for (let y = nw.y; y <= se.y; y++) {
			const tile = await reader.getTile(zoom, x, y);
			if (tile) {
				tileCount++;
				totalSize += tile.length;
			}
		}
	}

	log.info('Found', `${tileCount} tiles`);
	log.info('Total size', `${(totalSize / 1024).toFixed(2)} KB`);
}

{
	// Example 6: Get tile information without reading data
	log.section('Example 6: Check tile availability');

	const reader = await TileSource.open(containerPath);

	const testCoords = [
		{ z: 0, x: 0, y: 0, name: 'World (zoom 0)' },
		{ z: 5, x: 17, y: 10, name: 'Europe (zoom 5)' },
		{ z: 10, x: 550, y: 335, name: 'Berlin (zoom 10)' },
		{ z: 14, x: 8800, y: 5370, name: 'Berlin street (zoom 14)' },
	];

	for (const coord of testCoords) {
		const tile = await reader.getTile(coord.z, coord.x, coord.y);
		const status = tile ? 'available' : 'not available';
		log.info(coord.name, status);
	}
}

log.success('All examples completed!');
