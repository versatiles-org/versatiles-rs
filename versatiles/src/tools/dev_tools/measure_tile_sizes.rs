use anyhow::{Result, anyhow, bail};
use std::path::PathBuf;
use versatiles::{TileBBox, TileFormat, config::Config, progress::get_progress_bar};
use versatiles_container::get_reader;
use versatiles_image::{DynamicImage, DynamicImageTraitConvert, encode};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_help_flag = true, disable_version_flag = true)]
pub struct MeasureTileSizes {
	/// Input file
	#[arg(value_name = "INPUT_FILE")]
	input: PathBuf,

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
	let config = Config::default().arc();
	let input_file = &args.input;
	let output_file = &args.output;
	let level = args.level;
	let scale = args.scale;
	let width_original = 1 << level;
	let width_scaled = width_original / scale;

	log::info!(
		"Measuring tile sizes in {input_file:?} at zoom level {level}, generating an {width_scaled}x{width_scaled} image and saving it to {output_file:?}"
	);

	if !output_file.ends_with(".png") {
		bail!("Only PNG output is supported for now");
	}

	let reader = get_reader(
		input_file
			.as_os_str()
			.to_str()
			.ok_or(anyhow!("Invalid input file path"))?,
		config,
	)
	.await?;
	let bbox = TileBBox::new_full(level)?;
	let stream = reader.get_tile_stream(bbox).await?;

	let progress = get_progress_bar("Scanning tile sizes", (width_original * width_original) as u64);
	let compression = reader.parameters().tile_compression;
	let vec = stream
		.map_item_parallel(move |mut tile| Ok(tile.as_blob(compression).len()))
		.inspect(|| progress.inc(1))
		.to_vec()
		.await;
	progress.finish();

	log::info!("Saving image");
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

	log::info!("Done, saved to {output_file:?}");
	Ok(())
}
