use std::fmt;

use super::{Compression, DataConverter, TileBBoxPyramid, TileFormat};

#[derive(PartialEq, Eq)]
pub struct TileReaderParameters {
	bbox_pyramid: TileBBoxPyramid,
	decompressor: DataConverter,
	flip_y: bool,
	swap_xy: bool,
	tile_compression: Compression,
	tile_format: TileFormat,
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
			swap_xy: false,
			flip_y: false,
		}
	}
	#[cfg(test)]
	pub fn new_dummy() -> TileReaderParameters {
		TileReaderParameters {
			decompressor: DataConverter::new_empty(),
			tile_format: TileFormat::PBF,
			tile_compression: Compression::None,
			bbox_pyramid: TileBBoxPyramid::new_full(),
			swap_xy: false,
			flip_y: false,
		}
	}
	pub fn get_tile_format(&self) -> &TileFormat {
		&self.tile_format
	}
	#[allow(dead_code)]
	pub fn set_tile_format(&mut self, tile_format: TileFormat) {
		self.tile_format = tile_format;
	}
	pub fn get_tile_compression(&self) -> &Compression {
		&self.tile_compression
	}
	pub fn set_tile_compression(&mut self, compression: Compression) {
		self.tile_compression = compression;
	}
	pub fn get_bbox_pyramid(&self) -> TileBBoxPyramid {
		let mut bbox_pyramid = self.bbox_pyramid.clone();
		if self.swap_xy {
			bbox_pyramid.swap_xy();
		}
		if self.flip_y {
			bbox_pyramid.flip_y();
		}
		bbox_pyramid
	}
	pub fn get_flip_y(&self) -> bool {
		self.flip_y
	}
	pub fn set_flip_y(&mut self, flip: bool) {
		self.flip_y = flip;
	}
	pub fn get_swap_xy(&self) -> bool {
		self.swap_xy
	}
	pub fn set_swap_xy(&mut self, flip: bool) {
		self.swap_xy = flip;
	}
	#[allow(dead_code)]
	pub fn set_bbox_pyramid(&mut self, pyramid: TileBBoxPyramid) {
		self.bbox_pyramid = pyramid;
	}
}

impl fmt::Debug for TileReaderParameters {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("")
			.field("bbox_pyramid", &self.bbox_pyramid)
			.field("decompressor", &self.decompressor)
			.field("flip_y", &self.flip_y)
			.field("swap_xy", &self.swap_xy)
			.field("tile_compression", &self.tile_compression)
			.field("tile_format", &self.tile_format)
			.finish()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn basic_tests() {
		let test = |tile_format: TileFormat,
		            tile_compression: Compression,
		            bbox_pyramid: TileBBoxPyramid,
		            flip_y: bool,
		            swap_xy: bool| {
			let mut p = TileReaderParameters::new(tile_format.clone(), tile_compression, bbox_pyramid.clone());
			p.set_flip_y(flip_y);
			p.set_swap_xy(swap_xy);

			assert_eq!(p.get_tile_format(), &tile_format);
			assert_eq!(p.get_tile_compression(), &tile_compression);
			assert_eq!(p.get_bbox_pyramid(), bbox_pyramid);
			assert_eq!(p.get_flip_y(), flip_y);
			assert_eq!(p.get_swap_xy(), swap_xy);

			p.set_tile_format(TileFormat::PNG);
			p.set_tile_compression(Compression::Gzip);
			assert_eq!(p.get_tile_format(), &TileFormat::PNG);
			assert_eq!(p.get_tile_compression(), &Compression::Gzip);
		};

		test(
			TileFormat::JPG,
			Compression::None,
			TileBBoxPyramid::new_empty(),
			false,
			false,
		);
		test(
			TileFormat::JPG,
			Compression::None,
			TileBBoxPyramid::new_empty(),
			false,
			true,
		);

		test(
			TileFormat::PBF,
			Compression::Brotli,
			TileBBoxPyramid::new_full(),
			true,
			true,
		);
	}
}
