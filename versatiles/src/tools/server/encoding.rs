//! Accept-Encoding parsing and mapping to `TargetCompression`.
//!
//! We parse the HTTP `Accept-Encoding` header conservatively:
//! - `identity` (uncompressed) is allowed unless explicitly sent with `q=0`.
//! - Recognized encodings: `gzip`, `br` (Brotli). Others are ignored.
//! - `*` enables a conservative set (gzip + br) with `q>0`.
//!
//! Note: We **do not** implement full RFC 9110 quality-factor selection;
//! we only check `q=0` vs `q>0` to decide allowance. That keeps the
//! negotiation stable and predictable for our limited set of encodings.

use axum::http::{HeaderMap, header};
use versatiles_core::{TileCompression, utils::TargetCompression};

/// Parse `Accept-Encoding` and return the set of allowed compressions.
///
/// Identity (uncompressed) is considered allowed unless explicitly disabled via `q=0`.
pub(crate) fn get_encoding(headers: HeaderMap) -> TargetCompression {
	use TileCompression::*;
	let mut set = TargetCompression::from_none();

	let Some(val) = headers.get(header::ACCEPT_ENCODING) else {
		set.insert(Uncompressed);
		return set;
	};
	let s = val.to_str().unwrap_or("");

	// Parse tokens of the form "token[;q=val]".
	// We only differentiate q=0 (disallow) vs q>0 (allow).
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
				if let Some(rest) = p.strip_prefix("q=") {
					if let Ok(v) = rest.trim().parse::<f32>() {
						q = v;
					}
				}
			}
		}

		tokens.push((name, q));
	}

	// Identity is allowed unless explicitly disabled.
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

#[cfg(test)]
mod tests {
	use super::*;
	use axum::http::{HeaderMap, header::ACCEPT_ENCODING};
	use enumset::{EnumSet, enum_set};
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
		let got = get_encoding(headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed)));
	}

	#[test]
	fn gzip_and_identity() {
		let headers = mk_headers("gzip");
		let got = get_encoding(headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip)));
	}

	#[test]
	fn brotli_and_identity() {
		let headers = mk_headers("br");
		let got = get_encoding(headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Brotli)));
	}

	#[test]
	fn wildcard_enables_common() {
		let headers = mk_headers("*;q=0.8");
		let got = get_encoding(headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip | TC::Brotli)));
	}

	#[test]
	fn identity_disabled() {
		let headers = mk_headers("identity;q=0, gzip");
		let got = get_encoding(headers);
		assert_eq!(got, to_target(enum_set!(TC::Gzip)));
	}

	#[test]
	fn q_zero_disallows() {
		let headers = mk_headers("br;q=0, gzip;q=1");
		let got = get_encoding(headers);
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip)));
	}

	#[test]
	fn complex_header() {
		let headers = mk_headers("gzip, deflate, br;q=1.0, identity;q=0.5, *;q=0.25");
		let got = get_encoding(headers);
		// deflate ignored; identity allowed (q=0.5), gzip + br allowed; * adds nothing new
		assert_eq!(got, to_target(enum_set!(TC::Uncompressed | TC::Gzip | TC::Brotli)));
	}
}
