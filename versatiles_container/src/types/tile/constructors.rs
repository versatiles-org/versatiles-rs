//! Tile constructors for creating tiles from blobs, images, or vector data.

use super::Tile;
use crate::TileContent;
use anyhow::{Result, ensure};
use versatiles_core::{Blob, TileCompression, TileFormat};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::VectorTile;
use versatiles_image::DynamicImage;

impl Tile {
	/// Construct a `Tile` from an already encoded `blob`.
	///
	/// The provided `compression` describes the outer transport compression of the blob
	/// (e.g., `Gzip` or `Uncompressed`), and `format` describes the inner tile format
	/// (e.g., `PNG`, `WEBP`, `MVT`).
	///
	/// This does **not** decode the blob. Decoding happens lazily when content is requested.
	#[must_use]
	pub fn from_blob(blob: Blob, compression: TileCompression, format: TileFormat) -> Self {
		Self {
			blob: Some(blob),
			content: None,
			format,
			compression,
			format_quality: None,
			format_speed: None,
		}
	}

	pub(super) fn from_content(content: TileContent, format: TileFormat) -> Self {
		Self {
			blob: None,
			content: Some(content),
			format,
			compression: TileCompression::Uncompressed,
			format_quality: None,
			format_speed: None,
		}
	}

	/// Construct a raster `Tile` from a `DynamicImage`.
	///
	/// The `format` must be a raster format (e.g., `PNG`, `WEBP`). The tile starts with
	/// decoded content and no blob; a blob is created on first call to `as_blob`/`into_blob`.
	#[must_use = "this returns the new Tile, it doesn't modify anything"]
	#[context("creating raster tile (format={:?})", format)]
	pub fn from_image(image: DynamicImage, format: TileFormat) -> Result<Self> {
		ensure!(format.to_type().is_raster());
		Ok(Self::from_content(TileContent::from_image(image), format))
	}

	/// Construct a vector `Tile` from a `VectorTile`.
	///
	/// The `format` must be a vector format (e.g., `MVT`). The tile starts with decoded content.
	#[must_use = "this returns the new Tile, it doesn't modify anything"]
	#[context("creating vector tile (format={:?})", format)]
	pub fn from_vector(vector_tile: VectorTile, format: TileFormat) -> Result<Self> {
		ensure!(format.to_type().is_vector());
		Ok(Self::from_content(TileContent::from_vector(vector_tile), format))
	}
}
