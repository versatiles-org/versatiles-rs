#!/usr/bin/env node

/**
 * Probe and inspect tile containers
 *
 * This example shows how to inspect tile containers and retrieve
 * metadata, format information, and other details.
 */

import { TileSource } from '../index.js';
import { log } from './lib/logger.mjs';

log.title('VersaTiles Probe Example');

const containerPath = new URL('../../testdata/berlin.mbtiles', import.meta.url).pathname;
const container = await TileSource.open(containerPath);

log.section('Container Information');
const sourceType = await container.sourceType();
log.info('Type', `${sourceType.kind} (${sourceType.name})`);
if (sourceType.uri) {
	log.info('URI', sourceType.uri);
}

log.section('Tile Metadata');
const metadata = await container.metadata();
log.info('Format', metadata.tileFormat);
log.info('Compression', metadata.tileCompression);
log.info('Zoom Range', `${metadata.minZoom} - ${metadata.maxZoom}`);

// Parse and display TileJSON
const tileJSON = JSON.parse(await container.tileJson());
log.section('TileJSON Metadata');
log.info('Version', tileJSON.tilejson);
log.info('Bounds', tileJSON.bounds);
if (tileJSON.vector_layers) {
	log.info('Vector Layers', tileJSON.vector_layers.length);
}
