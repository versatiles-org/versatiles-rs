#![cfg(all(feature = "cli", feature = "server"))]

//! E2E tests for server configuration file integration.
//!
//! These tests verify that the server correctly loads and applies all settings
//! from a YAML configuration file.

mod test_utilities;

use reqwest::header::ORIGIN;
use std::{fs, net::TcpListener, process::Child, thread, time::Duration};
use tempfile::TempDir;
use test_utilities::*;

/// Convert a path to use forward slashes for YAML compatibility on all platforms.
/// Windows backslashes in YAML are interpreted as escape sequences, causing parsing errors.
fn to_yaml_path(path: &str) -> String {
	path.replace('\\', "/")
}

struct ConfigTestServer {
	host: String,
	child: Child,
	#[allow(dead_code)]
	temp_dir: TempDir,
}

impl ConfigTestServer {
	async fn new(config_content: &str) -> Self {
		let temp_dir = tempfile::tempdir().unwrap();
		let config_path = temp_dir.path().join("config.yml");
		fs::write(&config_path, config_content).unwrap();

		let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();

		let mut cmd = versatiles_cmd();
		cmd.args(["serve", "-c", config_path.to_str().unwrap(), "-p", &port.to_string()]);
		let mut child = cmd.spawn().unwrap();

		// Wait for server to be ready
		loop {
			thread::sleep(Duration::from_millis(100));
			if let Some(status) = child.try_wait().unwrap() {
				// Server exited - try to capture output for debugging
				use std::io::Read;
				let mut stdout_str = String::new();
				let mut stderr_str = String::new();
				if let Some(ref mut stdout) = child.stdout {
					let _ = stdout.read_to_string(&mut stdout_str);
				}
				if let Some(ref mut stderr) = child.stderr {
					let _ = stderr.read_to_string(&mut stderr_str);
				}
				panic!(
					"server process exited prematurely with status: {:?}\nconfig:\n{}\nstdout:\n{}\nstderr:\n{}",
					status.code(),
					config_content,
					stdout_str,
					stderr_str
				);
			}
			if reqwest::get(format!("http://127.0.0.1:{port}/")).await.is_ok() {
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

	async fn get(&self, path: &str) -> reqwest::Response {
		reqwest::get(format!("{}{path}", self.host)).await.unwrap()
	}

	async fn get_with_origin(&self, path: &str, origin: &str) -> reqwest::Response {
		let client = reqwest::Client::new();
		client
			.get(format!("{}{path}", self.host))
			.header(ORIGIN, origin)
			.send()
			.await
			.unwrap()
	}
}

impl Drop for ConfigTestServer {
	fn drop(&mut self) {
		self.shutdown();
	}
}

/// Test that tile sources from config are accessible.
#[tokio::test]
async fn e2e_config_tile_sources_accessible() {
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let config = format!(
		r#"
tiles:
  - name: myberlin
    src: "{tiles_path}"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Tile metadata should be accessible
	let resp = server.get("/tiles/myberlin/tiles.json").await;
	assert_eq!(resp.status(), 200, "Tile metadata should be accessible");

	// Tile index should list the source
	let resp = server.get("/tiles/index.json").await;
	assert_eq!(resp.status(), 200);
	let body = resp.text().await.unwrap();
	assert!(body.contains("myberlin"), "Index should list tile source");
}

/// Test that extra_response_headers are applied to responses.
#[tokio::test]
async fn e2e_config_extra_response_headers() {
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let config = format!(
		r#"
tiles:
  - name: test
    src: "{tiles_path}"

extra_response_headers:
  Cache-Control: "public, max-age=3600"
  X-Custom-Header: "custom-value"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	let resp = server.get("/tiles/test/tiles.json").await;
	assert_eq!(resp.status(), 200);

	// Check Cache-Control header
	let cache_control = resp.headers().get("cache-control");
	assert!(
		cache_control.is_some_and(|v| v.to_str().unwrap().contains("max-age=3600")),
		"Cache-Control header should be set from config"
	);

	// Check custom header
	let custom_header = resp.headers().get("x-custom-header");
	assert!(
		custom_header.is_some_and(|v| v.to_str().unwrap() == "custom-value"),
		"Custom header should be set from config"
	);
}

/// Test that CORS settings from config are applied.
#[tokio::test]
async fn e2e_config_cors_settings() {
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let config = format!(
		r#"
tiles:
  - name: test
    src: "{tiles_path}"

cors:
  allowed_origins:
    - "https://allowed.example.org"
  max_age_seconds: 7200
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Allowed origin should get CORS headers
	let resp = server
		.get_with_origin("/tiles/test/tiles.json", "https://allowed.example.org")
		.await;
	assert_eq!(resp.status(), 200);
	assert!(
		resp.headers().get("access-control-allow-origin").is_some(),
		"CORS header should be present for allowed origin"
	);

	// Disallowed origin should not get CORS headers
	let resp = server
		.get_with_origin("/tiles/test/tiles.json", "https://disallowed.example.org")
		.await;
	assert_eq!(resp.status(), 200);
	assert!(
		resp.headers().get("access-control-allow-origin").is_none(),
		"CORS header should not be present for disallowed origin"
	);
}

/// Test that static sources from config work.
#[tokio::test]
async fn e2e_config_static_sources() {
	let static_path = to_yaml_path(&get_testdata("static.tar.gz"));
	let config = format!(
		r#"
static:
  - src: "{static_path}"
    prefix: "/static"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Static content should be accessible at prefix
	let resp = server.get("/static/index.html").await;
	assert_eq!(resp.status(), 200, "Static content should be accessible");
	assert!(
		resp
			.headers()
			.get("content-type")
			.is_some_and(|v| v.to_str().unwrap().contains("text/html")),
		"Should serve HTML content"
	);
}

/// Test combined static and tile sources from config.
#[tokio::test]
async fn e2e_config_combined_static_and_tiles() {
	let static_path = to_yaml_path(&get_testdata("static.tar.gz"));
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let config = format!(
		r#"
static:
  - src: "{static_path}"
    prefix: "/"

tiles:
  - name: berlin
    src: "{tiles_path}"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Static content should work
	let resp = server.get("/index.html").await;
	assert_eq!(resp.status(), 200, "Static content should be accessible");

	// Tile metadata should work
	let resp = server.get("/tiles/berlin/tiles.json").await;
	assert_eq!(resp.status(), 200, "Tile metadata should be accessible");

	// Tile data should work
	let resp = server.get("/tiles/berlin/14/8800/5374").await;
	assert_eq!(resp.status(), 200, "Tile data should be accessible");
}

/// Test multiple tile sources from config.
#[tokio::test]
async fn e2e_config_multiple_tile_sources() {
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let pmtiles_path = to_yaml_path(&get_testdata("berlin.pmtiles"));
	let config = format!(
		r#"
tiles:
  - name: mbtiles
    src: "{tiles_path}"
  - name: pmtiles
    src: "{pmtiles_path}"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Both sources should be accessible
	let resp1 = server.get("/tiles/mbtiles/tiles.json").await;
	assert_eq!(resp1.status(), 200, "First tile source should be accessible");

	let resp2 = server.get("/tiles/pmtiles/tiles.json").await;
	assert_eq!(resp2.status(), 200, "Second tile source should be accessible");

	// Index should list both
	let resp = server.get("/tiles/index.json").await;
	let body = resp.text().await.unwrap();
	assert!(body.contains("mbtiles"), "Index should list first source");
	assert!(body.contains("pmtiles"), "Index should list second source");
}

/// Test that disable_api option hides API endpoints.
#[tokio::test]
async fn e2e_config_disable_api() {
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let config = format!(
		r#"
server:
  disable_api: true

tiles:
  - name: test
    src: "{tiles_path}"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Tiles should still work
	let resp = server.get("/tiles/test/14/8800/5374").await;
	assert_eq!(resp.status(), 200, "Tiles should still be served");

	// API index should be disabled (404 or empty)
	let resp = server.get("/tiles/index.json").await;
	// When API is disabled, index.json returns 404
	assert!(
		resp.status() == 404 || resp.text().await.unwrap() == "[]",
		"API index should be disabled or empty"
	);
}

/// Test extra headers on tile responses.
#[tokio::test]
async fn e2e_config_extra_headers_on_tiles() {
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let config = format!(
		r#"
tiles:
  - name: test
    src: "{tiles_path}"

extra_response_headers:
  Surrogate-Control: "max-age=604800"
  CDN-Cache-Control: "max-age=604800"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Request a tile
	let resp = server.get("/tiles/test/14/8800/5374").await;
	assert_eq!(resp.status(), 200);

	// Check CDN headers on tile response
	assert!(
		resp
			.headers()
			.get("surrogate-control")
			.is_some_and(|v| v.to_str().unwrap().contains("604800")),
		"Surrogate-Control should be present on tile responses"
	);
	assert!(
		resp
			.headers()
			.get("cdn-cache-control")
			.is_some_and(|v| v.to_str().unwrap().contains("604800")),
		"CDN-Cache-Control should be present on tile responses"
	);
}

/// Test wildcard CORS origin from config.
#[tokio::test]
async fn e2e_config_cors_wildcard_origin() {
	let tiles_path = to_yaml_path(&get_testdata("berlin.mbtiles"));
	let config = format!(
		r#"
tiles:
  - name: test
    src: "{tiles_path}"

cors:
  allowed_origins:
    - "*.example.org"
"#
	);

	let server = ConfigTestServer::new(&config).await;

	// Subdomain should be allowed
	let resp = server
		.get_with_origin("/tiles/test/tiles.json", "https://app.example.org")
		.await;
	assert!(
		resp.headers().get("access-control-allow-origin").is_some(),
		"Wildcard subdomain should be allowed"
	);

	// Different domain should not be allowed
	let resp = server
		.get_with_origin("/tiles/test/tiles.json", "https://other.com")
		.await;
	assert!(
		resp.headers().get("access-control-allow-origin").is_none(),
		"Different domain should not be allowed"
	);
}
