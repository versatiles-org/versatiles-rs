use super::{Compression, DataConverter, TileBBoxPyramide, TileFormat};

#[derive(Debug, PartialEq, Eq)]
pub struct TileReaderParameters {
	tile_format: TileFormat,
	tile_precompression: Compression,
	bbox_pyramide: TileBBoxPyramide,
	#[allow(dead_code)]
	decompressor: DataConverter,
	flip_vertically: bool,
}

impl TileReaderParameters {
	pub fn new(
		tile_format: TileFormat, tile_precompression: Compression, bbox_pyramide: TileBBoxPyramide,
	) -> TileReaderParameters {
		let decompressor = DataConverter::new_decompressor(&tile_precompression);

		TileReaderParameters {
			decompressor,
			tile_format,
			tile_precompression,
			bbox_pyramide,
			flip_vertically: false,
		}
	}
	pub fn new_dummy() -> TileReaderParameters {
		TileReaderParameters {
			decompressor: DataConverter::new_empty(),
			tile_format: TileFormat::PBF,
			tile_precompression: Compression::None,
			bbox_pyramide: TileBBoxPyramide::new_full(),
			flip_vertically: false,
		}
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		&self.tile_format
	}
	pub fn set_tile_format(&mut self, tile_format: TileFormat) {
		self.tile_format = tile_format;
	}
	pub fn get_tile_precompression(&self) -> &Compression {
		&self.tile_precompression
	}
	pub fn set_tile_precompression(&mut self, precompression: Compression) {
		self.tile_precompression = precompression;
	}
	#[allow(dead_code)]
	pub fn get_decompressor(&self) -> &DataConverter {
		&self.decompressor
	}
	pub fn get_bbox_pyramide(&self) -> &TileBBoxPyramide {
		&self.bbox_pyramide
	}
	pub fn get_vertical_flip(&self) -> bool {
		self.flip_vertically
	}
	pub fn set_vertical_flip(&mut self, flip: bool) {
		self.flip_vertically = flip;
	}
	pub fn set_bbox_pyramide(&mut self, pyramide: TileBBoxPyramide) {
		self.bbox_pyramide = pyramide;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn basic_tests() {
		let test =
			|tile_format: TileFormat, tile_precompression: Compression, bbox_pyramide: TileBBoxPyramide, flip: bool| {
				let mut p = TileReaderParameters::new(tile_format.clone(), tile_precompression, bbox_pyramide.clone());
				p.set_vertical_flip(flip);
				assert_eq!(p.get_tile_format(), &tile_format);
				assert_eq!(p.get_tile_precompression(), &tile_precompression);
				assert_eq!(p.get_bbox_pyramide(), &bbox_pyramide);
				assert_eq!(p.get_vertical_flip(), flip);

				p.set_tile_format(TileFormat::PNG);
				p.set_tile_precompression(Compression::Gzip);
				assert_eq!(p.get_tile_format(), &TileFormat::PNG);
				assert_eq!(p.get_tile_precompression(), &Compression::Gzip);
			};

		test(TileFormat::JPG, Compression::None, TileBBoxPyramide::new_empty(), false);

		test(TileFormat::PBF, Compression::Brotli, TileBBoxPyramide::new_full(), true);
	}
}
