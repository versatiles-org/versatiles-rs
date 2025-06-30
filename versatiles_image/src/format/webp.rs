use crate::Image;
use anyhow::{anyhow, bail, Result};
use image::codecs::webp::WebPEncoder;
use std::vec;
use versatiles_core::types::Blob;

pub fn image2blob(image: &Image, quality: Option<u8>) -> Result<Blob> {
	if image.value_type != crate::PixelValueType::U8 {
		bail!("webp only supports 8-bit images");
	}

	let encoder = match image.channels {
		3 => webp::Encoder::from_rgb(&image.data, image.width as u32, image.height as u32),
		4 => webp::Encoder::from_rgba(&image.data, image.width as u32, image.height as u32),
		_ => bail!("webp only supports RGB or RGBA images"),
	};

	Ok(Blob::from(
		encoder
			.encode_simple(false, quality.unwrap_or(95) as f32)
			.map_err(|e| anyhow!("{e:?}"))?
			.to_vec(),
	))
}

pub fn image2blob_lossless(image: &Image) -> Result<Blob> {
	if image.value_type != crate::PixelValueType::U8 {
		bail!("webp lossless only supports 8-bit images");
	}

	if (image.channels != 3) && (image.channels != 4) {
		bail!("webp lossless only supports RGB or RGBA images");
	};

	let mut result: Vec<u8> = vec![];
	let encoder = WebPEncoder::new_lossless(&mut result);
	encoder.encode(
		&image.data,
		image.width as u32,
		image.height as u32,
		image.get_extended_color_type()?,
	)?;

	Ok(Blob::from(result))
}

pub fn blob2image(blob: &Blob) -> Result<Image> {
	let decoder = webp::Decoder::new(blob.as_slice());
	let image = decoder.decode();
	if let Some(image) = image {
		Ok(Image::from_rgb(
			image.width() as usize,
			image.height() as usize,
			image.is_alpha(),
			(*image).to_vec(),
		))
	} else {
		bail!("cant read webp")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{create_image_grey, create_image_greya, create_image_rgb, create_image_rgba};
	use rstest::rstest;

	#[rstest]
	#[case::rgb(          create_image_rgb(),  false, 0.96, vec![0.9, 0.5, 1.5]     )]
	#[case::rgba(         create_image_rgba(), false, 0.76, vec![0.9, 0.5, 1.6, 0.0])]
	#[case::lossless_rgb( create_image_rgb(),  true,  0.08, vec![0.0, 0.0, 0.0]     )]
	#[case::lossless_rgba(create_image_rgba(), true,  0.07, vec![0.0, 0.0, 0.0, 0.0])]
	fn webp_ok(
		#[case] img: Image,
		#[case] lossless: bool,
		#[case] expected_compression_percent: f64,
		#[case] expected_diff: Vec<f64>,
	) -> Result<()> {
		let blob = if lossless {
			image2blob_lossless(&img)?
		} else {
			image2blob(&img, None)?
		};

		assert_eq!(img.diff(blob2image(&blob)?)?, expected_diff);

		assert_eq!(
			((10000 * blob.len()) as f64 / img.data.len() as f64).round() / 100.0,
			expected_compression_percent
		);

		Ok(())
	}

	#[rstest]
	#[case::grey(create_image_grey(), false, "webp only supports RGB or RGBA images")]
	#[case::greya(create_image_greya(), false, "webp only supports RGB or RGBA images")]
	#[case::lossless_grey(create_image_grey(), true, "webp lossless only supports RGB or RGBA images")]
	#[case::lossless_greya(create_image_greya(), true, "webp lossless only supports RGB or RGBA images")]
	fn webp_errors(#[case] img: Image, #[case] lossless: bool, #[case] expected_msg: &str) {
		let res = if lossless {
			image2blob_lossless(&img)
		} else {
			image2blob(&img, None)
		};
		assert_eq!(res.unwrap_err().to_string(), expected_msg);
	}
}
