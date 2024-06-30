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
	fn webp_lossless() -> Result<()> {
		let image1 = create_image_grey();
		assert!(image2blob(&image1).is_err());

		let image2 = create_image_greya();
		assert!(image2blob(&image2).is_err());

		let image3 = create_image_rgb();
		compare_images(blob2image(&image2blob(&image3)?)?, image3, 0);

		let image4 = create_image_rgba();
		compare_images(blob2image(&image2blob(&image4)?)?, image4, 6);

		Ok(())
	}
}
