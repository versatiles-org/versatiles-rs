#!/usr/bin/env node

/**
 * Serve tiles via HTTP server
 *
 * This example shows how to start an HTTP tile server that serves
 * tiles from various container formats.
 */

import { TileServer } from '../index.js';



console.log('VersaTiles Server Example\n');

{
	// Example 1: Simple server with one tile source
	console.log('\nExample 1: Basic tile server');

	const server = new TileServer({
		ip: '127.0.0.1',
		port: 8080,
		minimalRecompression: true,
	});

	// Add a tile source
	const tilesPath = new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname;
	await server.addTileSource('berlin', tilesPath);

	// Start the server
	await server.start();
	const port = await server.port;

	console.log(`  ✓ Server started successfully!`);
	console.log(`\n  Tile URLs:`);
	console.log(`    Tiles: http://127.0.0.1:${port}/tiles/berlin/{z}/{x}/{y}`);
	console.log(`    TileJSON: http://127.0.0.1:${port}/tiles/berlin/meta.json`);
	console.log(`    Status: http://127.0.0.1:${port}/status`);

	await sleepForOneSecond();
	await server.stop();
}

{
	// Example 2: Server with multiple tile sources (commented out)

	console.log('\nExample 2: Server with multiple tile sources');

	const server = new TileServer({ port: 8080 });

	// Add multiple tile sources
	await server.addTileSource('osm', new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname);
	// Uncomment these if you have more test data:
	// await server.addTileSource('satellite', './satellite.pmtiles');
	// await server.addTileSource('terrain', './terrain.versatiles');

	await server.start();
	const port = await server.port;

	console.log(`  ✓ Multi-source server started on port ${port}`);
	console.log('\n  Available sources:');
	console.log('    /tiles/osm/{z}/{x}/{y}');
	// console.log('  /tiles/satellite/{z}/{x}/{y}');
	// console.log('  /tiles/terrain/{z}/{x}/{y}');

	
	await sleepForOneSecond();
	await server.stop();
}

{
	// Example 3: Server with static files (commented out)

	console.log('\nExample 3: Server with tiles and static files');

	const server = new TileServer({ port: 8080 });

	// Add tile sources
	await server.addTileSource('tiles', new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname);

	// Add static files (if you have a static.tar file)
	// await server.addStaticSource('./static.tar', '/');

	await server.start();
	const port = await server.port;

	console.log(`  ✓ Server with static files started on port ${port}`);
	console.log('\n  URLs:');
	console.log('    Tiles: /tiles/tiles/{z}/{x}/{y}');
	console.log('    Static: / (index.html, style.css, etc.)');

	
	await sleepForOneSecond();
	await server.stop();
}

{
	// Example 4: Dynamic server (add sources while running)

	console.log('\nExample 4: Dynamic source management');

	const server = new TileServer({ port: 8080 });
	await server.start();
	const port = await server.port;

	console.log(`  ✓ Empty server started on port ${port}`);

	// Add sources dynamically
	console.log('\n  Adding tile source "berlin"...');
	await server.addTileSource('berlin', new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname);
	console.log('  ✓ Source added: /tiles/berlin/{z}/{x}/{y}');

	await sleepForOneSecond();
	console.log('\n  Removing tile source "berlin"...');
	await server.removeTileSource('berlin');
	console.log('  ✓ Source removed.');
	
	await sleepForOneSecond();
	await server.stop();
}

function sleepForOneSecond() {
	return new Promise((resolve) => setTimeout(resolve, 1000));
}