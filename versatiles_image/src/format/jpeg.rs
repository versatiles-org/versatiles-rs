use crate::traits::DynamicImageTraitInfo;
use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, ImageEncoder, ImageFormat, codecs::jpeg::JpegEncoder, load_from_memory_with_format};
use versatiles_core::Blob;

pub fn encode(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("JPEG only supports 8-bit images");
	}

	let quality = quality.unwrap_or(95);
	if quality >= 100 {
		bail!("JPEG does not support lossless compression, use a quality < 100");
	}

	match image.channel_count() {
		1 | 3 => image,
		_ => bail!("JPEG only supports Grey or RGB images without alpha channel"),
	};

	let mut buffer: Vec<u8> = Vec::new();
	JpegEncoder::new_with_quality(&mut buffer, quality).write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(buffer))
}

pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	encode(image, quality)
}

pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Jpeg)
		.map_err(|e| anyhow!("Failed to decode JPEG image: {e}"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::traits::DynamicImageTraitTest;
	use rstest::rstest;

	/* ---------- Success cases (no alpha) ---------- */
	#[rstest]
	#[case::grey( DynamicImage::new_test_grey(),  6.61, vec![0.0]           )]
	#[case::rgb(  DynamicImage::new_test_rgb(),   4.65, vec![0.6, 0.3, 0.7] )]
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

	/* ---------- Error cases (alpha not supported) ---------- */
	#[rstest]
	#[case::greya(DynamicImage::new_test_greya())]
	#[case::rgba(DynamicImage::new_test_rgba())]
	fn jpeg_rejects_alpha_images(#[case] img: DynamicImage) {
		let err = image2blob(&img, None).unwrap_err();
		assert_eq!(
			format!("{err}"),
			"JPEG only supports Grey or RGB images without alpha channel"
		);
	}
}
