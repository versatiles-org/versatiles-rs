use image::{ImageBuffer, Luma};
use std::vec::Vec;

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
	pub fn save_png(&self, filename: &str) {
		let image = ImageBuffer::from_fn(self.size as u32, self.size as u32, |x, y| {
			let index = y * (self.size as u32) + x;
			let v = self.data[index as usize];
			let c: u8 = (v as f64).sqrt() as u8;
			Luma([c])
		});
		image
			.save_with_format(filename, image::ImageFormat::Png)
			.unwrap();
	}
}
