#!/usr/bin/env node

/**
 * Serve tiles via HTTP server
 *
 * This example shows how to start an HTTP tile server that serves
 * tiles from various container formats.
 */

const { TileServer } = require('../index.js');
const path = require('path');

async function main() {
	console.log('VersaTiles Server Example\n');

	// Example 1: Simple server with one tile source
	console.log('Example 1: Basic tile server');
	try {
		const server = new TileServer({
			ip: '127.0.0.1',
			port: 8080,
			minimalRecompression: true,
		});

		// Add a tile source
		const tilesPath = path.join(__dirname, '../../testdata/berlin.mbtiles');
		await server.addTileSource('berlin', tilesPath);

		// Start the server
		await server.start();
		const port = await server.port;

		console.log(`✓ Server started successfully!`);
		console.log(`\nTile URLs:`);
		console.log(`  Tiles: http://127.0.0.1:${port}/tiles/berlin/{z}/{x}/{y}`);
		console.log(`  TileJSON: http://127.0.0.1:${port}/tiles/berlin/meta.json`);
		console.log(`  Status: http://127.0.0.1:${port}/status`);
		console.log('\nPress Ctrl+C to stop the server...');

		// Keep server running until interrupted
		await new Promise((resolve) => {
			process.on('SIGINT', async () => {
				console.log('\n\nStopping server...');
				await server.stop();
				console.log('✓ Server stopped');
				resolve();
			});
		});
	} catch (err) {
		console.error('✗ Server failed:', err.message);
		process.exit(1);
	}
}

// Example 2: Server with multiple tile sources (commented out)
async function multiSourceExample() {
	console.log('Example 2: Server with multiple tile sources');

	const server = new TileServer({ port: 8080 });

	// Add multiple tile sources
	await server.addTileSource('osm', path.join(__dirname, '../../testdata/berlin.mbtiles'));
	// Uncomment these if you have more test data:
	// await server.addTileSource('satellite', './satellite.mbtiles');
	// await server.addTileSource('terrain', './terrain.versatiles');

	await server.start();
	const port = await server.port;

	console.log(`✓ Multi-source server started on port ${port}`);
	console.log('\nAvailable sources:');
	console.log('  /tiles/osm/{z}/{x}/{y}');
	// console.log('  /tiles/satellite/{z}/{x}/{y}');
	// console.log('  /tiles/terrain/{z}/{x}/{y}');

	// Keep running...
	await new Promise(() => {});
}

// Example 3: Server with static files (commented out)
async function staticFilesExample() {
	console.log('Example 3: Server with tiles and static files');

	const server = new TileServer({ port: 8080 });

	// Add tile sources
	await server.addTileSource('tiles', './tiles.mbtiles');

	// Add static files (if you have a static.tar file)
	// await server.addStaticSource('./static.tar', '/');

	await server.start();
	const port = await server.port;

	console.log(`✓ Server with static files started on port ${port}`);
	console.log('\nURLs:');
	console.log('  Tiles: /tiles/tiles/{z}/{x}/{y}');
	console.log('  Static: / (index.html, style.css, etc.)');

	// Keep running...
	await new Promise(() => {});
}

// Example 4: Dynamic server (add sources while running)
async function dynamicExample() {
	console.log('Example 4: Dynamic source management');

	const server = new TileServer({ port: 8080 });
	await server.start();
	const port = await server.port;

	console.log(`✓ Empty server started on port ${port}`);

	// Add sources dynamically
	console.log('\nAdding tile source "berlin"...');
	await server.addTileSource('berlin', path.join(__dirname, '../../testdata/berlin.mbtiles'));
	console.log('✓ Source added: /tiles/berlin/{z}/{x}/{y}');

	// You could add more sources here while the server is running
	// setTimeout(async () => {
	//   console.log('\nAdding another source...');
	//   await server.addTileSource('world', './world.mbtiles');
	//   console.log('✓ Source added: /tiles/world/{z}/{x}/{y}');
	// }, 5000);

	console.log('\nServer is running. Press Ctrl+C to stop...');

	// Keep running...
	await new Promise((resolve) => {
		process.on('SIGINT', async () => {
			console.log('\n\nStopping server...');
			await server.stop();
			console.log('✓ Server stopped');
			resolve();
		});
	});
}

// Run the appropriate example
const example = process.argv[2] || '1';

switch (example) {
	case '1':
		main();
		break;
	case '2':
		multiSourceExample();
		break;
	case '3':
		staticFilesExample();
		break;
	case '4':
		dynamicExample();
		break;
	default:
		console.log('Usage: node serve.js [1|2|3|4]');
		console.log('  1: Basic server (default)');
		console.log('  2: Multiple sources');
		console.log('  3: Static files');
		console.log('  4: Dynamic sources');
		process.exit(0);
}
