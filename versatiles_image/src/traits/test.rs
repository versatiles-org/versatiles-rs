use super::convert::DynamicImageTraitConvert;
use image::DynamicImage;

pub trait DynamicImageTraitTest: DynamicImageTraitConvert {
	fn new_test_rgba() -> DynamicImage;
	fn new_test_rgb() -> DynamicImage;
	fn new_test_grey() -> DynamicImage;
	fn new_test_greya() -> DynamicImage;
}

impl DynamicImageTraitTest for DynamicImage
where
	DynamicImage: DynamicImageTraitConvert,
{
	/// Generate a Image with RGBA colors
	fn new_test_rgba() -> DynamicImage {
		DynamicImage::from_fn_rgba8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8, (255 - y) as u8])
	}

	/// Generate a Image with RGB colors
	fn new_test_rgb() -> DynamicImage {
		DynamicImage::from_fn_rgb8(256, 256, |x, y| [x as u8, (255 - x) as u8, y as u8])
	}

	/// Generate a Image with grayscale colors
	/// Returns a Image with 256x256 grayscale colors from black to white. Each pixel in the image
	/// is a Luma<u8> value.
	fn new_test_grey() -> DynamicImage {
		DynamicImage::from_fn_l8(256, 256, |x, _y| x as u8)
	}

	/// Generate a Image with grayscale alpha colors
	/// Returns a Image with 256x256 grayscale alpha colors from black to white. Each pixel in the
	/// image is a `LumaA`<u8> value, with the alpha value determined by the y coordinate.
	fn new_test_greya() -> DynamicImage {
		DynamicImage::from_fn_la8(256, 256, |x, y| [x as u8, y as u8])
	}
}
