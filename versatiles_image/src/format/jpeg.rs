use crate::types::Blob;
use anyhow::Result;
use image::{
	codecs::jpeg::JpegEncoder, load_from_memory_with_format, DynamicImage, ExtendedColorType,
	ImageEncoder, ImageFormat,
};

const JPEG_QUALITY: u8 = 95;

pub fn image2blob(image: &DynamicImage) -> Result<Blob> {
	let mut buffer: Vec<u8> = Vec::new();
	JpegEncoder::new_with_quality(&mut buffer, JPEG_QUALITY).write_image(
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
		ImageFormat::Jpeg,
	)?)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::helper::{compare_images, create_image_grey, create_image_rgb};

	/// Test JPEG encoding and decoding for grayscale and RGB images
	#[test]
	fn jpg() -> Result<()> {
		let image1 = create_image_grey();
		compare_images(blob2image(&image2blob(&image1)?)?, image1, 0);

		let image2 = create_image_rgb();
		compare_images(blob2image(&image2blob(&image2)?)?, image2, 4);

		Ok(())
	}
}
