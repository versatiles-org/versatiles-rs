use crate::EnhancedDynamicImageTrait;
use anyhow::{anyhow, bail, Result};
use image::{codecs::jpeg::JpegEncoder, load_from_memory_with_format, DynamicImage, ImageEncoder, ImageFormat};
use versatiles_core::types::Blob;

pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("jpeg only supports 8-bit images");
	}

	if image.channel_count() != 1 && image.channel_count() != 3 {
		bail!("jpeg only supports Grey or RGB images");
	}

	let mut buffer: Vec<u8> = Vec::new();
	JpegEncoder::new_with_quality(&mut buffer, quality.unwrap_or(95)).write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(buffer))
}

pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Jpeg)
		.map_err(|e| anyhow!("Failed to decode JPEG image: {e}"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{create_image_grey, create_image_greya, create_image_rgb, create_image_rgba};
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey(create_image_grey(), 6.61, vec![0.0]           )]
	#[case::rgb (create_image_rgb(),  4.65, vec![0.6, 0.3, 0.7] )]
	fn jpeg_ok(
		#[case] img: DynamicImage,
		#[case] expected_compression_percent: f64,
		#[case] expected_diff: Vec<f64>,
	) -> Result<()> {
		let blob = image2blob(&img, None)?;
		let decoded = blob2image(&blob)?;
		assert_eq!(img.diff(&decoded)?, expected_diff);

		assert_eq!(
			((10000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0,
			expected_compression_percent
		);

		Ok(())
	}

	/* ---------- Error cases ---------- */
	#[rstest]
	#[case::greya(create_image_greya(), "jpeg only supports Grey or RGB images")]
	#[case::rgba(create_image_rgba(), "jpeg only supports Grey or RGB images")]
	fn jpeg_errors(#[case] img: DynamicImage, #[case] expected_msg: &str) {
		assert_eq!(image2blob(&img, None).unwrap_err().to_string(), expected_msg);
	}
}
