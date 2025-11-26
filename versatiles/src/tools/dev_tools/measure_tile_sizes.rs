use anyhow::{Result, ensure};
use std::path::PathBuf;
use versatiles::get_registry;
use versatiles_container::ProcessingConfig;
use versatiles_core::{TileBBox, TileFormat, progress::get_progress_bar};
use versatiles_image::{DynamicImage, DynamicImageTraitConvert, encode};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_help_flag = true, disable_version_flag = true)]
/// Measure file sizes of the tiles in a container and generate an image visualizing the sizes.
///
/// The output image is a downscaled grayscale representation of the tile sizes at the specified zoom level.
/// Each pixel in the output image has a brightness value of 10*log2(size), where size is the average tile size in bytes for the corresponding area.
/// Example:
/// - A value of 0 means an average tile size of 1 byte or less
/// - A value of 100 means an average tile size of about 1 KB (2^10)
/// - A value of 200 means an average tile size of about 1 MB (2^20)
pub struct MeasureTileSizes {
	/// Input file
	#[arg(value_name = "INPUT_FILE")]
	input: String,

	/// Output image file (should end in .png)
	#[arg(value_name = "OUTPUT_FILE")]
	output: PathBuf,

	/// Zoom level to analyze
	#[arg(default_value = "14")]
	level: u8,

	/// Scale down factor
	#[arg(default_value = "4")]
	scale: usize,
}

pub async fn run(args: &MeasureTileSizes) -> Result<()> {
	let input = &args.input;
	let output_file = &args.output;
	let level = args.level;
	let scale = args.scale;
	let width_original = 1 << level;
	let width_scaled = width_original / scale;

	log::debug!(
		"Measuring tile sizes in {input:?} at zoom level {level}, generating an {width_scaled}x{width_scaled} image and saving it to {output_file:?}"
	);

	ensure!(
		output_file.extension() == Some("png".as_ref()),
		"Only PNG output is supported for now, got {:?}",
		output_file.extension().unwrap_or_default()
	);

	let reader = get_registry(ProcessingConfig::default())
		.get_reader_from_str(input)
		.await?;
	let bbox = TileBBox::new_full(level)?;
	let stream = reader.get_tile_stream(bbox).await?;

	let progress = get_progress_bar("Scanning tile sizes", (width_original * width_original) as u64);
	let compression = reader.parameters().tile_compression;
	let vec = stream
		.map_item_parallel(move |mut tile| Ok(tile.as_blob(compression)?.len()))
		.inspect(|| progress.inc(1))
		.to_vec()
		.await;
	progress.finish();

	log::debug!("Saving image");
	let mut result: Vec<u64> = vec![0; width_scaled * width_scaled];
	for (coord, size) in vec.iter() {
		let x = coord.x as usize / scale;
		let y = coord.y as usize / scale;
		if x >= width_scaled || y >= width_scaled {
			continue;
		}
		result[y * width_scaled + x] += *size;
	}

	let n = (scale * scale) as f64;
	let buffer = result
		.into_iter()
		.map(|v| ((v as f64 / n).max(1.0).log2() * 10.0).clamp(0.0, 255.0) as u8)
		.collect::<Vec<u8>>();

	let image = <DynamicImage as DynamicImageTraitConvert>::from_raw(width_scaled, width_scaled, buffer)?;

	let format = TileFormat::try_from_path(output_file)?;
	let blob = encode(&image, format, Some(100), Some(0))?;
	blob.save_to_file(output_file)?;

	log::debug!("Done, saved to {output_file:?}");
	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;
	use anyhow::Result;
	use assert_fs::TempDir;
	use versatiles_core::{Blob, TileFormat};
	use versatiles_image::{DynamicImageTraitOperation, GenericImageView};

	#[test]
	fn test_measure_tile_sizes() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let temp_file = temp_dir.path().join("image.png");

		run_command(vec![
			"versatiles",
			"dev",
			"measure-tile-sizes",
			"../testdata/berlin.mbtiles",
			&temp_file.display().to_string(),
		])?;

		let content = Blob::load_from_file(&temp_file)?;
		let image = versatiles_image::decode(&content, TileFormat::PNG)?;
		assert_eq!(image.dimensions(), (4096, 4096));

		let image = image.crop_imm(2195, 1339, 11, 9);
		assert_eq!(image.average_color(), [82]);

		Ok(())
	}
}
