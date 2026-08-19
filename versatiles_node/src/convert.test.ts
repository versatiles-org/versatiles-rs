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

function getTempPMTilesPath(): string {
	return path.join(tmpdir(), `output-test-${randomUUID()}.pmtiles`);
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

	it('should accept a writer option the output format declares', async () => {
		// The rejections above only prove a key can be refused. This is the other
		// half: a key PMTiles declares reaches the writer and the write completes.
		const outputPath = getTempPMTilesPath();
		await convert(MBTILES_PATH, outputPath, { maxZoom: 3, writerOptions: { allow_unclustered: 'true' } });
		expect(fs.existsSync(outputPath)).toBeTruthy();

		const newReader = await TileSource.fromPath(outputPath);
		expect(newReader.metadata().tileFormat).toBe('mvt');

		fs.unlinkSync(outputPath);
	});

	it('should carry more than one writer option through at once', async () => {
		// Two keys, one of them taking a path rather than a boolean.
		const outputPath = getTempPMTilesPath();
		const tempDir = fs.mkdtempSync(path.join(tmpdir(), 'writer-option-temp-'));
		await convert(MBTILES_PATH, outputPath, {
			maxZoom: 3,
			writerOptions: { reorder: 'true', temp_dir: tempDir },
		});
		expect(fs.existsSync(outputPath)).toBeTruthy();
		// Whether or not the extra pass ran, it leaves nothing behind.
		expect(fs.readdirSync(tempDir)).toStrictEqual([]);

		fs.unlinkSync(outputPath);
		fs.rmdirSync(tempDir);
	});

	it('should reject two writer options that ask for different outcomes', async () => {
		const outputPath = getTempPMTilesPath();
		await expect(
			convert(MBTILES_PATH, outputPath, { writerOptions: { allow_unclustered: 'true', reorder: 'true' } }),
		).rejects.toThrow(/Pick one/);
	});

	it('should reject a writer option value it cannot read as a boolean', async () => {
		// A value that is not a recognised true/false spelling is an error, not a
		// quiet false — same rule as an unknown key, applied to the value.
		const outputPath = getTempPMTilesPath();
		await expect(convert(MBTILES_PATH, outputPath, { writerOptions: { allow_unclustered: 'maybe' } })).rejects.toThrow(
			/expects true or false, got 'maybe'/,
		);
	});

	it('should accept an SSH identity that a local conversion never uses', async () => {
		// sshIdentity is read only for sftp:// sources; a local conversion must
		// behave exactly as if it were not set.
		const outputPath = getTempOutputPath();
		await convert(MBTILES_PATH, outputPath, { maxZoom: 3, sshIdentity: '/nonexistent/id_ed25519' });
		expect(fs.existsSync(outputPath)).toBeTruthy();
		fs.unlinkSync(outputPath);
	});

	it('should treat an sftp:// destination as a remote upload', async () => {
		// Without this, PathBuf::from() turned the URL into a local directory that
		// does not exist, and the error never mentioned SFTP at all. No host here,
		// so it fails before any connection is attempted.
		await expect(convert(MBTILES_PATH, 'sftp:///out.pmtiles', { maxZoom: 1 })).rejects.toThrow(/writing tiles to SFTP/);
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
