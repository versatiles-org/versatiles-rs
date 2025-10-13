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

#[cfg(test)]
mod tests {
	use super::*;
	use image::GenericImageView;
	use rstest::rstest;

	#[rstest]
	#[case::grey(DynamicImage::new_test_grey(), [
		"...# +++# ####",
		"...# +++# ####",
		"...# +++# ####"
	])]
	#[case::greya(DynamicImage::new_test_greya(), [
		".... +++. ###.",
		"...+ ++++ ###+",
		"...# +++# ####"
	])]
	#[case::rgb(DynamicImage::new_test_rgb(), [
		".#.# ++.# #..#",
		".#+# +++# #.+#",
		".### ++## #.##"
	])]
	#[case::rgba(DynamicImage::new_test_rgba(), [
		".#.# ++.# #..#",
		".#++ ++++ #.++",
		".##. ++#. #.#."
	])]
	fn check_dimensions_and_gradients(#[case] img: DynamicImage, #[case] colors: [&str; 3]) {
		assert_eq!(img.dimensions(), (256, 256));
		let get_pixel = |x: u32, y: u32| {
			img.get_pixel(x, y)
				.0
				.iter()
				.map(|v| match v {
					0 => '.',
					127 | 128 => '+',
					255 => '#',
					_ => panic!("unexpected value {v}"),
				})
				.collect::<String>()
		};
		let colors_result = [
			[get_pixel(0, 0), get_pixel(128, 0), get_pixel(255, 0)].join(" "),
			[get_pixel(0, 128), get_pixel(128, 128), get_pixel(255, 128)].join(" "),
			[get_pixel(0, 255), get_pixel(128, 255), get_pixel(255, 255)].join(" "),
		];
		assert_eq!(colors_result, colors);
	}
}
