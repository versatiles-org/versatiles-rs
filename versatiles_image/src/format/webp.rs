use crate::traits::*;
use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, ImageFormat, codecs::webp::WebPEncoder, load_from_memory_with_format};
use std::vec;
use versatiles_core::Blob;

pub fn compress(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("webp only supports 8-bit images");
	}

	if (image.channel_count() != 3) && (image.channel_count() != 4) {
		bail!("webp only supports RGB or RGBA images");
	};

	let mut image_ref = image;
	#[allow(unused_assignments)]
	let mut optional_image: Option<DynamicImage> = None;
	if image.has_alpha() && image.is_opaque() {
		let i = image.as_no_alpha()?;
		optional_image = Some(i);
		image_ref = optional_image.as_ref().unwrap();
	}

	let quality = quality.unwrap_or(95);

	if quality >= 100 {
		let mut result: Vec<u8> = vec![];
		let encoder = WebPEncoder::new_lossless(&mut result);
		encoder.encode(
			image_ref.as_bytes(),
			image_ref.width(),
			image_ref.height(),
			image_ref.extended_color_type(),
		)?;
		Ok(Blob::from(result))
	} else {
		let encoder = webp::Encoder::from_image(image_ref).map_err(|e| anyhow!("{e}"))?;
		Ok(Blob::from(
			encoder
				.encode_simple(false, quality as f32)
				.map_err(|e| anyhow!("{e:?}"))?
				.to_vec(),
		))
	}
}

pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	compress(image, quality)
}

pub fn image2blob_lossless(image: &DynamicImage) -> Result<Blob> {
	compress(image, Some(100))
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
	#[case::rgb(          DynamicImage::new_test_rgb(),  false, 0.96, vec![0.9, 0.5, 1.5]     )]
	#[case::rgba(         DynamicImage::new_test_rgba(), false, 0.76, vec![0.9, 0.5, 1.6, 0.0])]
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
	#[case::lossless_grey(DynamicImage::new_test_grey(), true, "webp only supports RGB or RGBA images")]
	#[case::lossless_greya(DynamicImage::new_test_greya(), true, "webp only supports RGB or RGBA images")]
	fn webp_errors(#[case] img: DynamicImage, #[case] lossless: bool, #[case] expected_msg: &str) {
		let res = if lossless {
			image2blob_lossless(&img)
		} else {
			image2blob(&img, None)
		};
		assert_eq!(res.unwrap_err().to_string(), expected_msg);
	}

	#[rstest]
	//#[case::greya(DynamicImage::new_test_greya())]
	#[case::rgba(DynamicImage::new_test_rgba())]
	#[test]
	fn opaque_is_saved_without_alpha(#[case] mut img: DynamicImage) -> Result<()> {
		assert!(img.has_alpha());
		img.make_opaque()?;
		assert!(!blob2image(&compress(&img, Some(80))?)?.has_alpha());
		assert!(!blob2image(&compress(&img, Some(100))?)?.has_alpha());
		Ok(())
	}
}
