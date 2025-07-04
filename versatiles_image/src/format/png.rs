use crate::EnhancedDynamicImageTrait;
use anyhow::{anyhow, bail, Result};
use image::{codecs::png, load_from_memory_with_format, DynamicImage, ImageEncoder, ImageFormat};
use versatiles_core::types::Blob;

pub fn image2blob(image: &DynamicImage) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("png only supports 8-bit images");
	}

	if image.channel_count() < 1 || image.channel_count() > 4 {
		bail!("png only supports Grey, GreyA, RGB or RGBA");
	}

	let mut buffer: Vec<u8> = Vec::new();
	png::PngEncoder::new_with_quality(&mut buffer, png::CompressionType::Best, png::FilterType::Adaptive).write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(buffer))
}

pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Png)
		.map_err(|e| anyhow!("Failed to decode PNG image: {e}"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), 0.57)]
	#[case::greya(DynamicImage::new_test_greya(), 0.39)]
	#[case::rgb(DynamicImage::new_test_rgb(), 0.29)]
	#[case::rgba(DynamicImage::new_test_rgba(), 0.33)]
	fn png_ok(#[case] img: DynamicImage, #[case] expected_compression_percent: f64) -> Result<()> {
		let blob = image2blob(&img)?;
		let decoded = blob2image(&blob)?;

		assert_eq!(img.diff(&decoded)?, vec![0.0; img.channel_count() as usize]);

		let compression_percent = ((10_000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0;
		assert_eq!(compression_percent, expected_compression_percent);

		Ok(())
	}
}
