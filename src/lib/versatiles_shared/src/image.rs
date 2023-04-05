use super::Blob;
use image::{
	codecs::{jpeg, png},
	load_from_memory_with_format, DynamicImage, ImageEncoder, ImageFormat,
};
use webp::{Decoder, Encoder};

pub fn img2png(image: &DynamicImage) -> Blob {
	let mut buffer: Vec<u8> = Vec::new();
	png::PngEncoder::new_with_quality(&mut buffer, png::CompressionType::Best, png::FilterType::Adaptive)
		.write_image(image.as_bytes(), image.width(), image.height(), image.color())
		.unwrap();

	Blob::from(buffer)
}

pub fn png2img(data: Blob) -> DynamicImage {
	load_from_memory_with_format(data.as_slice(), ImageFormat::Png).unwrap()
}

pub fn img2jpg(image: &DynamicImage) -> Blob {
	let mut buffer: Vec<u8> = Vec::new();
	jpeg::JpegEncoder::new_with_quality(&mut buffer, 95u8)
		.write_image(image.as_bytes(), image.width(), image.height(), image.color())
		.unwrap();

	Blob::from(buffer)
}

pub fn jpg2img(data: Blob) -> DynamicImage {
	load_from_memory_with_format(data.as_slice(), ImageFormat::Jpeg).unwrap()
}

pub fn img2webp(image: &DynamicImage) -> Blob {
	match image.color() {
		image::ColorType::Rgb8 => {}
		image::ColorType::Rgba8 => {}
		_ => panic!("currently only 8 bit RGB/RGBA is supported for WebP lossy encoding"),
	}
	let encoder = Encoder::from_image(image).unwrap();
	let memory = encoder.encode(95f32);

	Blob::from(memory.to_vec())
}

pub fn img2webplossless(image: &DynamicImage) -> Blob {
	match image.color() {
		image::ColorType::Rgb8 => {}
		_ => panic!("currently only 8 bit RGB is supported for WebP lossless encoding"),
	}
	let encoder = Encoder::from_image(image).unwrap();
	let memory = encoder.encode_lossless();
	Blob::from(memory.to_vec())
}

pub fn webp2img(data: Blob) -> DynamicImage {
	let decoder = Decoder::new(data.as_slice());
	let image = decoder.decode().unwrap();
	image.to_image()
	//load_from_memory_with_format(data.as_slice(), ImageFormat::WebP).unwrap()
}

#[cfg(test)]
mod tests {
	use std::panic;

	use crate::*;
	use ::image::{DynamicImage, GrayAlphaImage, GrayImage, Luma, LumaA, Rgb, RgbImage, Rgba, RgbaImage};

	#[test]
	fn png() {
		let image1 = get_image_grey();
		compare_images(png2img(img2png(&image1)), image1, 0);

		let image2 = get_image_greya();
		compare_images(png2img(img2png(&image2)), image2, 0);

		let image3 = get_image_rgb();
		compare_images(png2img(img2png(&image3)), image3, 0);

		let image4 = get_image_rgba();
		compare_images(png2img(img2png(&image4)), image4, 0);
	}

	#[test]
	fn jpg() {
		let image1 = get_image_grey();
		compare_images(jpg2img(img2jpg(&image1)), image1, 0);

		let image3 = get_image_rgb();
		compare_images(jpg2img(img2jpg(&image3)), image3, 4);
	}

	#[test]
	fn webp() {
		assert!(panic::catch_unwind(|| {
			img2webp(&get_image_grey());
		})
		.is_err());

		assert!(panic::catch_unwind(|| {
			img2webp(&get_image_greya());
		})
		.is_err());

		let image3 = get_image_rgb();
		compare_images(webp2img(img2webp(&image3)), image3, 4);

		let image4 = get_image_rgba();
		compare_images(webp2img(img2webp(&image4)), image4, 6);
	}

	#[test]
	fn webplossless() {
		assert!(panic::catch_unwind(|| {
			img2webplossless(&get_image_grey());
		})
		.is_err());

		assert!(panic::catch_unwind(|| {
			img2webplossless(&get_image_greya());
		})
		.is_err());

		let image3 = get_image_rgb();
		compare_images(webp2img(img2webplossless(&image3)), image3, 0);

		assert!(panic::catch_unwind(|| {
			img2webplossless(&get_image_rgba());
		})
		.is_err());
	}

	fn get_image_rgba() -> DynamicImage {
		DynamicImage::ImageRgba8(RgbaImage::from_fn(256, 256, |x, y| -> Rgba<u8> {
			Rgba([x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
		}))
	}

	fn get_image_rgb() -> DynamicImage {
		DynamicImage::ImageRgb8(RgbImage::from_fn(256, 256, |x, y| -> Rgb<u8> {
			Rgb([x as u8, (255 - x) as u8, y as u8])
		}))
	}

	fn get_image_grey() -> DynamicImage {
		DynamicImage::ImageLuma8(GrayImage::from_fn(256, 256, |x, _y| -> Luma<u8> { Luma([x as u8]) }))
	}

	fn get_image_greya() -> DynamicImage {
		DynamicImage::ImageLumaA8(GrayAlphaImage::from_fn(256, 256, |x, y| -> LumaA<u8> {
			LumaA([x as u8, y as u8])
		}))
	}

	fn compare_images(image1: DynamicImage, image2: DynamicImage, max_allowed_diff: u8) {
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

		assert!(max_diff <= max_allowed_diff);
	}
}
