use anyhow::{Context, Result, bail};
use std::path::PathBuf;
use versatiles::get_registry;
use versatiles_container::{ProcessingConfig, UrlPath};
use versatiles_core::progress::get_progress_bar;
use versatiles_geometry::{geo::GeoCollection, tile_outline::TileOutline};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_help_flag = true, disable_version_flag = true)]
/// Export the outline of all tiles present at a given zoom level as a GeoJSON file.
///
/// The output GeoJSON contains a single Feature with a Polygon geometry representing the outline of all tiles.
pub struct ExportOutline {
	/// Input file
	#[arg(value_name = "INPUT_FILE")]
	input: UrlPath,

	/// Output image file (should end in .geojson)
	#[arg(value_name = "OUTPUT_FILE")]
	output: PathBuf,

	/// Zoom level to analyze, defaults to the highest zoom level in the file
	#[arg(long)]
	level: Option<u8>,
}

pub async fn run(args: &ExportOutline) -> Result<()> {
	let input = &args.input;
	let output = &args.output;

	let reader = get_registry(ProcessingConfig::default()).get_reader(input).await?;

	let compression = reader.parameters().tile_compression;
	let bbox_pyramid = reader.parameters().bbox_pyramid.clone();
	let level = args.level.unwrap_or_else(|| bbox_pyramid.get_level_max().unwrap());

	log::debug!("Measuring the outline of the tiles in {input:?} at zoom level {level} and saving it to {output:?}");

	if output.extension() != Some(std::ffi::OsStr::new("geojson")) {
		bail!("Only GeoJSON output is supported for now");
	}

	let bbox = *bbox_pyramid.get_level_bbox(level);
	let mut stream = reader
		.get_tile_stream(bbox)
		.await?
		.map_item_parallel(move |mut tile| Ok(tile.as_blob(compression)?.len()));

	let progress = get_progress_bar("Scanning tile sizes", bbox.count_tiles());
	let mut outline = TileOutline::new();
	while let Some(entry) = stream.next().await {
		outline.add_coord(entry.0);
		progress.inc(1);
	}

	let feature = outline.to_feature();
	let json = GeoCollection::from(vec![feature]).to_json(Some(6)).stringify();
	let mut file = std::fs::File::create(output)
		.with_context(|| format!("Failed to create output file \"{}\"", output.display()))?;

	std::io::Write::write_all(&mut file, json.as_bytes())
		.with_context(|| format!("Failed to write to output file \"{}\"", output.display()))?;

	progress.finish();

	log::debug!("Done, saved to {output:?}");
	Ok(())
}

#[cfg(test)]
mod tests {
	use crate::tests::run_command;
	use anyhow::Result;
	use assert_fs::TempDir;

	#[test]
	fn test_mbtiles_to_geojson() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let temp_file = temp_dir.path().join("result.geojson").display().to_string();

		run_command(vec![
			"versatiles",
			"dev",
			"export-outline",
			"../testdata/berlin.mbtiles",
			&temp_file,
		])?;

		let content = std::fs::read_to_string(temp_file)?;
		assert_eq!(
			content,
			r#"{"features":[{"geometry":{"coordinates":[[[13.07373,52.375599],[13.095703,52.375599],[13.095703,52.362183],[13.161621,52.362183],[13.161621,52.375599],[13.205566,52.375599],[13.205566,52.389011],[13.293457,52.389011],[13.293457,52.375599],[13.337402,52.375599],[13.337402,52.362183],[13.557129,52.362183],[13.557129,52.375599],[13.579102,52.375599],[13.579102,52.362183],[13.601074,52.362183],[13.601074,52.348763],[13.623047,52.348763],[13.623047,52.321911],[13.666992,52.321911],[13.666992,52.335339],[13.688965,52.335339],[13.688965,52.348763],[13.710938,52.348763],[13.710938,52.375599],[13.73291,52.375599],[13.73291,52.389011],[13.754883,52.389011],[13.754883,52.429222],[13.776855,52.429222],[13.776855,52.456009],[13.754883,52.456009],[13.754883,52.469397],[13.73291,52.469397],[13.73291,52.536273],[13.710938,52.536273],[13.710938,52.549636],[13.64502,52.549636],[13.64502,52.562995],[13.601074,52.562995],[13.601074,52.589701],[13.623047,52.589701],[13.623047,52.603048],[13.535156,52.603048],[13.535156,52.61639],[13.557129,52.61639],[13.557129,52.629729],[13.579102,52.629729],[13.579102,52.66972],[13.513184,52.66972],[13.513184,52.683043],[13.425293,52.683043],[13.425293,52.66972],[13.337402,52.66972],[13.337402,52.683043],[13.249512,52.683043],[13.249512,52.656394],[13.227539,52.656394],[13.227539,52.66972],[13.205566,52.66972],[13.205566,52.643063],[13.183594,52.643063],[13.183594,52.61639],[13.07373,52.61639],[13.07373,52.57635],[13.095703,52.57635],[13.095703,52.562995],[13.07373,52.562995],[13.07373,52.522906],[13.095703,52.522906],[13.095703,52.469397],[13.07373,52.469397],[13.07373,52.375599]],[[13.666992,52.48278],[13.666992,52.509535],[13.710938,52.509535],[13.710938,52.49616],[13.688965,52.49616],[13.688965,52.48278],[13.666992,52.48278]]],"type":"Polygon"},"properties":{},"type":"Feature"}],"type":"FeatureCollection"}"#
		);

		Ok(())
	}
}
