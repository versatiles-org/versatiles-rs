use super::ProgressBar;
use image::{ImageBuffer, Luma, Rgb, RgbImage};
use std::vec::Vec;

pub struct StatusImage {
	size: u64,
	data: Vec<u64>,
}

impl StatusImage {
	/// Creates a new `StatusImage` with the specified size.
	pub fn new(size: u64) -> Self {
		let mut data: Vec<u64> = Vec::new();
		data.resize((size * size) as usize, 0);

		Self { size, data }
	}

	/// Sets the value of the pixel at the given coordinates to the given value.
	///
	/// # Arguments
	///
	/// * `x` - The x coordinate of the pixel.
	/// * `y` - The y coordinate of the pixel.
	/// * `v` - The value to set the pixel to.
	pub fn set(&mut self, x: u64, y: u64, v: u64) {
		assert!(x < self.size);
		assert!(y < self.size);

		let index = y * self.size + x;
		self.data[index as usize] = v;
	}

	/// Saves the image as a grayscale PNG file.
	///
	/// # Arguments
	///
	/// * `filename` - The name of the file to save the image to.
	pub fn save(&self, filename: &str) {
		let image = ImageBuffer::from_fn(self.size as u32, self.size as u32, |x, y| {
			let index = y * (self.size as u32) + x;
			let v = self.data[index as usize];
			let c: u8 = (v as f64).sqrt() as u8;
			Luma([c])
		});
		image.save(filename).unwrap();
	}

	/// Gets the color of the pixel at the given coordinates.
	///
	/// # Arguments
	///
	/// * `x` - The x coordinate of the pixel.
	/// * `y` - The y coordinate of the pixel.
	pub fn get_color(&self, x: u32, y: u32) -> Rgb<u8> {
		let index = (y as usize) * (self.size as usize) + (x as usize);
		let v = self.data[index];
		let f = (v as f64) / 65536.0;
		Rgb([
			(255.0 * f.powf(0.25)) as u8,
			(255.0 * f.powf(0.5)) as u8,
			(255.0 * f.powf(1.0)) as u8,
		])
	}
}

pub struct StatusImagePyramide {
	images: Vec<StatusImage>,
	max_size: u64,
}
impl StatusImagePyramide {
	pub fn new() -> Self {
		Self {
			images: Vec::new(),
			max_size: 0,
		}
	}
	pub fn get_level(&mut self, level: u8) -> &mut StatusImage {
		let index = level as usize;

		while self.images.len() <= index {
			// append image
			let i = self.images.len();
			let size = 2u64.pow(i as u32);
			let status_image = StatusImage::new(size);
			self.max_size = self.max_size.max(size);
			self.images.push(status_image);
		}

		assert!(self.images.get(index).is_some());

		return self.images.get_mut(index).unwrap();
	}
	pub fn save(&self, filename: &str) {
		let draw_list: Vec<&StatusImage> = self.images.iter().rev().collect();

		let mut progress = ProgressBar::new(
			"save status images",
			draw_list.iter().fold(0, |acc, img| acc + img.size * img.size),
		);

		let mut width: u32 = 0;
		let mut height: u32 = 0;
		let mut x_offset: u32 = 0;
		let mut y_offset: u32 = 0;
		let mut image_positions: Vec<[u32; 2]> = Vec::new();

		for (i, image) in draw_list.iter().enumerate() {
			let size = image.size as u32;
			image_positions.push([x_offset, y_offset]);
			width = width.max(size + x_offset);
			height = height.max(size + y_offset);
			if i == 0 {
				x_offset = size;
			} else {
				y_offset += size;
			}
		}

		let mut canvas: RgbImage = ImageBuffer::new(width, height);
		canvas.fill(16);

		for i in 0..draw_list.len() {
			let image = draw_list[i];
			let size = image.size as u32;
			let [x_offset, y_offset] = image_positions[i];
			for y in 0..size {
				for x in 0..size {
					canvas.put_pixel(x + x_offset, y + y_offset, image.get_color(x, y));
				}
				progress.inc(image.size);
			}
		}
		canvas.save(filename).unwrap();

		progress.finish();
	}
}

impl Default for StatusImagePyramide {
	fn default() -> Self {
		Self::new()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use assert_fs::fixture::NamedTempFile;

	#[test]
	fn set_get_color() {
		let image = &mut StatusImage::new(256);
		test_image(image);
		fill_image(image);

		let tmp_file = NamedTempFile::new("image.png").unwrap();
		image.save(tmp_file.to_str().unwrap());

		let size = tmp_file.path().metadata().unwrap().len();
		assert!(size > 10000, "{}", size);
		tmp_file.close().unwrap();
	}

	#[test]
	fn pyramide() {
		let mut pyramide = StatusImagePyramide::default();
		let image = pyramide.get_level(8);
		assert!(image.size == 256);
		test_image(image);
		fill_image(image);

		let tmp_file = NamedTempFile::new("pyramide.png").unwrap();
		pyramide.save(tmp_file.to_str().unwrap());

		let size = tmp_file.path().metadata().unwrap().len();
		assert!(size > 40000, "{}", size);
		tmp_file.close().unwrap();
	}

	fn test_image(image: &mut StatusImage) {
		image.set(0, 0, 0);
		image.set(1, 0, 100);
		image.set(0, 1, 10000);
		image.set(1, 1, 1000000);

		assert_eq!(image.get_color(0, 0), Rgb([0, 0, 0]));
		assert_eq!(image.get_color(1, 0), Rgb([50, 9, 0]));
		assert_eq!(image.get_color(0, 1), Rgb([159, 99, 38]));
		assert_eq!(image.get_color(1, 1), Rgb([255, 255, 255]));
	}

	fn fill_image(image: &mut StatusImage) {
		let size = image.size;
		for y in 0..size {
			for x in 0..size {
				image.set(x, y, x * y)
			}
		}
	}
}
