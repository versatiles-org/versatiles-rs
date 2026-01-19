//! `Tile` is a small, lazy container for raster/vector map tiles.
//!
//! It can hold either:
//! - a raw encoded **blob** (optionally compressed), or
//! - a decoded **content** representation (raster `DynamicImage` or vector `VectorTile`),
//!   and transparently materializes the other representation on demand.
//!
//! The type keeps track of the tile **format** (e.g. `PNG`, `MVT`) and the transport
//! **compression** (e.g. `Gzip`, `Uncompressed`). Quality and speed hints are stored
//! for formats that support them and are applied when (re-)encoding.
//!
//! All expensive conversions are performed lazily and are wrapped with contextual error
//! messages via the `#[context(...)]` attribute from `versatiles_derive`.
//!
//! Typical usage:
//! - Build from an image/vector and retrieve a blob ready to send over the wire.
//! - Build from a blob received from storage, then inspect or mutate the decoded content.

mod accessors;
mod cache;
mod constructors;
mod conversion;
mod transparency;

#[cfg(test)]
mod tests;

use std::fmt::Debug;
use versatiles_core::{Blob, TileCompression, TileFormat};

use crate::TileContent;

/// A lazy tile container that can hold either an encoded blob or decoded content.
///
/// The `Tile` ensures only the necessary representation is kept in memory:
/// when you request a blob, content is encoded on demand; when you request content,
/// a present blob is decoded on demand. Changing the content invalidates the blob,
/// but format/compression flags are preserved until re-materialization.
///
/// # Examples
/// Creating a raster tile and getting an uncompressed blob:
/// ```no_run
/// use versatiles_container::Tile;
/// use versatiles_core::{TileCompression::Uncompressed, TileFormat::PNG};
/// # let img = versatiles_image::DynamicImage::new_rgb8(1,1);
/// let mut tile = Tile::from_image(img, PNG).expect("raster tile");
/// let blob = tile.as_blob(Uncompressed).expect("to blob");
/// assert!(!blob.is_empty());
/// ```
///
/// Creating a vector tile and accessing its decoded data:
/// ```no_run
/// use versatiles_container::Tile;
/// use versatiles_core::TileFormat::MVT;
/// let vt = versatiles_geometry::vector_tile::VectorTile::default();
/// let mut tile = Tile::from_vector(vt, MVT).expect("vector tile");
/// let _vref = tile.as_vector().expect("decoded vector");
/// ```
#[derive(Clone, PartialEq)]
pub struct Tile {
	pub(super) blob: Option<Blob>,
	pub(super) content: Option<TileContent>,
	pub(super) format: TileFormat,
	pub(super) compression: TileCompression,
	pub(super) format_quality: Option<u8>,
	pub(super) format_speed: Option<u8>,
	/// Cached transparency info: (is_empty, is_opaque).
	/// Computed lazily and invalidated when content changes.
	pub(super) transparency_cache: Option<(bool, bool)>,
}

impl Debug for Tile {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Tile")
			.field("has_blob", &self.has_blob())
			.field("has_content", &self.has_content())
			.field("format", &self.format)
			.field("compression", &self.compression)
			.finish()
	}
}
