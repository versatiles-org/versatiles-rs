#!/usr/bin/env node

/**
 * Convert tiles between different formats
 *
 * This example shows how to convert tiles from one format to another,
 * with optional filtering by zoom level and bounding box.
 */

import { tmpdir } from 'os';
import { convert } from '../index.js';
import path from 'path';

console.log('VersaTiles Conversion Example\n');

// Example 1: Simple conversion
console.log('Example 1: Convert MBTiles to VersaTiles');
const inputPath = new URL('../../testdata/berlin.mbtiles', import.meta.url).pathname;
const outputPath = path.join(tmpdir(), 'output.versatiles');


await convert(inputPath, outputPath);
console.log('✓ Conversion complete:', outputPath);

// Example 2: Conversion with zoom filtering
console.log('\nExample 2: Convert with zoom level filtering');
const outputFiltered = path.join(tmpdir(), 'output-filtered.versatiles');

await convert(inputPath, outputFiltered, {
	minZoom: 5,
	maxZoom: 12,
});
console.log('✓ Filtered conversion complete:', outputFiltered);

// Example 3: Conversion with bounding box
console.log('\nExample 3: Convert with bounding box (Berlin area)');
const outputBbox = path.join(tmpdir(), 'output-bbox.versatiles');

await convert(inputPath, outputBbox, {
	bbox: [13.38, 52.46, 13.43, 52.49], // [west, south, east, north]
	bboxBorder: 2, // Add 2 tiles border around bbox
	minZoom: 10,
	maxZoom: 14,
});
console.log('✓ BBox conversion complete:', outputBbox);

// Example 4: Conversion with compression
console.log('\nExample 4: Convert with gzip compression');
const outputCompressed = path.join(tmpdir(), 'output-compressed.versatiles');

await convert(inputPath, outputCompressed, {
	compress: 'gzip',
	minZoom: 0,
	maxZoom: 14,
});
console.log('✓ Compressed conversion complete:', outputCompressed);

// Example 5: Conversion with coordinate transformations
console.log('\nExample 5: Convert with coordinate transformation');
const outputFlipped = path.join(tmpdir(), 'output-flipped.versatiles');

await convert(inputPath, outputFlipped, {
	flipY: true, // Flip tiles vertically
	swapXy: false, // Don't swap x and y
	minZoom: 0,
	maxZoom: 10,
});
console.log('✓ Transformed conversion complete:', outputFlipped);
