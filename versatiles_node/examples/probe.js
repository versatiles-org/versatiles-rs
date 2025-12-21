#!/usr/bin/env node

/**
 * Probe and inspect tile containers
 *
 * This example shows how to inspect tile containers and retrieve
 * metadata, format information, and other details.
 */

const { ContainerReader } = require('../index.js');
const path = require('path');

async function main() {
	console.log('VersaTiles Probe Example\n');

	const containerPath = path.join(__dirname, '../../testdata/berlin.mbtiles');

	// Example 1: Quick probe with probeTiles function
	console.log('Example 1: Quick probe using probeTiles()');
	try {
		const result = await probeTiles(containerPath);

		console.log('Container Information:');
		console.log('  Type:', result.containerName);
		console.log('  Source:', result.sourceName);
		console.log('\nTile Parameters:');
		console.log('  Format:', result.parameters.tileFormat);
		console.log('  Compression:', result.parameters.tileCompression);
		console.log('  Zoom Range:', result.parameters.minZoom, '-', result.parameters.maxZoom);

		// Parse and display TileJSON
		const tileJSON = JSON.parse(result.tileJson);
		console.log('\nTileJSON Metadata:');
		console.log('  Version:', tileJSON.tilejson);
		console.log('  Bounds:', tileJSON.bounds);
		console.log('  Center:', tileJSON.center);
		if (tileJSON.vector_layers) {
			console.log('  Vector Layers:', tileJSON.vector_layers.length);
		}
	} catch (err) {
		console.error('✗ Probe failed:', err.message);
	}

	// Example 2: Detailed inspection using ContainerReader
	console.log('\n\nExample 2: Detailed inspection using ContainerReader');
	try {
		const reader = await ContainerReader.open(containerPath);

		console.log('Container Details:');
		console.log('  Container Type:', await reader.containerName);
		console.log('  Source Name:', await reader.sourceName);

		const params = await reader.parameters;
		console.log('\nTile Information:');
		console.log('  Format:', params.tileFormat);
		console.log('  Compression:', params.tileCompression);
		console.log('  Min Zoom:', params.minZoom);
		console.log('  Max Zoom:', params.maxZoom);

		// Get full TileJSON
		const tileJSON = JSON.parse(await reader.tileJSON);
		console.log('\nTileJSON Details:');
		console.log('  Name:', tileJSON.name || 'N/A');
		console.log('  Description:', tileJSON.description || 'N/A');
		console.log('  Attribution:', tileJSON.attribution || 'N/A');
		console.log('  Tile Type:', tileJSON.tile_type || 'N/A');
		console.log('  Tile Schema:', tileJSON.tile_schema || 'N/A');

		if (tileJSON.bounds) {
			const [west, south, east, north] = tileJSON.bounds;
			console.log('\nGeographic Coverage:');
			console.log(`  West: ${west}°`);
			console.log(`  South: ${south}°`);
			console.log(`  East: ${east}°`);
			console.log(`  North: ${north}°`);
		}

		if (tileJSON.center) {
			const [lon, lat, zoom] = tileJSON.center;
			console.log('\nCenter Point:');
			console.log(`  Longitude: ${lon}°`);
			console.log(`  Latitude: ${lat}°`);
			console.log(`  Default Zoom: ${zoom}`);
		}
	} catch (err) {
		console.error('✗ Detailed inspection failed:', err.message);
	}

	// Example 3: Compare multiple containers
	console.log('\n\nExample 3: Compare container formats');
	const containers = [
		path.join(__dirname, '../../testdata/berlin.mbtiles'),
		// Add more containers here if available
	];

	for (const containerPath of containers) {
		try {
			const result = await probeTiles(containerPath);
			const tileJSON = JSON.parse(result.tileJson);

			console.log(`\n${path.basename(containerPath)}:`);
			console.log('  Type:', result.containerName);
			console.log('  Format:', result.parameters.tileFormat);
			console.log('  Compression:', result.parameters.tileCompression);
			console.log('  Zoom:', `${result.parameters.minZoom}-${result.parameters.maxZoom}`);
			console.log('  Bounds:', tileJSON.bounds);
		} catch (err) {
			console.error(`  ✗ Failed to probe: ${err.message}`);
		}
	}

	console.log('\n✓ Probe examples completed!');
}

// Run the examples
main().catch((err) => {
	console.error('Fatal error:', err);
	process.exit(1);
});
