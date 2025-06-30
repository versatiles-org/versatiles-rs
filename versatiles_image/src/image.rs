use anyhow::{anyhow, bail, ensure, Result};
use image::{ColorType, DynamicImage, ExtendedColorType};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelValueType {
	/// 8-bit
	U8,
	/// 16-bit
	U16,
}

impl PixelValueType {
	pub fn bytes_per_pixel(&self) -> u8 {
		match self {
			PixelValueType::U8 => 1,
			PixelValueType::U16 => 2,
		}
	}
}

impl TryFrom<ExtendedColorType> for PixelValueType {
	type Error = anyhow::Error;

	fn try_from(value: ExtendedColorType) -> Result<Self> {
		use ExtendedColorType::*;
		use PixelValueType::*;
		match value {
			Rgb8 | Rgba8 | L8 | La8 => Ok(U8),
			Rgb16 | Rgba16 | L16 | La16 => Ok(U16),
			_ => bail!("Unsupported color type: {:?}", value),
		}
	}
}

impl TryFrom<ColorType> for PixelValueType {
	type Error = anyhow::Error;

	fn try_from(value: ColorType) -> Result<Self> {
		use ColorType::*;
		use PixelValueType::*;
		match value {
			Rgb8 | Rgba8 | L8 | La8 => Ok(U8),
			Rgb16 | Rgba16 | L16 | La16 => Ok(U16),
			_ => bail!("Unsupported color type: {:?}", value),
		}
	}
}

impl std::fmt::Display for PixelValueType {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let name = match self {
			PixelValueType::U8 => "U8",
			PixelValueType::U16 => "U16",
		};
		write!(f, "{name}")
	}
}

#[derive(Clone)]
pub struct Image {
	/// The image data in bytes.
	pub data: Vec<u8>,
	/// The width of the image in pixels.
	pub width: usize,
	/// The height of the image in pixels.
	pub height: usize,
	/// The number of channels in the image.
	pub channels: u8,
	/// The bit depth of the image.
	pub value_type: PixelValueType,
}

impl Image {
	pub fn new_empty(width: usize, height: usize, channels: u8, value_type: PixelValueType) -> Self {
		let data = vec![0; width * height * channels as usize * value_type.bytes_per_pixel() as usize];
		Self {
			data,
			width,
			height,
			channels,
			value_type,
		}
	}

	pub fn new_grey8_from_fn(width: usize, height: usize, f: fn(usize, usize) -> u8) -> Self {
		let mut image = Self::new_empty(width, height, 1, PixelValueType::U8);
		for y in 0..height {
			for x in 0..width {
				let index = y * width + x;
				image.data[index] = f(x, y);
			}
		}
		image
	}

	pub fn new_greya8_from_fn(width: usize, height: usize, f: fn(usize, usize) -> [u8; 2]) -> Self {
		let mut image = Self::new_empty(width, height, 2, PixelValueType::U8);
		for y in 0..height {
			for x in 0..width {
				let index = (y * width + x) * 2;
				image.data[index..index + 2].copy_from_slice(&f(x, y));
			}
		}
		image
	}

	pub fn new_rgb8_from_fn(width: usize, height: usize, f: fn(usize, usize) -> [u8; 3]) -> Self {
		let mut image = Self::new_empty(width, height, 3, PixelValueType::U8);
		for y in 0..height {
			for x in 0..width {
				let index = (y * width + x) * 3;
				image.data[index..index + 3].copy_from_slice(&f(x, y));
			}
		}
		image
	}

	pub fn new_rgba8_from_fn(width: usize, height: usize, f: fn(usize, usize) -> [u8; 4]) -> Self {
		let mut image = Self::new_empty(width, height, 4, PixelValueType::U8);
		for y in 0..height {
			for x in 0..width {
				let index = (y * width + x) * 4;
				image.data[index..index + 4].copy_from_slice(&f(x, y));
			}
		}
		image
	}

	pub fn from_rgb(width: usize, height: usize, has_alpha: bool, data: Vec<u8>) -> Self {
		Image {
			data,
			width,
			height,
			channels: if has_alpha { 4 } else { 3 },
			value_type: PixelValueType::U8,
		}
	}

	pub fn get_extended_color_type(&self) -> Result<ExtendedColorType> {
		use ExtendedColorType::*;
		use PixelValueType::*;
		Ok(match (self.channels, &self.value_type) {
			(1, &U16) => L16,
			(1, &U8) => L8,
			(2, &U16) => La16,
			(2, &U8) => La8,
			(3, &U16) => Rgb16,
			(3, &U8) => Rgb8,
			(4, &U16) => Rgba16,
			(4, &U8) => Rgba8,
			_ => bail!("Unsupported image format"),
		})
	}

	pub fn diff(&self, image: Image) -> Result<Vec<f64>> {
		ensure!(
			self.width == image.width,
			"Image width mismatch: self has width {}, but the other image has width {}",
			self.width,
			image.width
		);
		ensure!(
			self.height == image.height,
			"Image height mismatch: self has height {}, but the other image has height {}",
			self.height,
			image.height
		);
		ensure!(
			self.value_type == image.value_type,
			"Pixel value type mismatch: self has {}, but the other image has {}",
			self.value_type,
			image.value_type
		);
		ensure!(
			self.channels == image.channels,
			"Channel count mismatch: self has {} channels, but the other image has {} channels",
			self.channels,
			image.channels
		);

		let bytes1 = &self.data;
		let bytes2 = &image.data;
		ensure!(bytes1.len() == bytes2.len(), "'data lengths' are not equal");

		let channel_count = self.channels as usize;
		let mut sqr_sum: Vec<i64> = vec![0; channel_count];
		for i in 0..bytes1.len() {
			sqr_sum[i % channel_count] += (bytes1[i] as i64 - bytes2[i] as i64).pow(2);
		}

		let n = (self.width * self.height) as f64;
		Ok(sqr_sum.iter().map(|v| (10.0 * (*v as f64) / n).ceil() / 10.0).collect())
	}
}

impl TryFrom<DynamicImage> for Image {
	type Error = anyhow::Error;
	fn try_from(image: DynamicImage) -> Result<Self> {
		Ok(Self {
			width: image.width() as usize,
			height: image.height() as usize,
			channels: image.color().channel_count(),
			value_type: image.color().try_into()?,
			data: image.into_bytes(),
		})
	}
}

impl TryInto<DynamicImage> for Image {
	type Error = anyhow::Error;
	fn try_into(self) -> Result<DynamicImage> {
		let color_type = self.get_extended_color_type()?;
		use image::ImageBuffer;
		use DynamicImage::*;
		use ExtendedColorType::*;
		let width = self.width as u32;
		let height = self.height as u32;
		Ok(match color_type {
			Rgb8 => {
				ImageRgb8(ImageBuffer::from_vec(width, height, self.data).ok_or(anyhow!("Failed to create Rgb8 image"))?)
			}
			Rgba8 => {
				ImageRgba8(ImageBuffer::from_vec(width, height, self.data).ok_or(anyhow!("Failed to create Rgba8 image"))?)
			}
			L8 => ImageLuma8(ImageBuffer::from_vec(width, height, self.data).ok_or(anyhow!("Failed to create L8 image"))?),
			La8 => {
				ImageLumaA8(ImageBuffer::from_vec(width, height, self.data).ok_or(anyhow!("Failed to create La8 image"))?)
			}
			_ => bail!("Unsupported image format"),
		})
	}
}
