//! This module defines metadata describing tile source output characteristics.

use crate::Traversal;
use versatiles_core::{TileBBoxPyramid, TileCompression, TileFormat, TileJSON, TileSchema, TileType};

/// Metadata describing the output characteristics of a tile source.
///
/// # Fields
/// - `bbox_pyramid`: The bounding box and zoom pyramid defining the tile coverage.
/// - `tile_compression`: The compression algorithm applied to tiles (e.g., gzip, brotli).
/// - `tile_format`: The format of the tiles (e.g., PNG, JPEG, PBF).
#[derive(Debug, Default, PartialEq, Clone)]
pub struct TileSourceMetadata {
	/// The bounding box and zoom pyramid defining the tile coverage.
	pub bbox_pyramid: TileBBoxPyramid,
	/// The compression algorithm applied to tiles (e.g., gzip, brotli).
	pub tile_compression: TileCompression,
	/// The format of the tiles (e.g., PNG, JPEG, PBF).
	pub tile_format: TileFormat,

	pub traversal: Traversal,
}

impl TileSourceMetadata {
	/// Create a new `TileSourceMetadata`.
	///
	/// # Arguments
	/// * `tile_format` - The format of the tiles.
	/// * `tile_compression` - The compression algorithm applied to tiles.
	/// * `bbox_pyramid` - The bounding box and zoom pyramid defining the tile coverage.
	///
	/// # Returns
	/// A new instance of `TileSourceMetadata` configured with the specified parameters.
	#[must_use]
	pub fn new(
		tile_format: TileFormat,
		tile_compression: TileCompression,
		bbox_pyramid: TileBBoxPyramid,
		traversal: Traversal,
	) -> TileSourceMetadata {
		TileSourceMetadata {
			bbox_pyramid,
			tile_compression,
			tile_format,
			traversal,
		}
	}

	#[cfg(test)]
	#[allow(dead_code)]
	/// Creates a `TileSourceMetadata` with a default full pyramid up to zoom level 31 for testing purposes.
	#[must_use]
	pub fn new_full(
		tile_format: TileFormat,
		tile_compression: TileCompression,
		traversal: Traversal,
	) -> TileSourceMetadata {
		TileSourceMetadata {
			tile_format,
			tile_compression,
			bbox_pyramid: TileBBoxPyramid::new_full(),
			traversal,
		}
	}

	/// Updates fields using information from [`TileSourceMetadata`].
	///
	/// - Applies [`TileJSON::update_from_pyramid`] to intersect/set bounds and min/max zoom.
	/// - Sets `tile_format` from the reader parameters and derives `tile_type` from it.
	/// - If `tile_schema` is absent or mismatched with `tile_type`, infers a suitable schema
	///   (e.g., `RasterRGB` for rasters; for vectors, derived from `vector_layers`).
	pub fn update_tilejson(&self, tile_json: &mut TileJSON) {
		tile_json.update_from_pyramid(&self.bbox_pyramid);

		tile_json.tile_format = Some(self.tile_format);

		tile_json.tile_type = tile_json.tile_format.map(|f| f.to_type());

		if let Some(tile_type) = tile_json.tile_type
			&& tile_json.tile_schema.map(|s| s.tile_type()) != tile_json.tile_type
		{
			tile_json.tile_schema = Some(match tile_type {
				TileType::Raster => TileSchema::RasterRGB,
				TileType::Vector => tile_json.vector_layers.get_tile_schema(),
				TileType::Unknown => TileSchema::Unknown,
			});
		}
	}
}

#[cfg(test)]
mod tests {
	use anyhow::Result;
	use versatiles_core::GeoBBox;

	use super::*;

	#[test]
	fn test_tiles_reader_parameters_new() {
		let bbox_pyramid = TileBBoxPyramid::new_full_up_to(10);
		let tile_format = TileFormat::PNG;
		let tile_compression = TileCompression::Gzip;

		let params = TileSourceMetadata::new(tile_format, tile_compression, bbox_pyramid.clone(), Traversal::ANY);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.bbox_pyramid, bbox_pyramid);
	}

	#[test]
	fn test_tiles_reader_parameters_new_full() {
		let tile_format = TileFormat::JPG;
		let tile_compression = TileCompression::Gzip;

		let params = TileSourceMetadata::new_full(tile_format, tile_compression, Traversal::ANY);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.bbox_pyramid, TileBBoxPyramid::new_full());
	}

	#[test]
	fn should_update_tile_json() -> Result<()> {
		let mut tj = TileJSON::default();
		// Prepare reader parameters
		let bbox_pyramid = TileBBoxPyramid::from_geo_bbox(1, 4, &GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap());
		let rp = TileSourceMetadata {
			bbox_pyramid,
			tile_format: TileFormat::PNG,
			..Default::default()
		};
		rp.update_tilejson(&mut tj);
		// Bounds and zooms
		assert_eq!(tj.min_zoom(), Some(1));
		assert_eq!(tj.max_zoom(), Some(4));
		// Format, content, and schema
		assert_eq!(tj.tile_format, Some(TileFormat::PNG));
		assert_eq!(tj.tile_type, Some(TileType::Raster));
		assert_eq!(tj.tile_schema, Some(TileSchema::RasterRGB));
		Ok(())
	}
}
