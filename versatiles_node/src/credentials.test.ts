import { afterAll, beforeAll } from 'vitest';
import { enableHttpNetrc, forgetHttpPassword, setHttpPassword, TileSource } from '../index.js';
import http from 'http';
import type { AddressInfo } from 'net';
import fs from 'fs';
import os from 'os';
import path from 'path';

// The password store and the netrc switch belong to the process. Vitest runs
// each test file in a process of its own, so nothing here reaches other files,
// and each test uses user names of its own.

/** Answers every request with 401 and keeps the Authorization headers it got. */
let server: http.Server;
let origin: string;
const received: (string | undefined)[] = [];

beforeAll(async () => {
	server = http.createServer((req, res) => {
		received.push(req.headers.authorization);
		res.writeHead(401, { 'Content-Length': '0' });
		res.end();
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	origin = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
});

afterAll(() => {
	server.close();
});

/** The Authorization header sent when opening `url`, which the server refuses. */
async function sentAuthorization(url: string): Promise<string | undefined> {
	received.length = 0;
	await expect(TileSource.fromPath(url)).rejects.toThrow(/401/);
	expect(received.length).toBeGreaterThan(0);
	return received[0];
}

function basic(credentials: string): string {
	return `Basic ${Buffer.from(credentials).toString('base64')}`;
}

function withUser(user: string): string {
	return origin.replace('http://', `http://${user}@`) + '/tiles.versatiles';
}

describe('setHttpPassword', () => {
	test('sends the password to the server and user it was registered for', async () => {
		setHttpPassword(`${origin}/some/path/`, 'store-alice', 'from-the-store');
		expect(await sentAuthorization(withUser('store-alice'))).toBe(basic('store-alice:from-the-store'));
		// Another user on the same server does not get it.
		expect(await sentAuthorization(withUser('store-bob'))).toBe(basic('store-bob:'));
	});

	test('a password in the URL wins', async () => {
		setHttpPassword(origin, 'wins-alice', 'from-the-store');
		expect(await sentAuthorization(withUser('wins-alice:from-the-url'))).toBe(basic('wins-alice:from-the-url'));
	});

	test('forgetHttpPassword removes it', async () => {
		setHttpPassword(origin, 'forget-alice', 'secret');
		forgetHttpPassword(origin, 'forget-alice');
		expect(await sentAuthorization(withUser('forget-alice'))).toBe(basic('forget-alice:'));
	});

	test('refuses an origin that is not an http URL', () => {
		expect(() => setHttpPassword('sftp://host/', 'alice', 'x')).toThrow(/'sftp'/);
		expect(() => setHttpPassword('not a url', 'alice', 'x')).toThrow(/not a URL/);
	});

	test('the error for a 401 never shows the password', async () => {
		await expect(TileSource.fromPath(withUser('leak-alice:hunter2'))).rejects.toThrow(
			expect.objectContaining({ message: expect.not.stringContaining('hunter2') }),
		);
	});
});

describe('enableHttpNetrc', () => {
	test('takes the password of the URL’s user from the file NETRC names', async () => {
		const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'versatiles-netrc-'));
		const file = path.join(dir, 'netrc');
		fs.writeFileSync(file, 'machine 127.0.0.1 login netrc-alice password from-netrc\n');
		process.env.NETRC = file;
		try {
			// Before it is enabled, the file is not read, and the error says how.
			await expect(TileSource.fromPath(withUser('netrc-alice'))).rejects.toThrow(/VERSATILES_HTTP_NETRC=on/);

			enableHttpNetrc();
			expect(await sentAuthorization(withUser('netrc-alice'))).toBe(basic('netrc-alice:from-netrc'));
			expect(await sentAuthorization(withUser('netrc-bob'))).toBe(basic('netrc-bob:'));
		} finally {
			delete process.env.NETRC;
			fs.unlinkSync(file);
			fs.rmdirSync(dir);
		}
	});
});
