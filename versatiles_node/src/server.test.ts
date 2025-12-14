import { beforeAll, afterAll } from 'vitest';
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
		http
			.get(url, (res) => {
				let data = '';
				res.on('data', (chunk) => (data += chunk));
				res.on('end', () => {
					if (res.statusCode && res.statusCode >= 400) {
						reject(new Error(`HTTP ${res.statusCode}: ${data}`));
					} else {
						resolve({ statusCode: res.statusCode!, data, headers: res.headers });
					}
				});
			})
			.on('error', reject);
	});
}

function httpGetBuffer(url: string): Promise<HttpBufferResponse> {
	return new Promise((resolve, reject) => {
		http
			.get(url, (res) => {
				const chunks: Buffer[] = [];
				res.on('data', (chunk) => chunks.push(chunk));
				res.on('end', () => {
					if (res.statusCode && res.statusCode >= 400) {
						reject(new Error(`HTTP ${res.statusCode}`));
					} else {
						resolve({ statusCode: res.statusCode!, data: Buffer.concat(chunks), headers: res.headers });
					}
				});
			})
			.on('error', reject);
	});
}

describe('TileServer', () => {
	describe('constructor', () => {
		test('should create server with default options', () => {
			const server: TileServer = new TileServer();
			expect(server).toBeDefined();
		});

		test('should create server with custom port', () => {
			const server: TileServer = new TileServer({ port: 8080 });
			expect(server).toBeDefined();
		});

		test('should create server with custom IP', () => {
			const server: TileServer = new TileServer({ ip: '127.0.0.1', port: 0 });
			expect(server).toBeDefined();
		});
	});

	describe('lifecycle', () => {
		let server: TileServer;

		beforeAll(() => {
			server = new TileServer({ port: 0 }); // Port 0 = random available port
		});

		afterAll(async () => {
			if (server) {
				await server.stop();
			}
		});

		test('should start server', async () => {
			await server.start();
			await server.addTileSource('berlin', MBTILES_PATH);
			const port = await server.port;
			expect(port).toBeGreaterThan(0);
		});

		test('should stop server', async () => {
			await server.stop();
			// Port getter should still work after stop
			const port = await server.port;
			expect(typeof port).toBe('number');
		});

		test('should restart server', async () => {
			await server.start();
			const port1 = await server.port;
			await server.stop();
			await server.start();
			const port2 = await server.port;
			// Ports might be different, just verify both are valid
			expect(port1).toBeGreaterThan(0);
			expect(port2).toBeGreaterThan(0);
		});
	});

	describe('addTileSource()', () => {
		let server: TileServer;

		beforeAll(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
		});

		afterAll(async () => {
			await server.stop();
		});

		test('should add MBTiles source', async () => {
			await server.addTileSource('berlin', MBTILES_PATH);
			const port = await server.port;
			expect(port).toBeGreaterThan(0);
		});

		test('should add PMTiles source', async () => {
			await server.addTileSource('berlin-pm', PMTILES_PATH);
			expect(true).toBeTruthy();
		});

		test('should add multiple sources', async () => {
			const server2: TileServer = new TileServer({ port: 0 });
			await server2.start();
			await server2.addTileSource('source1', MBTILES_PATH);
			await server2.addTileSource('source2', PMTILES_PATH);
			const port = await server2.port;
			expect(port).toBeGreaterThan(0);
			await server2.stop();
		});

		test('should throw error for non-existent file', async () => {
			await expect(server.addTileSource('invalid', '/nonexistent/file.mbtiles')).rejects.toThrow();
		});
	});

	describe('HTTP tile serving', () => {
		let server: TileServer;
		let baseUrl: string;

		beforeAll(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
			await server.addTileSource('berlin', MBTILES_PATH);

			// Restart server to apply sources
			await server.stop();
			await server.start();

			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;
		});

		afterAll(async () => {
			await server.stop();
		});

		test('should serve TileJSON', async () => {
			const { statusCode, data } = await httpGet(`${baseUrl}/tiles/berlin/tiles.json`);
			expect(statusCode).toBe(200);

			const tileJson = JSON.parse(data);
			expect(tileJson.tilejson).toBe('3.0.0');
			expect(Array.isArray(tileJson.tiles)).toBeTruthy();
		});

		test('should serve tiles', async () => {
			// Berlin tile at z=5, x=17, y=10
			const { statusCode, data, headers } = await httpGetBuffer(`${baseUrl}/tiles/berlin/5/17/10`);
			expect(statusCode).toBe(200);
			expect(data.length).toBeGreaterThan(0);
			expect(headers['content-type']).toBeDefined();
		});

		test('should return 404 for non-existent tile', async () => {
			// Request a tile far outside Berlin's bounds (Berlin is in Europe, this is in the Pacific)
			await expect(httpGet(`${baseUrl}/tiles/berlin/10/0/0`)).rejects.toThrow(/HTTP 404/);
		});

		test('should return 404 for non-existent source', async () => {
			await expect(httpGet(`${baseUrl}/tiles/nonexistent/5/17/10`)).rejects.toThrow(/HTTP 404/);
		});

		test('should serve multiple tile requests concurrently', async () => {
			const requests = [
				httpGetBuffer(`${baseUrl}/tiles/berlin/5/17/10`),
				httpGetBuffer(`${baseUrl}/tiles/berlin/6/34/20`),
				httpGetBuffer(`${baseUrl}/tiles/berlin/7/68/40`),
			];

			const results = await Promise.allSettled(requests);
			const successful = results.filter((r) => r.status === 'fulfilled');
			expect(successful.length).toBeGreaterThan(0);
		});
	});

	describe('addStaticSource()', () => {
		let server: TileServer;
		let baseUrl: string;

		beforeAll(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
			await server.addStaticSource(TESTDATA_DIR);

			// Restart server to apply sources
			await server.stop();
			await server.start();

			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;
		});

		afterAll(async () => {
			await server.stop();
		});

		test('should serve static files', async () => {
			const { statusCode, data } = await httpGet(`${baseUrl}/cities.csv`);
			expect(statusCode).toBe(200);
			expect(data.length).toBeGreaterThan(0);
		});

		test('should return 404 for non-existent static file', async () => {
			await expect(httpGet(`${baseUrl}/nonexistent.txt`)).rejects.toThrow(/HTTP 404/);
		});
	});

	describe('combined sources', () => {
		let server: TileServer;
		let baseUrl: string;

		beforeAll(async () => {
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

		afterAll(async () => {
			await server.stop();
		});

		test('should serve from multiple tile sources', async () => {
			const mb = await httpGetBuffer(`${baseUrl}/tiles/berlin-mb/5/17/10`);
			const pm = await httpGetBuffer(`${baseUrl}/tiles/berlin-pm/5/17/10`);

			expect(mb.statusCode).toBe(200);
			expect(pm.statusCode).toBe(200);
		});

		test('should serve static files with prefix', async () => {
			const { statusCode } = await httpGet(`${baseUrl}/static/cities.csv`);
			expect(statusCode).toBe(200);
		});
	});

	describe('port getter', () => {
		test('should return 0 before server starts', async () => {
			const server: TileServer = new TileServer({ port: 0 });
			const port = await server.port;
			expect(port).toBe(0);
		});

		test('should return actual port after server starts', async () => {
			const server: TileServer = new TileServer({ port: 0 });
			await server.start();
			await server.addTileSource('berlin', MBTILES_PATH);

			const port = await server.port;
			expect(port).toBeGreaterThan(0);
			expect(port).toBeLessThan(65536);

			await server.stop();
		});
	});

	describe('hot reload', () => {
		let server: TileServer;
		let baseUrl: string;

		beforeAll(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;
		});

		afterAll(async () => {
			await server.stop();
		});

		test('should hot-reload tile source addition without restart', async () => {
			// Add source to running server
			await server.addTileSource('berlin', MBTILES_PATH);

			// Verify source is immediately available without restart
			const { statusCode, data } = await httpGet(`${baseUrl}/tiles/berlin/tiles.json`);
			expect(statusCode).toBe(200);

			const tileJson = JSON.parse(data);
			expect(tileJson.tilejson).toBe('3.0.0');
		});

		test('should serve tiles from hot-reloaded source', async () => {
			// Tile should be immediately available after hot reload
			const { statusCode, data } = await httpGetBuffer(`${baseUrl}/tiles/berlin/5/17/10`);
			expect(statusCode).toBe(200);
			expect(data.length).toBeGreaterThan(0);
		});

		test('should hot-reload multiple sources without restart', async () => {
			// Add second source
			await server.addTileSource('berlin-pm', PMTILES_PATH);

			// Both sources should be available
			const mb = await httpGetBuffer(`${baseUrl}/tiles/berlin/5/17/10`);
			const pm = await httpGetBuffer(`${baseUrl}/tiles/berlin-pm/5/17/10`);

			expect(mb.statusCode).toBe(200);
			expect(pm.statusCode).toBe(200);
		});

		test('should hot-reload tile source removal without restart', async () => {
			// Remove the first source
			const removed = await server.removeTileSource('berlin');
			expect(removed).toBe(true);

			// Source should be immediately unavailable
			await expect(httpGet(`${baseUrl}/tiles/berlin/tiles.json`)).rejects.toThrow(/HTTP 404/);

			// Other source should still work
			const { statusCode } = await httpGetBuffer(`${baseUrl}/tiles/berlin-pm/5/17/10`);
			expect(statusCode).toBe(200);
		});

		test('should return false when removing non-existent source', async () => {
			const removed = await server.removeTileSource('nonexistent');
			expect(removed).toBe(false);
		});

		test('should preserve hot-reloaded sources after restart', async () => {
			// Add a new source
			await server.addTileSource('test-source', MBTILES_PATH);

			// Verify it works
			let response = await httpGet(`${baseUrl}/tiles/test-source/tiles.json`);
			expect(response.statusCode).toBe(200);

			// Restart server
			await server.stop();
			await server.start();

			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;

			// Source should still be available after restart
			response = await httpGet(`${baseUrl}/tiles/test-source/tiles.json`);
			expect(response.statusCode).toBe(200);

			// And berlin should NOT be available (was removed earlier)
			await expect(httpGet(`${baseUrl}/tiles/berlin/tiles.json`)).rejects.toThrow(/HTTP 404/);
		});
	});

	describe('hot reload - static sources', () => {
		let server: TileServer;
		let baseUrl: string;

		beforeAll(async () => {
			server = new TileServer({ port: 0 });
			await server.start();
			const port = await server.port;
			baseUrl = `http://127.0.0.1:${port}`;
		});

		afterAll(async () => {
			await server.stop();
		});

		test('should hot-reload static source addition without restart', async () => {
			const initialPort = await server.port;

			// Add static source while running
			await server.addStaticSource(TESTDATA_DIR, '/files');

			// Verify server still running (no restart) by checking port
			const currentPort = await server.port;
			expect(currentPort).toBe(initialPort);

			// Verify files are immediately accessible
			const { statusCode, data } = await httpGet(`${baseUrl}/files/cities.csv`);
			expect(statusCode).toBe(200);
			expect(data.length).toBeGreaterThan(0);
		});

		test('should serve files from hot-reloaded static source', async () => {
			// File should be immediately available after hot reload
			const { statusCode, data } = await httpGet(`${baseUrl}/files/cities.csv`);
			expect(statusCode).toBe(200);
			expect(data.length).toBeGreaterThan(0);
		});

		test('should hot-reload static source removal without restart', async () => {
			const initialPort = await server.port;

			// Remove while running
			const removed = await server.removeStaticSource('/files');
			expect(removed).toBe(true);

			// Verify server still running (no restart)
			const currentPort = await server.port;
			expect(currentPort).toBe(initialPort);

			// Source should be immediately unavailable
			await expect(httpGet(`${baseUrl}/files/cities.csv`)).rejects.toThrow(/HTTP 404/);
		});

		test('should return false when removing non-existent static source', async () => {
			const removed = await server.removeStaticSource('/nonexistent');
			expect(removed).toBe(false);
		});

		test('should hot-reload multiple static sources without restart', async () => {
			// Add two static sources with different prefixes
			await server.addStaticSource(TESTDATA_DIR, '/static1');
			await server.addStaticSource(TESTDATA_DIR, '/static2');

			// Both should be accessible
			const response1 = await httpGet(`${baseUrl}/static1/cities.csv`);
			const response2 = await httpGet(`${baseUrl}/static2/cities.csv`);

			expect(response1.statusCode).toBe(200);
			expect(response2.statusCode).toBe(200);
		});
	});
});
