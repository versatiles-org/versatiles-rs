#!/usr/bin/env node

/**
 * Convert tiles with progress monitoring
 *
 * This example demonstrates how to use the Progress API to monitor
 * tile conversion operations in real-time, including progress updates,
 * step changes, warnings, and errors.
 */

const { ContainerReader } = require('../index.js');
const path = require('path');

async function main() {
	console.log('VersaTiles Progress Monitoring Example\n');

	const inputPath = path.join(__dirname, '../../testdata/berlin.mbtiles');
	const outputPath = path.join(__dirname, 'output-with-progress.versatiles');

	console.log(`Input:  ${inputPath}`);
	console.log(`Output: ${outputPath}\n`);

	try {
		// Open the input container
		const reader = await ContainerReader.open(inputPath);
		console.log('✓ Opened container\n');

		// Start the conversion with progress monitoring
		const progress = reader.convertToWithProgress(outputPath, {
			minZoom: 0,
			maxZoom: 14,
			compress: 'gzip',
		});

		// Track conversion state
		let lastPercentage = -1;
		let stepCount = 0;

		// Listen for progress updates
		progress.on('progress', (data) => {
			// Only log when percentage changes significantly (every 5%)
			const roundedPercentage = Math.floor(data.percentage / 5) * 5;
			if (roundedPercentage !== lastPercentage && roundedPercentage % 5 === 0) {
				lastPercentage = roundedPercentage;

				const tilesPerSec = data.speed.toFixed(0);
				const etaSeconds = data.eta.toFixed(0);
				const percentage = data.percentage.toFixed(1);
				const position = data.position.toFixed(0);
				const total = data.total.toFixed(0);

				console.log(
					`  Progress: ${percentage}% (${position}/${total} tiles) | ` +
						`${tilesPerSec} tiles/sec | ETA: ${etaSeconds}s`,
				);
			}
		});

		// Listen for step changes (e.g., "Reading tiles", "Writing tiles")
		progress.on('step', (message) => {
			stepCount++;
			console.log(`\n[Step ${stepCount}] ${message}`);
		});

		// Listen for warnings
		progress.on('warning', (message) => {
			console.warn(`⚠️  Warning: ${message}`);
		});

		// Listen for errors
		progress.on('error', (message) => {
			console.error(`✗ Error: ${message}`);
		});

		// Listen for completion
		progress.on('complete', () => {
			console.log('\n✓ Conversion completed successfully!');
		});

		// Wait for the operation to finish
		// This will throw if there was an error
		await progress.done();

		console.log(`\n✓ Output saved to: ${outputPath}`);
	} catch (err) {
		console.error('\n✗ Conversion failed:', err.message);
		process.exit(1);
	}
}

// Run the example
main().catch((err) => {
	console.error('Fatal error:', err);
	process.exit(1);
});
