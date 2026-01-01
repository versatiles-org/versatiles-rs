#!/usr/bin/env node

/**
 * Probe and inspect tile containers
 *
 * This example shows how to inspect tile containers and retrieve
 * metadata, format information, and other details.
 */

import { TileSource } from '@versatiles/versatiles-rs';
import { log } from './lib/logger.js';

log.title('VersaTiles Probe Example');

const containerPath = new URL('../../testdata/berlin.mbtiles', import.meta.url).pathname;
const source = await TileSource.open(containerPath);

log.section('Container Information');
const sourceType = source.sourceType();
log.info('Type', `${sourceType.kind} (${sourceType.name})`);
if (sourceType.uri) {
	log.info('URI', sourceType.uri);
}

log.section('Tile Metadata');
const metadata = source.metadata();
log.info('Format', metadata.tileFormat);
log.info('Compression', metadata.tileCompression);
log.info('Zoom Range', `${metadata.minZoom} - ${metadata.maxZoom}`);

// Parse and display TileJSON
const tileJSON = source.tileJson();
log.section('TileJSON Metadata');
log.info('Version', tileJSON.tilejson);
log.info('Bounds', tileJSON.bounds ? tileJSON.bounds.join(', ') : 'None');
if (tileJSON.vectorLayers) {
	log.info('Vector Layers', tileJSON.vectorLayers.length.toString());
}
