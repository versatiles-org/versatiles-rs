#!/usr/bin/env node

/**
 * Convert tiles with progress monitoring
 *
 * This example demonstrates how to use the Progress API to monitor
 * tile conversion operations in real-time, including progress updates,
 * step changes, warnings, and errors.
 */

import { convert, type ProgressData } from '@versatiles/versatiles-rs';
import path from 'path';
import { tmpdir } from 'os';
import { fileURLToPath } from 'url';
import { log } from './lib/logger.js';

log.title('VersaTiles Progress Monitoring Example');

const inputPath = fileURLToPath(new URL('../../testdata/berlin.mbtiles', import.meta.url));
const outputPath = path.join(tmpdir(), 'output-with-progress.versatiles');

log.path('Input', inputPath);
log.path('Output', outputPath);
console.log();

// Start the conversion with progress monitoring
await convert(
	inputPath,
	outputPath,
	{
		minZoom: 0,
		maxZoom: 13,
		compress: 'brotli',
	},
	(data: ProgressData) => {
		log.progress(data);
	},
);

log.success(`Output saved to: ${outputPath}`);
