use anyhow::{Context, Result, anyhow, bail};
use std::path::PathBuf;
use versatiles_container::get_reader;
use versatiles_core::{Config, progress::get_progress_bar};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_help_flag = true, disable_version_flag = true)]
pub struct ExportOutline {
	/// Input file
	#[arg(value_name = "INPUT_FILE")]
	input: PathBuf,

	/// Output image file (should end in .geojson)
	#[arg(value_name = "OUTPUT_FILE")]
	output: PathBuf,

	/// Zoom level to analyze, defaults to the highest zoom level in the file
	#[arg(long)]
	level: Option<u8>,
}

pub async fn run(args: &ExportOutline) -> Result<()> {
	let config = Config::default().arc();
	let input_file = &args.input;
	let output_file = &args.output;

	let reader = get_reader(
		input_file
			.as_os_str()
			.to_str()
			.ok_or(anyhow!("Invalid input file path"))?,
		config,
	)
	.await?;

	let compression = reader.parameters().tile_compression;
	let bbox_pyramid = reader.parameters().bbox_pyramid.clone();
	let level = args.level.unwrap_or_else(|| bbox_pyramid.get_level_max().unwrap());

	log::info!(
		"Measuring the outline of the tiles in {input_file:?} at zoom level {level} and saving it to {output_file:?}"
	);

	if output_file.extension() != Some(std::ffi::OsStr::new("geojson")) {
		bail!("Only GeoJSON output is supported for now");
	}

	let bbox = *bbox_pyramid.get_level_bbox(level);
	let mut stream = reader
		.get_tile_stream(bbox)
		.await?
		.map_item_parallel(move |mut tile| Ok(tile.as_blob(compression).len()));

	let progress = get_progress_bar("Scanning tile sizes", bbox.count_tiles());
	let mut outline = versatiles_geometry::tile_outline::TileOutline::new();
	while let Some(entry) = stream.next().await {
		outline.add_coord(entry.0);
		progress.inc(1);
	}

	let json = outline.to_json_string();
	let mut file = std::fs::File::create(output_file)
		.with_context(|| format!("Failed to create output file \"{}\"", output_file.display()))?;

	std::io::Write::write_all(&mut file, json.as_bytes())
		.with_context(|| format!("Failed to write to output file \"{}\"", output_file.display()))?;

	progress.finish();

	log::info!("Done, saved to {output_file:?}");
	Ok(())
}
