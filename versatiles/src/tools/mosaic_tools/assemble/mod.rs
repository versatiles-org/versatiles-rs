//! Two-pass tile assembly: scan → write opaque → batch-composite translucent.
//!
//! # How it works
//!
//! **First pass** — streams every source once:
//! - **Opaque** tiles are written directly to the sink and recorded in `done`.
//! - **Empty** tiles are skipped.
//! - **Translucent** tiles are recorded as `(TileCoord, Vec<source_index>)`.
//!
//! **Between passes** — coords already in `done` are removed, tiles are collapsed
//! into signature groups (by source set) and partitioned into batches via PCA-based
//! recursive bisection, bounded by `--max-buffer-size`.
//!
//! **Second pass** — for each batch, only the needed sources are opened. Tiles are
//! composited onto a `TranslucentBuffer` and flushed to the sink once the batch is
//! complete.

mod cli;
mod partitioning;
mod pipeline;
mod tiles;
mod translucent_buffer;

pub use cli::Assemble;

use anyhow::{Result, ensure};
use versatiles_container::TilesRuntime;
use versatiles_core::TileCompression;
use versatiles_core::TileFormat;

/// Encoding configuration shared across assemble functions.
#[derive(Clone)]
struct AssembleConfig {
	quality: [Option<u8>; 32],
	lossless: bool,
	tile_format: TileFormat,
	tile_compression: TileCompression,
}

pub async fn run(args: &Assemble, runtime: &TilesRuntime) -> Result<()> {
	ensure!(args.paths.len() >= 2, "Need at least one input and one output path");
	let (input_args, output) = args.paths.split_at(args.paths.len() - 1);
	let output = &output[0];

	log::info!("mosaic assemble to {output:?}");

	let paths = cli::resolve_inputs(input_args)?;
	ensure!(!paths.is_empty(), "No input container paths resolved");

	let quality = cli::parse_quality(&args.quality)?;
	let max_buffer_size = cli::parse_buffer_size(&args.max_buffer_size)?;

	log::info!("assembling {} containers (two-pass)", paths.len());

	pipeline::assemble_two_pass(
		output,
		&paths,
		&quality,
		args.lossless,
		args.min_zoom,
		args.max_zoom,
		max_buffer_size,
		runtime,
	)
	.await
}
