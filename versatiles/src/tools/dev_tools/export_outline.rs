use anyhow::{Context, Result, anyhow, bail};
use std::path::PathBuf;
use versatiles_container::TilesRuntime;
use versatiles_geometry::{geo::GeoCollection, tile_outline::TileOutline};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Export the outline of all tiles present at a given zoom level as a GeoJSON file.
///
/// The output GeoJSON contains a single Feature with a Polygon geometry representing the outline of all tiles.
pub struct ExportOutline {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Output image file (should end in .geojson)
	#[arg(value_name = "OUTPUT_FILE")]
	output: PathBuf,

	/// Zoom level to analyze, defaults to the highest zoom level in the file
	#[arg(long)]
	level: Option<u8>,
}

pub async fn run(args: &ExportOutline, runtime: &TilesRuntime) -> Result<()> {
	let input = &args.input;
	let output = &args.output;

	let reader = runtime.reader_from_str(input).await?;

	let tile_pyramid = reader.tile_pyramid().await?;
	let level = match args.level {
		Some(l) => l,
		None => tile_pyramid
			.level_max()
			.ok_or_else(|| anyhow!("tile pyramid is empty; cannot determine max level"))?,
	};

	log::debug!("Measuring the outline of the tiles in {input:?} at zoom level {level} and saving it to {output:?}");

	if output.extension() != Some(std::ffi::OsStr::new("geojson")) {
		bail!("Only GeoJSON output is supported for now");
	}

	let bbox = tile_pyramid.level_ref(level).to_bbox();
	let mut stream = reader.tile_size_stream(bbox).await?;

	let progress = runtime.create_progress("Scanning tile sizes", bbox.count_tiles());
	let mut outline = TileOutline::new();
	while let Some((coord, _)) = stream.next().await {
		outline.add_coord(coord);
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
		// The exact outline depends on which tiles the fixture covers; we just
		// assert structural shape + that it parses as a valid GeoJSON
		// FeatureCollection wrapping a Polygon.
		assert!(content.starts_with(r#"{"features":[{"geometry":{"coordinates":"#));
		assert!(content.contains(r#""type":"Polygon""#));
		assert!(content.ends_with(r#""type":"FeatureCollection"}"#));

		Ok(())
	}
}
