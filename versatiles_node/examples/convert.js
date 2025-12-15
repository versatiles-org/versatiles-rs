#!/usr/bin/env node

/**
 * Convert tiles between different formats
 *
 * This example shows how to convert tiles from one format to another,
 * with optional filtering by zoom level and bounding box.
 */

const { convertTiles } = require('../index.js');
const path = require('path');

async function main() {
	console.log('VersaTiles Conversion Example\n');

	// Example 1: Simple conversion
	console.log('Example 1: Convert MBTiles to VersaTiles');
	const inputPath = path.join(__dirname, '../../testdata/berlin.mbtiles');
	const outputPath = path.join(__dirname, 'output.versatiles');

	try {
		await convertTiles(inputPath, outputPath);
		console.log('✓ Conversion complete:', outputPath);
	} catch (err) {
		console.error('✗ Conversion failed:', err.message);
	}

	// Example 2: Conversion with zoom filtering
	console.log('\nExample 2: Convert with zoom level filtering');
	const outputFiltered = path.join(__dirname, 'output-filtered.versatiles');

	try {
		await convertTiles(inputPath, outputFiltered, {
			minZoom: 5,
			maxZoom: 12,
		});
		console.log('✓ Filtered conversion complete:', outputFiltered);
	} catch (err) {
		console.error('✗ Conversion failed:', err.message);
	}

	// Example 3: Conversion with bounding box
	console.log('\nExample 3: Convert with bounding box (Berlin area)');
	const outputBbox = path.join(__dirname, 'output-bbox.versatiles');

	try {
		await convertTiles(inputPath, outputBbox, {
			bbox: [13.38, 52.46, 13.43, 52.49], // [west, south, east, north]
			bboxBorder: 2, // Add 2 tiles border around bbox
			minZoom: 10,
			maxZoom: 14,
		});
		console.log('✓ BBox conversion complete:', outputBbox);
	} catch (err) {
		console.error('✗ Conversion failed:', err.message);
	}

	// Example 4: Conversion with compression
	console.log('\nExample 4: Convert with gzip compression');
	const outputCompressed = path.join(__dirname, 'output-compressed.versatiles');

	try {
		await convertTiles(inputPath, outputCompressed, {
			compress: 'gzip',
			minZoom: 0,
			maxZoom: 14,
		});
		console.log('✓ Compressed conversion complete:', outputCompressed);
	} catch (err) {
		console.error('✗ Conversion failed:', err.message);
	}

	// Example 5: Conversion with coordinate transformations
	console.log('\nExample 5: Convert with coordinate transformation');
	const outputFlipped = path.join(__dirname, 'output-flipped.versatiles');

	try {
		await convertTiles(inputPath, outputFlipped, {
			flipY: true, // Flip tiles vertically
			swapXy: false, // Don't swap x and y
			minZoom: 0,
			maxZoom: 10,
		});
		console.log('✓ Transformed conversion complete:', outputFlipped);
	} catch (err) {
		console.error('✗ Conversion failed:', err.message);
	}

	console.log('\n✓ All examples completed!');
}

// Run the examples
main().catch((err) => {
	console.error('Fatal error:', err);
	process.exit(1);
});
