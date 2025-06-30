use crate::Image;
use anyhow::{bail, Result};
use image::{
	codecs::avif::{AvifEncoder, ColorSpace},
	load_from_memory_with_format, ImageEncoder, ImageFormat,
};
use versatiles_core::types::Blob;

pub fn image2blob(image: &Image, quality: Option<u8>) -> Result<Blob> {
	if image.value_type != crate::PixelValueType::U8 {
		bail!("avif only supports 8-bit images");
	}

	let mut result: Vec<u8> = vec![];
	let encoder = AvifEncoder::new_with_speed_quality(&mut result, 4, quality.unwrap_or(90))
		.with_colorspace(ColorSpace::Srgb)
		.with_num_threads(Some(1));

	encoder.write_image(
		&image.data,
		image.width as u32,
		image.height as u32,
		image.get_extended_color_type()?,
	)?;

	Ok(Blob::from(result))
}

pub fn blob2image(blob: &Blob) -> Result<Image> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Avif)?.try_into()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{create_image_grey, create_image_greya, create_image_rgb, create_image_rgba};
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey( create_image_grey(),  0.88, vec![0.1, 0.1, 0.3, 0.0])]
	#[case::greya(create_image_greya(), 0.65, vec![0.1, 0.1, 0.3, 0.1])]
	#[case::rgb(  create_image_rgb(),   0.29, vec![0.2, 0.1, 0.1, 0.0])]
	#[case::rgba( create_image_rgba(),  0.32, vec![0.2, 0.1, 0.2, 0.0])]
	fn avif_ok(
		#[case] img: Image,
		#[case] expected_compression_percent: f64,
		#[case] expected_diff: Vec<f64>,
	) -> Result<()> {
		let blob = image2blob(&img, None)?;
		let decoded = blob2image(&blob)?;

		assert_eq!(img.as_rgba()?.diff(decoded)?, expected_diff);

		let compression_percent = ((10_000 * blob.len()) as f64 / img.data.len() as f64).round() / 100.0;
		assert_eq!(compression_percent, expected_compression_percent);

		Ok(())
	}
}
