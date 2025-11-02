//! Router composition for the VersaTiles server.
//!
//! This module wires handlers into an Axum `Router` without mixing in server
//! lifecycle or CORS logic. It’s intentionally tiny and declarative.

use anyhow::Result;
use axum::{Router, routing::get};

use crate::tools::server::handlers::{StaticHandlerState, TileHandlerState, ok_json, serve_static, serve_tile};
use crate::tools::server::sources::{StaticSource, TileSource};

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
