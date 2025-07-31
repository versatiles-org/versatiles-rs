use ab_glyph::{FontArc, PxScale};
use imageproc::{
	drawing::draw_text_mut,
	image::{DynamicImage, Rgba, RgbaImage},
};
use lazy_static::lazy_static;
use versatiles_core::TileCoord3;

lazy_static! {
	static ref FONT: FontArc = FontArc::try_from_slice(include_bytes!("./trim.ttf")).unwrap();
}

pub fn create_debug_image(coord: &TileCoord3) -> DynamicImage {
	let br = ((coord.x + coord.y) % 2) as u8 * 255;
	let mut image1 = RgbaImage::from_pixel(512, 512, Rgba::from([br, br, br, 16]));

	let font: &FontArc = &FONT;
	let mut draw =
		|y: i32, c: Rgba<u8>, text: String| draw_text_mut(&mut image1, c, 220, y, PxScale::from(40f32), font, &text);

	draw(195, Rgba([127, 30, 16, 255]), format!("z: {}", coord.z));
	draw(225, Rgba([0, 92, 45, 255]), format!("x: {}", coord.x));
	draw(255, Rgba([30, 23, 98, 255]), format!("y: {}", coord.y));

	DynamicImage::ImageRgba8(image1)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn test_create_debug_image() {
		let coord = TileCoord3 { x: 1, y: 2, z: 3 };
		let image = create_debug_image(&coord);

		assert_eq!(image.width(), 512);
		assert_eq!(image.height(), 512);
	}
}
