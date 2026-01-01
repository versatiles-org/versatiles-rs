#!/usr/bin/env node

/**
 * Serve tiles via HTTP server
 *
 * This example shows how to start an HTTP tile server that serves
 * tiles from various container formats.
 */

import { TileServer } from '@versatiles/versatiles-rs';
import { log } from './lib/logger.js';

log.title('VersaTiles Server Example');

{
	// Example 1: Simple server with one tile source
	log.section('Example 1: Basic tile server');

	const server = new TileServer({
		ip: '127.0.0.1',
		port: 8080,
		minimalRecompression: true,
	});

	// Add a tile source
	const tilesPath = new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname;
	await server.addTileSourceFromPath('berlin', tilesPath);

	// Start the server
	await server.start();
	const port = await server.port;

	log.success('Server started successfully!');
	console.log();
	log.url('Tiles', `http://127.0.0.1:${port}/tiles/berlin/{z}/{x}/{y}`);
	log.url('TileJSON', `http://127.0.0.1:${port}/tiles/berlin/meta.json`);
	log.url('Status', `http://127.0.0.1:${port}/status`);

	await sleepForOneSecond();
	await server.stop();
}

{
	// Example 2: Server with multiple tile sources (commented out)

	log.section('Example 2: Server with multiple tile sources');

	const server = new TileServer({ port: 8080 });

	// Add multiple tile sources
	await server.addTileSourceFromPath('osm', new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname);
	// Uncomment these if you have more test data:
	// await server.addTileSourceFromPath('satellite', './satellite.pmtiles');
	// await server.addTileSourceFromPath('terrain', './terrain.versatiles');

	await server.start();
	const port = await server.port;

	log.success(`Multi-source server started on port ${port}`);
	log.text('\nAvailable sources:');
	log.text('/tiles/osm/{z}/{x}/{y}', 4);
	// log.text('/tiles/satellite/{z}/{x}/{y}', 4);
	// log.text('/tiles/terrain/{z}/{x}/{y}', 4);

	await sleepForOneSecond();
	await server.stop();
}

{
	// Example 3: Server with static files (commented out)

	log.section('Example 3: Server with tiles and static files');

	const server = new TileServer({ port: 8080 });

	// Add tile sources
	await server.addTileSourceFromPath('tiles', new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname);

	// Add static files (if you have a static.tar file)
	// await server.addStaticSource('./static.tar', '/');

	await server.start();
	const port = await server.port;

	log.success(`Server with static files started on port ${port}`);
	log.text('\nURLs:');
	log.text('Tiles: /tiles/tiles/{z}/{x}/{y}', 4);
	log.text('Static: / (index.html, style.css, etc.)', 4);

	await sleepForOneSecond();
	await server.stop();
}

{
	// Example 4: Dynamic server (add sources while running)

	log.section('Example 4: Dynamic source management');

	const server = new TileServer({ port: 8080 });
	await server.start();
	const port = await server.port;

	log.success(`Empty server started on port ${port}`);

	// Add sources dynamically
	log.text('\nAdding tile source "berlin"...');
	await server.addTileSourceFromPath('berlin', new URL('../../testdata/berlin.pmtiles', import.meta.url).pathname);
	log.success('Source added: /tiles/berlin/{z}/{x}/{y}');

	await sleepForOneSecond();
	log.text('\nRemoving tile source "berlin"...');
	await server.removeTileSource('berlin');
	log.success('Source removed.');

	await sleepForOneSecond();
	await server.stop();
}

function sleepForOneSecond() {
	return new Promise((resolve) => setTimeout(resolve, 1000));
}
