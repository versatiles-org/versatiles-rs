use crate::types::Blob;
use anyhow::{bail, Result};
use image::{
	codecs::{
		//avif,
		jpeg,
		png,
	},
	load_from_memory_with_format, DynamicImage, ExtendedColorType, ImageEncoder, ImageFormat,
};
use webp::{Decoder, Encoder};

const JPEG_QUALITY: u8 = 95;
const WEBP_QUALITY: f32 = 95.0;

/// Encodes a DynamicImage into PNG format and returns it as a Blob.
/// # Arguments
/// * `image` - A `DynamicImage` object representing the image to encode.
/// # Returns
/// A `Blob` object containing the PNG-encoded image.
pub fn img2png(image: &DynamicImage) -> Result<Blob> {
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

/// Decodes a PNG-encoded image from a Blob and returns it as a DynamicImage.
/// # Arguments
/// * `data` - A `Blob` object containing the PNG-encoded image data.
/// # Returns
/// A `DynamicImage` object representing the decoded image.
pub fn png2img(blob: &Blob) -> Result<DynamicImage> {
	Ok(load_from_memory_with_format(
		blob.as_slice(),
		ImageFormat::Png,
	)?)
}

/// Encodes a DynamicImage into AVIF format and returns it as a Blob.
/// # Arguments
/// * `image` - A `DynamicImage` object representing the image to encode.
/// # Returns
/// A `Blob` object containing the AVIF-encoded image.
//pub fn img2avif(image: DynamicImage) -> Result<Blob> {
//	let mut buffer: Vec<u8> = Vec::new();
//	avif::AvifEncoder::new_with_speed_quality(&mut buffer, 1, 100).write_image(
//		image.as_bytes(),
//		image.width(),
//		image.height(),
//		ExtendedColorType::from(image.color()),
//	)?;
//
//	Ok(Blob::from(buffer))
//}

/// Decodes a AVIF-encoded image from a Blob and returns it as a DynamicImage.
/// # Arguments
/// * `data` - A `Blob` object containing the AVIF-encoded image data.
/// # Returns
/// A `DynamicImage` object representing the decoded image.
//pub fn avif2img(blob: Blob) -> Result<DynamicImage> {
//	println!("hallo {}", blob.len());
//	let image = load_from_memory_with_format(blob.as_slice(), ImageFormat::Avif)?;
//	println!("duda");
//	Ok(image)
//}

/// Encodes a DynamicImage into JPEG format and returns it as a Blob.
/// # Arguments
/// * `image` - A `DynamicImage` object representing the image to encode.
/// # Returns
/// A `Blob` object containing the JPEG-encoded image.
pub fn img2jpg(image: &DynamicImage) -> Result<Blob> {
	let mut buffer: Vec<u8> = Vec::new();
	jpeg::JpegEncoder::new_with_quality(&mut buffer, JPEG_QUALITY).write_image(
		image.as_bytes(),
		image.width(),
		image.height(),
		ExtendedColorType::from(image.color()),
	)?;

	Ok(Blob::from(buffer))
}

/// Decodes a JPEG-encoded image from a Blob and returns it as a DynamicImage.
/// # Arguments
/// * `data` - A `Blob` object containing the JPEG-encoded image data.
/// # Returns
/// A `DynamicImage` object representing the decoded image.
pub fn jpg2img(blob: &Blob) -> Result<DynamicImage> {
	Ok(load_from_memory_with_format(
		blob.as_slice(),
		ImageFormat::Jpeg,
	)?)
}

/// Encodes a DynamicImage into WebP format and returns it as a Blob.
/// # Arguments
/// * `image` - A `DynamicImage` object representing the image to encode.
/// # Returns
/// A `Blob` object containing the WebP-encoded image.
/// # Panics
/// Panics if the image color type is not 8-bit RGB or RGBA, as the crate "WebP" only supports these formats.
pub fn img2webp(image: &DynamicImage) -> Result<Blob> {
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

/// Encodes a DynamicImage into WebP lossless format and returns it as a Blob.
/// # Arguments
/// * `image` - A reference to a `DynamicImage` that is to be encoded.
/// # Panics
/// This function will panic if the image color type is not `Rgb8`, since currently only 8-bit RGB is supported for WebP lossless encoding.
/// # Returns
/// A `Blob` containing the WebP-encoded image data.
pub fn img2webplossless(image: &DynamicImage) -> Result<Blob> {
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

/// Decodes an image from WebP format.
/// # Arguments
/// * `data` - A `Blob` containing the WebP-encoded image data.
/// # Returns
/// A `DynamicImage` containing the decoded image.
pub fn webp2img(blob: &Blob) -> Result<DynamicImage> {
	let decoder = Decoder::new(blob.as_slice());
	let image = decoder.decode();
	if let Some(image) = image {
		Ok(image.to_image())
	} else {
		bail!("cant read webp")
	}
}

#[cfg(test)]
use image::{GrayAlphaImage, GrayImage, Luma, LumaA, Rgb, RgbImage, Rgba, RgbaImage};

/// Generate a DynamicImage with RGBA colors
#[cfg(test)]
pub fn create_image_rgba() -> DynamicImage {
	DynamicImage::ImageRgba8(RgbaImage::from_fn(256, 256, |x, y| -> Rgba<u8> {
		Rgba([x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
	}))
}

/// Generate a DynamicImage with RGB colors
#[cfg(test)]
pub fn create_image_rgb() -> DynamicImage {
	DynamicImage::ImageRgb8(RgbImage::from_fn(256, 256, |x, y| -> Rgb<u8> {
		Rgb([x as u8, (255 - x) as u8, y as u8])
	}))
}

/// Generate a DynamicImage with grayscale colors
/// Returns a DynamicImage with 256x256 grayscale colors from black to white. Each pixel in the image
/// is a Luma<u8> value.
#[cfg(test)]
pub fn create_image_grey() -> DynamicImage {
	DynamicImage::ImageLuma8(GrayImage::from_fn(256, 256, |x, _y| -> Luma<u8> {
		Luma([x as u8])
	}))
}

/// Generate a DynamicImage with grayscale alpha colors
/// Returns a DynamicImage with 256x256 grayscale alpha colors from black to white. Each pixel in the
/// image is a LumaA<u8> value, with the alpha value determined by the y coordinate.
#[cfg(test)]
pub fn create_image_greya() -> DynamicImage {
	DynamicImage::ImageLumaA8(GrayAlphaImage::from_fn(256, 256, |x, y| -> LumaA<u8> {
		LumaA([x as u8, y as u8])
	}))
}

/// Compare two DynamicImages for similarity
/// Compares two DynamicImages to ensure that they have the same dimensions and that the maximum
/// difference between the pixel values in each image is less than or equal to a given threshold.
/// # Arguments
/// * `image1` - The first DynamicImage to compare
/// * `image2` - The second DynamicImage to compare
/// * `max_allowed_diff` - The maximum allowed difference between pixel values in the two images
/// # Panics
/// This function will panic if the two images have different dimensions or if the maximum difference
/// between the pixel values in the two images is greater than the specified threshold.
#[cfg(test)]
pub fn compare_images(image1: DynamicImage, image2: DynamicImage, max_allowed_diff: u8) {
	assert_eq!(image1.width(), image2.width());
	assert_eq!(image1.height(), image2.height());

	let bytes1 = image1.as_bytes();
	let bytes2 = image2.as_bytes();
	assert_eq!(bytes1.len(), bytes2.len());

	let mut max_diff: u8 = 0;
	for (c1, c2) in bytes1.iter().zip(bytes2) {
		let diff = c1.abs_diff(*c2);
		if diff > max_diff {
			max_diff = diff;
		}
	}

	assert!(
		max_diff <= max_allowed_diff,
		"max_diff ({max_diff}) > max_allowed_diff ({max_allowed_diff})"
	);
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Test PNG encoding and decoding for grayscale images
	#[test]
	fn png() -> Result<()> {
		let image1 = create_image_grey();
		compare_images(png2img(&img2png(&image1)?)?, image1, 0);

		let image2 = create_image_greya();
		compare_images(png2img(&img2png(&image2)?)?, image2, 0);

		let image3 = create_image_rgb();
		compare_images(png2img(&img2png(&image3)?)?, image3, 0);

		let image4 = create_image_rgba();
		compare_images(png2img(&img2png(&image4)?)?, image4, 0);

		Ok(())
	}

	/// Test JPEG encoding and decoding for grayscale and RGB images
	#[test]
	fn jpg() -> Result<()> {
		let image1 = create_image_grey();
		compare_images(jpg2img(&img2jpg(&image1)?)?, image1, 0);

		let image3 = create_image_rgb();
		compare_images(jpg2img(&img2jpg(&image3)?)?, image3, 4);

		Ok(())
	}

	/// Test WebP encoding and decoding for grayscale, grayscale with alpha, RGB, and RGBA images
	#[test]
	fn webp() -> Result<()> {
		assert!(img2webp(&create_image_grey()).is_err());

		assert!(img2webp(&create_image_greya()).is_err());

		let image3 = create_image_rgb();
		compare_images(webp2img(&img2webp(&image3)?)?, image3, 4);

		let image4 = create_image_rgba();
		compare_images(webp2img(&img2webp(&image4)?)?, image4, 6);

		Ok(())
	}

	/// Test lossless WebP encoding and decoding for grayscale and grayscale with alpha images
	#[test]
	fn webplossless() -> Result<()> {
		assert!(img2webplossless(&create_image_grey()).is_err());

		assert!(img2webplossless(&create_image_greya()).is_err());

		let image3 = create_image_rgb();
		compare_images(webp2img(&img2webplossless(&image3)?)?, image3, 0);

		assert!(img2webplossless(&create_image_rgba()).is_err());

		Ok(())
	}
}
