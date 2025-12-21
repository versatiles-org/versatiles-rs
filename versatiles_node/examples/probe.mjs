#!/usr/bin/env node

/**
 * Probe and inspect tile containers
 *
 * This example shows how to inspect tile containers and retrieve
 * metadata, format information, and other details.
 */

import { ContainerReader } from '../index.js';

console.log('VersaTiles Probe Example\n');

const containerPath = new URL('../../testdata/berlin.mbtiles', import.meta.url).pathname;
const container = await ContainerReader.open(containerPath);

console.log('Container Information:');
console.log('  Type:', await container.containerName());
console.log('  Source:', await container.sourceName());

console.log('\nTile Parameters:');
const parameters = await container.parameters();
console.log('  Format:', parameters.tileFormat);
console.log('  Compression:', parameters.tileCompression);
console.log('  Zoom Range:', parameters.minZoom, '-', parameters.maxZoom);

// Parse and display TileJSON
const tileJSON = JSON.parse(await container.tileJson());
console.log('\nTileJSON Metadata:');
console.log('  Version:', tileJSON.tilejson);
console.log('  Bounds:', tileJSON.bounds);
if (tileJSON.vector_layers) {
	console.log('  Vector Layers:', tileJSON.vector_layers.length);
}
