use crate::types::Blob;
use anyhow::Result;
use image::{
	codecs::png, load_from_memory_with_format, DynamicImage, ExtendedColorType, ImageEncoder,
	ImageFormat,
};

pub fn image2blob(image: &DynamicImage) -> Result<Blob> {
	let mut buffer: Vec<u8> = Vec::new();
	png::PngEncoder::new_with_quality(
		&mut buffer,
		png::CompressionType::Best,
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

	/// Test PNG encoding and decoding for grayscale images
	#[test]
	fn png() -> Result<()> {
		let image1 = create_image_grey();
		compare_images(blob2image(&image2blob(&image1)?)?, image1, 0);

		let image2 = create_image_greya();
		compare_images(blob2image(&image2blob(&image2)?)?, image2, 0);

		let image3 = create_image_rgb();
		compare_images(blob2image(&image2blob(&image3)?)?, image3, 0);

		let image4 = create_image_rgba();
		compare_images(blob2image(&image2blob(&image4)?)?, image4, 0);

		Ok(())
	}
}
