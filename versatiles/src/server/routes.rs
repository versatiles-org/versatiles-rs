//! Router composition for the VersaTiles server.
//!
//! This module wires handlers into an Axum `Router` without mixing in server
//! lifecycle or CORS logic. It’s intentionally tiny and declarative.

use super::{
	handlers::{StaticHandlerState, TileHandlerState, ok_json, serve_static, serve_tile},
	sources::{StaticSource, TileSource},
};
use anyhow::Result;
use axum::{Router, routing::get};
use versatiles_derive::context;

/// Attach all tile sources under their prefixes (`/tiles/<id>/{*path}`).
pub fn add_tile_sources_to_app(mut app: Router, sources: &[TileSource], minimal_recompression: bool) -> Router {
	for tile_source in sources.iter() {
		let state = TileHandlerState {
			tile_source: tile_source.clone(),
			minimal_recompression,
		};
		let route = tile_source.prefix.join_as_string("{*path}");
		let tile_app = Router::new().route(&route, get(serve_tile)).with_state(state);
		app = app.merge(tile_app);
	}
	app
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
pub async fn add_api_to_app(app: Router, sources: &[TileSource]) -> Result<Router> {
	let mut api_app = Router::new();

	// Precompute a tiny JSON list of source IDs to avoid recomputing on each request.
	let tiles_index_json: String = format!(
		"[{}]",
		sources
			.iter()
			.map(|s| format!("\"{}\"", s.id))
			.collect::<Vec<String>>()
			.join(","),
	);

	api_app = api_app.route(
		"/tiles/index.json",
		get({
			let tiles_index_json = tiles_index_json.clone();
			move || async move { ok_json(&tiles_index_json) }
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
		let app = add_api_to_app(app, &[]).await.unwrap();

		let (status, body) = get_body_text(app, "/tiles/index.json").await;
		assert_eq!(status, StatusCode::OK);
		assert_eq!(body, "[]");
	}

	#[tokio::test]
	async fn no_tile_sources_yields_404() {
		let app = Router::new();
		let app = add_tile_sources_to_app(app, &[], false);

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
