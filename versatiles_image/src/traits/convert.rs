use crate::format::{avif, jpeg, png, webp};
use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, EncodableLayout, ImageBuffer, Luma, LumaA, Rgb, Rgba};
use versatiles_core::{Blob, TileFormat};

pub trait DynamicImageTraitConvert {
	fn from_fn_l8(width: u32, height: u32, f: fn(u32, u32) -> u8) -> DynamicImage;
	fn from_fn_la8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 2]) -> DynamicImage;
	fn from_fn_rgb8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 3]) -> DynamicImage;
	fn from_fn_rgba8(width: u32, height: u32, f: fn(u32, u32) -> [u8; 4]) -> DynamicImage;
	fn from_raw(width: u32, height: u32, data: Vec<u8>) -> Result<DynamicImage>;
	fn from_blob(blob: &Blob, format: TileFormat) -> Result<DynamicImage>;
	fn to_blob(&self, format: TileFormat) -> Result<Blob>;
	fn iter_pixels(&self) -> impl Iterator<Item = &[u8]>;
}

impl DynamicImageTraitConvert for DynamicImage {
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
			_ => bail!("Unsupported channel count: {channel_count}"),
		})
	}

	fn to_blob(&self, format: TileFormat) -> Result<Blob> {
		use TileFormat::{AVIF, JPG, PNG, WEBP};
		match format {
			AVIF => avif::image2blob(self, None),
			JPG => jpeg::image2blob(self, None),
			PNG => png::image2blob(self),
			WEBP => webp::image2blob(self, None),
			_ => bail!("Unsupported image format for encoding: {format:?}"),
		}
	}

	fn from_blob(blob: &Blob, format: TileFormat) -> Result<DynamicImage> {
		use TileFormat::{AVIF, JPG, PNG, WEBP};
		match format {
			AVIF => avif::blob2image(blob),
			JPG => jpeg::blob2image(blob),
			PNG => png::blob2image(blob),
			WEBP => webp::blob2image(blob),
			_ => bail!("Unsupported image format for decoding: {format:?}"),
		}
	}

	fn iter_pixels(&self) -> impl Iterator<Item = &[u8]> {
		match self {
			DynamicImage::ImageLuma8(img) => img.as_bytes().chunks_exact(1),
			DynamicImage::ImageLumaA8(img) => img.as_bytes().chunks_exact(2),
			DynamicImage::ImageRgb8(img) => img.as_bytes().chunks_exact(3),
			DynamicImage::ImageRgba8(img) => img.as_bytes().chunks_exact(4),
			_ => panic!("Unsupported image type for pixel iteration"),
		}
	}
}
