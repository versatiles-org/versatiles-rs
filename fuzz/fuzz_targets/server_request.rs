//! Pass an arbitrary request path and `Accept-Encoding` header through every
//! function a request reaches before any data is read.
//!
//! The other targets cover what the server reads from files. This one covers
//! what it reads from a request, which is the input an attacker controls. Each
//! of these functions has had fixes for cases nobody thought of at first — dot
//! segments, `%2e%2e`, `%2F`, `\` and `C:` inside a segment, a scheme turning
//! the remote folder into a fetch of another host, leading zeros in tile
//! paths — so the target asserts the property each one is responsible for,
//! not only that it does not panic.
//!
//! The input is up to three lines, so seeds stay readable:
//!
//! ```text
//! <path>
//! <Accept-Encoding bytes>
//! <base URL of a remote folder>
//! ```
//!
//! A missing second line sends no header; a missing third line uses
//! `https://example.com/assets/`.

#![no_main]

use std::path::{Component, Path};

use http::{HeaderMap, HeaderValue, header::ACCEPT_ENCODING};
use libfuzzer_sys::fuzz_target;
use versatiles::server::{
	Url,
	encoding::get_encoding,
	fuzzing::{RemoteUrl, parse_tile_coord, remote_folder_target, tar_resolve_path},
};
use versatiles_core::TileCompression;

fuzz_target!(|data: &[u8]| {
	let mut lines = data.splitn(3, |&byte| byte == b'\n');
	let Ok(path) = std::str::from_utf8(lines.next().unwrap_or_default()) else {
		return;
	};
	let accept_encoding = lines.next();
	let Ok(base) = std::str::from_utf8(lines.next().unwrap_or(b"https://example.com/assets/")) else {
		return;
	};

	let url = Url::from(path);
	check_url(&url);
	check_tile_coord(&url);
	check_encoding(accept_encoding);
	check_remote_folder(base, &url);
	check_tar_path(path);
});

/// `Url::new` leaves no dot or empty segment, and normalising is idempotent.
/// `decoded_segments` and `to_pathbug` stay inside their segments and base.
fn check_url(url: &Url) {
	let text = url.str.as_str();
	assert!(text.starts_with('/'), "{text:?} does not start with '/'");

	// A trailing slash is kept on purpose: `is_dir` reads it.
	let inner = &text[1..];
	let inner = inner.strip_suffix('/').unwrap_or(inner);
	if !inner.is_empty() {
		for segment in inner.split('/') {
			assert!(!matches!(segment, "" | "." | ".."), "{text:?} kept segment {segment:?}");
		}
	}

	assert_eq!(&Url::from(text), url, "normalising {text:?} again changed it");

	if let Some(segments) = url.decoded_segments() {
		for segment in segments {
			assert!(
				!matches!(segment.as_str(), "" | "." | "..") && !segment.contains(['/', '\\', '\0']),
				"{text:?} decoded to segment {segment:?}"
			);
		}
	}

	for base in [Path::new("/srv/www"), Path::new("relative/dir")] {
		let path = url.to_pathbug(base);
		let rest = path
			.strip_prefix(base)
			.unwrap_or_else(|_| panic!("{text:?} left {base:?}: {path:?}"));
		for component in rest.components() {
			assert!(
				matches!(component, Component::Normal(_)),
				"{text:?} produced {component:?} under {base:?}"
			);
		}
	}
}

/// One URL per tile: an accepted `z/x/y[.ext]` prints back as its input, so
/// no leading zero, sign or trailing junk gets through.
fn check_tile_coord(url: &Url) {
	// Split the way `ServerTileSource::get_data` does, empty segments included.
	let segments: Vec<&str> = url.str.trim_start_matches('/').split('/').collect();
	if let Ok(coord) = parse_tile_coord(&segments) {
		let y = segments[2].split_once('.').map_or(segments[2], |(y, _)| y);
		assert_eq!(
			format!("{}/{}/{}", coord.level, coord.x, coord.y),
			format!("{}/{}/{y}", segments[0], segments[1]),
			"{} parsed as a different tile",
			url.str
		);
	}
}

/// Any header bytes, and uncompressed is always allowed.
fn check_encoding(accept_encoding: Option<&[u8]>) {
	let mut headers = HeaderMap::new();
	if let Some(bytes) = accept_encoding {
		let Ok(value) = HeaderValue::from_bytes(bytes) else {
			return;
		};
		headers.insert(ACCEPT_ENCODING, value);
	}
	assert!(get_encoding(&headers).contains(TileCompression::Uncompressed));
}

/// A remote folder only ever fetches below its base: same scheme, host and
/// port, and a path under the base path.
fn check_remote_folder(base: &str, url: &Url) {
	let Ok(base) = RemoteUrl::parse(base) else {
		return;
	};
	let (base, target) = remote_folder_target(&base, url);
	let Some(target) = target else {
		return;
	};
	assert_eq!(
		target.scheme(),
		base.scheme(),
		"{} changed the scheme: {target}",
		url.str
	);
	assert_eq!(
		target.host_str(),
		base.host_str(),
		"{} changed the host: {target}",
		url.str
	);
	assert_eq!(
		target.port_or_known_default(),
		base.port_or_known_default(),
		"{} changed the port: {target}",
		url.str
	);
	assert!(
		target.path().starts_with(base.path()),
		"{} escaped {base}: {target}",
		url.str
	);
}

/// Tar entry names, not request paths, but the same kind of input: a name
/// resolves to plain segments or to nothing, never above the archive root.
fn check_tar_path(path: &str) {
	for base in ["", "dir/sub"] {
		if let Some(name) = tar_resolve_path(base, Path::new(path))
			&& !name.is_empty()
		{
			for segment in name.split('/') {
				assert!(
					!matches!(segment, "" | "." | ".."),
					"{path:?} under {base:?} resolved to {name:?}"
				);
			}
		}
	}
}
