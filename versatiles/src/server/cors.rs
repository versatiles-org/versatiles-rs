//! CORS configuration helpers.
//!
//! Supported `allowed_origins` patterns (strings), preferred forms first:
//! - `"*"` → allow all origins
//! - `"https://maps.example.org"` → exact origin
//! - `"https://*.example.org"` → any **subdomain**, matched on host labels
//! - `"https://maps.example.org:*"` → that host, any port
//! - `"null"` → the null origin (sandboxed iframes, `file://`)
//! - `"/^https://(foo|bar)\.example\.com$/"` → regex (leading and trailing `/`)
//!
//! Two older glob forms are still accepted and still mean exactly what they
//! always did, but they match on raw string edges rather than on the structure
//! of an origin, and both are anchored at one end only:
//!
//! - `"prefix*"` is a `starts_with` test, so `"https://example.com*"` also
//!   allows `https://example.com.attacker.test`;
//! - `"*suffix"` is an `ends_with` test, so `"*example.com"` also allows
//!   `https://notexample.com` — the dot in `"*.example.com"` is what makes that
//!   form mean what it looks like.
//!
//! Each of those logs a warning at startup naming the origins it lets through,
//! because the failure is otherwise silent. `https://*.example.org` and
//! `https://host:*` express the two things people reach for them for — any
//! subdomain, any port — without the open end.
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

/// An origin as RFC 6454 serialises it: scheme, host and port, nothing else.
#[derive(Debug, PartialEq, Eq)]
struct Origin {
	scheme: String,
	host: String,
	/// The scheme's default when the origin does not spell one out.
	port: Option<u16>,
}

/// Parse an `Origin` header value, or `None` if it is not one.
///
/// Anything carrying a path, query, fragment or space is rejected rather than
/// trimmed: a browser never sends those, so a value containing one is either
/// a hand-made request — which CORS does not protect anyone from anyway — or an
/// attempt to make a string test read the wrong part of the value.
fn parse_origin(value: &str) -> Option<Origin> {
	let (scheme, rest) = value.split_once("://")?;
	if scheme.is_empty() || rest.is_empty() || rest.contains(['/', '?', '#', ' ']) {
		return None;
	}

	// A port is digits after the last colon; anything else belongs to the host,
	// which keeps `[::1]` in one piece.
	let (host, port) = match rest.rsplit_once(':') {
		Some((host, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) => {
			(host, Some(port.parse().ok()?))
		}
		_ => (rest, None),
	};
	if host.is_empty() {
		return None;
	}

	let scheme = scheme.to_ascii_lowercase();
	let mut host = host.to_ascii_lowercase();
	// `example.com.` names the same host as `example.com`.
	if host.len() > 1 && host.ends_with('.') {
		host.pop();
	}
	let port = port.or_else(|| default_port(&scheme));

	Some(Origin { scheme, host, port })
}

/// The port a scheme implies when an origin omits it.
fn default_port(scheme: &str) -> Option<u16> {
	match scheme {
		"http" | "ws" => Some(80),
		"https" | "wss" => Some(443),
		_ => None,
	}
}

/// How a rule matches the host part of an origin.
#[derive(Debug)]
enum HostMatch {
	/// Exactly this host.
	Exact(String),
	/// Any host under this suffix, which includes the leading dot. The apex
	/// itself does not match — `*.example.com` is subdomains, not `example.com`.
	SubdomainOf(String),
}

/// How a rule matches the port part of an origin.
#[derive(Debug)]
enum PortMatch {
	Any,
	Exactly(Option<u16>),
}

/// A pattern matched against the parts of an origin rather than its spelling.
#[derive(Debug)]
struct OriginRule {
	scheme: String,
	host: HostMatch,
	port: PortMatch,
}

impl OriginRule {
	/// Recognise `scheme://*.host`, `scheme://host:*` and combinations.
	///
	/// Returns `None` for anything without a wildcard in the host-label or port
	/// position, which leaves every pre-existing pattern on the code path it has
	/// always taken.
	fn parse(pattern: &str) -> Option<Self> {
		let (scheme, rest) = pattern.split_once("://")?;
		if scheme.is_empty() || scheme.contains('*') || rest.contains(['/', '?', '#', ' ']) {
			return None;
		}

		let (rest, port) = if let Some(rest) = rest.strip_suffix(":*") {
			(rest, PortMatch::Any)
		} else if let Some((host, port)) = rest.rsplit_once(':')
			&& !port.is_empty()
			&& port.bytes().all(|b| b.is_ascii_digit())
		{
			(host, PortMatch::Exactly(Some(port.parse().ok()?)))
		} else {
			(rest, PortMatch::Exactly(default_port(&scheme.to_ascii_lowercase())))
		};

		let host = match rest.strip_prefix("*.") {
			// `*.example.com` keeps the dot: a suffix match on labels, so
			// `notexample.com` cannot satisfy it.
			Some(suffix) if !suffix.is_empty() && !suffix.contains('*') => {
				HostMatch::SubdomainOf(format!(".{}", suffix.to_ascii_lowercase()))
			}
			Some(_) => return None,
			None => {
				if rest.contains('*') || rest.is_empty() {
					return None;
				}
				// No wildcard anywhere: leave it to the exact-match path.
				if matches!(port, PortMatch::Exactly(_)) {
					return None;
				}
				HostMatch::Exact(rest.to_ascii_lowercase())
			}
		};

		Some(OriginRule {
			scheme: scheme.to_ascii_lowercase(),
			host,
			port,
		})
	}

	fn matches(&self, value: &str) -> bool {
		let Some(origin) = parse_origin(value) else {
			return false;
		};
		if origin.scheme != self.scheme {
			return false;
		}
		let host_ok = match &self.host {
			HostMatch::Exact(host) => &origin.host == host,
			HostMatch::SubdomainOf(suffix) => origin.host.ends_with(suffix.as_str()),
		};
		host_ok
			&& match self.port {
				PortMatch::Any => true,
				PortMatch::Exactly(port) => origin.port == port,
			}
	}
}

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
			} else if let Some(rule) = OriginRule::parse(pattern) {
				// "scheme://*.host", "scheme://host:*" → matched on the parts of
				// an origin, so neither end is left open.
				Box::new(move |origin: &str| rule.matches(origin))
			} else if Regex::new(r"^\*[^*]+$")?.is_match(pattern) {
				// "*suffix" → suffix match
				let suffix = pattern[1..].to_string();
				if !suffix.starts_with('.') {
					log::warn!(
						"CORS: the pattern {pattern:?} matches any origin ending in {suffix:?}, including \
						 https://not{suffix} — write \"*.{suffix}\" for subdomains only"
					);
				}
				Box::new(move |origin: &str| origin.ends_with(&suffix))
			} else if Regex::new(r"^[^*]+\*$")?.is_match(pattern) {
				// "prefix*" → prefix match
				let prefix = pattern[..pattern.len() - 1].to_string();
				log::warn!(
					"CORS: the pattern {pattern:?} matches any origin starting with {prefix:?}, including \
					 {prefix}.attacker.test — write \"{prefix}:*\" for any port, or \"scheme://*.host\" for \
					 any subdomain"
				);
				Box::new(move |origin: &str| origin.starts_with(&prefix))
			} else if Regex::new(r"^/.+/$")?.is_match(pattern) {
				// "/regex/" → full regex (strip slashes)
				let source = &pattern[1..pattern.len() - 1];
				if !(source.starts_with('^') && source.ends_with('$')) {
					log::warn!(
						"CORS: the regex {source:?} is not anchored, so it matches any origin *containing* it. \
						 Wrap it in ^...$ unless that is what you meant"
					);
				}
				let re = Regex::new(source)?;
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
	use axum::{
		Router,
		body::Body,
		http::{Request, header},
		routing::get,
	};
	use tower::ServiceExt;

	use super::*; // for `oneshot`

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

	/// `scheme://*.host` matches subdomains on label boundaries — the thing the
	/// suffix glob only approximates.
	#[tokio::test]
	async fn subdomain_form_matches_labels() {
		let layer = build_cors_layer(&["https://*.example.org".into()], 3600).unwrap();

		assert!(has_acao(&layer, "https://maps.example.org").await);
		assert!(has_acao(&layer, "https://a.b.example.org").await);
		// Explicit default port is the same origin.
		assert!(has_acao(&layer, "https://maps.example.org:443").await);

		// The apex is not a subdomain of itself.
		assert!(!has_acao(&layer, "https://example.org").await);
		// The bypass the suffix glob allows.
		assert!(!has_acao(&layer, "https://notexample.org").await);
		assert!(!has_acao(&layer, "https://example.org.attacker.test").await);
		// The scheme is part of an origin, so a downgrade does not match.
		assert!(!has_acao(&layer, "http://maps.example.org").await);
		// A non-default port is a different origin.
		assert!(!has_acao(&layer, "https://maps.example.org:8443").await);
	}

	/// `scheme://host:*` is the safe way to say "any port" — the reason people
	/// reach for a trailing glob.
	#[tokio::test]
	async fn any_port_form_is_bounded() {
		let layer = build_cors_layer(&["https://maps.example.org:*".into()], 3600).unwrap();

		assert!(has_acao(&layer, "https://maps.example.org").await);
		assert!(has_acao(&layer, "https://maps.example.org:8443").await);

		// What "https://maps.example.org*" would have allowed:
		assert!(!has_acao(&layer, "https://maps.example.org.attacker.test").await);
		assert!(!has_acao(&layer, "http://maps.example.org:8443").await);
	}

	/// The two wildcards combine.
	#[tokio::test]
	async fn subdomain_and_port_combine() {
		let layer = build_cors_layer(&["https://*.example.org:*".into()], 3600).unwrap();
		assert!(has_acao(&layer, "https://maps.example.org:8443").await);
		assert!(!has_acao(&layer, "https://example.org:8443").await);
		assert!(!has_acao(&layer, "https://notexample.org:8443").await);
	}

	/// The null origin is matched by naming it, and by nothing else.
	#[tokio::test]
	async fn null_origin_needs_naming() {
		let named = build_cors_layer(&["null".into()], 3600).unwrap();
		assert!(has_acao(&named, "null").await);

		let subdomains = build_cors_layer(&["https://*.example.org".into()], 3600).unwrap();
		assert!(!has_acao(&subdomains, "null").await);
	}

	/// A value that is not a serialised origin never satisfies a structural
	/// rule, whatever it contains.
	#[tokio::test]
	async fn malformed_origins_do_not_match() {
		let layer = build_cors_layer(&["https://*.example.org".into()], 3600).unwrap();
		for value in [
			"https://maps.example.org/path",
			"https://maps.example.org ",
			"https://maps.example.org#x",
			"maps.example.org",
			"https://",
		] {
			assert!(!has_acao(&layer, value).await, "{value} matched");
		}
	}

	#[test]
	fn origins_parse_into_their_parts() {
		assert_eq!(
			parse_origin("https://Example.ORG."),
			Some(Origin {
				scheme: "https".into(),
				host: "example.org".into(),
				port: Some(443),
			})
		);
		assert_eq!(
			parse_origin("http://[::1]:8080"),
			Some(Origin {
				scheme: "http".into(),
				host: "[::1]".into(),
				port: Some(8080),
			})
		);
		assert_eq!(
			parse_origin("http://[::1]"),
			Some(Origin {
				scheme: "http".into(),
				host: "[::1]".into(),
				port: Some(80),
			})
		);
		assert_eq!(parse_origin("https://example.org/path"), None);
		assert_eq!(parse_origin("null"), None);
	}

	/// The older forms keep working exactly as before — including the loose ends,
	/// which is the point of not breaking them yet.
	#[tokio::test]
	async fn the_legacy_globs_are_unchanged() {
		let prefix = build_cors_layer(&["https://dev-*".into()], 3600).unwrap();
		assert!(has_acao(&prefix, "https://dev-01.example.com").await);

		let suffix = build_cors_layer(&["*example.com".into()], 3600).unwrap();
		assert!(has_acao(&suffix, "https://foo.example.com").await);
		assert!(has_acao(&suffix, "https://notexample.com").await);
	}

	/// The globs are anchored at one end only. This is the documented behaviour
	/// — `https://dev-*` matching `https://dev-01.example.com` is the point —
	/// but it also means a pattern meant to name one host allows anything
	/// extending it. Pinned here so the trade-off is visible rather than
	/// discovered.
	#[tokio::test]
	async fn a_glob_is_anchored_at_one_end_only() {
		let prefix = build_cors_layer(&["https://example.com*".into()], 3600).unwrap();
		assert!(has_acao(&prefix, "https://example.com").await);
		assert!(has_acao(&prefix, "https://example.com.attacker.test").await);

		let suffix = build_cors_layer(&["*example.com".into()], 3600).unwrap();
		assert!(has_acao(&suffix, "https://foo.example.com").await);
		assert!(has_acao(&suffix, "https://notexample.com").await);

		// Written with the dot, the suffix form means what it looks like.
		let dotted = build_cors_layer(&["*.example.com".into()], 3600).unwrap();
		assert!(has_acao(&dotted, "https://foo.example.com").await);
		assert!(!has_acao(&dotted, "https://notexample.com").await);
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
			.map(std::string::ToString::to_string)
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
