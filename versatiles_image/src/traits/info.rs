use super::convert::DynamicImageTraitConvert;
use anyhow::{Result, ensure};
use image::{DynamicImage, ExtendedColorType};

pub trait DynamicImageTraitInfo: DynamicImageTraitConvert {
	fn bits_per_value(&self) -> u8;
	fn channel_count(&self) -> u8;
	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>>;
	fn ensure_same_meta(&self, other: &DynamicImage) -> Result<()>;
	fn ensure_same_size(&self, other: &DynamicImage) -> Result<()>;
	fn extended_color_type(&self) -> ExtendedColorType;
	fn has_alpha(&self) -> bool;
	fn into_optional(self) -> Option<DynamicImage>;
	fn is_empty(&self) -> bool;
	fn is_opaque(&self) -> bool;
}

impl DynamicImageTraitInfo for DynamicImage
where
	DynamicImage: DynamicImageTraitConvert,
{
	fn bits_per_value(&self) -> u8 {
		(self.color().bits_per_pixel() / self.color().channel_count() as u16) as u8
	}

	fn channel_count(&self) -> u8 {
		self.color().channel_count()
	}

	fn diff(&self, other: &DynamicImage) -> Result<Vec<f64>> {
		self.ensure_same_meta(other)?;

		let channels = self.color().channel_count() as usize;
		let mut sqr_sum = vec![0u64; channels];

		for (p1, p2) in self.iter_pixels().zip(other.iter_pixels()) {
			for i in 0..channels {
				let d = p1[i] as i64 - p2[i] as i64;
				sqr_sum[i] += (d * d) as u64;
			}
		}

		let n = (self.width() * self.height()) as f64;
		Ok(sqr_sum.iter().map(|v| (10.0 * (*v as f64) / n).ceil() / 10.0).collect())
	}

	fn ensure_same_meta(&self, other: &DynamicImage) -> Result<()> {
		self.ensure_same_size(other)?;
		ensure!(
			self.color() == other.color(),
			"Pixel value type mismatch: self has {:?}, but the other image has {:?}",
			self.color(),
			other.color()
		);
		Ok(())
	}

	fn ensure_same_size(&self, other: &DynamicImage) -> Result<()> {
		ensure!(
			self.width() == other.width(),
			"Image width mismatch: self has width {}, but the other image has width {}",
			self.width(),
			other.width()
		);
		ensure!(
			self.height() == other.height(),
			"Image height mismatch: self has height {}, but the other image has height {}",
			self.height(),
			other.height()
		);
		Ok(())
	}

	fn extended_color_type(&self) -> ExtendedColorType {
		self.color().into()
	}

	fn has_alpha(&self) -> bool {
		self.color().has_alpha()
	}

	fn into_optional(self) -> Option<DynamicImage> {
		if self.is_empty() { None } else { Some(self) }
	}

	fn is_empty(&self) -> bool {
		if !self.color().has_alpha() {
			return false;
		}
		let alpha_channel = (self.color().channel_count() - 1) as usize;
		return !self.iter_pixels().any(|p| p[alpha_channel] != 0);
	}

	fn is_opaque(&self) -> bool {
		if !self.color().has_alpha() {
			return true;
		}
		let alpha_channel = (self.color().channel_count() - 1) as usize;
		return self.iter_pixels().all(|p| p[alpha_channel] == 255);
	}
}
