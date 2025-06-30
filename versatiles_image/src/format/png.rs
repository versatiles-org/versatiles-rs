use crate::Image;
use anyhow::{bail, Result};
use image::{codecs::png, load_from_memory_with_format, ImageEncoder, ImageFormat};
use versatiles_core::types::Blob;

pub fn image2blob(image: &Image) -> Result<Blob> {
	if image.value_type != crate::PixelValueType::U8 {
		bail!("png only supports 8-bit images");
	}

	if image.channels < 1 || image.channels > 4 {
		bail!("png only supports Grey, GreyA, RGB or RGBA");
	}

	let mut buffer: Vec<u8> = Vec::new();
	png::PngEncoder::new_with_quality(&mut buffer, png::CompressionType::Best, png::FilterType::Adaptive).write_image(
		&image.data,
		image.width as u32,
		image.height as u32,
		image.get_extended_color_type()?,
	)?;

	Ok(Blob::from(buffer))
}

pub fn blob2image(blob: &Blob) -> Result<Image> {
	load_from_memory_with_format(blob.as_slice(), ImageFormat::Png)?.try_into()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{create_image_grey, create_image_greya, create_image_rgb, create_image_rgba};
	use rstest::rstest;

	/* ---------- Success cases ---------- */
	#[rstest]
	#[case::grey(create_image_grey(), 0.57)]
	#[case::greya(create_image_greya(), 0.39)]
	#[case::rgb(create_image_rgb(), 0.29)]
	#[case::rgba(create_image_rgba(), 0.33)]
	fn png_ok(#[case] img: Image, #[case] expected_compression_percent: f64) -> Result<()> {
		let blob = image2blob(&img)?;
		let decoded = blob2image(&blob)?;

		assert_eq!(img.diff(decoded)?, vec![0.0; img.channels as usize]);

		let compression_percent = ((10_000 * blob.len()) as f64 / img.data.len() as f64).round() / 100.0;
		assert_eq!(compression_percent, expected_compression_percent);

		Ok(())
	}

	/* ---------- Error cases ---------- */
	#[rstest]
	#[case::wrong_bit_depth(
		{
			let mut img = create_image_rgb();
			img.value_type = crate::PixelValueType::U16; // invalid for PNG encoder
			img
		},
		"png only supports 8-bit images"
	)]
	#[case::too_many_channels(
		{
			let mut img = create_image_rgb();
			img.channels = 5; // invalid
			img
		},
		"png only supports Grey, GreyA, RGB or RGBA"
	)]
	fn png_errors(#[case] img: Image, #[case] expected_msg: &str) {
		let err = image2blob(&img).unwrap_err();
		assert_eq!(err.to_string(), expected_msg);
	}
}
