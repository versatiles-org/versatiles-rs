//! This module defines configuration parameters for creating and initializing `TilesReader` instances.

use super::{TileBBoxPyramid, TileCompression, TileFormat};

/// Configuration parameters for creating and initializing a `TilesReader`.
///
/// # Fields
/// - `bbox_pyramid`: The bounding box and zoom pyramid defining the tile coverage.
/// - `tile_compression`: The compression algorithm applied to tiles (e.g., gzip, brotli).
/// - `tile_format`: The format of the tiles (e.g., PNG, JPEG, PBF).
#[derive(Debug, Default, PartialEq, Clone)]
pub struct TilesReaderParameters {
	/// The bounding box and zoom pyramid defining the tile coverage.
	pub bbox_pyramid: TileBBoxPyramid,
	/// The compression algorithm applied to tiles (e.g., gzip, brotli).
	pub tile_compression: TileCompression,
	/// The format of the tiles (e.g., PNG, JPEG, PBF).
	pub tile_format: TileFormat,
}

impl TilesReaderParameters {
	/// Create a new `TilesReaderParameters`.
	///
	/// # Arguments
	/// * `tile_format` - The format of the tiles.
	/// * `tile_compression` - The compression algorithm applied to tiles.
	/// * `bbox_pyramid` - The bounding box and zoom pyramid defining the tile coverage.
	///
	/// # Returns
	/// A new instance of `TilesReaderParameters` configured with the specified parameters.
	#[must_use]
	pub fn new(
		tile_format: TileFormat,
		tile_compression: TileCompression,
		bbox_pyramid: TileBBoxPyramid,
	) -> TilesReaderParameters {
		TilesReaderParameters {
			bbox_pyramid,
			tile_compression,
			tile_format,
		}
	}

	#[cfg(test)]
	#[allow(dead_code)]
	/// Creates a `TilesReaderParameters` with a default full pyramid up to zoom level 31 for testing purposes.
	#[must_use]
	pub fn new_full(tile_format: TileFormat, tile_compression: TileCompression) -> TilesReaderParameters {
		TilesReaderParameters {
			tile_format,
			tile_compression,
			bbox_pyramid: TileBBoxPyramid::new_full(31),
		}
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_tiles_reader_parameters_new() {
		let bbox_pyramid = TileBBoxPyramid::new_full(10);
		let tile_format = TileFormat::PNG;
		let tile_compression = TileCompression::Gzip;

		let params = TilesReaderParameters::new(tile_format, tile_compression, bbox_pyramid.clone());

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.bbox_pyramid, bbox_pyramid);
	}

	#[test]
	fn test_tiles_reader_parameters_new_full() {
		let tile_format = TileFormat::JPG;
		let tile_compression = TileCompression::Gzip;

		let params = TilesReaderParameters::new_full(tile_format, tile_compression);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.bbox_pyramid, TileBBoxPyramid::new_full(31));
	}
}
