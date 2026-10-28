//! The private functions a request passes through, for the `server_request`
//! fuzz target.
//!
//! Compiled only under `cargo fuzz`, which sets `--cfg fuzzing` on its own, so
//! neither a normal build nor the public API sees it.

use std::path::Path;

use anyhow::Result;
pub use reqwest::Url as RemoteUrl;
use versatiles_core::TileCoord;

use super::{Url, sources};

/// Parses the segments after a tile source's prefix as `z/x/y[.ext]`.
///
/// # Errors
/// When the segments are not a tile path.
pub fn parse_tile_coord(segments: &[&str]) -> Result<TileCoord> {
	sources::parse_tile_coord(segments)
}

/// The base URL a remote folder configured with `base` uses, and the URL it
/// fetches for a request to `url`.
#[must_use]
pub fn remote_folder_target(base: &RemoteUrl, url: &Url) -> (RemoteUrl, Option<RemoteUrl>) {
	let folder = sources::RemoteFolder::from(base);
	let target = folder.target_url(url);
	(folder.base_url, target)
}

/// Resolves the entry name `path` in a tar archive, relative to the archive
/// directory `base`.
#[must_use]
pub fn tar_resolve_path(base: &str, path: &Path) -> Option<String> {
	sources::resolve_path(base, path)
}
