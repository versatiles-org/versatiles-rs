use super::{TileBBoxPyramid, TileCompression, TileFormat};

/// Parameters for configuring a `TilesReader`.
#[derive(Debug, PartialEq, Clone)]
pub struct TilesReaderParameters {
	pub bbox_pyramid: TileBBoxPyramid,
	pub tile_compression: TileCompression,
	pub tile_format: TileFormat,
}

impl TilesReaderParameters {
	/// Create a new `TilesReaderParameters`.
	pub fn new(
		tile_format: TileFormat,
		tile_compression: TileCompression,
		bbox_pyramid: TileBBoxPyramid,
	) -> TilesReaderParameters {
		TilesReaderParameters {
			tile_format,
			tile_compression,
			bbox_pyramid,
		}
	}

	#[cfg(test)]
	#[allow(dead_code)]
	pub fn new_full(
		tile_format: TileFormat,
		tile_compression: TileCompression,
	) -> TilesReaderParameters {
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

		let params = TilesReaderParameters::new(
			tile_format.clone(),
			tile_compression.clone(),
			bbox_pyramid.clone(),
		);

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.bbox_pyramid, bbox_pyramid);
	}

	#[test]
	fn test_tiles_reader_parameters_new_full() {
		let tile_format = TileFormat::JPG;
		let tile_compression = TileCompression::Gzip;

		let params = TilesReaderParameters::new_full(tile_format.clone(), tile_compression.clone());

		assert_eq!(params.tile_format, tile_format);
		assert_eq!(params.tile_compression, tile_compression);
		assert_eq!(params.bbox_pyramid, TileBBoxPyramid::new_full(31));
	}
}
