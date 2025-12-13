import { describe, test, before, after } from 'node:test';
import assert from 'node:assert';
import { TileServer } from '../index.js';
import path from 'path';
import http from 'http';

const TESTDATA_DIR = path.join(__dirname, '../../testdata');
const MBTILES_PATH = path.join(TESTDATA_DIR, 'berlin.mbtiles');
const PMTILES_PATH = path.join(TESTDATA_DIR, 'berlin.pmtiles');

interface HttpResponse {
	statusCode: number;
	data: string;
	headers: http.IncomingHttpHeaders;
}

interface HttpBufferResponse {
	statusCode: number;
	data: Buffer;
	headers: http.IncomingHttpHeaders;
}

function httpGet(url: string): Promise<HttpResponse> {
	return new Promise((resolve, reject) => {
		http.get(url, (res) => {
			let data = '';
			res.on('data', chunk => data += chunk);
			res.on('end', () => {
				if (res.statusCode && res.statusCode >= 400) {
					reject(new Error(`HTTP ${res.statusCode}: ${data}`));
				} else {
					resolve({ statusCode: res.statusCode!, data, headers: res.headers });
				}
			});
		}).on('error', reject);
	});
}

function httpGetBuffer(url: string): Promise<HttpBufferResponse> {
	return new Promise((resolve, reject) => {
		http.get(url, (res) => {
			const chunks: Buffer[] = [];
			res.on('data', chunk => chunks.push(chunk));
			res.on('end', () => {
				if (res.statusCode && res.statusCode >= 400) {
					reject(new Error(`HTTP ${res.statusCode}`));
				} else {
					resolve({ statusCode: res.statusCode!, data: Buffer.concat(chunks), headers: res.headers });
				}
			});
		}).on('error', reject);
	});
}

describe('TileServer', () => {
	describe('constructor', () => {
		test('should create server with default options', () => {
			const server: TileServer = new TileServer();
			assert.ok(server, 'Server should be created');
		});

		test('should create server with custom port', () => {
			const server: TileServer = new TileServer({ port: 8080 });
			assert.ok(server, 'Server should be created');
		});

		test('should create server with custom IP', () => {
			const server: TileServer = new TileServer({ ip: '127.0.0.1', port: 0 });
			assert.ok(server, 'Server should be created');
		});
	});

	describe('lifecycle', () => {
		let server: TileServer;

		before(() => {
			server = new TileServer({ port: 0 }); // Port 0 = random available port
		});

		after(async () => {
			if (server) {
				await server.stop();
			}
		});

		test('should start server', async () => {
			await server.start();
			await server.addTileSource('berlin', MBTILES_PATH);
			const port = await server.port;
			assert.ok(port > 0, 'Server should have a valid port');
		});

		test('should stop server', async () => {
			await server.stop();
			// Port getter should still work after stop
			const port = await server.port;
			assert.ok(typeof port === 'number', 'Port should be a number');
		});

		test('should restart server', async () => {
			await server.start();
			const port1 = await server.port;
			await server.stop();
			await server.start();
			const port2 = await server.port;
			// Ports might be different, just verify both are valid
			assert.ok(port1 > 0, 'First port should be valid');
			assert.ok(port2 > 0, 'Second port should be valid');
		});
	});

	describe('addTileSource()', () => {
		let server: TileServer;

		before(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
		});

		after(async () => {
			await server.stop();
		});

		test('should add MBTiles source', async () => {
			await server.addTileSource('berlin', MBTILES_PATH);
			const port = await server.port;
			assert.ok(port > 0, 'Server should be running');
		});

		test('should add PMTiles source', async () => {
			await server.addTileSource('berlin-pm', PMTILES_PATH);
			assert.ok(true, 'PMTiles source should be added');
		});

		test('should add multiple sources', async () => {
			const server2: TileServer = new TileServer({ port: 0 });
			await server2.start();
			await server2.addTileSource('source1', MBTILES_PATH);
			await server2.addTileSource('source2', PMTILES_PATH);
			const port = await server2.port;
			assert.ok(port > 0, 'Server should be running with multiple sources');
			await server2.stop();
		});

		test('should throw error for non-existent file', async () => {
			await assert.rejects(
				async () => await server.addTileSource('invalid', '/nonexistent/file.mbtiles'),
				'Should throw error for non-existent file'
			);
		});
	});

	describe('HTTP tile serving', () => {
		let server: TileServer;
		let baseUrl: string;

		before(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
			await server.addTileSource('berlin', MBTILES_PATH);

			// Restart server to apply sources
			await server.stop();
			await server.start();

			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;
		});

		after(async () => {
			await server.stop();
		});

		test('should serve TileJSON', async () => {
			const { statusCode, data } = await httpGet(`${baseUrl}/tiles/berlin/tiles.json`);
			assert.strictEqual(statusCode, 200, 'Should return 200 OK');

			const tileJson = JSON.parse(data);
			assert.strictEqual(tileJson.tilejson, '3.0.0', 'Should have TileJSON version');
			assert.ok(Array.isArray(tileJson.tiles), 'Should have tiles array');
		});

		test('should serve tiles', async () => {
			// Berlin tile at z=5, x=17, y=10
			const { statusCode, data, headers } = await httpGetBuffer(`${baseUrl}/tiles/berlin/5/17/10`);
			assert.strictEqual(statusCode, 200, 'Should return 200 OK');
			assert.ok(data.length > 0, 'Tile should have content');
			assert.ok(headers['content-type'], 'Should have content-type header');
		});

		test('should return 404 for non-existent tile', async () => {
			await assert.rejects(
				async () => await httpGet(`${baseUrl}/tiles/berlin/0/0/0`),
				/HTTP 404/,
				'Should return 404 for non-existent tile'
			);
		});

		test('should return 404 for non-existent source', async () => {
			await assert.rejects(
				async () => await httpGet(`${baseUrl}/tiles/nonexistent/5/17/10`),
				/HTTP 404/,
				'Should return 404 for non-existent source'
			);
		});

		test('should serve multiple tile requests concurrently', async () => {
			const requests = [
				httpGetBuffer(`${baseUrl}/tiles/berlin/5/17/10`),
				httpGetBuffer(`${baseUrl}/tiles/berlin/6/34/20`),
				httpGetBuffer(`${baseUrl}/tiles/berlin/7/68/40`),
			];

			const results = await Promise.allSettled(requests);
			const successful = results.filter(r => r.status === 'fulfilled');
			assert.ok(successful.length > 0, 'At least one request should succeed');
		});
	});

	describe('addStaticSource()', () => {
		let server: TileServer;
		let baseUrl: string;

		before(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
			await server.addStaticSource(TESTDATA_DIR);

			// Restart server to apply sources
			await server.stop();
			await server.start();

			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;
		});

		after(async () => {
			await server.stop();
		});

		test('should serve static files', async () => {
			const { statusCode, data } = await httpGet(`${baseUrl}/cities.csv`);
			assert.strictEqual(statusCode, 200, 'Should return 200 OK');
			assert.ok(data.length > 0, 'File should have content');
		});

		test('should return 404 for non-existent static file', async () => {
			await assert.rejects(
				async () => await httpGet(`${baseUrl}/nonexistent.txt`),
				/HTTP 404/,
				'Should return 404 for non-existent file'
			);
		});
	});

	describe('combined sources', () => {
		let server: TileServer;
		let baseUrl: string;

		before(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
			await server.addTileSource('berlin-mb', MBTILES_PATH);
			await server.addTileSource('berlin-pm', PMTILES_PATH);
			await server.addStaticSource(TESTDATA_DIR, '/static');

			// Restart server to apply sources
			await server.stop();
			await server.start();

			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;
		});

		after(async () => {
			await server.stop();
		});

		test('should serve from multiple tile sources', async () => {
			const mb = await httpGetBuffer(`${baseUrl}/tiles/berlin-mb/5/17/10`);
			const pm = await httpGetBuffer(`${baseUrl}/tiles/berlin-pm/5/17/10`);

			assert.strictEqual(mb.statusCode, 200, 'MBTiles source should work');
			assert.strictEqual(pm.statusCode, 200, 'PMTiles source should work');
		});

		test('should serve static files with prefix', async () => {
			const { statusCode } = await httpGet(`${baseUrl}/static/cities.csv`);
			assert.strictEqual(statusCode, 200, 'Static file should be served with prefix');
		});
	});

	describe('port getter', () => {
		test('should return 0 before server starts', async () => {
			const server: TileServer = new TileServer({ port: 0 });
			const port = await server.port;
			assert.strictEqual(port, 0, 'Port should be 0 before start');
		});

		test('should return actual port after server starts', async () => {
			const server: TileServer = new TileServer({ port: 0 });
			await server.start();
			await server.addTileSource('berlin', MBTILES_PATH);

			const port = await server.port;
			assert.ok(port > 0, 'Port should be > 0 after start');
			assert.ok(port < 65536, 'Port should be valid');

			await server.stop();
		});
	});
});
