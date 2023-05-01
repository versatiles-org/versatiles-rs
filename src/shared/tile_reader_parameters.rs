use super::{Compression, DataConverter, TileBBoxPyramid, TileFormat};

#[derive(Debug, PartialEq, Eq)]
pub struct TileReaderParameters {
	tile_format: TileFormat,
	tile_compression: Compression,
	bbox_pyramid: TileBBoxPyramid,
	decompressor: DataConverter,
	flip_vertically: bool,
}

impl TileReaderParameters {
	pub fn new(
		tile_format: TileFormat, tile_compression: Compression, bbox_pyramid: TileBBoxPyramid,
	) -> TileReaderParameters {
		let decompressor = DataConverter::new_decompressor(&tile_compression);

		TileReaderParameters {
			decompressor,
			tile_format,
			tile_compression,
			bbox_pyramid,
			flip_vertically: false,
		}
	}
	#[cfg(test)]
	pub fn new_dummy() -> TileReaderParameters {
		TileReaderParameters {
			decompressor: DataConverter::new_empty(),
			tile_format: TileFormat::PBF,
			tile_compression: Compression::None,
			bbox_pyramid: TileBBoxPyramid::new_full(),
			flip_vertically: false,
		}
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		&self.tile_format
	}
	pub fn set_tile_format(&mut self, tile_format: TileFormat) {
		self.tile_format = tile_format;
	}
	pub fn get_tile_compression(&self) -> &Compression {
		&self.tile_compression
	}
	pub fn set_tile_compression(&mut self, compression: Compression) {
		self.tile_compression = compression;
	}
	pub fn get_bbox_pyramid(&self) -> &TileBBoxPyramid {
		&self.bbox_pyramid
	}
	pub fn get_vertical_flip(&self) -> bool {
		self.flip_vertically
	}
	pub fn set_vertical_flip(&mut self, flip: bool) {
		self.flip_vertically = flip;
	}
	pub fn set_bbox_pyramid(&mut self, pyramid: TileBBoxPyramid) {
		self.bbox_pyramid = pyramid;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn basic_tests() {
		let test = |tile_format: TileFormat, tile_compression: Compression, bbox_pyramid: TileBBoxPyramid, flip: bool| {
			let mut p = TileReaderParameters::new(tile_format.clone(), tile_compression, bbox_pyramid.clone());
			p.set_vertical_flip(flip);
			assert_eq!(p.get_tile_format(), &tile_format);
			assert_eq!(p.get_tile_compression(), &tile_compression);
			assert_eq!(p.get_bbox_pyramid(), &bbox_pyramid);
			assert_eq!(p.get_vertical_flip(), flip);

			p.set_tile_format(TileFormat::PNG);
			p.set_tile_compression(Compression::Gzip);
			assert_eq!(p.get_tile_format(), &TileFormat::PNG);
			assert_eq!(p.get_tile_compression(), &Compression::Gzip);
		};

		test(TileFormat::JPG, Compression::None, TileBBoxPyramid::new_empty(), false);

		test(TileFormat::PBF, Compression::Brotli, TileBBoxPyramid::new_full(), true);
	}
}
