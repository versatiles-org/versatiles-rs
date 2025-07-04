use crate::{avif, jpeg, png, webp, EnhancedDynamicImageTrait};
use anyhow::{bail, Result};
use image::DynamicImage;
use versatiles_core::types::{Blob, TileFormat};

/// Generate a Image with RGBA colors
pub fn create_image_rgba() -> DynamicImage {
	DynamicImage::from_fn_rgba8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
}

/// Generate a Image with RGB colors
pub fn create_image_rgb() -> DynamicImage {
	DynamicImage::from_fn_rgb8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8])
}

/// Generate a Image with grayscale colors
/// Returns a Image with 256x256 grayscale colors from black to white. Each pixel in the image
/// is a Luma<u8> value.
pub fn create_image_grey() -> DynamicImage {
	DynamicImage::from_fn_l8(256, 256, |x, _y| x as u8)
}

/// Generate a Image with grayscale alpha colors
/// Returns a Image with 256x256 grayscale alpha colors from black to white. Each pixel in the
/// image is a LumaA<u8> value, with the alpha value determined by the y coordinate.
pub fn create_image_greya() -> DynamicImage {
	DynamicImage::from_fn_la8(256, 256, |x, y| [x as u8, y as u8])
}

pub fn image2blob(image: &DynamicImage, format: TileFormat) -> Result<Blob> {
	use TileFormat::*;
	match format {
		AVIF => avif::image2blob(image, None),
		JPG => jpeg::image2blob(image, None),
		PNG => png::image2blob(image),
		WEBP => webp::image2blob(image, None),
		_ => bail!("Unsupported image format for encoding: {:?}", format),
	}
}

pub fn blob2image(blob: &Blob, format: TileFormat) -> Result<DynamicImage> {
	use TileFormat::*;
	match format {
		AVIF => avif::blob2image(blob),
		JPG => jpeg::blob2image(blob),
		PNG => png::blob2image(blob),
		WEBP => webp::blob2image(blob),
		_ => bail!("Unsupported image format for decoding: {:?}", format),
	}
}
#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_create_image_rgba() {
		let image = create_image_rgba();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::Rgba8);
	}

	#[test]
	fn test_create_image_rgb() {
		let image = create_image_rgb();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::Rgb8);
	}

	#[test]
	fn test_create_image_grey() {
		let image = create_image_grey();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::L8);
	}

	#[test]
	fn test_create_image_greya() {
		let image = create_image_greya();
		assert_eq!(image.width(), 256);
		assert_eq!(image.height(), 256);
		assert_eq!(image.color(), image::ColorType::La8);
	}

	#[test]
	fn test_image2blob_png() {
		let image = create_image_rgba();
		let blob = image2blob(&image, TileFormat::PNG).expect("Failed to convert image to blob");
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_png() {
		let image = create_image_rgba();
		let blob = image2blob(&image, TileFormat::PNG).expect("Failed to convert image to blob");
		let decoded_image = blob2image(&blob, TileFormat::PNG).expect("Failed to decode blob to image");
		assert_eq!(decoded_image.width(), image.width());
		assert_eq!(decoded_image.height(), image.height());
	}

	#[test]
	fn test_image2blob_jpg() {
		let image = create_image_rgb();
		let blob = image2blob(&image, TileFormat::JPG).expect("Failed to convert image to blob");
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_jpg() {
		let image = create_image_rgb();
		let blob = image2blob(&image, TileFormat::JPG).expect("Failed to convert image to blob");
		let decoded_image = blob2image(&blob, TileFormat::JPG).expect("Failed to decode blob to image");
		assert_eq!(decoded_image.width(), image.width());
		assert_eq!(decoded_image.height(), image.height());
	}

	#[test]
	fn test_image2blob_avif() {
		let image = create_image_rgba();
		let blob = image2blob(&image, TileFormat::AVIF).expect("Failed to convert image to blob");
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_avif() {
		let image = create_image_rgba();
		let blob = image2blob(&image, TileFormat::AVIF).expect("Failed to convert image to blob");

		assert_eq!(
			blob2image(&blob, TileFormat::AVIF).unwrap_err().to_string(),
			"AVIF decoding not implemented"
		);
	}

	#[test]
	fn test_image2blob_webp() {
		let image = create_image_rgba();
		let blob = image2blob(&image, TileFormat::WEBP).expect("Failed to convert image to blob");
		assert!(!blob.is_empty());
	}

	#[test]
	fn test_blob2image_webp() {
		let image = create_image_rgba();
		let blob = image2blob(&image, TileFormat::WEBP).expect("Failed to convert image to blob");
		let decoded_image = blob2image(&blob, TileFormat::WEBP).expect("Failed to decode blob to image");
		assert_eq!(decoded_image.width(), image.width());
		assert_eq!(decoded_image.height(), image.height());
	}
}
