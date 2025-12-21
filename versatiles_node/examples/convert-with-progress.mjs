#!/usr/bin/env node

/**
 * Convert tiles with progress monitoring
 *
 * This example demonstrates how to use the Progress API to monitor
 * tile conversion operations in real-time, including progress updates,
 * step changes, warnings, and errors.
 */

import { convert } from '../index.js';
import path from 'path';
import { tmpdir } from 'os';

console.log('VersaTiles Progress Monitoring Example\n');

const inputPath = new URL('../../testdata/berlin.mbtiles', import.meta.url).pathname;
const outputPath = path.join(tmpdir(), 'output-with-progress.versatiles');

console.log(`Input:  ${inputPath}`);
console.log(`Output: ${outputPath}\n`);

// Start the conversion with progress monitoring
await convert(
	inputPath,
	outputPath,
	{
		minZoom: 0,
		maxZoom: 13,
		compress: 'brotli',
	},
	(data) => {
		console.log(
			[
				`Progress: ${data.percentage.toFixed(1)}%`,
				`(${data.position.toFixed(0)}/${data.total.toFixed(0)} tiles)`,
				`| ${data.speed.toFixed(0)} tiles/sec`,
				`| ETA: ${new Date(data.eta).toTimeString().split(' ')[0]}`,
			].join(' '),
		);
	},
);
console.log(`\n✓ Output saved to: ${outputPath}`);
