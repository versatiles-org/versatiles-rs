#[cfg(feature = "full")]
use super::PrettyPrint;
use super::{transform_coord::TransformCoord, Compression, DataConverter, TileBBoxPyramid, TileFormat};
use anyhow::Result;
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
			bbox_pyramid: TileBBoxPyramid::new_dummy(),
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
		print
			.add_key_value("bbox", &format!("{:?}", self.bbox_pyramid.get_geo_bbox()))
			.await;
		print.add_key_value("decompressor", &self.decompressor).await;
		print.add_key_value("flip_y", &self.flip_y).await;
		print.add_key_value("swap_xy", &self.swap_xy).await;
		print.add_key_value("tile_compression", &self.tile_compression).await;
		print.add_key_value("tile_format", &self.tile_format).await;
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
	use crate::shared::{TileBBox, TileCoord3};

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

	// Testing the new_dummy method
	#[test]
	fn new_dummy_test() {
		let p = TileReaderParameters::new_dummy();

		assert_eq!(p.tile_format, TileFormat::PBF);
		assert_eq!(p.tile_compression, Compression::None);
		assert_eq!(p.decompressor.as_string(), "");
		assert_eq!(p.bbox_pyramid.to_string(), "[0: [0,0,0,0] (1), 1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,1,6,7] (49), 4: [1,3,12,14] (144), 5: [3,6,25,28] (529), 6: [6,12,51,57] (2116), 7: [12,25,102,115] (8281), 8: [25,51,204,230] (32400), 9: [51,102,409,460] (128881), 10: [102,204,819,921] (515524), 11: [204,409,1638,1843] (2059225), 12: [409,819,3276,3686] (8225424), 13: [819,1638,6553,7372] (32890225), 14: [1638,3276,13107,14745] (131560900), 15: [3276,6553,26214,29491] (526197721)]");
		assert_eq!(p.swap_xy, false);
		assert_eq!(p.flip_y, false);
	}

	// Testing transform_forward and transform_backward methods
	#[test]
	fn transform_forward_and_backward_test() {
		fn test1(mut t: impl TransformCoord + Clone + std::fmt::Debug + Eq, flip_y: bool, swap_xy: bool) {
			let mut p = TileReaderParameters::new_dummy();
			p.flip_y = flip_y;
			p.swap_xy = swap_xy;

			let original_t = t.clone();
			p.transform_forward(&mut t);
			p.transform_backward(&mut t);

			assert_eq!(original_t, t);
		}
		fn test2(t: impl TransformCoord + Clone + std::fmt::Debug + Eq) {
			test1(t.clone(), false, false);
			test1(t.clone(), false, true);
			test1(t.clone(), true, false);
			test1(t, true, true);
		}

		let mut pyramide = TileBBoxPyramid::new_empty();
		pyramide.include_bbox(&TileBBox::new(7, 12, 34, 56, 78));
		pyramide.include_bbox(&TileBBox::new(8, 12, 34, 56, 78));
		pyramide.include_bbox(&TileBBox::new(9, 12, 34, 56, 78));
		test2(TileBBoxPyramid::new_empty());
		test2(TileBBox::new(8, 12, 34, 56, 78));
		test2(TileCoord3::new(12, 34, 6));
	}

	// Testing transform_forward and transform_backward methods
	#[test]
	fn transform_coord() {
		fn test(flip_y: bool, swap_xy: bool, t: TileCoord3) {
			let mut bbox = TileCoord3::new(12, 34, 6);
			let mut p = TileReaderParameters::new_dummy();

			p.flip_y = flip_y;
			p.swap_xy = swap_xy;

			p.transform_forward(&mut bbox);
			assert_eq!(bbox, t);
		}

		test(false, false, TileCoord3::new(12, 34, 6));
		test(false, true, TileCoord3::new(34, 12, 6));
		test(true, false, TileCoord3::new(12, 29, 6));
		test(true, true, TileCoord3::new(29, 12, 6));
	}

	// Testing transform_forward and transform_backward methods
	#[test]
	fn transform_bbox() {
		fn test(flip_y: bool, swap_xy: bool, t: TileBBox) {
			let mut bbox = TileBBox::new(8, 12, 34, 56, 78);
			let mut p = TileReaderParameters::new_dummy();

			p.flip_y = flip_y;
			p.swap_xy = swap_xy;

			p.transform_forward(&mut bbox);
			assert_eq!(bbox, t);
		}

		test(false, false, TileBBox::new(8, 12, 34, 56, 78));
		test(false, true, TileBBox::new(8, 34, 12, 78, 56));
		test(true, false, TileBBox::new(8, 12, 177, 56, 221));
		test(true, true, TileBBox::new(8, 177, 12, 221, 56));
	}
}
