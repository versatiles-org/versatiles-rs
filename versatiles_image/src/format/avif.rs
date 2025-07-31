use crate::EnhancedDynamicImageTrait;
use anyhow::{Result, bail};
use image::{
	DynamicImage, ImageEncoder,
	codecs::avif::{AvifEncoder, ColorSpace},
};
use versatiles_core::Blob;

pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("avif only supports 8-bit images");
	}

	let quality = quality.unwrap_or(90);
	if quality >= 100 {
		bail!("Lossless AVIF encoding is not supported, quality must be less than 100");
	}

	let mut result: Vec<u8> = vec![];
	let encoder = AvifEncoder::new_with_speed_quality(&mut result, 10, quality)
		.with_colorspace(ColorSpace::Srgb)
		.with_num_threads(Some(1));

	encoder.write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(result))
}

pub fn image2blob_lossless(image: &DynamicImage) -> Result<Blob> {
	image2blob(image, Some(100))
}

pub fn blob2image(_blob: &Blob) -> Result<DynamicImage> {
	bail!("AVIF decoding not implemented")
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), 2.01)]
	#[case::greya(DynamicImage::new_test_greya(), 1.23)]
	#[case::rgb(DynamicImage::new_test_rgb(), 0.59)]
	#[case::rgba(DynamicImage::new_test_rgba(), 0.55)]
	fn avif_ok(#[case] img: DynamicImage, #[case] expected_compression_percent: f64) -> Result<()> {
		let blob = image2blob(&img, None)?;

		let compression_percent = ((10_000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0;
		assert_eq!(compression_percent, expected_compression_percent);

		Ok(())
	}

	#[rstest]
	#[case::grey(DynamicImage::new_test_grey())]
	#[case::greya(DynamicImage::new_test_greya())]
	#[case::rgb(DynamicImage::new_test_rgb())]
	#[case::rgba(DynamicImage::new_test_rgba())]
	fn avif_lossless_ok(#[case] img: DynamicImage) -> Result<()> {
		assert_eq!(
			image2blob_lossless(&img).unwrap_err().to_string(),
			"Lossless AVIF encoding is not supported, quality must be less than 100"
		);

		Ok(())
	}
}
