use super::{transform_coord::TransformCoord, Compression, DataConverter, TileBBoxPyramid, TileFormat};
#[cfg(feature = "full")]
use super::{PrettyPrint, Result};
use std::fmt;

#[derive(Clone, PartialEq, Eq)]
pub struct TileReaderParameters {
	pub bbox_pyramid: TileBBoxPyramid,
	pub decompressor: DataConverter,
	pub flip_y: bool,
	pub swap_xy: bool,
	pub tile_compression: Compression,
	pub tile_format: TileFormat,
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
	pub fn transform_forward<T>(&self, bbox: &mut T)
	where
		T: TransformCoord,
	{
		self.flip_y_if_needed(bbox);
		self.swap_xy_if_needed(bbox);
	}
	pub fn transform_backward<T>(&self, bbox: &mut T)
	where
		T: TransformCoord,
	{
		self.swap_xy_if_needed(bbox);
		self.flip_y_if_needed(bbox);
	}
	fn flip_y_if_needed(&self, data: &mut impl TransformCoord) {
		if self.flip_y {
			data.flip_y();
		}
	}
	fn swap_xy_if_needed(&self, data: &mut impl TransformCoord) {
		if self.swap_xy {
			data.swap_xy();
		}
	}
	#[cfg(feature = "full")]
	pub async fn probe<'a>(&self, mut print: PrettyPrint) -> Result<()> {
		let p = print.get_list("bbox_pyramid").await;
		for level in self.bbox_pyramid.iter_levels() {
			p.add_value(level).await
		}
		print.add_key_value(&"decompressor", &self.decompressor).await;
		print.add_key_value(&"flip_y", &self.flip_y).await;
		print.add_key_value(&"swap_xy", &self.swap_xy).await;
		print.add_key_value(&"tile_compression", &self.tile_compression).await;
		print.add_key_value(&"tile_format", &self.tile_format).await;
		Ok(())
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
		let test = |tile_format: TileFormat, tile_compression: Compression, bbox_pyramid: TileBBoxPyramid| {
			let p = TileReaderParameters::new(tile_format.clone(), tile_compression, bbox_pyramid.clone());

			assert_eq!(p.tile_format, tile_format);
			assert_eq!(p.tile_compression, tile_compression);
			assert_eq!(p.bbox_pyramid, bbox_pyramid);
		};

		test(TileFormat::JPG, Compression::None, TileBBoxPyramid::new_empty());
		test(TileFormat::JPG, Compression::None, TileBBoxPyramid::new_empty());
		test(TileFormat::PBF, Compression::Brotli, TileBBoxPyramid::new_full());
	}
}
