use std::fmt::Debug;

use anyhow::{Context, Result, bail, ensure};
use gdal::{DriverManager, raster::ColorInterpretation, spatial_ref::SpatialRef};
use log::{debug, trace, warn};
use versatiles_derive::context;

pub struct BandMapping {
	map: Vec<usize>,
	alpha: Option<usize>,
}

impl BandMapping {
	#[context("Failed to create band mapping from GDAL dataset")]
	pub fn try_from(dataset: &gdal::Dataset) -> Result<Self> {
		trace!("Computing band mapping (raster_count={})", dataset.raster_count());
		let mut color_index = [0, 0, 0];
		let mut grey_index = 0;
		let mut alpha: Option<usize> = None;

		for i in 1..=dataset.raster_count() {
			let band = dataset
				.rasterband(i)
				.with_context(|| format!("Failed to get raster band {i} from GDAL dataset"))?;
			use gdal::raster::ColorInterpretation::*;
			match band.color_interpretation() {
				RedBand => color_index[0] = i,
				GreenBand => color_index[1] = i,
				BlueBand => color_index[2] = i,
				AlphaBand => alpha = Some(i),
				GrayIndex => grey_index = i,
				_ => warn!(
					"GDAL band {i} has unsupported color interpretation: {:?}",
					band.color_interpretation()
				),
			}
		}

		let mut map = vec![];
		if color_index.iter().all(|&i| i > 0) {
			if grey_index > 0 {
				bail!("GDAL dataset has both color and grey bands, which is not supported");
			}
			map.push(color_index[0]);
			map.push(color_index[1]);
			map.push(color_index[2]);
		} else if grey_index > 0 {
			map.push(grey_index);
		} else {
			bail!("GDAL dataset has no color or grey bands, cannot read image data");
		}

		if let Some(alpha_index) = alpha {
			map.push(alpha_index);
		}
		debug!("Band mapping result: {:?}", map);

		ensure!(!map.is_empty(), "Band mapping is empty, cannot read image data");

		Ok(BandMapping { map, alpha })
	}

	pub fn len(&self) -> usize {
		self.map.len()
	}

	pub fn iter(&self) -> impl Iterator<Item = (usize, usize)> + '_ {
		self.map.iter().cloned().enumerate()
	}

	pub fn create_mem_dataset(&self, width: u32, height: u32) -> Result<gdal::Dataset> {
		let driver = DriverManager::get_driver_by_name("MEM").context("Failed to get GDAL MEM driver")?;

		// Create destination dataset in EPSG:3857 for the requested bbox
		let mut dst = driver
			.create_with_band_type::<u8, _>("", width as usize, height as usize, self.len())
			.context("Failed to create in-memory dataset")?;
		dst.set_spatial_ref(&SpatialRef::from_epsg(3857)?)?;

		if let Some(alpha) = self.alpha {
			trace!("Setting alpha band for destination dataset");
			dst.rasterband(alpha)?
				.set_color_interpretation(ColorInterpretation::AlphaBand)?;
		} else {
			trace!("No alpha band in band mapping, skipping alpha setup");
		}

		Ok(dst)
	}
}

impl Debug for BandMapping {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "BandMapping {{ map: {:?}, alpha: {:?} }}", self.map, self.alpha)
	}
}
