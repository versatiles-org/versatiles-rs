use anyhow::{Context, Result, bail, ensure};
use gdal::{DriverManager, raster::ColorInterpretation, spatial_ref::SpatialRef};
use std::fmt::Debug;
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
		log::trace!("Computing band mapping (raster_count={})", dataset.raster_count());

		let bands: Vec<(usize, ColorInterpretation)> = (1..=dataset.raster_count())
			.map(|i| {
				let band = dataset
					.rasterband(i)
					.with_context(|| format!("Failed to get raster band {i} from GDAL dataset"))?;
				Ok((i, band.color_interpretation()))
			})
			.collect::<Result<_>>()?;

		let band_string = bands
			.iter()
			.map(|(_, ci)| format!("{ci:?}"))
			.collect::<Vec<_>>()
			.join(", ");

		let channels = (|| {
			// gray, red, green, blue, alpha
			let mut channels: [Option<usize>; 5] = [None, None, None, None, None];

			for (band_index, ci) in bands.iter() {
				use ColorInterpretation::*;
				let channel_index = match ci {
					GrayIndex => 0,
					RedBand => 1,
					GreenBand => 2,
					BlueBand => 3,
					AlphaBand => 4,
					Undefined => {
						if band_index > &4 {
							continue;
						};
						*band_index // 1 => red, 2 => green, 3 => blue, 4 => alpha
					}
					_ => bail!("GDAL band {band_index} has unsupported color interpretation: {ci:?}"),
				};

				ensure!(
					channels[channel_index].is_none(),
					"GDAL dataset band {band_index} uses the same channel ({}) as band {}",
					["grey", "red", "green", "blue", "alpha"][channel_index],
					channels[channel_index].unwrap()
				);
				channels[channel_index] = Some(*band_index);
			}
			Ok::<_, anyhow::Error>(channels)
		})()
		.with_context(|| format!("Failed to compute channel mapping from bands [{band_string}]",))?;

		let map: Vec<usize> = match channels {
			[None, Some(red), Some(green), Some(blue), Some(alpha)] => {
				log::trace!("Found RGBA bands: red={red}, green={green}, blue={blue}, alpha={alpha}");
				vec![red, green, blue, alpha]
			}
			[None, Some(red), Some(green), Some(blue), None] => {
				log::trace!("Found RGB  band: red={red}, green={green}, blue={blue}");
				vec![red, green, blue]
			}
			[Some(gray), None, None, None, Some(alpha)] => {
				log::trace!("Found gray + alpha band: gray={gray}, alpha={alpha}");
				vec![gray, alpha]
			}
			[Some(gray), None, None, None, None] => {
				log::trace!("Found gray band: gray={gray}");
				vec![gray]
			}
			_ => {
				bail!("The found bands ({band_string}) cannot be interpreted as grey/RGB (+alpha)",);
			}
		};
		log::trace!("Band mapping result: {map:?}");

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

#[cfg(test)]
mod tests {
	use super::*;
	use anyhow::Result;
	use gdal::{DriverManager, raster::ColorInterpretation};
	use rstest::rstest;

	fn mem_dataset_with_bands(cis: Vec<ColorInterpretation>) -> Result<gdal::Dataset> {
		let driver = DriverManager::get_driver_by_name("MEM")?;
		let ds = driver.create_with_band_type::<u8, _>("", 2, 2, cis.len())?;
		for (i, ci) in cis.into_iter().enumerate() {
			ds.rasterband(i + 1)?.set_color_interpretation(ci)?;
		}
		Ok(ds)
	}

	fn parse_color_interpretations(text: &str) -> Vec<ColorInterpretation> {
		use ColorInterpretation::*;
		text
			.split(',')
			.filter_map(|s| {
				let t = s.trim().to_ascii_lowercase();
				Some(match t.as_str() {
					"grey" | "gray" => GrayIndex,
					"r" | "red" => RedBand,
					"g" | "green" => GreenBand,
					"b" | "blue" => BlueBand,
					"a" | "alpha" => AlphaBand,
					"u" | "undefined" => Undefined,
					"palette" | "pal" => PaletteIndex,
					_ => return None,
				})
			})
			.collect()
	}

	#[rstest]
	#[case("Grey", "Grey", &[1])]
	#[case("Grey,A", "Grey,A", &[1,2])]
	#[case("R,G,B", "R,G,B", &[1,2,3])]
	#[case("B,G,R", "R,G,B", &[3,2,1])]
	#[case("R,G,B,A", "R,G,B,A", &[1,2,3,4])]
	#[case("A,R,G,B", "R,G,B,A", &[2,3,4,1])]
	fn bandmapping_ok_cases(#[case] colors_in: &str, #[case] colors_out: &str, #[case] mapping: &[usize]) -> Result<()> {
		let ds = mem_dataset_with_bands(parse_color_interpretations(colors_in))?;
		let bm = BandMapping::try_from(&ds)?;
		assert_eq!(bm.len(), mapping.len());

		let got: Vec<_> = bm
			.iter()
			.enumerate()
			.map(|(i, it)| {
				assert_eq!(i, it.channel_index);
				it.band_index
			})
			.collect();
		assert_eq!(got, mapping);

		// create_mem_dataset mirrors color interpretation layout
		let out = bm.create_mem_dataset(8, 8)?;
		let expected_colors = parse_color_interpretations(colors_out);
		assert_eq!(out.raster_count() as usize, expected_colors.len());
		for (i, ci) in expected_colors.into_iter().enumerate() {
			assert_eq!(out.rasterband(i + 1)?.color_interpretation(), ci);
		}
		Ok(())
	}

	#[rstest]
	#[case(
		"Palette",
		"Failed to compute channel mapping from bands [PaletteIndex]",
		"GDAL band 1 has unsupported color interpretation: PaletteIndex"
	)]
	#[case(
		"Red,Red",
		"Failed to compute channel mapping from bands [RedBand, RedBand]",
		"GDAL dataset band 2 uses the same channel (red) as band 1"
	)]
	#[case(
		"Undefined,Undefined,Green",
		"Failed to compute channel mapping from bands [Undefined, Undefined, GreenBand]",
		"GDAL dataset band 3 uses the same channel (green) as band 2"
	)]
	fn bandmapping_error_cases(#[case] colors_in: &str, #[case] msg1: &str, #[case] msg2: &str) -> Result<()> {
		let ds = mem_dataset_with_bands(parse_color_interpretations(colors_in))?;
		let err = BandMapping::try_from(&ds)
			.unwrap_err()
			.chain()
			.rev()
			.take(2)
			.map(|e| e.to_string())
			.collect::<Vec<_>>();
		assert_eq!(err, [msg2, msg1]);
		Ok(())
	}

	#[test]
	fn debug_fmt_includes_map() -> Result<()> {
		use ColorInterpretation::*;
		let ds = mem_dataset_with_bands(vec![RedBand, GreenBand, BlueBand])?;
		let bm = BandMapping::try_from(&ds)?;
		assert_eq!(format!("{:?}", bm), "BandMapping { map: [1, 2, 3] }");
		Ok(())
	}
}
