#![cfg(all(feature = "cli", feature = "server"))]

//! E2E tests for static content serving from TAR archives.
//!
//! These tests verify that the server correctly serves static files from
//! .tar, .tar.gz, and .tar.br archives.

mod test_utilities;

use reqwest::header::CONTENT_TYPE;
use std::{fs, process::Child};
use tempfile::TempDir;
use test_utilities::*;

/// Convert a path to use forward slashes for YAML compatibility on all platforms.
/// Windows backslashes in YAML are interpreted as escape sequences, causing parsing errors.
fn to_yaml_path(path: &str) -> String {
	path.replace('\\', "/")
}

struct StaticTestServer {
	host: String,
	child: Child,
	#[allow(dead_code)]
	temp_dir: Option<TempDir>,
}

impl StaticTestServer {
	async fn with_static_source(static_source: &str) -> Self {
		let (host, child) = spawn_server(&["-s", static_source], "/").await;
		Self {
			host,
			child,
			temp_dir: None,
		}
	}

	async fn with_config(config_content: &str) -> Self {
		let temp_dir = tempfile::tempdir().unwrap();
		let config_path = temp_dir.path().join("config.yml");
		fs::write(&config_path, config_content).unwrap();

		let (host, child) = spawn_server(&["-c", config_path.to_str().unwrap()], "/").await;
		Self {
			host,
			child,
			temp_dir: Some(temp_dir),
		}
	}

	fn shutdown(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}

	async fn get(&self, path: &str) -> (u16, Option<String>, String) {
		let resp = reqwest::get(format!("{}{path}", self.host)).await.unwrap();
		let status = resp.status().as_u16();
		let content_type = resp
			.headers()
			.get(CONTENT_TYPE)
			.map(|v| v.to_str().unwrap().to_string());
		let body = resp.text().await.unwrap();
		(status, content_type, body)
	}
}

impl Drop for StaticTestServer {
	fn drop(&mut self) {
		self.shutdown();
	}
}

/// Test serving static content from a gzip-compressed tar archive.
#[tokio::test]
async fn e2e_serve_static_from_tar_gz() {
	let static_path = get_testdata("static.tar.gz");
	let server = StaticTestServer::with_static_source(&static_path).await;

	// Request index.html
	let (status, content_type, body) = server.get("/index.html").await;
	assert_eq!(status, 200);
	assert!(
		content_type.as_ref().is_some_and(|ct| ct.contains("text/html")),
		"Content-Type should be text/html, got: {content_type:?}"
	);
	assert!(body.contains("html"), "Body should contain HTML content");

	// Request style.css
	let (status, content_type, body) = server.get("/style.css").await;
	assert_eq!(status, 200);
	assert!(
		content_type.as_ref().is_some_and(|ct| ct.contains("text/css")),
		"Content-Type should be text/css, got: {content_type:?}"
	);
	assert!(!body.is_empty(), "CSS body should not be empty");
}

/// Test serving static content from a brotli-compressed tar archive.
#[tokio::test]
async fn e2e_serve_static_from_tar_br() {
	let static_path = get_testdata("static.tar.br");
	let server = StaticTestServer::with_static_source(&static_path).await;

	// Request index.html
	let (status, content_type, body) = server.get("/index.html").await;
	assert_eq!(status, 200);
	assert!(
		content_type.as_ref().is_some_and(|ct| ct.contains("text/html")),
		"Content-Type should be text/html, got: {content_type:?}"
	);
	assert!(body.contains("html"), "Body should contain HTML content");
}

/// Test that non-existent files return 404.
#[tokio::test]
async fn e2e_serve_static_returns_404_for_missing_file() {
	let static_path = get_testdata("static.tar.gz");
	let server = StaticTestServer::with_static_source(&static_path).await;

	let (status, _, _) = server.get("/nonexistent.txt").await;
	assert_eq!(status, 404, "Should return 404 for non-existent file");
}

/// Test serving index.html at root path.
#[tokio::test]
async fn e2e_serve_static_index_at_root() {
	let static_path = get_testdata("static.tar.gz");
	let server = StaticTestServer::with_static_source(&static_path).await;

	// Request root path - should serve index.html
	let (status, content_type, body) = server.get("/").await;
	assert_eq!(status, 200);
	assert!(
		content_type.as_ref().is_some_and(|ct| ct.contains("text/html")),
		"Root should serve HTML content"
	);
	assert!(body.contains("html"), "Root should contain HTML content");
}

/// Test static source with prefix via config file.
#[tokio::test]
async fn e2e_serve_static_with_prefix() {
	let static_path = to_yaml_path(&get_testdata("static.tar.gz"));
	let config = format!(
		r#"
static:
  - src: "{static_path}"
    prefix: "/assets"
"#
	);

	let server = StaticTestServer::with_config(&config).await;

	// Request with prefix
	let (status, content_type, _) = server.get("/assets/index.html").await;
	assert_eq!(status, 200);
	assert!(
		content_type.as_ref().is_some_and(|ct| ct.contains("text/html")),
		"Should serve HTML at prefixed path"
	);

	// Request without prefix should 404
	let (status, _, _) = server.get("/index.html").await;
	assert_eq!(status, 404, "Should 404 without prefix");
}

/// Test multiple static sources with different prefixes.
#[tokio::test]
async fn e2e_serve_multiple_static_sources() {
	let static_gz = to_yaml_path(&get_testdata("static.tar.gz"));
	let static_br = to_yaml_path(&get_testdata("static.tar.br"));
	let config = format!(
		r#"
static:
  - src: "{static_gz}"
    prefix: "/gz"
  - src: "{static_br}"
    prefix: "/br"
"#
	);

	let server = StaticTestServer::with_config(&config).await;

	// Request from first source
	let (status1, _, _) = server.get("/gz/index.html").await;
	assert_eq!(status1, 200, "Should serve from first static source");

	// Request from second source
	let (status2, _, _) = server.get("/br/index.html").await;
	assert_eq!(status2, 200, "Should serve from second static source");
}

/// Test correct Content-Type for different file extensions.
#[tokio::test]
async fn e2e_serve_static_content_type_by_extension() {
	let static_path = get_testdata("static.tar.gz");
	let server = StaticTestServer::with_static_source(&static_path).await;

	// HTML file
	let (_, html_ct, _) = server.get("/index.html").await;
	assert!(
		html_ct.as_ref().is_some_and(|ct| ct.contains("text/html")),
		"HTML should have text/html Content-Type"
	);

	// CSS file
	let (_, css_ct, _) = server.get("/style.css").await;
	assert!(
		css_ct.as_ref().is_some_and(|ct| ct.contains("text/css")),
		"CSS should have text/css Content-Type"
	);
}

/// Test static serving alongside tile sources.
#[tokio::test]
async fn e2e_serve_static_with_tiles() {
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

	let server = StaticTestServer::with_config(&config).await;

	// Static content should work
	let (static_status, _, _) = server.get("/index.html").await;
	assert_eq!(static_status, 200, "Static content should be served");

	// Tiles should also work
	let (tiles_status, _, _) = server.get("/tiles/berlin/tiles.json").await;
	assert_eq!(tiles_status, 200, "Tile metadata should be served");
}
