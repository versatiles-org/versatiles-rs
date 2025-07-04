use crate::EnhancedDynamicImageTrait;
use anyhow::{anyhow, bail, Result};
use image::{codecs::webp::WebPEncoder, load_from_memory_with_format, DynamicImage, ImageFormat};
use std::vec;
use versatiles_core::types::Blob;

pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("webp only supports 8-bit images");
	}

	if (image.channel_count() != 3) && (image.channel_count() != 4) {
		bail!("webp only supports RGB or RGBA images");
	};

	let encoder = webp::Encoder::from_image(image).map_err(|e| anyhow!("{e}"))?;

	Ok(Blob::from(
		encoder
			.encode_simple(false, quality.unwrap_or(95) as f32)
			.map_err(|e| anyhow!("{e:?}"))?
			.to_vec(),
	))
}

pub fn image2blob_lossless(image: &DynamicImage) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("webp lossless only supports 8-bit images");
	}

	if (image.channel_count() != 3) && (image.channel_count() != 4) {
		bail!("webp lossless only supports RGB or RGBA images");
	};

	let mut result: Vec<u8> = vec![];
	let encoder = WebPEncoder::new_lossless(&mut result);
	encoder.encode(
		image.as_bytes(),
		image.width(),
		image.height(),
		image.extended_color_type(),
	)?;

	Ok(Blob::from(result))
}

pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::WebP)
		.map_err(|e| anyhow!("Failed to decode WebP image: {e}"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	#[rstest]
	#[case::rgb(          DynamicImage::new_test_rgb(),  false, 0.96, vec![1.4, 0.8, 1.9]     )]
	#[case::rgba(         DynamicImage::new_test_rgba(), false, 0.76, vec![1.4, 0.8, 2.0, 0.0])]
	#[case::lossless_rgb( DynamicImage::new_test_rgb(),  true,  0.08, vec![0.0, 0.0, 0.0]     )]
	#[case::lossless_rgba(DynamicImage::new_test_rgba(), true,  0.07, vec![0.0, 0.0, 0.0, 0.0])]
	fn webp_ok(
		#[case] img: DynamicImage,
		#[case] lossless: bool,
		#[case] expected_compression_percent: f64,
		#[case] expected_diff: Vec<f64>,
	) -> Result<()> {
		let blob = if lossless {
			image2blob_lossless(&img)?
		} else {
			image2blob(&img, None)?
		};

		assert_eq!(img.diff(&blob2image(&blob)?)?, expected_diff);

		assert_eq!(
			((10000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0,
			expected_compression_percent
		);

		Ok(())
	}

	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), false, "webp only supports RGB or RGBA images")]
	#[case::greya(DynamicImage::new_test_greya(), false, "webp only supports RGB or RGBA images")]
	#[case::lossless_grey(
		DynamicImage::new_test_grey(),
		true,
		"webp lossless only supports RGB or RGBA images"
	)]
	#[case::lossless_greya(
		DynamicImage::new_test_greya(),
		true,
		"webp lossless only supports RGB or RGBA images"
	)]
	fn webp_errors(#[case] img: DynamicImage, #[case] lossless: bool, #[case] expected_msg: &str) {
		let res = if lossless {
			image2blob_lossless(&img)
		} else {
			image2blob(&img, None)
		};
		assert_eq!(res.unwrap_err().to_string(), expected_msg);
	}
}
