use crate::Image;
use anyhow::{bail, Result};
use image::{
	codecs::avif::{AvifEncoder, ColorSpace},
	ImageEncoder,
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

pub fn blob2image(_blob: &Blob) -> Result<Image> {
	bail!("AVIF decoding not implemented")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{create_image_grey, create_image_greya, create_image_rgb, create_image_rgba};
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey(create_image_grey(), 0.88)]
	#[case::greya(create_image_greya(), 0.65)]
	#[case::rgb(create_image_rgb(), 0.29)]
	#[case::rgba(create_image_rgba(), 0.32)]
	fn avif_ok(#[case] img: Image, #[case] expected_compression_percent: f64) -> Result<()> {
		let blob = image2blob(&img, None)?;

		let compression_percent = ((10_000 * blob.len()) as f64 / img.data.len() as f64).round() / 100.0;
		assert_eq!(compression_percent, expected_compression_percent);

		Ok(())
	}
}
