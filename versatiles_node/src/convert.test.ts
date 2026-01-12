import { TileSource, convert } from '../index.js';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';
import { tmpdir } from 'os';
import { randomUUID } from 'crypto';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const PMTILES_PATH = path.join(TESTDATA_DIR, 'berlin.pmtiles');

function getTempOutputPath(): string {
	return path.join(tmpdir(), `output-test-${randomUUID()}.versatiles`);
}

describe('convertTo()', () => {
	it('should convert from MBTiles to versatiles format', async () => {
		const outputPath = getTempOutputPath();
		await convert(MBTILES_PATH, outputPath);
		expect(fs.existsSync(outputPath)).toBeTruthy();

		// Verify we can open the converted file
		const newReader = await TileSource.open(outputPath);
		expect(newReader.metadata()).toStrictEqual({
			maxZoom: 14,
			minZoom: 0,
			tileCompression: 'gzip',
			tileFormat: 'mvt',
		});

		// Clean up
		fs.unlinkSync(outputPath);
	});

	it('should convert from PMTiles to versatiles format', async () => {
		const outputPath = getTempOutputPath();
		await convert(PMTILES_PATH, outputPath);
		expect(fs.existsSync(outputPath)).toBeTruthy();

		// Verify we can open the converted file
		const newReader = await TileSource.open(outputPath);
		expect(newReader.metadata()).toStrictEqual({
			maxZoom: 14,
			minZoom: 0,
			tileCompression: 'gzip',
			tileFormat: 'mvt',
		});

		// Clean up
		fs.unlinkSync(outputPath);
	});

	it('should convert with options', async () => {
		const outputPath = getTempOutputPath();
		await convert(MBTILES_PATH, outputPath, {
			minZoom: 5,
			maxZoom: 7,
			compress: 'gzip',
		});
		expect(fs.existsSync(outputPath)).toBeTruthy();

		const newReader = await TileSource.open(outputPath);
		expect(newReader.metadata()).toStrictEqual({
			maxZoom: 7,
			minZoom: 5,
			tileCompression: 'gzip',
			tileFormat: 'mvt',
		});

		// Clean up
		fs.unlinkSync(outputPath);
	});
});
