use crate::{avif, jpeg, png, webp};
use anyhow::{Result, anyhow, bail, ensure};
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

impl EnhancedDynamicImageTrait for DynamicImage {
	fn from_fn_l8(width: u32, height: u32, f: fn(u32, u32) -> u8) -> DynamicImage {
		DynamicImage::ImageLuma8(ImageBuffer::from_fn(width, height, |x, y| Luma([f(x, y)])))
	}
	fn from_fn_la8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 2]) -> DynamicImage {
		DynamicImage::ImageLumaA8(ImageBuffer::from_fn(width, height, |x, y| LumaA(f(x, y))))
	}
	fn from_fn_rgb8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 3]) -> DynamicImage {
		DynamicImage::ImageRgb8(ImageBuffer::from_fn(width, height, |x, y| Rgb(f(x, y))))
	}
	fn from_fn_rgba8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 4]) -> DynamicImage {
		DynamicImage::ImageRgba8(ImageBuffer::from_fn(width, height, |x, y| Rgba(f(x, y))))
	}

	fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<DynamicImage> {
		let channel_count = data.len() / (width * height) as usize;
		Ok(match channel_count {
			1 => DynamicImage::ImageLuma8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create Luma8 image buffer with provided data"))?,
			),
			2 => DynamicImage::ImageLumaA8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create LumaA8 image buffer with provided data"))?,
			),
			3 => DynamicImage::ImageRgb8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create RGB8 image buffer with provided data"))?,
			),
			4 => DynamicImage::ImageRgba8(
				ImageBuffer::from_vec(width, height, data)
					.ok_or_else(|| anyhow!("Failed to create RGBA8 image buffer with provided data"))?,
			),
			_ => bail!("Unsupported channel count: {}", channel_count),
		})
	}

	fn to_blob(&self, format: TileFormat) -> Result<Blob> {
		use TileFormat::*;
		match format {
			AVIF => avif::image2blob(self, None),
			JPG => jpeg::image2blob(self, None),
			PNG => png::image2blob(self),
			WEBP => webp::image2blob(self, None),
			_ => bail!("Unsupported image format for encoding: {:?}", format),
		}
	}

	fn from_blob(blob: &Blob, format: TileFormat) -> Result<DynamicImage> {
		use TileFormat::*;
		match format {
			AVIF => avif::blob2image(blob),
			JPG => jpeg::blob2image(blob),
			PNG => png::blob2image(blob),
			WEBP => webp::blob2image(blob),
			_ => bail!("Unsupported image format for decoding: {:?}", format),
		}
	}

	fn pixels(&self) -> impl Iterator<Item = &[u8]> {
		match self {
			DynamicImage::ImageLuma8(img) => img.as_bytes().chunks_exact(1),
			DynamicImage::ImageLumaA8(img) => img.as_bytes().chunks_exact(2),
			DynamicImage::ImageRgb8(img) => img.as_bytes().chunks_exact(3),
			DynamicImage::ImageRgba8(img) => img.as_bytes().chunks_exact(4),
			_ => panic!("Unsupported image type for pixel iteration"),
		}
	}

	fn get_scaled_down(&self, factor: u32) -> DynamicImage {
		assert!(factor > 0, "Scaling factor must be greater than zero");
		let new_width = self.width() / factor;
		let new_height = self.height() / factor;
		self.resize_exact(new_width, new_height, image::imageops::FilterType::Triangle)
	}

	fn into_scaled_down(self, factor: u32) -> DynamicImage {
		if factor == 1 {
			self
		} else {
			self.get_scaled_down(factor)
		}
	}

	fn average_color(&self) -> Vec<u8> {
		let img = self.resize_exact(1, 1, image::imageops::FilterType::Triangle);
		img.pixels().next().unwrap().to_vec()
	}

	fn ensure_same_size(&self, other: &DynamicImage) -> Result<()> {
		ensure!(
			self.width() == other.width(),
			"Image width mismatch: self has width {}, but the other image has width {}",
			self.width(),
			other.width()
		);
		ensure!(
			self.height() == other.height(),
			"Image height mismatch: self has height {}, but the other image has height {}",
			self.height(),
			other.height()
		);
		Ok(())
	}

	fn ensure_same_meta(&self, other: &DynamicImage) -> Result<()> {
		self.ensure_same_size(other)?;
		ensure!(
			self.color() == other.color(),
			"Pixel value type mismatch: self has {:?}, but the other image has {:?}",
			self.color(),
			other.color()
		);
		Ok(())
	}

	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>> {
		self.ensure_same_meta(other)?;

		let channels = self.color().channel_count() as usize;
		let mut sqr_sum = vec![0u64; channels];

		for (p1, p2) in self.pixels().zip(other.pixels()) {
			for i in 0..channels {
				let d = p1[i] as i64 - p2[i] as i64;
				sqr_sum[i] += (d * d) as u64;
			}
		}

		let n = (self.width() * self.height()) as f64;
		Ok(sqr_sum.iter().map(|v| (10.0 * (*v as f64) / n).ceil() / 10.0).collect())
	}

	fn bits_per_value(&self) -> u8 {
		self.color().bits_per_pixel().div(self.color().channel_count() as u16) as u8
	}

	fn extended_color_type(&self) -> ExtendedColorType {
		self.color().into()
	}

	fn channel_count(&self) -> u8 {
		self.color().channel_count()
	}

	fn overlay(&mut self, top: &DynamicImage) -> Result<()> {
		self.ensure_same_size(top)?;
		overlay(self, top, 0, 0);
		Ok(())
	}

	fn get_flattened(self, color: Rgb<u8>) -> Result<DynamicImage> {
		match self {
			DynamicImage::ImageLuma8(img) => Ok(DynamicImage::ImageLuma8(img)),
			DynamicImage::ImageRgb8(img) => Ok(DynamicImage::ImageRgb8(img)),
			DynamicImage::ImageRgba8(img) => {
				let c = [color[0] as u16, color[1] as u16, color[2] as u16];
				Ok(DynamicImage::from(map_colors(&img, |p| {
					if p[3] == 255 {
						Rgb([p[0], p[1], p[2]])
					} else {
						let a = (p[3]) as u16;
						let b = (255 - p[3]) as u16;
						Rgb([
							(((p[0] as u16 * a) + c[0] * b + 127) / 255) as u8,
							(((p[1] as u16 * a) + c[1] * b + 127) / 255) as u8,
							(((p[2] as u16 * a) + c[2] * b + 127) / 255) as u8,
						])
					}
				})))
			}
			_ => bail!("Unsupported image type {:?} for flattening", self.color()),
		}
	}

	/// Generate a Image with RGBA colors
	fn new_test_rgba() -> DynamicImage {
		DynamicImage::from_fn_rgba8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
	}

	/// Generate a Image with RGB colors
	fn new_test_rgb() -> DynamicImage {
		DynamicImage::from_fn_rgb8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8])
	}

	/// Generate a Image with grayscale colors
	/// Returns a Image with 256x256 grayscale colors from black to white. Each pixel in the image
	/// is a Luma<u8> value.
	fn new_test_grey() -> DynamicImage {
		DynamicImage::from_fn_l8(256, 256, |x, _y| x as u8)
	}

	/// Generate a Image with grayscale alpha colors
	/// Returns a Image with 256x256 grayscale alpha colors from black to white. Each pixel in the
	/// image is a LumaA<u8> value, with the alpha value determined by the y coordinate.
	fn new_test_greya() -> DynamicImage {
		DynamicImage::from_fn_la8(256, 256, |x, y| [x as u8, y as u8])
	}

	fn is_empty(&self) -> bool {
		if !self.color().has_alpha() {
			return false;
		}
		let alpha_channel = (self.color().channel_count() - 1) as usize;
		return !self.pixels().any(|p| p[alpha_channel] != 0);
	}

	fn is_opaque(&self) -> bool {
		if !self.color().has_alpha() {
			return true;
		}
		let alpha_channel = (self.color().channel_count() - 1) as usize;
		return self.pixels().all(|p| p[alpha_channel] == 255);
	}

	fn into_optional(self) -> Option<DynamicImage> {
		if self.is_empty() { None } else { Some(self) }
	}
}

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
}
