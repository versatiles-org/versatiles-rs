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
