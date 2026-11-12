//! Two-pass tile assembly: scan → write opaque → batch-composite translucent.
//!
//! # How it works
//!
//! **First pass** — streams every source once:
//! - **Opaque** tiles are written directly to the sink and recorded in `done`.
//! - **Empty** tiles are skipped.
//! - **Translucent** tiles are recorded as `(TileCoord, Vec<source_index>)`.
//!
//! **Between passes** — coords already in `done` are removed; the remaining tiles
//! are packed greedily into batches. Each batch is seeded with the tile needing the
//! most sources, then filled with tiles that maximise source overlap, admission
//! gated by the batch's peak in-memory footprint (`max_tiles_in_memory`) so it stays
//! within `--max-buffer-size`.
//!
//! **Second pass** — for each batch, only the needed sources are opened. Tiles are
//! composited onto a `TranslucentBuffer` and flushed to the sink as soon as their
//! last contributing source has been processed, keeping peak memory bounded.

mod cli;
mod pipeline;
mod tiles;
mod translucent_buffer;

use std::sync::Arc;

use anyhow::{Context, Result, anyhow, ensure};
pub use cli::Assemble;
use versatiles_container::TilesRuntime;
use versatiles_core::{TileCompression, TileFormat};
use versatiles_pipeline::{QualityByZoom, QualityMap, TileQuality};

/// Encoding configuration shared across assemble functions.
#[derive(Clone)]
struct AssembleConfig {
	quality: TileQuality,
	lossless: bool,
	tile_format: TileFormat,
	tile_compression: TileCompression,
}

pub async fn run(args: &Assemble, runtime: &TilesRuntime) -> Result<()> {
	ensure!(args.paths.len() >= 2, "need at least one input and one output path");
	let (input_args, output) = args.paths.split_at(args.paths.len() - 1);
	let output = &output[0];

	log::info!("mosaic assemble to {output:?}");

	let paths = crate::tools::inputs::resolve_inputs(input_args)?;
	ensure!(!paths.is_empty(), "no input container paths resolved");

	let quality = read_quality(&args.quality, args.quality_map.as_deref())?;
	let max_buffer_size = cli::parse_buffer_size(&args.max_buffer_size)?;

	log::info!("assembling {} containers (two-pass)", paths.len());

	let first = pipeline::scan_sources(
		output,
		&paths,
		&quality,
		args.lossless,
		args.min_zoom,
		args.max_zoom,
		runtime,
	)
	.await?;

	let batches = pipeline::prepare_batches(first.translucent_map, first.done, first.tile_dim, max_buffer_size);

	pipeline::composite_batches(&batches, &paths, &first.config, &first.sink, runtime).await?;

	let sink = Arc::try_unwrap(first.sink).map_err(|_| anyhow!("sink still has references"))?;
	sink.finish(&first.tilejson, runtime).await?;

	log::info!("finished mosaic assemble");
	Ok(())
}

/// `--quality`, with the map its ranges read.
fn read_quality(quality: &str, quality_map: Option<&str>) -> Result<TileQuality> {
	let levels = QualityByZoom::try_from(quality)?;
	let map = match quality_map {
		None => None,
		Some(path) => {
			// A map nothing reads is a command that does not do what it says.
			ensure!(
				levels.uses_map(),
				"--quality-map is read only by a quality range such as '16:30-50', and --quality has none"
			);
			let png = std::fs::read(path).with_context(|| format!("reading the quality map {path}"))?;
			let map = QualityMap::from_png(&png).with_context(|| format!("reading the quality map {path}"))?;
			Some(Arc::new(map))
		}
	};
	TileQuality::new(levels, map).context("give the map with --quality-map")
}

#[cfg(test)]
mod tests {
	use super::*;

	fn error(quality: &str, quality_map: Option<&str>) -> String {
		format!("{:#}", read_quality(quality, quality_map).unwrap_err())
	}

	#[test]
	fn a_range_and_a_map_need_each_other() {
		assert!(read_quality("70,14:50", None).is_ok());
		assert!(error("70,14:30-50", None).contains("--quality-map"));
		assert!(error("70,14:50", Some("missing.png")).contains("read only by a quality range"));
		assert!(error("70,14:30-50", Some("missing.png")).contains("missing.png"));
	}
}
