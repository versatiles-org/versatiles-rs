//! Router composition for the VersaTiles server.
//!
//! This module wires handlers into an Axum `Router` without mixing in server
//! lifecycle or CORS logic. It's intentionally tiny and declarative.

use super::{
	handlers::{StaticHandlerState, error_404, ok_json, serve_static, serve_tile_from_source},
	sources::{StaticSource, TileSource},
	utils::Url,
};
use anyhow::Result;
use axum::{
	Router,
	body::Body,
	extract::State,
	http::{HeaderMap, Uri},
	response::Response,
	routing::get,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use versatiles_derive::context;

/// State for dynamic tile routing - looks up sources at request time.
#[derive(Clone)]
pub struct DynamicTileHandlerState {
	pub tile_sources: Arc<RwLock<HashMap<String, Arc<TileSource>>>>,
	pub minimal_recompression: bool,
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

	// Lookup source (read lock)
	let sources = state.tile_sources.read().await;
	let tile_source = match sources.get(source_id) {
		Some(src) => Arc::clone(src),
		None => {
			log::debug!("tile source '{}' not found", source_id);
			drop(sources); // release lock before returning
			return error_404();
		}
	};
	drop(sources); // Release lock ASAP

	// Delegate to core serving logic
	serve_tile_from_source(path, headers, tile_source, state.minimal_recompression).await
}

/// Attach dynamic tile routing with single catch-all route.
pub fn add_tile_sources_to_app(
	app: Router,
	sources: Arc<RwLock<HashMap<String, Arc<TileSource>>>>,
	minimal_recompression: bool,
) -> Router {
	let state = DynamicTileHandlerState {
		tile_sources: sources,
		minimal_recompression,
	};

	let tile_router = Router::new()
		.route("/tiles/{*path}", get(serve_dynamic_tile))
		.with_state(state);

	app.merge(tile_router)
}

/// Attach static sources as a catch-all fallback.
/// Sources are checked in order; the first one returning data wins.
pub fn add_static_sources_to_app(app: Router, static_sources: &[StaticSource], minimal_recompression: bool) -> Router {
	let state = StaticHandlerState {
		sources: static_sources.to_vec(),
		minimal_recompression,
	};
	let static_app = Router::new().fallback(get(serve_static)).with_state(state);
	app.merge(static_app)
}

/// Attach small JSON API endpoints (currently `/tiles/index.json`).
#[context("adding API routes to app")]
pub async fn add_api_to_app(app: Router, sources: Arc<RwLock<HashMap<String, Arc<TileSource>>>>) -> Result<Router> {
	let mut api_app = Router::new();

	api_app = api_app.route(
		"/tiles/index.json",
		get({
			let sources = Arc::clone(&sources);
			move || async move {
				let sources_lock = sources.read().await;
				let mut ids: Vec<_> = sources_lock.keys().map(|s| s.as_str()).collect();
				ids.sort();
				let tiles_index_json = format!(
					"[{}]",
					ids.iter().map(|id| format!("\"{}\"", id)).collect::<Vec<_>>().join(",")
				);
				ok_json(&tiles_index_json)
			}
		}),
	);

	Ok(app.merge(api_app))
}

// --- tests -------------------------------------------------------------------
#[cfg(test)]
mod tests {
	use super::*;
	use axum::{body::Body, http::StatusCode};
	use tower::ServiceExt as _; // for `oneshot`

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
		let sources = Arc::new(RwLock::new(HashMap::new()));
		let app = add_api_to_app(app, sources).await.unwrap();

		let (status, body) = get_body_text(app, "/tiles/index.json").await;
		assert_eq!(status, StatusCode::OK);
		assert_eq!(body, "[]");
	}

	#[tokio::test]
	async fn no_tile_sources_yields_404() {
		let app = Router::new();
		let sources = Arc::new(RwLock::new(HashMap::new()));
		let app = add_tile_sources_to_app(app, sources, false);

		let (status, _body) = get_body_text(app, "/tiles/any/1/2/3").await;
		assert_eq!(status, StatusCode::NOT_FOUND);
	}

	#[tokio::test]
	async fn no_static_sources_yields_404() {
		let app = Router::new();
		let app = add_static_sources_to_app(app, &[], false);

		let (status, _body) = get_body_text(app, "/").await;
		assert_eq!(status, StatusCode::NOT_FOUND);
	}
}
