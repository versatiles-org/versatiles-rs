use anyhow::{bail, Result};
use image::DynamicImage;
use versatiles_core::types::Blob;
use webp::{Decoder, Encoder};

pub fn image2blob(image: &DynamicImage) -> Result<Blob> {
	match image.color() {
		image::ColorType::Rgb8 => Ok(Blob::from(
			Encoder::from_image(image)
				.map_err(|e| anyhow::Error::msg(e.to_owned()))?
				.encode_lossless()
				.to_vec(),
		)),
		_ => bail!("currently only 8 bit RGB is supported for WebP lossless encoding"),
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

	#[test]
	fn grey() {
		let i = create_image_grey();
		assert!(image2blob(&i).is_err());
	}

	#[test]
	fn greya() {
		let i = create_image_greya();
		assert!(image2blob(&i).is_err());
	}

	#[test]
	fn rgb() {
		let i = create_image_rgb();
		compare_images(blob2image(&image2blob(&i).unwrap()).unwrap(), i, 0);
	}

	#[test]
	fn rgba() {
		let i = create_image_rgba();
		assert!(image2blob(&i).is_err());
	}
}
