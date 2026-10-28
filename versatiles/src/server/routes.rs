//! Router composition for the VersaTiles server.
//!
//! This module wires handlers into an Axum `Router` without mixing in server
//! lifecycle or CORS logic. It's intentionally tiny and declarative.

use std::sync::Arc;

use anyhow::Result;
use axum::{
	Router,
	body::Body,
	extract::State,
	http::{HeaderMap, Uri},
	response::Response,
	routing::get,
};
use dashmap::DashMap;
use versatiles_derive::context;

use super::{
	handlers::{StaticHandlerState, error_404, ok_json, serve_static, serve_tile_from_source},
	sources::{ServerTileSource, StaticSource},
	utils::Url,
};

/// State for dynamic tile routing - looks up sources at request time.
#[derive(Clone)]
pub struct DynamicTileHandlerState {
	pub tile_sources: Arc<DashMap<String, Arc<ServerTileSource>>>,
	pub minimal_recompression: bool,
	pub cache_control: Arc<str>,
}

/// Dynamic tile handler that extracts source_id from the path and looks it up.
pub async fn serve_dynamic_tile(
	uri: Uri,
	headers: HeaderMap,
	State(state): State<DynamicTileHandlerState>,
) -> Response<Body> {
	let path = Url::from(uri.path());
	log::debug!("handle dynamic tile request: {path}");

	let parts = path.as_vec();

	// Extract source_id from /tiles/{source_id}/...
	if parts.len() < 2 || parts[0] != "tiles" {
		log::debug!("invalid tile path format: {path}");
		return error_404();
	}

	let source_id = &parts[1];

	// A source id may contain slashes, so where the id ends is not known.
	// What follows it is: `meta.json` or `tiles.json`, or `z/x/y`
	// (`ServerTileSource::get_data`), so one or three segments. Only the two
	// ids that leave those are looked up.
	//
	// This used to try every prefix of the path, longest first, joining each
	// into a new string: quadratic in the segment count. A 64 KB path of
	// `a/a/a/…` took a second of CPU to answer with a 404 (#287).
	let tile_source = if let Some(entry) = state.tile_sources.get(source_id) {
		Arc::clone(entry.value())
	} else if let Some(source) = [1, 3]
		.into_iter()
		.filter_map(|rest| parts.len().checked_sub(rest + 1))
		.filter(|&last| last >= 2)
		.find_map(|last| {
			let entry = state.tile_sources.get(&parts[1..=last].join("/"))?;
			Some(Arc::clone(entry.value()))
		}) {
		source
	} else {
		log::debug!("no tile source matches path: {path}");
		return error_404();
	};

	// Delegate to core serving logic
	serve_tile_from_source(
		path,
		headers,
		tile_source,
		state.minimal_recompression,
		&state.cache_control,
	)
	.await
}

/// Attach dynamic tile routing with single catch-all route.
pub fn add_tile_sources_to_app(
	app: Router,
	sources: Arc<DashMap<String, Arc<ServerTileSource>>>,
	minimal_recompression: bool,
	cache_control: Arc<str>,
) -> Router {
	let state = DynamicTileHandlerState {
		tile_sources: sources,
		minimal_recompression,
		cache_control,
	};

	let tile_router = Router::new()
		.route("/tiles/{*path}", get(serve_dynamic_tile))
		.with_state(state);

	app.merge(tile_router)
}

/// Attach static sources as a catch-all fallback.
/// Sources are checked in order; the first one returning data wins.
pub fn add_static_sources_to_app(
	app: Router,
	static_sources: Arc<arc_swap::ArcSwap<Vec<StaticSource>>>,
	minimal_recompression: bool,
	cache_control: Arc<str>,
) -> Router {
	let state = StaticHandlerState {
		sources: static_sources,
		minimal_recompression,
		cache_control,
	};
	let static_app = Router::new().fallback(get(serve_static)).with_state(state);
	app.merge(static_app)
}

/// Attach small JSON API endpoints (currently `/tiles/index.json`).
#[context("adding API routes to app")]
pub async fn add_api_to_app(app: Router, sources: Arc<DashMap<String, Arc<ServerTileSource>>>) -> Result<Router> {
	let mut api_app = Router::new();

	api_app = api_app.route(
		"/tiles/index.json",
		get({
			let sources = Arc::clone(&sources);
			move || async move {
				let mut ids: Vec<_> = sources.iter().map(|entry| entry.key().clone()).collect();
				ids.sort();
				let tiles_index_json = format!(
					"[{}]",
					ids.iter().map(|id| format!("\"{id}\"")).collect::<Vec<_>>().join(",")
				);
				ok_json(&tiles_index_json).await
			}
		}),
	);

	Ok(app.merge(api_app))
}

// --- tests -------------------------------------------------------------------
#[cfg(test)]
mod tests {
	use axum::{body::Body, http::StatusCode};
	use tower::ServiceExt as _;
	use versatiles_container::{MockReader, MockReaderProfile, TileSource as _};

	use super::*; // for `oneshot`

	async fn get_body_text(app: Router, path: &str) -> (StatusCode, String) {
		let req = axum::http::Request::builder().uri(path).body(Body::empty()).unwrap();
		let res = app.clone().oneshot(req).await.unwrap();
		let status = res.status();
		let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
		(status, String::from_utf8_lossy(&bytes).into_owned())
	}

	#[tokio::test]
	async fn api_index_json_is_precomputed_and_empty_when_no_sources() {
		let app = Router::new();
		let sources = Arc::new(DashMap::new());
		let app = add_api_to_app(app, sources).await.unwrap();

		let (status, body) = get_body_text(app, "/tiles/index.json").await;
		assert_eq!(status, StatusCode::OK);
		assert_eq!(body, "[]");
	}

	#[tokio::test]
	async fn no_tile_sources_yields_404() {
		let app = Router::new();
		let sources = Arc::new(DashMap::new());
		let app = add_tile_sources_to_app(
			app,
			sources,
			false,
			Arc::from(crate::server::handlers::DEFAULT_CACHE_CONTROL),
		);

		let (status, _body) = get_body_text(app, "/tiles/any/1/2/3").await;
		assert_eq!(status, StatusCode::NOT_FOUND);
	}

	#[tokio::test]
	async fn no_static_sources_yields_404() {
		let app = Router::new();
		let static_sources = Arc::new(arc_swap::ArcSwap::from_pointee(Vec::new()));
		let app = add_static_sources_to_app(
			app,
			static_sources,
			false,
			Arc::from(crate::server::handlers::DEFAULT_CACHE_CONTROL),
		);

		let (status, _body) = get_body_text(app, "/").await;
		assert_eq!(status, StatusCode::NOT_FOUND);
	}

	/// Builds a router serving `ids`, bypassing `TileServer::add_tile_source` so a
	/// test can register whatever id it needs to exercise routing.
	fn app_with_sources(ids: &[&str]) -> Router {
		let sources: Arc<DashMap<String, Arc<ServerTileSource>>> = Arc::new(DashMap::new());
		for id in ids {
			let reader = MockReader::new_mock_profile(MockReaderProfile::Png)
				.unwrap()
				.into_shared();
			let source = ServerTileSource::from(reader, id).unwrap();
			sources.insert((*id).to_string(), Arc::new(source));
		}
		add_tile_sources_to_app(
			Router::new(),
			sources,
			false,
			Arc::from(crate::server::handlers::DEFAULT_CACHE_CONTROL),
		)
	}

	/// Source ids may contain slashes — servers configured before v3 rely on it.
	/// The route is a single catch-all, so `/tiles/{*path}` cannot tell where the
	/// id ends and the tile path begins; the id is recovered by matching
	/// progressively shorter prefixes of the path against the registered sources.
	#[tokio::test]
	async fn slashed_source_id_is_reachable() {
		let app = app_with_sources(&["2025/de/wahlkreise"]);

		let (status, body) = get_body_text(app.clone(), "/tiles/2025/de/wahlkreise/meta.json").await;
		assert_eq!(status, StatusCode::OK);
		// The prefix has to survive into the tilejson, or the URLs it advertises
		// point at a source that does not answer.
		assert!(
			body.contains(r#""tiles":["/tiles/2025/de/wahlkreise/{z}/{x}/{y}"]"#),
			"unexpected tilejson: {body}"
		);

		let (status, _) = get_body_text(app, "/tiles/2025/de/wahlkreise/2/1/1").await;
		assert_eq!(status, StatusCode::OK);
	}

	/// The prefix search only runs once an exact match has failed, so this guards
	/// the ordinary single-segment case against regressions in that fallback.
	#[tokio::test]
	async fn plain_source_id_still_resolves() {
		let app = app_with_sources(&["cheese"]);

		let (status, _) = get_body_text(app.clone(), "/tiles/cheese/meta.json").await;
		assert_eq!(status, StatusCode::OK);

		let (status, _) = get_body_text(app, "/tiles/cheddar/meta.json").await;
		assert_eq!(status, StatusCode::NOT_FOUND);
	}

	/// Both shapes of request under a slashed id, and an id that is a prefix of
	/// another: the lookup only considers ids followed by one or three segments.
	#[tokio::test]
	async fn nested_source_ids_resolve_by_what_follows() {
		let app = app_with_sources(&["a/b", "a/b/c/d"]);

		for (path, status) in [
			("/tiles/a/b/meta.json", StatusCode::OK),
			("/tiles/a/b/2/1/1", StatusCode::OK),
			("/tiles/a/b/c/d/meta.json", StatusCode::OK),
			("/tiles/a/b/c/d/2/1/1", StatusCode::OK),
			// Two segments after either id: neither metadata nor a tile.
			("/tiles/a/b/c/meta.json", StatusCode::NOT_FOUND),
			("/tiles/a/b/c/d/e/f/meta.json", StatusCode::NOT_FOUND),
		] {
			assert_eq!(get_body_text(app.clone(), path).await.0, status, "{path}");
		}
	}

	/// The longest path `http::Uri` accepts, in one-byte segments: about a
	/// second while the lookup was quadratic (#287), in debug and release
	/// builds alike, and 1.5 ms now. The bound is far from both.
	#[tokio::test]
	async fn longest_path_is_answered_quickly() {
		let app = app_with_sources(&["cheese"]);
		let path = format!("/tiles/{}", "a/".repeat((65_534 - "/tiles/".len()) / 2));

		let start = std::time::Instant::now();
		let (status, _) = get_body_text(app, &path).await;
		let elapsed = start.elapsed();

		assert_eq!(status, StatusCode::NOT_FOUND);
		assert!(elapsed < std::time::Duration::from_millis(100), "took {elapsed:?}");
	}
}
