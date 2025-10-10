use super::info::DynamicImageTraitInfo;
use anyhow::{Result, bail};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, Rgb, imageops::overlay};
use imageproc::map::map_colors;

pub trait DynamicImageTraitOperation: DynamicImageTraitInfo {
	fn as_no_alpha(&self) -> Result<DynamicImage>;
	fn average_color(&self) -> Vec<u8>;
	fn get_extract(&self, x: f64, y: f64, w: f64, h: f64, width_dst: u32, height_dst: u32) -> Result<DynamicImage>;
	fn get_scaled_down(&self, factor: u32) -> Result<DynamicImage>;
	fn into_flattened(self, color: Rgb<u8>) -> Result<DynamicImage>;
	fn into_no_alpha_if_opaque(self) -> Result<DynamicImage>;
	fn into_no_alpha(self) -> Result<DynamicImage>;
	fn into_scaled_down(self, factor: u32) -> Result<DynamicImage>;
	fn make_opaque(&mut self) -> Result<()>;
	fn mut_color_values<F>(&mut self, f: F)
	where
		F: Fn(u8) -> u8;
	fn overlay(&mut self, top: &DynamicImage) -> Result<()>;
}

impl DynamicImageTraitOperation for DynamicImage
where
	DynamicImage: DynamicImageTraitInfo,
{
	fn as_no_alpha(&self) -> Result<DynamicImage> {
		Ok(match self {
			DynamicImage::ImageRgba8(_) => DynamicImage::from(self.to_rgb8()),
			DynamicImage::ImageLumaA8(_) => DynamicImage::from(self.to_luma8()),
			DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => self.clone(),
			_ => bail!("Unsupported image type for removing alpha: {:?}", self.color()),
		})
	}

	fn average_color(&self) -> Vec<u8> {
		let img = self.resize_exact(1, 1, image::imageops::FilterType::Triangle);
		img.into_bytes()
	}

	fn get_extract(&self, x: f64, y: f64, w: f64, h: f64, width_dst: u32, height_dst: u32) -> Result<DynamicImage> {
		let mut dst_image = DynamicImage::new(width_dst, height_dst, self.color());
		Resizer::new().resize(self, &mut dst_image, &ResizeOptions::default().crop(x, y, w, h))?;
		Ok(dst_image)
	}

	fn get_scaled_down(&self, factor: u32) -> Result<DynamicImage> {
		assert!(factor > 0, "Scaling factor must be greater than zero");

		let mut dst_image = DynamicImage::new(self.width() / factor, self.height() / factor, self.color());
		Resizer::new().resize(
			self,
			&mut dst_image,
			&ResizeOptions::default().resize_alg(ResizeAlg::Convolution(FilterType::Box)),
		)?;

		Ok(dst_image)
	}

	fn into_flattened(self, color: Rgb<u8>) -> Result<DynamicImage> {
		if !self.has_alpha() {
			return Ok(self);
		}
		match self {
			DynamicImage::ImageRgba8(img) => {
				let c = [u16::from(color[0]), u16::from(color[1]), u16::from(color[2])];
				Ok(DynamicImage::from(map_colors(&img, |p| {
					if p[3] == 255 {
						Rgb([p[0], p[1], p[2]])
					} else {
						let a = u16::from((p[3]));
						let b = u16::from(255 - p[3]);
						Rgb([
							(((u16::from(p[0]) * a) + c[0] * b + 127) / 255) as u8,
							(((u16::from(p[1]) * a) + c[1] * b + 127) / 255) as u8,
							(((u16::from(p[2]) * a) + c[2] * b + 127) / 255) as u8,
						])
					}
				})))
			}
			_ => bail!("Unsupported image type {:?} for flattening", self.color()),
		}
	}

	fn into_no_alpha(self) -> Result<DynamicImage> {
		Ok(match self {
			DynamicImage::ImageRgba8(_) => DynamicImage::from(self.into_rgb8()),
			DynamicImage::ImageLumaA8(_) => DynamicImage::from(self.into_luma8()),
			DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => self,
			_ => bail!("Unsupported image type for removing alpha: {:?}", self.color()),
		})
	}

	fn into_no_alpha_if_opaque(self) -> Result<DynamicImage> {
		if self.has_alpha() && self.is_opaque() {
			self.into_no_alpha()
		} else {
			Ok(self)
		}
	}

	fn into_scaled_down(self, factor: u32) -> Result<DynamicImage> {
		if factor == 1 {
			Ok(self)
		} else {
			self.get_scaled_down(factor)
		}
	}

	fn make_opaque(&mut self) -> Result<()> {
		match *self {
			DynamicImage::ImageRgba8(ref mut img) => {
				for p in img.pixels_mut() {
					p[3] = 255;
				}
			}
			DynamicImage::ImageLumaA8(ref mut img) => {
				for p in img.pixels_mut() {
					p[1] = 255;
				}
			}
			DynamicImage::ImageRgb8(_) | DynamicImage::ImageLuma8(_) => {}
			_ => bail!("Unsupported image type for removing alpha: {:?}", self.color()),
		}
		Ok(())
	}

	fn mut_color_values<F>(&mut self, f: F)
	where
		F: Fn(u8) -> u8,
	{
		match self {
			DynamicImage::ImageLuma8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
				}
			}
			DynamicImage::ImageLumaA8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
				}
			}
			DynamicImage::ImageRgb8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
					p[1] = f(p[1]);
					p[2] = f(p[2]);
				}
			}
			DynamicImage::ImageRgba8(img) => {
				for p in img.pixels_mut() {
					p[0] = f(p[0]);
					p[1] = f(p[1]);
					p[2] = f(p[2]);
				}
			}
			_ => panic!("Unsupported image type for mutating color values: {:?}", self.color()),
		}
	}

	fn overlay(&mut self, top: &DynamicImage) -> Result<()> {
		self.ensure_same_size(top)?;
		overlay(self, top, 0, 0);
		Ok(())
	}
}
