import { ContainerReader, convert } from '../index.js';
import path from 'path';
import fs from 'fs';
import { fileURLToPath } from 'url';
import { tmpdir } from 'os';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const PMTILES_PATH = path.join(TESTDATA_DIR, 'berlin.pmtiles');

describe('convertTo()', () => {
	const OUTPUT_PATH = path.join(tmpdir(), 'output-test.versatiles');

	it('should convert from MBTiles to versatiles format', async () => {
		await convert(MBTILES_PATH, OUTPUT_PATH);
		expect(fs.existsSync(OUTPUT_PATH)).toBeTruthy();

		// Verify we can open the converted file
		const newReader = await ContainerReader.open(OUTPUT_PATH);
		expect(await newReader.parameters()).toStrictEqual({
			maxZoom: 14,
			minZoom: 0,
			tileCompression: 'gzip',
			tileFormat: 'mvt',
		});

		// Clean up
		fs.unlinkSync(OUTPUT_PATH);
	});

	it('should convert from PMTiles to versatiles format', async () => {
		await convert(PMTILES_PATH, OUTPUT_PATH);
		expect(fs.existsSync(OUTPUT_PATH)).toBeTruthy();

		// Verify we can open the converted file
		const newReader = await ContainerReader.open(OUTPUT_PATH);
		expect(await newReader.parameters()).toStrictEqual({
			maxZoom: 14,
			minZoom: 0,
			tileCompression: 'gzip',
			tileFormat: 'mvt',
		});

		// Clean up
		fs.unlinkSync(OUTPUT_PATH);
	});

	it('should convert with options', async () => {
		await convert(MBTILES_PATH, OUTPUT_PATH, {
			minZoom: 5,
			maxZoom: 7,
			compress: 'gzip',
		});
		expect(fs.existsSync(OUTPUT_PATH)).toBeTruthy();

		const newReader = await ContainerReader.open(OUTPUT_PATH);
		expect(await newReader.parameters()).toStrictEqual({
			maxZoom: 7,
			minZoom: 5,
			tileCompression: 'gzip',
			tileFormat: 'mvt',
		});

		// Clean up
		fs.unlinkSync(OUTPUT_PATH);
	});
});
