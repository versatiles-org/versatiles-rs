use crate::{avif, jpeg, png, webp};
use anyhow::{Result, bail};
use image::DynamicImage;
use versatiles_core::{Blob, TileFormat};

pub fn encode(image: &DynamicImage, format: TileFormat, quality: Option<u8>, speed: Option<u8>) -> Result<Blob> {
	match format {
		TileFormat::AVIF => avif::encode(image, quality, speed),
		TileFormat::JPG => jpeg::encode(image, quality),
		TileFormat::PNG => png::encode(image, speed),
		TileFormat::WEBP => webp::encode(image, quality),
		_ => bail!("Unsupported format '{format}' for image encoding"),
	}
}

pub fn decode(blob: &Blob, format: TileFormat) -> Result<DynamicImage> {
	match format {
		TileFormat::AVIF => avif::blob2image(blob),
		TileFormat::JPG => jpeg::blob2image(blob),
		TileFormat::PNG => png::blob2image(blob),
		TileFormat::WEBP => webp::blob2image(blob),
		_ => bail!("Unsupported format '{format}' for image decoding"),
	}
}
