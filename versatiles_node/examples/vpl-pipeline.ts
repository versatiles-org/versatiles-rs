#!/usr/bin/env node

/**
 * Build VPL (VersaTiles Pipeline Language) pipelines programmatically
 */

import { VPL } from '@versatiles/versatiles-rs/vpl';
import { fileURLToPath } from 'url';
import { log } from './lib/logger.js';

log.title('VersaTiles VPL Pipeline Examples');

const testdataDir = fileURLToPath(new URL('../../testdata/', import.meta.url));

{
	// Example 1: Read from a container and chain transforms
	log.section('Example 1: Read and transform');

	const pipeline = VPL.fromContainer({ filename: 'berlin.mbtiles' })
		.filter({ levelMin: 6, levelMax: 10, bbox: [13.3, 52.4, 13.5, 52.6] })
		.vectorFilterLayers({ filter: 'ocean' });

	const source = await pipeline.fromPath(testdataDir);
	const tile = await source.getTile(8, 137, 83);

	log.info('Pipeline', pipeline.toString());
	log.info('Tile 8/137/83', tile ? `${tile.length} bytes` : 'not found');
}

{
	// Example 2: Stack multiple sources (first available tile wins)
	log.section('Example 2: Stack sources with fallback');

	const primary = VPL.fromContainer({ filename: 'berlin.mbtiles' }).filter({ levelMax: 8 });
	const fallback = VPL.fromDebug({ format: 'mvt' });
	const pipeline = VPL.fromStacked([primary, fallback]);

	const source = await pipeline.fromPath(testdataDir);
	const tile = await source.getTile(10, 550, 335);

	log.info('Pipeline', pipeline.toString());
	log.info('Tile z=10 (fallback)', tile ? `${tile.length} bytes` : 'not found');
}

{
	// Example 3: Merge vector tile layers from different sources
	log.section('Example 3: Merge vector sources');

	const labels = VPL.fromContainer({ filename: 'berlin.mbtiles' }).vectorFilterLayers({
		filter: 'place_labels',
		invert: true,
	});
	const borders = VPL.fromContainer({ filename: 'berlin.mbtiles' }).vectorFilterLayers({
		filter: 'boundaries',
		invert: true,
	});
	const pipeline = VPL.fromMergedVector([labels, borders]);

	const source = await pipeline.fromPath(testdataDir);
	const tile = await source.getTile(5, 17, 10);

	log.info('Pipeline', pipeline.toString());
	log.info('Merged tile', tile ? `${tile.length} bytes` : 'not found');
}

log.success('All VPL examples completed!');
