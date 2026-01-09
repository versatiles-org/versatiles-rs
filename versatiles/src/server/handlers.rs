//! HTTP handlers and small response helpers for the tile/static server.
//!
//! - `serve_tile` serves tiles from a single `ServerTileSource`.
//! - `serve_static` serves files from a list of `StaticSource`s.
//! - `ok_json` is a tiny helper used by the API routes.
//!
//! Note: CORS headers are handled exclusively by the `CorsLayer`. Don’t set
//! `Access-Control-Allow-Origin` here; that avoids header drift.

use super::{
	encoding::get_encoding,
	sources::{ServerTileSource, SourceResponse, StaticSource},
	utils::Url,
};
use axum::{
	body::Body,
	extract::State,
	http::{HeaderMap, Uri, header},
	response::Response,
};
use std::sync::Arc;
use versatiles_core::{
	Blob, TileCompression,
	utils::{TargetCompression, optimize_compression},
};

/// State for static file requests across multiple `StaticSource`s.
#[derive(Clone)]
pub struct StaticHandlerState {
	pub sources: Arc<arc_swap::ArcSwap<Vec<StaticSource>>>,
	pub minimal_recompression: bool,
}

/// Core tile serving logic extracted for reuse in dynamic routing.
/// Takes an Arc<ServerTileSource> to support both static and dynamic routing.
pub async fn serve_tile_from_source(
	path: Url,
	headers: HeaderMap,
	tile_source: Arc<ServerTileSource>,
	minimal_recompression: bool,
) -> Response<Body> {
	log::debug!("handle tile request: {path}");

	let mut target = get_encoding(&headers);
	if minimal_recompression {
		target.set_fast_compression();
	}

	let stripped_path = match path.strip_prefix(&tile_source.prefix) {
		Ok(p) => p,
		Err(err) => {
			log::error!(
				"Path prefix mismatch: path '{}' does not start with expected prefix '{}':\n{}",
				path,
				tile_source.prefix,
				format_error_chain(&err)
			);
			return error_500();
		}
	};

	let response = tile_source.get_data(&stripped_path, &target).await;

	match response {
		Ok(Some(result)) => {
			log::debug!("send response for tile request: {path}");
			ok_data(result, target)
		}
		Ok(None) => {
			log::debug!("send 404 for tile request: {path}");
			error_404()
		}
		Err(err) => {
			log::warn!(
				"send 500 for tile request: {path}. Error:\n{}",
				format_error_chain(&err)
			);
			error_500()
		}
	}
}

/// Static handler: tries each source in order until one returns data.
pub async fn serve_static(uri: Uri, headers: HeaderMap, State(state): State<StaticHandlerState>) -> Response<Body> {
	let mut url = Url::from(uri.path());
	log::debug!("handle static request: {url}");

	if url.is_dir() {
		url.push("index.html");
	}

	let mut target = get_encoding(&headers);
	if state.minimal_recompression {
		target.set_fast_compression();
	}

	// Load sources (lock-free!)
	let sources = state.sources.load();

	for source in sources.iter() {
		if let Some(result) = source.get_data(&url, &target) {
			log::debug!("send response to static request: {url}");
			return ok_data(result, target);
		}
	}
	log::debug!("send 404 to static request: {url}");
	error_404()
}

// --- small helpers -----------------------------------------------------------

fn format_error_chain(err: &anyhow::Error) -> String {
	let mut result = err.to_string();

	// Iterate through the error chain
	for (i, cause) in err.chain().skip(1).enumerate() {
		if i == 0 {
			result.push_str("\n  Caused by:");
		}
		result.push_str(&format!("\n    {}", cause));
	}

	result
}

fn error_with(status: u16, message: &str) -> Response<Body> {
	Response::builder()
		.status(status)
		.header(header::CONTENT_TYPE, "text/plain; charset=utf-8")
		// Leave CORS to the middleware.
		.body(Body::from(message.as_bytes().to_vec()))
		.expect("failed to build error response")
}

pub fn error_404() -> Response<Body> {
	error_with(404, "Not Found")
}

pub fn error_500() -> Response<Body> {
	error_with(500, "Internal Server Error")
}

fn ok_data(result: SourceResponse, mut target: TargetCompression) -> Response<Body> {
	// Binary images are effectively incompressible; avoid recompression.
	if matches!(
		result.mime.as_str(),
		"image/png" | "image/jpeg" | "image/webp" | "image/avif"
	) {
		target.set_incompressible();
	}

	let mut response = Response::builder()
		.status(200)
		.header(header::CONTENT_TYPE, &result.mime)
		.header(header::CACHE_CONTROL, "public, max-age=2419200, no-transform")
		.header(header::VARY, "accept-encoding");

	log::trace!(
		"optimize_compression from {:?} with target {:?}",
		result.compression,
		target
	);

	let (blob, compression) = match optimize_compression(result.blob, result.compression, &target) {
		Ok(result) => result,
		Err(err) => {
			log::error!(
				"Compression optimization failed for mime type '{}' (compression: {:?}, target: {:?}):\n{}",
				result.mime,
				result.compression,
				target,
				format_error_chain(&err)
			);
			return error_500();
		}
	};

	use TileCompression::{Brotli, Gzip, Uncompressed};
	match compression {
		Uncompressed => {}
		Gzip => response = response.header(header::CONTENT_ENCODING, "gzip"),
		Brotli => response = response.header(header::CONTENT_ENCODING, "br"),
	}

	log::trace!("send response with headers: {:?}", response.headers_ref());

	response
		.body(Body::from(blob.into_vec()))
		.expect("failed to build OK response")
}

/// Tiny JSON helper used by API routes.
pub fn ok_json(message: &str) -> Response<Body> {
	ok_data(
		SourceResponse {
			blob: Blob::from(message),
			compression: TileCompression::Uncompressed,
			mime: String::from("application/json"),
		},
		TargetCompression::from_none(),
	)
}

// --- tests -------------------------------------------------------------------
#[cfg(test)]
mod tests {
	use super::*;
	use axum::http::header;

	#[test]
	fn ok_json_sets_expected_headers() {
		let resp = ok_json(r#"{"ok":true}"#);
		assert_eq!(resp.status(), 200);

		let headers = resp.headers();
		assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "application/json");
		assert_eq!(
			headers.get(header::CACHE_CONTROL).unwrap(),
			"public, max-age=2419200, no-transform"
		);
		assert_eq!(headers.get(header::VARY).unwrap(), "accept-encoding");
		// No content-encoding for plain JSON
		assert!(headers.get(header::CONTENT_ENCODING).is_none());
	}

	#[test]
	fn ok_data_plain_text_gzip_when_allowed() {
		// Source is uncompressed text; client allows gzip
		let src = SourceResponse {
			blob: Blob::from("The quick brown fox jumps over the lazy dog"),
			compression: TileCompression::Uncompressed,
			mime: "text/plain".into(),
		};
		let mut target = TargetCompression::from_none();
		target.insert(TileCompression::Gzip);

		let resp = super::ok_data(src, target);
		assert_eq!(resp.status(), 200);
		let headers = resp.headers();

		assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "text/plain");
		assert_eq!(
			headers.get(header::CACHE_CONTROL).unwrap(),
			"public, max-age=2419200, no-transform"
		);
		assert_eq!(headers.get(header::VARY).unwrap(), "accept-encoding");

		// Expect gzip because requester allowed it and source was uncompressed text
		assert_eq!(headers.get(header::CONTENT_ENCODING).unwrap(), "gzip");
	}

	#[test]
	fn format_error_chain_single_error() {
		let err = anyhow::anyhow!("Simple error");
		let formatted = super::format_error_chain(&err);
		assert_eq!(formatted, "Simple error");
	}

	#[test]
	fn format_error_chain_with_context() {
		// Create a multi-level error chain
		let err = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
		let err = anyhow::Error::from(err)
			.context("Failed to read configuration")
			.context("Server startup failed");
		let formatted = format_error_chain(&err);

		// Should contain main error and all causes
		assert!(formatted.contains("Server startup failed"));
		assert!(formatted.contains("Caused by:"));
		assert!(formatted.contains("Failed to read configuration"));
		assert!(formatted.contains("file not found"));

		// Should be multi-line
		assert!(formatted.lines().count() > 1);

		// Verify the format structure
		let lines: Vec<&str> = formatted.lines().collect();
		assert_eq!(lines[0], "Server startup failed");
		assert_eq!(lines[1], "  Caused by:");
		assert!(lines[2].starts_with("    "));
		assert!(lines[3].starts_with("    "));
	}

	#[test]
	fn ok_data_png_is_not_recompressed() {
		// PNG should be treated as incompressible even if br is allowed
		let png_bytes = vec![137, 80, 78, 71, 13, 10, 26, 10]; // just a PNG signature; enough for header tests
		let src = SourceResponse {
			blob: Blob::from(png_bytes),
			compression: TileCompression::Uncompressed,
			mime: "image/png".into(),
		};
		let mut target = TargetCompression::from_none();
		target.insert(TileCompression::Brotli);

		let resp = super::ok_data(src, target);
		assert_eq!(resp.status(), 200);
		let headers = resp.headers();

		assert_eq!(headers.get(header::CONTENT_TYPE).unwrap(), "image/png");
		assert_eq!(
			headers.get(header::CACHE_CONTROL).unwrap(),
			"public, max-age=2419200, no-transform"
		);
		assert_eq!(headers.get(header::VARY).unwrap(), "accept-encoding");

		// No content-encoding because we avoid recompressing images
		assert!(headers.get(header::CONTENT_ENCODING).is_none());
	}
}
