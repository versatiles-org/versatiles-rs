use crate::types::Blob;
use anyhow::{bail, Result};
use image::DynamicImage;
use webp::{Decoder, Encoder};

const WEBP_QUALITY: f32 = 95.0;

pub fn image2blob(image: &DynamicImage) -> Result<Blob> {
	match image.color() {
		image::ColorType::Rgb8 | image::ColorType::Rgba8 => Ok(Blob::from(
			Encoder::from_image(image)
				.map_err(|e| anyhow::Error::msg(e.to_owned()))?
				.encode(WEBP_QUALITY)
				.to_vec(),
		)),
		_ => bail!("currently only 8 bit RGB/RGBA is supported for WebP lossy encoding"),
	}
}

pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	let decoder = Decoder::new(blob.as_slice());
	let image = decoder.decode();
	if let Some(image) = image {
		Ok(image.to_image())
	} else {
		bail!("cant read webp")
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{
		compare_images, create_image_grey, create_image_greya, create_image_rgb, create_image_rgba,
	};

	/// Test WebP encoding and decoding for grayscale, grayscale with alpha, RGB, and RGBA images
	#[test]
	fn webp() -> Result<()> {
		assert!(image2blob(&create_image_grey()).is_err());

		assert!(image2blob(&create_image_greya()).is_err());

		let image3 = create_image_rgb();
		compare_images(blob2image(&image2blob(&image3)?)?, image3, 4);

		let image4 = create_image_rgba();
		compare_images(blob2image(&image2blob(&image4)?)?, image4, 6);

		Ok(())
	}
}
