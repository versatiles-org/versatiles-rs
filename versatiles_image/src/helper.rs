use crate::{jpeg, png, webp, Image};
use anyhow::Result;
use versatiles_core::types::{Blob, TileFormat};

/// Generate a Image with RGBA colors
pub fn create_image_rgba() -> Image {
	Image::new_rgba8_from_fn(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
}

/// Generate a Image with RGB colors
pub fn create_image_rgb() -> Image {
	Image::new_rgb8_from_fn(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8])
}

/// Generate a Image with grayscale colors
/// Returns a Image with 256x256 grayscale colors from black to white. Each pixel in the image
/// is a Luma<u8> value.
pub fn create_image_grey() -> Image {
	Image::new_grey8_from_fn(256, 256, |x, _y| x as u8)
}

/// Generate a Image with grayscale alpha colors
/// Returns a Image with 256x256 grayscale alpha colors from black to white. Each pixel in the
/// image is a LumaA<u8> value, with the alpha value determined by the y coordinate.
pub fn create_image_greya() -> Image {
	Image::new_greya8_from_fn(256, 256, |x, y| [x as u8, y as u8])
}

pub fn image2blob(image: &Image, format: TileFormat) -> Result<Blob> {
	use TileFormat::*;
	match format {
		AVIF => todo!(),
		BIN => todo!(),
		GEOJSON => todo!(),
		JPG => jpeg::image2blob(image, None),
		JSON => todo!(),
		MVT => todo!(),
		PNG => png::image2blob(image),
		SVG => todo!(),
		TOPOJSON => todo!(),
		WEBP => webp::image2blob(image, None),
	}
}
