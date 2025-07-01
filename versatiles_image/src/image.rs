use anyhow::{anyhow, ensure, Result};
use image::{DynamicImage, EncodableLayout, ExtendedColorType, ImageBuffer, Luma, LumaA, Rgb, Rgba};
use std::{ops::Div, vec};

pub trait EnhancedDynamicImageTrait {
	fn from_fn_l8(width: u32, height: u32, f: fn(u32, u32) -> u8) -> DynamicImage;
	fn from_fn_la8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 2]) -> DynamicImage;
	fn from_fn_rgb8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 3]) -> DynamicImage;
	fn from_fn_rgba8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 4]) -> DynamicImage;
	fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<DynamicImage>;
	fn pixels(&self) -> impl Iterator<Item = &[u8]>;
	fn compare(&self, other: &DynamicImage) -> Result<()>;
	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>>;
	fn bits_per_value(&self) -> u8;
	fn channel_count(&self) -> u8;
	fn extended_color_type(&self) -> ExtendedColorType;
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
		ensure!(
			data.len() == (width * height) as usize,
			"Data length does not match expected size for L8 image"
		);
		Ok(DynamicImage::ImageLuma8(
			ImageBuffer::from_vec(width, height, data)
				.ok_or_else(|| anyhow!("Failed to create image buffer with provided data"))?,
		))
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

	fn compare(&self, other: &DynamicImage) -> Result<()> {
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
		ensure!(
			self.color() == other.color(),
			"Pixel value type mismatch: self has {:?}, but the other image has {:?}",
			self.color(),
			other.color()
		);
		Ok(())
	}

	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>> {
		self.compare(other)?;

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
}
