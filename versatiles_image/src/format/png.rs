use anyhow::Result;
use image::{
	codecs::png, load_from_memory_with_format, DynamicImage, ExtendedColorType, ImageEncoder,
	ImageFormat,
};
use versatiles_core::types::Blob;

pub fn image2blob(image: &DynamicImage, best: bool) -> Result<Blob> {
	let mut buffer: Vec<u8> = Vec::new();
	png::PngEncoder::new_with_quality(
		&mut buffer,
		if best {
			png::CompressionType::Best
		} else {
			png::CompressionType::Fast
		},
		png::FilterType::Adaptive,
	)
	.write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		ExtendedColorType::from(image.color()),
	)?;

	Ok(Blob::from(buffer))
}

pub fn blob2image(blob: &Blob) -> Result<DynamicImage> {
	Ok(load_from_memory_with_format(
		blob.as_slice(),
		ImageFormat::Png,
	)?)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{
		compare_images, create_image_grey, create_image_greya, create_image_rgb, create_image_rgba,
	};

	#[test]
	fn png_best() -> Result<()> {
		let image1 = create_image_grey();
		compare_images(blob2image(&image2blob(&image1, true)?)?, image1, 0);

		let image2 = create_image_greya();
		compare_images(blob2image(&image2blob(&image2, true)?)?, image2, 0);

		let image3 = create_image_rgb();
		compare_images(blob2image(&image2blob(&image3, true)?)?, image3, 0);

		let image4 = create_image_rgba();
		compare_images(blob2image(&image2blob(&image4, true)?)?, image4, 0);

		Ok(())
	}

	#[test]
	fn png_fast() -> Result<()> {
		let image1 = create_image_grey();
		compare_images(blob2image(&image2blob(&image1, false)?)?, image1, 0);

		let image2 = create_image_greya();
		compare_images(blob2image(&image2blob(&image2, false)?)?, image2, 0);

		let image3 = create_image_rgb();
		compare_images(blob2image(&image2blob(&image3, false)?)?, image3, 0);

		let image4 = create_image_rgba();
		compare_images(blob2image(&image2blob(&image4, false)?)?, image4, 0);

		Ok(())
	}
}
