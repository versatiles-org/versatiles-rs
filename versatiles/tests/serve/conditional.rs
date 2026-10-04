//! E2E tests for revalidation: `ETag` on responses, `304` on a match (#279).

use std::process::Child;

use reqwest::{
	StatusCode,
	header::{ACCEPT_ENCODING, ETAG, HeaderValue, IF_NONE_MATCH},
};

use crate::test_utilities::*;

struct ConditionalTestServer {
	host: String,
	child: Child,
}

impl ConditionalTestServer {
	async fn new(args: &[&str]) -> Self {
		let (host, child) = spawn_server(args, "/").await;
		Self { host, child }
	}

	/// Status, `ETag` and body length for `path`, sending the given headers.
	async fn get(&self, path: &str, headers: &[(&str, &str)]) -> (StatusCode, Option<HeaderValue>, usize) {
		// No automatic decompression, so `Accept-Encoding` is what the test says.
		let client = reqwest::Client::builder()
			.no_gzip()
			.no_brotli()
			.no_deflate()
			.no_zstd()
			.build()
			.unwrap();
		let mut req = client.get(format!("{}{path}", self.host));
		for (name, value) in headers {
			req = req.header(*name, *value);
		}
		let resp = req.send().await.unwrap();
		let status = resp.status();
		let etag = resp.headers().get(ETAG).cloned();
		let len = resp.bytes().await.unwrap().len();
		(status, etag, len)
	}

	/// Fetch `path`, then revalidate it with the `ETag` it came with.
	async fn assert_revalidates(&self, path: &str) {
		let (status, etag, len) = self.get(path, &[]).await;
		assert_eq!(status, StatusCode::OK, "{path}");
		assert!(len > 0, "{path}");
		let etag = etag.unwrap_or_else(|| panic!("{path} has no ETag"));
		let etag = etag.to_str().unwrap();

		let (status, again, len) = self.get(path, &[(IF_NONE_MATCH.as_str(), etag)]).await;
		assert_eq!(status, StatusCode::NOT_MODIFIED, "{path}");
		assert_eq!(again.as_ref().map(|v| v.to_str().unwrap()), Some(etag), "{path}");
		assert_eq!(len, 0, "{path}: a 304 has no body");

		// The tag is weak, so it holds whatever encoding the client asks for.
		let (status, _, _) = self
			.get(
				path,
				&[(IF_NONE_MATCH.as_str(), etag), (ACCEPT_ENCODING.as_str(), "br")],
			)
			.await;
		assert_eq!(status, StatusCode::NOT_MODIFIED, "{path} with br");

		let (status, _, len) = self.get(path, &[(IF_NONE_MATCH.as_str(), "W/\"stale\"")]).await;
		assert_eq!(status, StatusCode::OK, "{path} with a stale tag");
		assert!(len > 0, "{path} with a stale tag");
	}
}

impl Drop for ConditionalTestServer {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

#[tokio::test]
async fn e2e_tiles_and_static_files_revalidate() {
	let tiles = get_testdata("berlin.mbtiles");
	let static_files = get_testdata("static.tar.br");
	let server = ConditionalTestServer::new(&[&tiles, "-s", &static_files]).await;

	server.assert_revalidates("/tiles/berlin/14/8800/5374").await;
	server.assert_revalidates("/index.html").await;
}
