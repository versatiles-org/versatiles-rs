//! E2E tests for vector tiles that hold no data: answered with `204` (#281).

use std::process::Child;

use reqwest::{StatusCode, header::CACHE_CONTROL};

use crate::test_utilities::*;

struct Server {
	host: String,
	child: Child,
}

impl Server {
	async fn new(args: &[&str]) -> Self {
		let (host, child) = spawn_server(args, "/tiles/index.json").await;
		Self { host, child }
	}

	/// Status, `Cache-Control` and body length for a tile of `berlin`.
	async fn tile(&self, coord: &str) -> (StatusCode, Option<String>, usize) {
		let resp = reqwest::get(format!("{}/tiles/berlin/{coord}", self.host))
			.await
			.unwrap();
		let status = resp.status();
		let cache_control = resp
			.headers()
			.get(CACHE_CONTROL)
			.map(|value| value.to_str().unwrap().to_string());
		(status, cache_control, resp.bytes().await.unwrap().len())
	}
}

impl Drop for Server {
	fn drop(&mut self) {
		let _ = self.child.kill();
		let _ = self.child.wait();
	}
}

// `berlin` holds zoom 0 to 14 for central Berlin. Zoom 12, x 0, y 0 is inside
// that zoom range and far outside Berlin; zoom 15 is outside the range.
const EMPTY: &str = "12/0/0";
const PRESENT: &str = "12/2200/1343";
const BEYOND: &str = "15/17600/10745";

#[rstest::rstest]
#[case("berlin.mbtiles")]
#[case("berlin.pmtiles")]
#[tokio::test]
async fn e2e_a_vector_tile_without_data_is_answered_with_204(#[case] file: &str) {
	let input = get_testdata(file);
	let server = Server::new(&[&input, "--cache-control", "public, max-age=60"]).await;

	let (status, cache_control, len) = server.tile(EMPTY).await;
	assert_eq!(status, StatusCode::NO_CONTENT);
	assert_eq!(len, 0, "a 204 has no body");
	// Cached like a tile, because it is one.
	assert_eq!(cache_control.as_deref(), Some("public, max-age=60"));

	assert_eq!(server.tile(PRESENT).await.0, StatusCode::OK);

	// Not everything without data is an empty tile.
	for coord in [BEYOND, "99/0/0", "3/99/0", "a/b/c"] {
		let (status, cache_control, _) = server.tile(coord).await;
		assert_eq!(status, StatusCode::NOT_FOUND, "{coord}");
		assert_eq!(cache_control, None, "{coord}: a 404 is not cached like a tile");
	}
	let resp = reqwest::get(format!("{}/tiles/unknown/{EMPTY}", server.host))
		.await
		.unwrap();
	assert_eq!(resp.status(), StatusCode::NOT_FOUND, "unknown tileset");
}
