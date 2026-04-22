//! This module defines metadata describing tile source output characteristics.

use crate::Traversal;
use anyhow::Result;
use std::sync::{Arc, RwLock};
use versatiles_core::{TileBBox, TileCompression, TileFormat, TileJSON, TilePyramid, TileSchema, TileType};

/// Metadata describing the output characteristics of a tile source.
///
/// # Fields
/// - `tile_pyramid`: The bounding box and zoom pyramid defining the tile coverage.
/// - `tile_compression`: The compression algorithm applied to tiles (e.g., gzip, brotli).
/// - `tile_format`: The format of the tiles (e.g., PNG, JPEG, PBF).
#[derive(Debug, Default, Clone)]
pub struct TileSourceMetadata {
	/// The compression algorithm applied to tiles (e.g., gzip, brotli).
	tile_compression: TileCompression,
	/// The format of the tiles (e.g., PNG, JPEG, PBF).
	tile_format: TileFormat,

	traversal: Traversal,

	tile_pyramid: Arc<RwLock<Option<Arc<TilePyramid>>>>,
}

impl PartialEq for TileSourceMetadata {
	fn eq(&self, other: &Self) -> bool {
		self.tile_compression == other.tile_compression
			&& self.tile_format == other.tile_format
			&& self.traversal == other.traversal
	}
}

impl TileSourceMetadata {
	/// Create a new `TileSourceMetadata`.
	///
	/// # Arguments
	/// * `tile_format` - The format of the tiles.
	/// * `tile_compression` - The compression algorithm applied to tiles.
	/// * `tile_pyramid` - The bounding box and zoom pyramid defining the tile coverage.
	///
	/// # Returns
	/// A new instance of `TileSourceMetadata` configured with the specified parameters.
	#[must_use]
	pub fn new(
		tile_format: TileFormat,
		tile_compression: TileCompression,
		traversal: Traversal,
		tile_pyramid: Option<TilePyramid>,
	) -> TileSourceMetadata {
		TileSourceMetadata {
			tile_compression,
			tile_format,
			traversal,
			tile_pyramid: Arc::new(RwLock::new(tile_pyramid.map(Arc::new))),
		}
	}

	/// Updates fields using information from [`TileSourceMetadata`].
	///
	/// - Applies [`TileJSON::update_from_pyramid`] to intersect/set bounds and min/max zoom.
	/// - Sets `tile_format` from the reader parameters and derives `tile_type` from it.
	/// - If `tile_schema` is absent or mismatched with `tile_type`, infers a suitable schema
	///   (e.g., `RasterRGB` for rasters; for vectors, derived from `vector_layers`).
	pub fn update_tilejson(&self, tile_json: &mut TileJSON) {
		if let Some(tile_pyramid) = self.tile_pyramid() {
			tile_json.update_from_pyramid(tile_pyramid.as_ref());
		}

		tile_json.tile_format = Some(self.tile_format);

		tile_json.tile_type = tile_json.tile_format.map(|f| f.to_type());

		if let Some(tile_type) = tile_json.tile_type
			&& tile_json.tile_schema.map(|s| s.tile_type()) != tile_json.tile_type
		{
			tile_json.tile_schema = Some(match tile_type {
				TileType::Raster => TileSchema::RasterRGB,
				TileType::Vector => tile_json.vector_layers.tile_schema(),
				TileType::Unknown => TileSchema::Unknown,
			});
		}
	}

	#[must_use]
	pub fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	#[must_use]
	pub fn tile_compression(&self) -> &TileCompression {
		&self.tile_compression
	}

	#[must_use]
	pub fn tile_format(&self) -> &TileFormat {
		&self.tile_format
	}

	pub fn set_traversal(&mut self, traversal: Traversal) {
		self.traversal = traversal;
	}

	pub fn set_tile_compression(&mut self, tile_compression: TileCompression) {
		self.tile_compression = tile_compression;
	}

	pub fn set_tile_format(&mut self, tile_format: TileFormat) {
		self.tile_format = tile_format;
	}

	#[must_use]
	pub fn tile_pyramid(&self) -> Option<Arc<TilePyramid>> {
		self.tile_pyramid.read().expect("poisoned RwLock").clone()
	}

	pub fn set_tile_pyramid(&self, tile_pyramid: TilePyramid) {
		*self.tile_pyramid.write().expect("poisoned RwLock") = Some(Arc::new(tile_pyramid));
	}

	pub fn get_or_compute_tile_pyramid(
		&self,
		compute_fn: impl FnOnce() -> Result<TilePyramid>,
	) -> Result<Arc<TilePyramid>> {
		if let Some(pyramid) = self.tile_pyramid() {
			Ok(pyramid)
		} else {
			let mut write_guard = self.tile_pyramid.write().expect("poisoned RwLock");
			let pyramid = Arc::new(compute_fn()?);
			*write_guard = Some(pyramid.clone());
			Ok(pyramid)
		}
	}

	// takes a requested tile bbox and returns the intersection of it with the tile pyramid, if available
	#[must_use]
	pub fn intersection_bbox(&self, bbox: &TileBBox) -> TileBBox {
		if let Some(tile_pyramid) = self.tile_pyramid() {
			bbox.intersection_pyramid(&tile_pyramid)
		} else {
			*bbox
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
		let tile_pyramid = TilePyramid::from_geo_bbox(0, 10, &GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap()).unwrap();
		let tile_format = TileFormat::PNG;
		let tile_compression = TileCompression::Gzip;

		let params = TileSourceMetadata::new(
			tile_format,
			tile_compression,
			Traversal::ANY,
			Some(tile_pyramid.clone()),
		);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.tile_pyramid().unwrap().as_ref(), &tile_pyramid);
	}

	#[test]
	fn test_tiles_reader_parameters_new_full() {
		let tile_format = TileFormat::JPG;
		let tile_compression = TileCompression::Gzip;

		let params = TileSourceMetadata::new(tile_format, tile_compression, Traversal::ANY, None);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert!(params.tile_pyramid().is_none());
	}

	#[test]
	fn should_update_tile_json() -> Result<()> {
		let mut tj = TileJSON::default();
		// Prepare reader parameters
		let tile_pyramid = TilePyramid::from_geo_bbox(1, 4, &GeoBBox::new(-180.0, -90.0, 180.0, 90.0).unwrap())?;
		let rp = TileSourceMetadata::new(
			TileFormat::PNG,
			TileCompression::Uncompressed,
			Traversal::ANY,
			Some(tile_pyramid),
		);
		rp.update_tilejson(&mut tj);
		// Bounds and zooms
		assert_eq!(tj.zoom_min(), Some(1));
		assert_eq!(tj.zoom_max(), Some(4));
		// Format, content, and schema
		assert_eq!(tj.tile_format, Some(TileFormat::PNG));
		assert_eq!(tj.tile_type, Some(TileType::Raster));
		assert_eq!(tj.tile_schema, Some(TileSchema::RasterRGB));
		Ok(())
	}
}
