use crate::{avif, jpeg, png, webp};
use anyhow::{Result, anyhow, bail, ensure};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, EncodableLayout, ExtendedColorType, ImageBuffer, Luma, LumaA, Rgb, Rgba, imageops::overlay};
use imageproc::map::map_colors;
use std::{ops::Div, vec};
use versatiles_core::{Blob, TileFormat};

pub trait EnhancedDynamicImageTrait {
	fn from_fn_l8(width: u32, height: u32, f: fn(u32, u32) -> u8) -> DynamicImage;
	fn from_fn_la8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 2]) -> DynamicImage;
	fn from_fn_rgb8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 3]) -> DynamicImage;
	fn from_fn_rgba8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 4]) -> DynamicImage;
	fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<DynamicImage>;
	fn pixels(&self) -> impl Iterator<Item = &[u8]>;

	fn ensure_same_meta(&self, other: &DynamicImage) -> Result<()>;
	fn ensure_same_size(&self, other: &DynamicImage) -> Result<()>;

	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>>;
	fn bits_per_value(&self) -> u8;
	fn channel_count(&self) -> u8;
	fn extended_color_type(&self) -> ExtendedColorType;
	fn to_blob(&self, format: TileFormat) -> Result<Blob>;
	fn from_blob(blob: &Blob, format: TileFormat) -> Result<DynamicImage>;
	fn overlay(&mut self, other: &DynamicImage) -> Result<()>;
	fn average_color(&self) -> Vec<u8>;
	fn get_scaled_down(&self, factor: u32) -> DynamicImage;
	fn into_scaled_down(self, factor: u32) -> DynamicImage;
	fn into_optional(self) -> Option<DynamicImage>;
	fn is_empty(&self) -> bool;
	fn is_opaque(&self) -> bool;
	fn get_flattened(self, color: Rgb<u8>) -> Result<DynamicImage>;

	fn new_test_rgba() -> DynamicImage;
	fn new_test_rgb() -> DynamicImage;
	fn new_test_grey() -> DynamicImage;
	fn new_test_greya() -> DynamicImage;
}

impl EnhancedDynamicImageTrait for DynamicImage {}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_from_fn_l8() {
		let width = 4;
		let height = 4;
		let image = DynamicImage::from_fn_l8(width, height, |x, y| (x + y) as u8);
		assert_eq!(image.width(), width);
		assert_eq!(image.height(), height);
		assert_eq!(image.color().channel_count(), 1);
	}

	#[test]
	fn test_from_fn_rgb8() {
		let width = 4;
		let height = 4;
		let image = DynamicImage::from_fn_rgb8(width, height, |x, y| [x as u8, y as u8, 0]);
		assert_eq!(image.width(), width);
		assert_eq!(image.height(), height);
		assert_eq!(image.color().channel_count(), 3);
	}

	#[test]
	fn test_from_raw_valid_data() {
		let width = 4;
		let height = 4;
		let data = vec![0u8; (width * height) as usize];
		let image = DynamicImage::from_raw(width, height, data).unwrap();
		assert_eq!(image.width(), width);
		assert_eq!(image.height(), height);
	}

	#[test]
	fn test_from_raw_invalid_data() {
		let width = 4;
		let height = 4;
		let data = vec![0u8; ((width * height) as usize) - 1];
		let result = DynamicImage::from_raw(width, height, data);
		assert!(result.is_err());
	}

	#[test]
	fn test_compare_same_images() {
		let width = 4;
		let height = 4;
		let image1 = DynamicImage::from_fn_l8(width, height, |x, y| (x + y) as u8);
		let image2 = DynamicImage::from_fn_l8(width, height, |x, y| (x + y) as u8);
		assert!(image1.ensure_same_meta(&image2).is_ok());
	}

	#[test]
	fn test_compare_different_images() {
		let width = 4;
		let height = 4;
		let image1 = DynamicImage::from_fn_l8(width, height, |x, y| (x + y) as u8);
		let image2 = DynamicImage::from_fn_l8(width + 1, height, |x, y| (x * y) as u8);
		assert!(image1.ensure_same_meta(&image2).is_err());
	}

	#[test]
	fn test_diff() {
		let width = 4;
		let height = 4;
		let image1 = DynamicImage::from_fn_l8(width, height, |x, y| (x + y) as u8);
		let image2 = DynamicImage::from_fn_l8(width, height, |x, y| (x + y + 1) as u8);
		assert_eq!(image1.diff(&image2).unwrap(), vec![1.0; 1]);
	}

	#[test]
	fn test_bits_per_value() {
		let image = DynamicImage::from_fn_rgb8(4, 4, |x, y| [x as u8, y as u8, 0]);
		assert_eq!(image.bits_per_value(), 8);
	}

	#[test]
	fn test_channel_count() {
		let image = DynamicImage::from_fn_rgba8(4, 4, |x, y| [x as u8, y as u8, 0, 255]);
		assert_eq!(image.channel_count(), 4);
	}

	#[test]
	fn test_extended_color_type() {
		let image = DynamicImage::from_fn_l8(4, 4, |x, y| (x + y) as u8);
		assert_eq!(image.extended_color_type(), ExtendedColorType::L8);
	}

	#[test]
	fn test_create_image_rgba() {
		let image = DynamicImage::new_test_rgba();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::Rgba8);
	}

	#[test]
	fn test_create_image_rgb() {
		let image = DynamicImage::new_test_rgb();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::Rgb8);
	}

	#[test]
	fn test_create_image_grey() {
		let image = DynamicImage::new_test_grey();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::L8);
	}

	#[test]
	fn test_create_image_greya() {
		let image = DynamicImage::new_test_greya();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::La8);
	}

	#[test]
	fn test_image2blob_png() {
		let image = DynamicImage::new_test_rgba();
		let blob = image.to_blob(TileFormat::PNG).unwrap();
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_png() {
		let image = DynamicImage::new_test_rgba();
		let blob = image.to_blob(TileFormat::PNG).unwrap();
		let decoded_image = DynamicImage::from_blob(&blob, TileFormat::PNG).unwrap();
		assert_eq!(decoded_image.width(), image.width());
		assert_eq!(decoded_image.height(), image.height());
	}

	#[test]
	fn test_image2blob_jpg() {
		let image = DynamicImage::new_test_rgb();
		let blob = image.to_blob(TileFormat::JPG).unwrap();
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_jpg() {
		let image = DynamicImage::new_test_rgb();
		let blob = image.to_blob(TileFormat::JPG).unwrap();
		let decoded_image = DynamicImage::from_blob(&blob, TileFormat::JPG).unwrap();
		assert_eq!(decoded_image.width(), image.width());
		assert_eq!(decoded_image.height(), image.height());
	}

	#[test]
	fn test_image2blob_avif() {
		let image = DynamicImage::new_test_rgba();
		let blob = image.to_blob(TileFormat::AVIF).unwrap();
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_avif() {
		let image = DynamicImage::new_test_rgba();
		let blob = image.to_blob(TileFormat::AVIF).unwrap();

		assert_eq!(
			DynamicImage::from_blob(&blob, TileFormat::AVIF)
				.unwrap_err()
				.to_string(),
			"AVIF decoding not implemented"
		);
	}

	#[test]
	fn test_image2blob_webp() {
		let image = DynamicImage::new_test_rgba();
		let blob = image.to_blob(TileFormat::WEBP).unwrap();
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_webp() {
		let image = DynamicImage::new_test_rgba();
		let blob = image.to_blob(TileFormat::WEBP).unwrap();
		let decoded_image = DynamicImage::from_blob(&blob, TileFormat::WEBP).unwrap();
		assert_eq!(decoded_image.width(), image.width());
		assert_eq!(decoded_image.height(), image.height());
	}

	#[test]
	fn test_rgba8_variants_empty_and_opaque() {
		fn test(cb: fn(u32, u32) -> [u8; 4], expected_empty: bool, expected_opaque: bool) {
			let img = DynamicImage::from_fn_rgba8(9, 9, cb);
			assert_eq!(img.is_empty(), expected_empty);
			assert_eq!(img.is_opaque(), expected_opaque);
			let img = img.into_optional();
			assert_eq!(img.is_none(), expected_empty);
			assert_eq!(img.is_some(), !expected_empty);
		}
		test(|_x, _y| [0, 0, 0, 0], true, false);
		test(|x, y| [0, 0, 0, if x == 4 && y == 4 { 1 } else { 0 }], false, false);
		test(|_x, _y| [0, 0, 0, 255], false, true);
		test(|x, y| [0, 0, 0, if x == 4 && y == 4 { 254 } else { 255 }], false, false);
	}

	#[test]
	fn test_la8_variants_empty_and_opaque() {
		fn test(cb: fn(u32, u32) -> [u8; 2], expected_empty: bool, expected_opaque: bool) {
			let img = DynamicImage::from_fn_la8(9, 9, cb);
			assert_eq!(img.is_empty(), expected_empty);
			assert_eq!(img.is_opaque(), expected_opaque);
			let img = img.into_optional();
			assert_eq!(img.is_none(), expected_empty);
			assert_eq!(img.is_some(), !expected_empty);
		}
		test(|_x, _y| [0, 0], true, false);
		test(|x, y| [0, if x == 4 && y == 4 { 1 } else { 0 }], false, false);
		test(|_x, _y| [0, 255], false, true);
		test(|x, y| [0, if x == 4 && y == 4 { 254 } else { 255 }], false, false);
	}

	#[test]
	fn test_variants_empty_and_opaque() {
		fn test(img: DynamicImage, expected_empty: bool, expected_opaque: bool) {
			assert_eq!(img.is_empty(), expected_empty);
			assert_eq!(img.is_opaque(), expected_opaque);
			let img = img.into_optional();
			assert_eq!(img.is_none(), expected_empty);
			assert_eq!(img.is_some(), !expected_empty);
		}
		test(DynamicImage::new_test_grey(), false, true);
		test(DynamicImage::new_test_greya(), false, false);
		test(DynamicImage::new_test_rgb(), false, true);
		test(DynamicImage::new_test_rgba(), false, false);
	}
}
