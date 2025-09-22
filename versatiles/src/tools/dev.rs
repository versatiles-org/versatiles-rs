use anyhow::{Result, bail};
use log::info;
use versatiles::{TileBBox, config::Config, progress::get_progress_bar};
use versatiles_container::get_reader;
use versatiles_image::{DynamicImage, DynamicImageTraitConvert, avif};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_help_flag = true, disable_version_flag = true)]
pub struct Subcommand {
	#[command(subcommand)]
	sub_command: DevCommands,
}

#[derive(clap::Subcommand, Debug)]
enum DevCommands {
	MeasureTileSizes(MeasureTileSizes),
}

#[derive(clap::Args, Debug)]
struct MeasureTileSizes {
	#[arg(help = "Input file", value_name = "INPUT_FILE")]
	input: String,

	#[arg(help = "Output image file", value_name = "OUTPUT_FILE")]
	output: String,

	#[arg(help = "Zoom level to analyze", value_name = "int", default_value = "14")]
	level: u8,

	#[arg(help = "Scale factor", value_name = "int", default_value = "4")]
	scale: usize,
}

#[tokio::main]
pub async fn run(command: &Subcommand) -> Result<()> {
	let config = Config::default().arc();
	match &command.sub_command {
		DevCommands::MeasureTileSizes(args) => {
			let input_file = &args.input;
			let output_file = &args.output;
			let level = args.level;
			let scale = args.scale;
			let width_original = 1 << level;
			let width_scaled = width_original / scale;

			info!(
				"Measuring tile sizes in {input_file} at zoom level {level}, generating an {width_scaled}x{width_scaled} image and saving it to {output_file}"
			);

			if !output_file.ends_with(".avif") {
				bail!("Only AVIF output is supported for now");
			}

			let reader = get_reader(input_file, config).await?;
			let bbox = TileBBox::new_full(level)?;

			let progress = get_progress_bar("Scanning tile sizes", (width_original * width_original) as u64);
			let stream = reader.get_tile_stream(bbox).await?;
			let vec = stream
				.map_item_parallel(|tile| Ok(tile.len()))
				.inspect(|| progress.inc(1))
				.to_vec()
				.await;
			progress.finish();

			info!("Saving image");
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
				.map(|v| ((v as f64 / n).max(1.0).log2() - 10.0).clamp(0.0, 255.0) as u8)
				.collect::<Vec<u8>>();

			let image =
				<DynamicImage as DynamicImageTraitConvert>::from_raw(width_scaled as u32, width_scaled as u32, buffer)?;

			let blob = avif::compress(&image, Some(99), Some(0))?;
			blob.save_to_file(output_file)?;

			info!("Done, saved to {output_file}");
		}
	};

	Ok(())
}
