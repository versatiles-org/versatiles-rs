use crate::types::TileCoord3;
use ab_glyph::{FontArc, PxScale};
use imageproc::{
	drawing::draw_text_mut,
	image::{DynamicImage, Rgb, RgbImage},
};

static mut FONT: Option<FontArc> = None;

pub fn create_debug_image(coord: &TileCoord3) -> DynamicImage {
	let font = unsafe {
		if FONT.is_none() {
			FONT.insert(FontArc::try_from_slice(include_bytes!("./trim.ttf")).unwrap())
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
