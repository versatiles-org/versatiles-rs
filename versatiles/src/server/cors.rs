//! CORS configuration helpers.
//!
//! Supports the following `allowed_origins` patterns (strings):
//! - `"*"`                     → allow all origins
//! - `"*.example.com"`        → suffix match (implemented as `*example.com`)
//! - `"https://example.com*"` → prefix match
//! - `"/^https://(foo|bar)\.example\.com$/"` → custom regex (leading and trailing `/`)
//! - exact strings like `"https://maps.example.org"`
//!
//! The returned [`CorsLayer`] can be added to the Axum router. We only set
//! the origin predicate here to avoid surprising defaults; methods/headers
//! are left to Axum/Tower-HTTP defaults unless you want to extend them.

use std::time::Duration;

use anyhow::Result;
use axum::http::{header::HeaderValue, request::Parts};
use regex::Regex;
use tower_http::cors::{AllowOrigin, CorsLayer};

type Predicate = Box<dyn Fn(&str) -> bool + Send + Sync + 'static>;

/// Build a `CorsLayer` with a predicate assembled from `allowed_origins`.
///
/// See module docs for supported pattern forms.
pub fn build_cors_layer(allowed_origins: &[String], max_age_seconds: u64) -> Result<CorsLayer> {
	// Compile the list of origin checks.
	let checks: Vec<Predicate> = allowed_origins
		.iter()
		.map(|pattern| {
			Ok::<Predicate, anyhow::Error>(if pattern == "*" {
				// Allow everything.
				Box::new(|_: &str| true)
			} else if Regex::new(r"^\*[^*]+$")?.is_match(pattern) {
				// "*suffix" → suffix match
				let suffix = pattern[1..].to_string();
				Box::new(move |origin: &str| origin.ends_with(&suffix))
			} else if Regex::new(r"^[^*]+\*$")?.is_match(pattern) {
				// "prefix*" → prefix match
				let prefix = pattern[..pattern.len() - 1].to_string();
				Box::new(move |origin: &str| origin.starts_with(&prefix))
			} else if Regex::new(r"^/.+/$")?.is_match(pattern) {
				// "/regex/" → full regex (strip slashes)
				let re = Regex::new(&pattern[1..pattern.len() - 1])?;
				Box::new(move |origin: &str| re.is_match(origin))
			} else {
				// Exact match
				let exact = pattern.clone();
				Box::new(move |origin: &str| origin == exact)
			})
		})
		.collect::<Result<Vec<_>>>()?;

	// Build the layer with a predicate function that ORs all checks.
	let layer = CorsLayer::new()
		.allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _req: &Parts| {
			let origin_str = origin.to_str().unwrap_or("");
			checks.iter().any(|f| f(origin_str))
		}))
		.max_age(Duration::from_secs(max_age_seconds));

	Ok(layer)
}

#[cfg(test)]
mod tests {
	use super::*;
	use axum::{
		Router,
		body::Body,
		http::{Request, header},
		routing::get,
	};
	use tower::ServiceExt; // for `oneshot`

	async fn has_acao(layer: &CorsLayer, origin: &str) -> bool {
		let app = Router::new().route("/", get(|| async { "ok" })).layer(layer.clone());

		let req = Request::builder()
			.uri("/")
			.header(header::ORIGIN, origin)
			.body(Body::empty())
			.unwrap();

		let resp = app.oneshot(req).await.unwrap();
		resp.headers().get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_some()
	}

	#[tokio::test]
	async fn exact_match() {
		let layer = build_cors_layer(&["https://maps.example.org".into()], 3600).unwrap();
		assert!(has_acao(&layer, "https://maps.example.org").await);
		assert!(!has_acao(&layer, "https://maps.example.com").await);
	}

	#[tokio::test]
	async fn star_all() {
		let layer = build_cors_layer(&["*".into()], 3600).unwrap();
		assert!(has_acao(&layer, "http://anything.local").await);
		assert!(has_acao(&layer, "https://whatever.example").await);
	}

	#[tokio::test]
	async fn suffix_match() {
		let layer = build_cors_layer(&["*example.com".into()], 3600).unwrap();
		assert!(has_acao(&layer, "https://foo.example.com").await);
		assert!(has_acao(&layer, "https://bar.example.com").await);
		assert!(!has_acao(&layer, "https://example.org").await);
	}

	#[tokio::test]
	async fn prefix_match() {
		let layer = build_cors_layer(&["https://dev-*".into()], 3600).unwrap();
		assert!(has_acao(&layer, "https://dev-01.example.com").await);
		assert!(!has_acao(&layer, "https://prod-01.example.com").await);
	}

	#[tokio::test]
	async fn regex_match() {
		let layer = build_cors_layer(&["/^https://(foo|bar)\\.example\\.com$/".into()], 3600).unwrap();
		assert!(has_acao(&layer, "https://foo.example.com").await);
		assert!(has_acao(&layer, "https://bar.example.com").await);
		assert!(!has_acao(&layer, "https://baz.example.com").await);
	}

	async fn preflight_max_age(layer: &CorsLayer, origin: &str) -> Option<String> {
		let app = Router::new().route("/", get(|| async { "ok" })).layer(layer.clone());

		let req = Request::builder()
			.method("OPTIONS")
			.uri("/")
			.header(header::ORIGIN, origin)
			.header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
			.body(Body::empty())
			.unwrap();

		let resp = app.oneshot(req).await.unwrap();
		resp
			.headers()
			.get(header::ACCESS_CONTROL_MAX_AGE)
			.and_then(|h| h.to_str().ok())
			.map(|s| s.to_string())
	}

	#[tokio::test]
	async fn max_age_is_set_on_preflight() {
		let layer = build_cors_layer(&["*".into()], 7200).unwrap();
		let value = preflight_max_age(&layer, "https://example.test").await;
		assert_eq!(value.as_deref(), Some("7200"));
	}

	#[tokio::test]
	async fn max_age_reflects_input_value() {
		let layer_short = build_cors_layer(&["*".into()], 10).unwrap();
		let layer_long = build_cors_layer(&["*".into()], 999).unwrap();

		let v_short = preflight_max_age(&layer_short, "https://example.test").await;
		let v_long = preflight_max_age(&layer_long, "https://example.test").await;

		assert_eq!(v_short.as_deref(), Some("10"));
		assert_eq!(v_long.as_deref(), Some("999"));
	}
}
