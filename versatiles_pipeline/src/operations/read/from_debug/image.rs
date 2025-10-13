use ab_glyph::{FontArc, PxScale};
use imageproc::{
	drawing::draw_text_mut,
	image::{DynamicImage, Rgba, RgbaImage},
};
use lazy_static::lazy_static;
use versatiles_core::TileCoord;

lazy_static! {
	static ref FONT: FontArc = FontArc::try_from_slice(include_bytes!("./trim.ttf")).unwrap();
}

pub fn create_debug_image(coord: &TileCoord, use_alpha: bool) -> DynamicImage {
	let br = ((coord.x + coord.y) % 2) as u8 * 255;

	// Build everything as RGBA; for RGB output we drop alpha at the end.
	let mut img = RgbaImage::from_pixel(512, 512, Rgba([br, br, br, if use_alpha { 16 } else { 255 }]));

	let font: &FontArc = &FONT;
	let mut draw =
		|y: i32, c: Rgba<u8>, text: String| draw_text_mut(&mut img, c, 220, y, PxScale::from(40f32), font, &text);

	draw(195, Rgba([127, 30, 16, 255]), format!("z: {}", coord.level));
	draw(225, Rgba([0, 92, 45, 255]), format!("x: {}", coord.x));
	draw(255, Rgba([30, 23, 98, 255]), format!("y: {}", coord.y));

	let dynimg = DynamicImage::ImageRgba8(img);
	if use_alpha {
		dynimg
	} else {
		// Drop alpha by converting to RGB; avoids duplicating draw logic above.
		DynamicImage::ImageRgb8(dynimg.to_rgb8())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_create_debug_image() {
		let coord = TileCoord { x: 1, y: 2, level: 3 };
		let image = create_debug_image(&coord, true);

		assert_eq!(image.width(), 512);
		assert_eq!(image.height(), 512);
	}
}
