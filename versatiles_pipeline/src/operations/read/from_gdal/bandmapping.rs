use std::fmt::Debug;

use anyhow::{Context, Result, bail, ensure};
use gdal::{DriverManager, raster::ColorInterpretation, spatial_ref::SpatialRef};
use log::{debug, trace};
use versatiles_derive::context;

pub struct BandMappingItem {
	pub band_index: usize,
	pub channel_index: usize,
}

pub struct BandMapping {
	map: Vec<usize>,
}

impl BandMapping {
	#[context("Failed to create band mapping from GDAL dataset")]
	pub fn try_from(dataset: &gdal::Dataset) -> Result<Self> {
		trace!("Computing band mapping (raster_count={})", dataset.raster_count());
		// gray, red, green, blue, alpha
		let mut colors: [Option<usize>; 5] = [None, None, None, None, None];

		for i in 1..=dataset.raster_count() {
			let mut set_channel = |channel: usize| -> Result<()> {
				ensure!(
					colors[channel].is_none(),
					"GDAL dataset band {i} (numbered from 1) should be used for channel {channel} (numbered from 0), but it is already in use by band {}",
					colors[channel].unwrap()
				);
				colors[channel] = Some(i);
				Ok(())
			};

			let band = dataset
				.rasterband(i)
				.with_context(|| format!("Failed to get raster band {i} from GDAL dataset"))?;
			use gdal::raster::ColorInterpretation::*;
			match band.color_interpretation() {
				GrayIndex => set_channel(0)?,
				RedBand => set_channel(1)?,
				GreenBand => set_channel(2)?,
				BlueBand => set_channel(3)?,
				AlphaBand => set_channel(4)?,
				Undefined => set_channel(i)?,
				_ => bail!(
					"GDAL band {i} has unsupported color interpretation: {:?}",
					band.color_interpretation()
				),
			}
		}

		let map: Vec<usize> = match colors {
			[None, Some(red), Some(green), Some(blue), Some(alpha)] => {
				debug!("Found RGBA bands: red={red}, green={green}, blue={blue}, alpha={alpha}");
				vec![red, green, blue, alpha]
			}
			[None, Some(red), Some(green), Some(blue), None] => {
				debug!("Found RGB  band: red={red}, green={green}, blue={blue}");
				vec![red, green, blue]
			}
			[Some(gray), None, None, None, Some(alpha)] => {
				debug!("Found gray + alpha band: gray={gray}, alpha={alpha}");
				vec![gray, alpha]
			}
			[Some(gray), None, None, None, None] => {
				debug!("Found gray band: gray={gray}");
				vec![gray]
			}
			_ => {
				bail!("The found bands ({colors:?}) cannot be interpreted  as (grey, red, green, blue, alpha)");
			}
		};
		debug!("Band mapping result: {map:?}");

		Ok(BandMapping { map })
	}

	pub fn len(&self) -> usize {
		self.map.len()
	}

	pub fn iter(&self) -> impl Iterator<Item = BandMappingItem> + '_ {
		self
			.map
			.iter()
			.enumerate()
			.map(|(channel_index, &band_index)| BandMappingItem {
				band_index,
				channel_index,
			})
	}

	pub fn create_mem_dataset(&self, width: u32, height: u32) -> Result<gdal::Dataset> {
		let driver = DriverManager::get_driver_by_name("MEM").context("Failed to get GDAL MEM driver")?;

		// Create destination dataset in EPSG:3857 for the requested bbox
		let mut dst = driver
			.create_with_band_type::<u8, _>("", width as usize, height as usize, self.len())
			.context("Failed to create in-memory dataset")?;
		dst.set_spatial_ref(&SpatialRef::from_epsg(3857)?)?;

		use ColorInterpretation::*;

		match self.len() {
			1 => dst.rasterband(1)?.set_color_interpretation(GrayIndex)?,
			2 => {
				dst.rasterband(1)?.set_color_interpretation(GrayIndex)?;
				dst.rasterband(2)?.set_color_interpretation(AlphaBand)?;
			}
			3 => {
				dst.rasterband(1)?.set_color_interpretation(RedBand)?;
				dst.rasterband(2)?.set_color_interpretation(GreenBand)?;
				dst.rasterband(3)?.set_color_interpretation(BlueBand)?;
			}
			4 => {
				dst.rasterband(1)?.set_color_interpretation(RedBand)?;
				dst.rasterband(2)?.set_color_interpretation(GreenBand)?;
				dst.rasterband(3)?.set_color_interpretation(BlueBand)?;
				dst.rasterband(4)?.set_color_interpretation(AlphaBand)?;
			}
			_ => bail!("Unsupported number of bands in band mapping: {}", self.len()),
		}

		Ok(dst)
	}
}

impl Debug for BandMapping {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "BandMapping {{ map: {:?} }}", self.map)
	}
}
