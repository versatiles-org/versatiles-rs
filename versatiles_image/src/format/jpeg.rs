use crate::traits::*;
use anyhow::{Result, anyhow, bail};
use image::{DynamicImage, ImageEncoder, ImageFormat, codecs::jpeg::JpegEncoder, load_from_memory_with_format};
use versatiles_core::Blob;

pub fn compress(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	if image.bits_per_value() != 8 {
		bail!("jpeg only supports 8-bit images");
	}

	let quality = quality.unwrap_or(95);
	if quality >= 100 {
		bail!("jpeg does not support lossless compression, use a quality < 100");
	}

	// Will hold a converted copy *if* we need one.
	let mut _temp: Option<DynamicImage> = None;

	// `img` is the reference we pass to the encoder.
	let img: &DynamicImage = match image.channel_count() {
		1 | 3 => image, // already Grey or RGB → keep original borrow
		2 => {
			_temp = Some(DynamicImage::ImageLuma8(image.to_luma8()));
			_temp.as_ref().unwrap()
		}
		4 => {
			_temp = Some(DynamicImage::ImageRgb8(image.to_rgb8()));
			_temp.as_ref().unwrap()
		}
		_ => bail!("jpeg only supports Grey or RGB images"),
	};

	let mut buffer: Vec<u8> = Vec::new();
	JpegEncoder::new_with_quality(&mut buffer, quality).write_image(
		img.as_bytes(),
		img.width(),
		img.height(),
		img.extended_color_type(),
	)?;

	Ok(Blob::from(buffer))
}

pub fn image2blob(image: &DynamicImage, quality: Option<u8>) -> Result<Blob> {
	compress(image, quality)
}

pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Jpeg)
		.map_err(|e| anyhow!("Failed to decode JPEG image: {e}"))
}

#[cfg(test)]
mod tests {
	use super::*;
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey( DynamicImage::new_test_grey(),  6.61, vec![0.0]           )]
	#[case::greya(DynamicImage::new_test_greya(), 3.30, vec![0.0, 21717.5]  )]
	#[case::rgb(  DynamicImage::new_test_rgb(),   4.65, vec![0.6, 0.3, 0.7] )]
	#[case::rgba( DynamicImage::new_test_rgba(),  3.49, vec![0.6, 0.3, 0.7, 21717.5] )]
	fn jpeg_ok(
		#[case] img: DynamicImage,
		#[case] expected_compression_percent: f64,
		#[case] expected_diff: Vec<f64>,
	) -> Result<()> {
		let blob = image2blob(&img, None)?;
		let mut decoded = blob2image(&blob)?;
		match img {
			DynamicImage::ImageLumaA8(_) => {
				decoded = DynamicImage::ImageLumaA8(decoded.to_luma_alpha8());
			}
			DynamicImage::ImageRgba8(_) => {
				decoded = DynamicImage::ImageRgba8(decoded.to_rgba8());
			}
			_ => {}
		}
		assert_eq!(img.diff(&decoded)?, expected_diff);

		assert_eq!(
			((10000 * blob.len()) as f64 / img.as_bytes().len() as f64).round() / 100.0,
			expected_compression_percent
		);

		Ok(())
	}
}
