use image::{ImageBuffer, Luma, Rgb, RgbImage};
use std::vec::Vec;

use super::ProgressBar;

pub struct StatusImage {
	size: u64,
	data: Vec<u64>,
}
impl StatusImage {
	pub fn new(size: u64) -> Self {
		let mut data: Vec<u64> = Vec::new();
		data.resize((size * size) as usize, 0);

		Self { size, data }
	}
	pub fn set(&mut self, x: u64, y: u64, v: u64) {
		assert!(x < self.size);
		assert!(y < self.size);
		let index = y * self.size + x;
		self.data[index as usize] = v;
	}
	#[allow(dead_code)]
	pub fn save(&self, filename: &str) {
		let image = ImageBuffer::from_fn(self.size as u32, self.size as u32, |x, y| {
			let index = y * (self.size as u32) + x;
			let v = self.data[index as usize];
			let c: u8 = (v as f64).sqrt() as u8;
			Luma([c])
		});
		image.save(filename).unwrap();
	}
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

		if self.images.get(index).is_some() {
			return self.images.get_mut(index).unwrap();
		} else {
			let size = 2u64.pow(index as u32);
			let status_image = StatusImage::new(size);

			self.max_size = self.max_size.max(size);
			self.images.insert(index, status_image);

			return self.images.get_mut(index).unwrap();
		}
	}
	pub fn save(&self, filename: &str) {
		let mut progress = ProgressBar::new(
			"save status images",
			self
				.images
				.iter()
				.fold(0, |acc, img| acc + img.size * img.size),
		);

		let width = (self.max_size * 2 - 1) as u32;
		let height = (self.max_size) as u32;
		let mut canvas: RgbImage = ImageBuffer::new(width, height);
		canvas.fill(16);

		for image in self.images.iter() {
			let size = image.size as u32;
			let x_offset = width - (size * 2 - 1);
			for y in 0..size {
				for x in 0..size {
					canvas.put_pixel(x + x_offset, y, image.get_color(x, y));
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

	#[test]
	fn basic_tests() {
		let mut image = StatusImage::new(2);
		image.set(0, 0, 0);
		assert_eq!(image.get_color(0, 0), Rgb([0, 0, 0]));

		image.set(1, 0, 100);
		assert_eq!(image.get_color(1, 0), Rgb([50, 9, 0]));

		image.set(0, 1, 10000);
		assert_eq!(image.get_color(0, 1), Rgb([159, 99, 38]));

		image.set(1, 1, 1000000);
		assert_eq!(image.get_color(1, 1), Rgb([255, 255, 255]));
	}
}
