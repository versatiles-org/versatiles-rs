use crate::{GeoBBox, GeoCenter, PyramidInfo, TileBBoxPyramid};
use anyhow::{Result, ensure};

impl TileBBoxPyramid {
	/// Determines a geographical bounding box from the highest zoom level that contains tiles.
	///
	/// Returns `None` if the pyramid is empty.
	#[must_use]
	pub fn get_geo_bbox(&self) -> Option<GeoBBox> {
		let max_zoom = self.get_level_max()?;
		self.get_level_bbox(max_zoom).to_geo_bbox()
	}

	/// Calculates a geographic center based on the bounding box at a middle zoom level.
	///
	/// This tries to pick a zoom that is "2 levels above the min," but not exceeding the max.
	/// Returns `None` if the pyramid is empty or if the bounding box is invalid.
	#[must_use]
	pub fn get_geo_center(&self) -> Option<GeoCenter> {
		let bbox = self.get_geo_bbox()?;
		let zoom = (self.get_level_min()? + 2).min(self.get_level_max()?);
		let center_lon = f64::midpoint(bbox.x_min, bbox.x_max);
		let center_lat = f64::midpoint(bbox.y_min, bbox.y_max);
		Some(GeoCenter(center_lon, center_lat, zoom))
	}

	#[must_use]
	pub fn get_zoom_min(&self) -> Option<u8> {
		self.get_level_min()
	}

	#[must_use]
	pub fn get_zoom_max(&self) -> Option<u8> {
		self.get_level_max()
	}

	pub fn weighted_bbox(&self) -> Result<GeoBBox> {
		let mut x_min_sum: f64 = 0.0;
		let mut y_min_sum: f64 = 0.0;
		let mut x_max_sum: f64 = 0.0;
		let mut y_max_sum: f64 = 0.0;
		let mut weight_sum: f64 = 0.0;
		for l in &self.level_bbox {
			if let Some(bbox) = l.to_geo_bbox() {
				let weight = l.count_tiles() as f64;
				x_min_sum += bbox.x_min * weight;
				y_min_sum += bbox.y_min * weight;
				x_max_sum += bbox.x_max * weight;
				y_max_sum += bbox.y_max * weight;
				weight_sum += weight;
			}
		}
		ensure!(weight_sum > 0.0, "Cannot compute weighted bbox for an empty pyramid");
		GeoBBox::new(
			x_min_sum / weight_sum,
			y_min_sum / weight_sum,
			x_max_sum / weight_sum,
			y_max_sum / weight_sum,
		)
	}
}

impl PyramidInfo for TileBBoxPyramid {
	fn get_geo_bbox(&self) -> Option<GeoBBox> {
		self.get_geo_bbox()
	}

	fn get_zoom_min(&self) -> Option<u8> {
		self.get_zoom_min()
	}

	fn get_zoom_max(&self) -> Option<u8> {
		self.get_zoom_max()
	}
}
