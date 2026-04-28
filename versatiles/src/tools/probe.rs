use anyhow::Result;
use versatiles_container::{TileSource, TilesRuntime};
use versatiles_core::{ProbeDepth, utils::PrettyPrint};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// Tile container to probe (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(required = true, verbatim_doc_comment)]
	filename: String,

	/// deep scan (depending on the container implementation)
	///   -d: scans container metadata
	///  -dd: scans all tile sizes
	/// -ddd: scans all tile contents
	#[arg(long, short, action = clap::ArgAction::Count, verbatim_doc_comment)]
	deep: u8,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	log::info!("probe {:?}", arguments.filename);

	let reader = runtime.reader_from_str(&arguments.filename).await?;

	let level = match arguments.deep {
		0 => ProbeDepth::Shallow,
		1 => ProbeDepth::Container,
		2 => ProbeDepth::TileSizes,
		3..=255 => ProbeDepth::TileContents,
	};

	log::debug!("probing {:?} at depth {:?}", arguments.filename, level);
	probe(&**reader, level, runtime).await?;

	Ok(())
}

/// Performs a hierarchical CLI probe of `source` at the specified depth.
///
/// Writes metadata, container specifics, tiles, and tile contents to a fresh
/// `PrettyPrint` reporter based on `level`. Format-specific details are
/// delegated to the source via `TileSource::probe_container` and
/// `TileSource::probe_tile_contents`.
pub async fn probe(source: &dyn TileSource, level: ProbeDepth, runtime: &TilesRuntime) -> Result<()> {
	use ProbeDepth::{Container, TileContents, TileSizes};

	let mut print = PrettyPrint::new();

	let cat = print.category("meta_data").await;
	cat.add_key_value("source_type", &source.source_type().to_string())
		.await;
	cat.add_key_json("meta", &source.tilejson().as_json_value()).await;

	probe_metadata(source, &mut print.category("parameters").await).await?;

	if matches!(level, Container | TileSizes | TileContents) {
		log::debug!("probing source {:?} at depth {:?}", source.source_type(), level);
		source
			.probe_container(&mut print.category("container").await, runtime)
			.await?;
	}

	if matches!(level, TileSizes | TileContents) {
		log::debug!(
			"probing tiles {:?} at depth {:?}",
			source.tilejson().as_json_value(),
			level
		);
		probe_tile_sizes(source, &mut print.category("tiles").await, runtime).await?;
	}

	if matches!(level, TileContents) {
		log::debug!(
			"probing tile contents {:?} at depth {:?}",
			source.tilejson().as_json_value(),
			level
		);
		probe_tile_contents(source, &mut print.category("tile contents").await, runtime).await?;
	}

	Ok(())
}

/// Writes source metadata (tile pyramid, formats, compression) to `print`.
pub async fn probe_metadata(source: &dyn TileSource, print: &mut PrettyPrint) -> Result<()> {
	let metadata = source.metadata();
	let tile_pyramid = source.tile_pyramid().await?;
	let rows: Vec<Vec<String>> = tile_pyramid
		.iter()
		.filter(|level| !level.is_empty())
		.map(|level| {
			let bbox = level.to_bbox();
			let tiles = level.count_tiles();
			let coverage = tiles * 100 / bbox.count_tiles();
			vec![
				format!("{}", level.level()),
				format_integer_str(&bbox.x_min().unwrap().to_string()),
				format_integer_str(&bbox.x_max().unwrap().to_string()),
				format_integer_str(&bbox.y_min().unwrap().to_string()),
				format_integer_str(&bbox.y_max().unwrap().to_string()),
				format_integer_str(&tiles.to_string()),
				format!("{coverage}%"),
			]
		})
		.collect();
	print
		.add_table(
			"tile_pyramid",
			&["level", "x0", "x1", "y0", "y1", "tiles", "coverage"],
			&rows,
		)
		.await;
	print
		.add_key_value("tile compression", metadata.tile_compression())
		.await;
	print.add_key_value("tile format", metadata.tile_format()).await;
	Ok(())
}

/// Formats a `u64` with underscores as thousands separators (e.g. `1_234_567`).
fn format_integer_str(v: &str) -> String {
	let mut result = String::new();
	for (i, c) in v.chars().enumerate() {
		if i > 0 && (v.len() - i).is_multiple_of(3) {
			result.push('_');
		}
		result.push(c);
	}
	result
}

/// Scans all tiles, reporting average size and the top-10 biggest tiles.
#[allow(clippy::too_many_lines)]
pub async fn probe_tile_sizes(source: &dyn TileSource, print: &mut PrettyPrint, runtime: &TilesRuntime) -> Result<()> {
	#[derive(Debug)]
	#[allow(dead_code)]
	struct Entry {
		size: u64,
		x: u32,
		y: u32,
		z: u8,
	}

	let mut biggest_tiles: Vec<Entry> = Vec::new();
	let mut min_size: u64 = 0;
	let mut size_sum: u64 = 0;
	let mut tile_count: u64 = 0;
	let mut level_stats: Vec<(u8, u64, u64)> = Vec::new();

	let tile_pyramid = source.tile_pyramid().await?;
	let total_tiles = tile_pyramid.count_tiles();
	let progress = runtime.create_progress("scanning tiles", total_tiles);

	for bbox in tile_pyramid.to_iter_bboxes().filter(|b| !b.is_empty()) {
		let mut level_size_sum: u64 = 0;
		let mut level_count: u64 = 0;
		let mut stream = source.tile_size_stream(bbox).await?;
		while let Some((coord, size_u32)) = stream.next().await {
			let size = u64::from(size_u32);

			tile_count += 1;
			size_sum += size;
			level_size_sum += size;
			level_count += 1;
			progress.inc(1);

			if size < min_size {
				continue;
			}

			let pos = biggest_tiles
				.binary_search_by(|e| e.size.cmp(&size).reverse())
				.unwrap_or_else(|p| p);
			biggest_tiles.insert(
				pos,
				Entry {
					size,
					x: coord.x,
					y: coord.y,
					z: coord.level,
				},
			);
			if biggest_tiles.len() > 10 {
				biggest_tiles.pop();
			}
			min_size = biggest_tiles.last().expect("biggest_tiles is non-empty").size;
		}
		level_stats.push((bbox.level(), level_count, level_size_sum));
	}
	progress.finish();

	if tile_count > 0 {
		print.add_key_value("tile count", &tile_count).await;
		print
			.add_key_value("average tile size", &size_sum.div_euclid(tile_count))
			.await;

		let rows: Vec<Vec<String>> = biggest_tiles
			.iter()
			.enumerate()
			.map(|(i, e)| {
				vec![
					format!("{}", i + 1),
					format!("{}", e.z),
					format!("{}", e.x),
					format!("{}", e.y),
					format_integer_str(&e.size.to_string()),
				]
			})
			.collect();
		print
			.add_table("biggest tiles", &["#", "z", "x", "y", "size"], &rows)
			.await;

		let rows: Vec<Vec<String>> = level_stats
			.iter()
			.map(|(level, count, size)| {
				let avg = if *count > 0 { size / count } else { 0 };
				vec![
					format!("{level}"),
					format_integer_str(&count.to_string()),
					format_integer_str(&size.to_string()),
					format_integer_str(&avg.to_string()),
				]
			})
			.collect();
		print
			.add_table(
				"tile size analysis per level",
				&["level", "count", "size_sum", "avg_size"],
				&rows,
			)
			.await;
	} else {
		print.add_warning("no tiles found").await;
	}

	Ok(())
}

/// Writes sample tile content diagnostics or a placeholder if not implemented.
///
/// Format-specific hook for the CLI probe orchestration. Override to inspect
/// tile payloads (e.g., vector layer stats, raster histograms).
async fn probe_tile_contents(_source: &dyn TileSource, print: &mut PrettyPrint, _runtime: &TilesRuntime) -> Result<()> {
	print
		.add_warning("deep tile contents probing is not implemented for this source")
		.await;
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::tests::run_command;
	use versatiles::runtime::create_test_runtime;

	#[test]
	fn test_local() -> Result<()> {
		run_command(vec!["versatiles", "probe", "-q", "../testdata/berlin.mbtiles"])?;
		Ok(())
	}

	#[test]
	fn test_remote() -> Result<()> {
		run_command(vec![
			"versatiles",
			"probe",
			"-q",
			"https://download.versatiles.org/osm.versatiles",
		])?;
		Ok(())
	}

	/// Exercises the full orchestration and each free function against a real
	/// MBTiles file. This replaces the per-method tests that used to live on
	/// `TileSource` before the probe helpers moved out of the trait.
	#[tokio::test]
	async fn probe_all_levels_against_mbtiles() -> Result<()> {
		let runtime = create_test_runtime();
		let reader = runtime.reader_from_str("../testdata/berlin.mbtiles").await?;
		let source: &dyn TileSource = &**reader;

		probe(source, ProbeDepth::Shallow, &runtime).await?;
		probe(source, ProbeDepth::Container, &runtime).await?;

		let mut printer = PrettyPrint::new();
		probe_metadata(source, &mut printer).await?;
		let out = printer.stringify().await;
		assert!(out.contains("tile compression"), "got: {out}");
		assert!(out.contains("tile format"), "got: {out}");

		let mut printer = PrettyPrint::new();
		probe_tile_sizes(source, &mut printer.category("tiles").await, &runtime).await?;
		let out = printer.stringify().await;
		assert!(out.contains("tile count"), "got: {out}");
		assert!(out.contains("biggest tiles"), "got: {out}");
		assert!(out.contains("tile size analysis per level"), "got: {out}");

		Ok(())
	}
}
