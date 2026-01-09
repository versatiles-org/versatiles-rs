//! Parsing of the HTTP `Accept-Encoding` header into our internal compression set.
//!
//! ### Design goals
//! - **Conservative & predictable.** We only decide whether an encoding is *allowed*,
//!   not which is “best”. That choice is made later by the compression optimizer.
//! - **Identity is safe.** The uncompressed representation (`identity`) remains allowed
//!   unless explicitly disabled with `q=0` (RFC 9110 §12.5.3).
//! - **Narrow scope.** We recognize `gzip` and `br` (Brotli). Unknown tokens are ignored.
//!   A wildcard `*` enables a conservative set (gzip + br) when `q>0`.
//!
//! ### Why not full quality-factor selection?
//! RFC 9110 allows clients to rank encodings with `q` values (e.g. `gzip;q=0.8, br;q=1`).
//! For our use-case the *optimizer* later considers both source format and cost/ratio trade-offs.
//! Here we only gate what’s permitted. This keeps negotiation simple and avoids surprising
//! flips when clients send exotic rankings.
//!
//! ### Examples
//! ```rust
//! use axum::http::{HeaderMap, header};
//! use versatiles_core::{utils::TargetCompression, TileCompression as TC};
//! use enumset::{enum_set, EnumSet};
//! use versatiles::server::encoding::get_encoding;
//!
//! // No header → identity is allowed.
//! let mut h = HeaderMap::new();
//! let set = get_encoding(&h);
//! assert_eq!(set, TargetCompression::from_set(enum_set!(TC::Uncompressed)));
//!
//! // Gzip requested → identity + gzip allowed.
//! h.insert(header::ACCEPT_ENCODING, "gzip".parse().unwrap());
//! let set = get_encoding(&h);
//! assert_eq!(set, TargetCompression::from_set(enum_set!(TC::Uncompressed | TC::Gzip)));
//!
//! // Explicitly disable identity.
//! let mut h = HeaderMap::new();
//! h.insert(header::ACCEPT_ENCODING, "identity;q=0, br".parse().unwrap());
//! let set = get_encoding(&h);
//! assert_eq!(set, TargetCompression::from_set(enum_set!(TC::Uncompressed | TC::Brotli)));
//! ```
//!
//! ### Notes
//! - We treat header parsing failures as empty/absent headers (fail-open to identity).
//! - This module is intentionally tiny; tests cover a matrix of realistic client headers.

use axum::http::{HeaderMap, header};
use versatiles_core::{TileCompression, utils::TargetCompression};

/// Convert `Accept-Encoding` into a set of **allowed** encodings.
///
/// Behavior:
/// - Returns a `TargetCompression` bitset where each bit (gzip, br, identity)
///   indicates it is permitted by the client.
/// - `identity` is included unless explicitly disabled with `q=0`.
/// - Unknown tokens are ignored; a wildcard `*` includes gzip and br if `q>0`.
///
/// Robustness:
/// - If the header is missing or invalid UTF‑8, we allow `identity` only.
/// - If `q` cannot be parsed, it is treated as `1.0`.
///
/// This function does **not** pick the final encoding; it only gates options.
/// The compression optimizer (in `versatiles_core`) picks among allowed options.
pub fn get_encoding(headers: &HeaderMap) -> TargetCompression {
	use TileCompression::{Brotli, Gzip, Uncompressed};
	let mut set = TargetCompression::from_none();

	let Some(val) = headers.get(header::ACCEPT_ENCODING) else {
		set.insert(Uncompressed);
		return set;
	};
	let s = val.to_str().unwrap_or("");

	// Parse tokens of the form "token[;q=val]".
	// We only differentiate q=0 (disallow) vs q>0 (allow).
	// We only care about the on/off decision: `q=0` disables, any other value enables.
	let mut tokens: Vec<(&str, f32)> = Vec::new();
	for raw in s.split(',') {
		let token = raw.trim();
		if token.is_empty() {
			continue;
		}
		let mut name = token;
		let mut q = 1.0f32;

		if let Some((n, params)) = token.split_once(';') {
			name = n.trim();
			for p in params.split(';') {
				let p = p.trim();
				if let Some(rest) = p.strip_prefix("q=")
					&& let Ok(v) = rest.trim().parse::<f32>()
				{
					q = v;
				}
			}
		}

		tokens.push((name, q));
	}

	// Identity is allowed unless explicitly disabled.
	// This mirrors common server behavior and ensures a safe default for intermediaries.
	let identity_disabled = tokens.iter().any(|(n, q)| *n == "identity" && *q == 0.0);
	if !identity_disabled {
		set.insert(Uncompressed);
	}

	for (name, q) in tokens {
		if q <= 0.0 {
			continue;
		}
		match name {
			"gzip" => set.insert(Gzip),
			"br" => set.insert(Brotli),
			"*" => {
				// Be conservative with wildcard: allow our common encodings.
				set.insert(Gzip);
				set.insert(Brotli);
			}
			_ => {
				// Ignore unknown encodings.
			}
		}
	}

	set
}

// --- tests -------------------------------------------------------------------

#[cfg(test)]
mod tests {
	use super::*;
	use axum::http::{HeaderMap, header::ACCEPT_ENCODING};
	use enumset::{EnumSet, enum_set};
	use rstest::rstest;
	use versatiles_core::TileCompression as TC;

	fn mk_headers(s: &str) -> HeaderMap {
		let mut m = HeaderMap::new();
		if s != "NONE" {
			m.insert(ACCEPT_ENCODING, s.parse().unwrap());
		}
		m
	}

	fn to_target(set: EnumSet<TC>) -> TargetCompression {
		TargetCompression::from_set(set)
	}

	#[test]
	fn no_header_means_identity_allowed() {
		let headers = mk_headers("NONE");
		let got = get_encoding(&headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed)));
	}

	#[test]
	fn gzip_and_identity() {
		let headers = mk_headers("gzip");
		let got = get_encoding(&headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip)));
	}

	#[test]
	fn brotli_and_identity() {
		let headers = mk_headers("br");
		let got = get_encoding(&headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Brotli)));
	}

	#[test]
	fn wildcard_enables_common() {
		let headers = mk_headers("*;q=0.8");
		let got = get_encoding(&headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip | TC::Brotli)));
	}

	#[test]
	fn identity_disabled() {
		let headers = mk_headers("identity;q=0, gzip");
		let got = get_encoding(&headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip)));
	}

	#[test]
	fn q_zero_disallows() {
		let headers = mk_headers("br;q=0, gzip;q=1");
		let got = get_encoding(&headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip)));
	}

	#[test]
	fn complex_header() {
		let headers = mk_headers("gzip, deflate, br;q=1.0, identity;q=0.5, *;q=0.25");
		let got = get_encoding(&headers);
		// deflate ignored; identity allowed (q=0.5), gzip + br allowed; * adds nothing new
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip | TC::Brotli)));
	}

	#[rstest]
	#[case("NONE", enum_set!(TC::Uncompressed))]
	#[case("", enum_set!(TC::Uncompressed))]
	#[case("*", enum_set!(TC::Uncompressed | TC::Brotli | TC::Gzip))]
	#[case("br", enum_set!(TC::Uncompressed | TC::Brotli))]
	#[case("br;q=1.0, gzip;q=0.8, *;q=0.1", enum_set!(TC::Uncompressed | TC::Brotli | TC::Gzip))]
	#[case("compress", enum_set!(TC::Uncompressed))]
	#[case("compress, gzip", enum_set!(TC::Uncompressed | TC::Gzip))]
	#[case("compress;q=0.5, gzip;q=1.0", enum_set!(TC::Uncompressed | TC::Gzip))]
	#[case("deflate", enum_set!(TC::Uncompressed))]
	#[case("deflate, gzip;q=1.0;q=0.5", enum_set!(TC::Uncompressed | TC::Gzip))]
	#[case("gzip", enum_set!(TC::Uncompressed | TC::Gzip))]
	#[case("gzip, compress, br", enum_set!(TC::Uncompressed | TC::Brotli | TC::Gzip))]
	#[case(
		"gzip, deflate, br;q=1.0, identity;q=0.5, *;q=0.25",
		enum_set!(TC::Uncompressed | TC::Brotli | TC::Gzip),
	)]
	#[case("gzip;q=1.0, identity; q=0.5, *;q=0", enum_set!(TC::Uncompressed | TC::Gzip))]
	#[case("identity", enum_set!(TC::Uncompressed))]
	fn test_get_encoding(#[case] encoding: &str, #[case] comp0: EnumSet<TileCompression>) {
		let mut map = HeaderMap::new();
		if encoding != "NONE" {
			map.insert(ACCEPT_ENCODING, encoding.parse().unwrap());
		}
		let comp0 = TargetCompression::from_set(comp0);
		let comp = get_encoding(&map);
		assert_eq!(comp, comp0);
	}
}
