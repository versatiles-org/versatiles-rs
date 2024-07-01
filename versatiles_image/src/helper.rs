use crate::format::*;
use ab_glyph::{FontArc, PxScale};
use anyhow::Result;
use image::{DynamicImage, GrayAlphaImage, GrayImage, Luma, LumaA, Rgb, RgbImage, Rgba, RgbaImage};
use imageproc::drawing::draw_text_mut;
use versatiles_core::types::{Blob, TileCoord3, TileFormat};

static mut FONT: Option<FontArc> = None;

pub fn create_dummy_image(coord: &TileCoord3) -> DynamicImage {
	let font = unsafe {
		if FONT.is_none() {
			FONT.insert(FontArc::try_from_slice(include_bytes!("../assets/trim.ttf")).unwrap())
		} else {
			FONT.as_ref().unwrap()
		}
	};

	let br = ((coord.x + coord.y) % 2) as u8 * 16 + 224;
	let mut image1 = RgbImage::from_pixel(512, 512, Rgb::from([br, br, br]));

	let mut draw = |y: i32, c: Rgb<u8>, text: String| {
		draw_text_mut(&mut image1, c, 220, y, PxScale::from(40f32), font, &text)
	};

	draw(195, Rgb([127, 30, 16]), format!("z: {}", coord.z));
	draw(225, Rgb([0, 92, 45]), format!("x: {}", coord.x));
	draw(255, Rgb([30, 23, 98]), format!("y: {}", coord.y));

	DynamicImage::ImageRgb8(image1)
}

/// Generate a DynamicImage with RGBA colors
pub fn create_image_rgba() -> DynamicImage {
	DynamicImage::ImageRgba8(RgbaImage::from_fn(256, 256, |x, y| -> Rgba<u8> {
		Rgba([x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
	}))
}

/// Generate a DynamicImage with RGB colors
pub fn create_image_rgb() -> DynamicImage {
	DynamicImage::ImageRgb8(RgbImage::from_fn(256, 256, |x, y| -> Rgb<u8> {
		Rgb([x as u8, (255 - x) as u8, y as u8])
	}))
}

/// Generate a DynamicImage with grayscale colors
/// Returns a DynamicImage with 256x256 grayscale colors from black to white. Each pixel in the image
/// is a Luma<u8> value.
pub fn create_image_grey() -> DynamicImage {
	DynamicImage::ImageLuma8(GrayImage::from_fn(256, 256, |x, _y| -> Luma<u8> {
		Luma([x as u8])
	}))
}

/// Generate a DynamicImage with grayscale alpha colors
/// Returns a DynamicImage with 256x256 grayscale alpha colors from black to white. Each pixel in the
/// image is a LumaA<u8> value, with the alpha value determined by the y coordinate.
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

pub fn image2blob(image: &DynamicImage, format: TileFormat) -> Result<Blob> {
	match format {
		TileFormat::AVIF => todo!(),
		TileFormat::BIN => todo!(),
		TileFormat::GEOJSON => todo!(),
		TileFormat::JPG => jpeg::image2blob(image),
		TileFormat::JSON => todo!(),
		TileFormat::PBF => todo!(),
		TileFormat::PNG => png::image2blob(image),
		TileFormat::SVG => todo!(),
		TileFormat::TOPOJSON => todo!(),
		TileFormat::WEBP => webp::image2blob(image),
	}
}
