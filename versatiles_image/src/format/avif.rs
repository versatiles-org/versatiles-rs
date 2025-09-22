use crate::traits::*;
use anyhow::{Result, bail};
use image::{
	DynamicImage, ImageEncoder,
	codecs::avif::{AvifEncoder, ColorSpace},
};
use versatiles_core::Blob;

pub fn encode(image: &DynamicImage, quality: Option<u8>, speed: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("avif only supports 8-bit images");
	}

	let quality = quality.unwrap_or(90);
	if quality >= 100 {
		bail!("Lossless AVIF encoding is not supported, quality must be less than 100");
	}

	let speed = speed
		.map(|s| (s as f32 / 100.0 * 9.0 + 1.0).round().clamp(1.0, 10.0) as u8)
		.unwrap_or(10);

	let mut result: Vec<u8> = vec![];
	let encoder = AvifEncoder::new_with_speed_quality(&mut result, speed, quality)
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

pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	encode(image, quality, None)
}

pub fn image2blob_lossless(image: &DynamicImage) -> Result<Blob> {
	encode(image, Some(100), None)
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
	#[case::grey(DynamicImage::new_test_grey(), 1.99)]
	#[case::greya(DynamicImage::new_test_greya(), 1.23)]
	#[case::rgb(DynamicImage::new_test_rgb(), 0.58)]
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

	//#[rstest]
	//#[case::greya(DynamicImage::new_test_greya())]
	//#[case::rgba(DynamicImage::new_test_rgba())]
	//#[test]
	//fn opaque_is_saved_without_alpha(#[case] mut img: DynamicImage) -> Result<()> {
	//	assert!(img.has_alpha());
	//	img.make_opaque()?;
	//	assert!(!blob2image(&compress(&img, Some(80), None)?)?.has_alpha());
	//	assert!(!blob2image(&compress(&img, Some(100), None)?)?.has_alpha());
	//	Ok(())
	//}
}
