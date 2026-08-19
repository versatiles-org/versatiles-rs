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
		const newReader = await TileSource.fromPath(outputPath);
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
		const newReader = await TileSource.fromPath(outputPath);
		expect(newReader.metadata()).toStrictEqual({
			maxZoom: 14,
			minZoom: 0,
			tileCompression: 'gzip',
			tileFormat: 'mvt',
		});

		// Clean up
		fs.unlinkSync(outputPath);
	});

	it('should reject a writer option the output format does not accept', async () => {
		// A writer option that quietly does nothing is the failure this
		// mechanism exists to prevent, so an unknown key is an error.
		const outputPath = getTempOutputPath();
		await expect(convert(MBTILES_PATH, outputPath, { writerOptions: { allowUnclusterd: 'true' } })).rejects.toThrow(
			/unknown writer option/,
		);
		expect(fs.existsSync(outputPath)).toBeFalsy();
	});

	it('should name the key and what the format accepts', async () => {
		const outputPath = getTempOutputPath();
		await expect(convert(MBTILES_PATH, outputPath, { writerOptions: { nope: '1' } })).rejects.toThrow(
			/nope.*it accepts none/s,
		);
	});

	it('should convert normally when no writer options are given', async () => {
		const outputPath = getTempOutputPath();
		await convert(MBTILES_PATH, outputPath, { maxZoom: 3, writerOptions: {} });
		expect(fs.existsSync(outputPath)).toBeTruthy();
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

		const newReader = await TileSource.fromPath(outputPath);
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
