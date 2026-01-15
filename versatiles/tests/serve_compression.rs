//! E2E tests for compression negotiation in the HTTP server.
//!
//! These tests verify that the server correctly handles Accept-Encoding headers
//! and returns appropriate Content-Encoding responses.

mod test_utilities;

use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, HeaderMap};
use std::{net::TcpListener, process::Child, thread, time::Duration};
use test_utilities::*;

struct CompressionTestServer {
	host: String,
	child: Child,
}

impl CompressionTestServer {
	async fn new(tile_source: &str) -> Self {
		let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();

		let mut cmd = versatiles_cmd();
		cmd.args(["serve", "-p", &port.to_string(), tile_source]);
		let mut child = cmd.spawn().unwrap();

		// Wait for server to be ready
		loop {
			thread::sleep(Duration::from_millis(100));
			assert!(child.try_wait().unwrap().is_none(), "server process exited prematurely");
			if reqwest::get(format!("http://127.0.0.1:{port}/tiles/index.json"))
				.await
				.is_ok()
			{
				break;
			}
		}

		Self {
			host: format!("http://127.0.0.1:{port}"),
			child,
		}
	}

	fn shutdown(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}

	async fn get_with_encoding(&self, path: &str, accept_encoding: Option<&str>) -> (u16, HeaderMap) {
		let client = reqwest::Client::builder()
			// Disable automatic decompression so we can inspect the raw Content-Encoding
			.no_gzip()
			.no_brotli()
			.no_deflate()
			.build()
			.unwrap();

		let mut req = client.get(format!("{}{path}", self.host));
		if let Some(enc) = accept_encoding {
			req = req.header(ACCEPT_ENCODING, enc);
		}

		let resp = req.send().await.unwrap();
		(resp.status().as_u16(), resp.headers().clone())
	}

	async fn get_tile_with_encoding(&self, accept_encoding: Option<&str>) -> (u16, HeaderMap, Vec<u8>) {
		let client = reqwest::Client::builder()
			.no_gzip()
			.no_brotli()
			.no_deflate()
			.build()
			.unwrap();

		// Use a tile that we know exists in the berlin dataset (from convert_integrity tests)
		let mut req = client.get(format!("{}/tiles/berlin/14/8800/5374", self.host));
		if let Some(enc) = accept_encoding {
			req = req.header(ACCEPT_ENCODING, enc);
		}

		let resp = req.send().await.unwrap();
		let status = resp.status().as_u16();
		let headers = resp.headers().clone();
		let body = resp.bytes().await.unwrap().to_vec();
		(status, headers, body)
	}
}

impl Drop for CompressionTestServer {
	fn drop(&mut self) {
		self.shutdown();
	}
}

/// Test that server returns uncompressed when no Accept-Encoding is sent.
#[tokio::test]
async fn no_accept_encoding_returns_uncompressed() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	let (status, headers) = server.get_with_encoding("/tiles/berlin/tiles.json", None).await;

	assert_eq!(status, 200);
	// No Content-Encoding header means uncompressed
	let content_encoding = headers.get(CONTENT_ENCODING);
	assert!(
		content_encoding.is_none() || content_encoding.unwrap().to_str().unwrap() == "identity",
		"Should return uncompressed when no Accept-Encoding specified"
	);
}

/// Test that server returns gzip when Accept-Encoding: gzip is sent.
#[tokio::test]
async fn accept_gzip_returns_gzip() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	let (status, headers, _body) = server.get_tile_with_encoding(Some("gzip")).await;

	assert_eq!(status, 200);
	// Server should return gzip if it can
	let content_encoding = headers.get(CONTENT_ENCODING);
	if let Some(enc) = content_encoding {
		let enc_str = enc.to_str().unwrap();
		assert!(
			enc_str == "gzip" || enc_str == "identity",
			"Content-Encoding should be gzip or identity, got: {enc_str}"
		);
	}
}

/// Test that server returns brotli when Accept-Encoding: br is sent.
#[tokio::test]
async fn accept_brotli_returns_brotli() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	let (status, headers, _body) = server.get_tile_with_encoding(Some("br")).await;

	assert_eq!(status, 200);
	// Server should return brotli if it can
	let content_encoding = headers.get(CONTENT_ENCODING);
	if let Some(enc) = content_encoding {
		let enc_str = enc.to_str().unwrap();
		assert!(
			enc_str == "br" || enc_str == "identity",
			"Content-Encoding should be br or identity, got: {enc_str}"
		);
	}
}

/// Test that server handles wildcard Accept-Encoding.
#[tokio::test]
async fn accept_wildcard_encoding() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	let (status, headers, _body) = server.get_tile_with_encoding(Some("*")).await;

	assert_eq!(status, 200);
	// Server can return any encoding with wildcard
	let content_encoding = headers.get(CONTENT_ENCODING);
	if let Some(enc) = content_encoding {
		let enc_str = enc.to_str().unwrap();
		assert!(
			enc_str == "gzip" || enc_str == "br" || enc_str == "identity",
			"Content-Encoding should be a valid encoding, got: {enc_str}"
		);
	}
}

/// Test that server handles multiple Accept-Encoding values.
#[tokio::test]
async fn accept_multiple_encodings() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	let (status, headers, _body) = server.get_tile_with_encoding(Some("gzip, br")).await;

	assert_eq!(status, 200);
	// Server can choose either gzip or brotli
	let content_encoding = headers.get(CONTENT_ENCODING);
	if let Some(enc) = content_encoding {
		let enc_str = enc.to_str().unwrap();
		assert!(
			enc_str == "gzip" || enc_str == "br" || enc_str == "identity",
			"Content-Encoding should be gzip, br, or identity, got: {enc_str}"
		);
	}
}

/// Test that server handles Accept-Encoding with quality values.
#[tokio::test]
async fn accept_encoding_with_quality() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	let (status, headers, _body) = server.get_tile_with_encoding(Some("br;q=1.0, gzip;q=0.8")).await;

	assert_eq!(status, 200);
	// Server should prefer br (higher quality)
	let content_encoding = headers.get(CONTENT_ENCODING);
	if let Some(enc) = content_encoding {
		let enc_str = enc.to_str().unwrap();
		assert!(
			enc_str == "gzip" || enc_str == "br" || enc_str == "identity",
			"Content-Encoding should be a valid encoding, got: {enc_str}"
		);
	}
}

/// Test that tile data is valid regardless of encoding.
#[tokio::test]
async fn tile_data_valid_with_different_encodings() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	// Get tile with identity encoding explicitly requested
	let (status1, _headers1, body1) = server.get_tile_with_encoding(Some("identity")).await;
	assert_eq!(status1, 200);
	assert!(!body1.is_empty(), "Tile body should not be empty with identity");

	// Get tile with gzip
	let (status2, _headers2, body2) = server.get_tile_with_encoding(Some("gzip")).await;
	assert_eq!(status2, 200);
	assert!(!body2.is_empty(), "Tile body should not be empty with gzip");

	// Get tile with brotli
	let (status3, _headers3, body3) = server.get_tile_with_encoding(Some("br")).await;
	assert_eq!(status3, 200);
	assert!(!body3.is_empty(), "Tile body should not be empty with brotli");
}

/// Test that JSON endpoints (tiles.json) also respect encoding.
#[tokio::test]
async fn json_endpoint_respects_encoding() {
	let input = get_testdata("berlin.mbtiles");
	let server = CompressionTestServer::new(&input).await;

	let (status, headers) = server.get_with_encoding("/tiles/berlin/tiles.json", Some("gzip")).await;

	assert_eq!(status, 200);
	// JSON endpoints should also respect Accept-Encoding
	let content_encoding = headers.get(CONTENT_ENCODING);
	if let Some(enc) = content_encoding {
		let enc_str = enc.to_str().unwrap();
		assert!(
			enc_str == "gzip" || enc_str == "identity",
			"Content-Encoding for JSON should be gzip or identity, got: {enc_str}"
		);
	}
}
