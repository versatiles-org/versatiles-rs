//! E2E tests for CORS header handling in the HTTP server.
//!
//! These tests verify that the server correctly sets CORS headers based on configuration.

mod test_utilities;

use reqwest::header::{HeaderMap, ORIGIN};
use std::{fs, net::TcpListener, process::Child, thread, time::Duration};
use tempfile::TempDir;
use test_utilities::*;

struct CorsTestServer {
	host: String,
	child: Child,
	#[allow(dead_code)]
	temp_dir: TempDir,
}

impl CorsTestServer {
	async fn new(cors_origins: &[&str], max_age: Option<u64>) -> Self {
		let temp_dir = tempfile::tempdir().unwrap();
		let config_path = temp_dir.path().join("config.yml");
		let tiles_path = get_testdata("berlin.mbtiles");

		// Build CORS config
		let origins_yaml = cors_origins
			.iter()
			.map(|o| format!("    - \"{o}\""))
			.collect::<Vec<_>>()
			.join("\n");

		let max_age_yaml = max_age.map(|s| format!("  max_age_seconds: {s}")).unwrap_or_default();

		let config = format!(
			r#"
tiles:
  - name: test
    src: "{tiles_path}"

cors:
  allowed_origins:
{origins_yaml}
{max_age_yaml}
"#
		);

		fs::write(&config_path, &config).unwrap();

		let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();

		let mut cmd = versatiles_cmd();
		cmd.args(["serve", "-c", config_path.to_str().unwrap(), "-p", &port.to_string()]);
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
			temp_dir,
		}
	}

	fn shutdown(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}

	async fn get_with_origin(&self, path: &str, origin: &str) -> (u16, HeaderMap) {
		let client = reqwest::Client::new();
		let resp = client
			.get(format!("{}{path}", self.host))
			.header(ORIGIN, origin)
			.send()
			.await
			.unwrap();

		(resp.status().as_u16(), resp.headers().clone())
	}

	async fn options_with_origin(&self, path: &str, origin: &str) -> (u16, HeaderMap) {
		let client = reqwest::Client::new();
		let resp = client
			.request(reqwest::Method::OPTIONS, format!("{}{path}", self.host))
			.header(ORIGIN, origin)
			.header("Access-Control-Request-Method", "GET")
			.send()
			.await
			.unwrap();

		(resp.status().as_u16(), resp.headers().clone())
	}
}

impl Drop for CorsTestServer {
	fn drop(&mut self) {
		self.shutdown();
	}
}

/// Test that CORS headers are present for allowed origins.
#[tokio::test]
async fn cors_headers_for_allowed_origin() {
	let server = CorsTestServer::new(&["https://example.org"], Some(86400)).await;

	let (status, headers) = server
		.get_with_origin("/tiles/test/tiles.json", "https://example.org")
		.await;

	assert_eq!(status, 200);

	// Check Access-Control-Allow-Origin header
	let acao = headers
		.get("access-control-allow-origin")
		.expect("Access-Control-Allow-Origin header should be present");
	assert_eq!(acao.to_str().unwrap(), "https://example.org");
}

/// Test that CORS headers are NOT present for disallowed origins.
#[tokio::test]
async fn cors_headers_absent_for_disallowed_origin() {
	let server = CorsTestServer::new(&["https://example.org"], None).await;

	let (status, headers) = server
		.get_with_origin("/tiles/test/tiles.json", "https://evil.com")
		.await;

	assert_eq!(status, 200);

	// Access-Control-Allow-Origin should NOT be present for disallowed origins
	assert!(
		headers.get("access-control-allow-origin").is_none(),
		"Access-Control-Allow-Origin should not be set for disallowed origins"
	);
}

/// Test that wildcard origins work correctly.
#[tokio::test]
async fn cors_wildcard_subdomain() {
	let server = CorsTestServer::new(&["*.example.org"], None).await;

	// Subdomain should be allowed
	let (status, headers) = server
		.get_with_origin("/tiles/test/tiles.json", "https://app.example.org")
		.await;
	assert_eq!(status, 200);
	assert!(
		headers.get("access-control-allow-origin").is_some(),
		"Wildcard should match subdomain"
	);

	// Different domain should not be allowed
	let (_, headers2) = server
		.get_with_origin("/tiles/test/tiles.json", "https://example.com")
		.await;
	assert!(
		headers2.get("access-control-allow-origin").is_none(),
		"Wildcard should not match different domain"
	);
}

/// Test preflight OPTIONS request handling.
#[tokio::test]
async fn cors_preflight_options() {
	let server = CorsTestServer::new(&["https://example.org"], Some(3600)).await;

	let (status, headers) = server
		.options_with_origin("/tiles/test/tiles.json", "https://example.org")
		.await;

	// Preflight should succeed
	assert!(status == 200 || status == 204, "Preflight should succeed");

	// Check CORS headers
	assert!(
		headers.get("access-control-allow-origin").is_some(),
		"Access-Control-Allow-Origin should be present in preflight response"
	);

	// Check max-age if configured
	if let Some(max_age) = headers.get("access-control-max-age") {
		let max_age_val: u64 = max_age.to_str().unwrap().parse().unwrap();
		assert_eq!(max_age_val, 3600, "Max-Age should match configured value");
	}
}

/// Test multiple allowed origins.
#[tokio::test]
async fn cors_multiple_origins() {
	let server = CorsTestServer::new(&["https://first.example.org", "https://second.example.org"], None).await;

	// First origin should be allowed
	let (_, headers1) = server
		.get_with_origin("/tiles/test/tiles.json", "https://first.example.org")
		.await;
	assert!(
		headers1.get("access-control-allow-origin").is_some(),
		"First origin should be allowed"
	);

	// Second origin should be allowed
	let (_, headers2) = server
		.get_with_origin("/tiles/test/tiles.json", "https://second.example.org")
		.await;
	assert!(
		headers2.get("access-control-allow-origin").is_some(),
		"Second origin should be allowed"
	);

	// Other origin should not be allowed
	let (_, headers3) = server
		.get_with_origin("/tiles/test/tiles.json", "https://third.example.org")
		.await;
	assert!(
		headers3.get("access-control-allow-origin").is_none(),
		"Third origin should not be allowed"
	);
}

/// Test that tile endpoints include CORS headers.
#[tokio::test]
async fn cors_on_tile_endpoint() {
	let server = CorsTestServer::new(&["https://example.org"], None).await;

	// Request an actual tile
	let (status, headers) = server.get_with_origin("/tiles/test/0/0/0", "https://example.org").await;

	assert_eq!(status, 200);
	assert!(
		headers.get("access-control-allow-origin").is_some(),
		"CORS headers should be present on tile endpoints"
	);
}
