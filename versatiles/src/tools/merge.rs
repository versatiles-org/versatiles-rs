//! `versatiles merge`: joins many tile containers into one, building the
//! stacking pipeline that `convert` would otherwise need written in VPL.

use std::{path::PathBuf, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use futures::{StreamExt, TryStreamExt, future::BoxFuture, stream};
use versatiles_container::{DataLocation, SharedTileSource, TileSource, TilesRuntime, convert_tiles_container_to_str};
use versatiles_core::{TileFormat, TileType, io::location_for_display};
use versatiles_pipeline::PipelineFactory;

use super::{convert::OutputOptions, inputs::resolve_inputs};

/// Above this many inputs, `merge` warns about open files: every input stays
/// open until the output is written. 256 is the default `ulimit -n` on macOS.
const MANY_INPUTS: usize = 256;

/// How many inputs are opened at the same time. Opening a remote container
/// takes a round trip or two, which adds up over hundreds of them.
const OPEN_CONCURRENCY: usize = 16;

/// The URL scheme by which the stacking pipeline refers to an opened input,
/// `merge-input://<index>`. Never written by a user, and never opened as a URL:
/// the pipeline factory's reader callback answers it.
const INPUT_SCHEME: &str = "merge-input";

#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
enum Strategy {
	/// each tile from the first input that has it, as it is (raster or vector)
	First,
	/// raster tiles alpha-composited, the first input on top
	Blend,
	/// vector tiles with the layers of every input, same-named layers merged
	Layers,
}

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	long_about = "\
Merge many tile containers into one.

Where more than one input has a tile, the earlier input takes priority. Every
input is opened, and its tile type checked, before any tile is read.

EXAMPLES

  # Join regional extracts into one file
  versatiles merge -o merged.versatiles 'regions/*.versatiles' extra.pmtiles

  # Put an overlay's layers into a basemap's tiles
  versatiles merge -o out.versatiles --strategy layers basemap.versatiles overlay.mbtiles

  # Inputs listed in a file, one per line
  versatiles merge -o out.pmtiles --max-zoom 14 @inputs.txt

For large raster mosaics, `versatiles mosaic assemble` reads the inputs in two
passes and keeps memory within --max-buffer-size."
)]
pub struct Subcommand {
	/// Input tile containers, earlier ones taking priority.
	/// Each may be anything `convert` reads (path, URL, data source expression),
	/// a glob such as 'tiles/*.versatiles' (quoted, matches sorted),
	/// a `.txt` file or an `@file` listing one input per line (`#` comments).
	#[arg(required = true, verbatim_doc_comment)]
	inputs: Vec<String>,

	/// output tile container path or SFTP URL
	#[arg(long, short, value_name = "path", display_order = 0)]
	output: String,

	/// how to combine tiles that more than one input has
	#[arg(long, value_enum, default_value_t = Strategy::First, display_order = 0)]
	strategy: Strategy,

	/// with `--strategy blend`, fill each input's gaps from its lower zoom levels
	#[arg(long, display_order = 0)]
	auto_overscale: bool,

	#[command(flatten)]
	output_options: OutputOptions,

	/// list the inputs and exit without opening any
	#[arg(long, display_order = 4)]
	dry_run: bool,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	ensure!(
		!arguments.auto_overscale || arguments.strategy == Strategy::Blend,
		"--auto-overscale needs --strategy blend"
	);

	let inputs = resolve_inputs(&arguments.inputs)?;
	ensure!(!inputs.is_empty(), "no inputs: every input list is empty");

	log::info!(
		"merge {} inputs into {:?}",
		inputs.len(),
		location_for_display(&arguments.output)
	);

	arguments.output_options.configure_runtime(runtime)?;
	let parameters = arguments.output_options.converter_parameters()?;

	if arguments.dry_run {
		for (index, input) in inputs.iter().enumerate() {
			log::info!("input {}: {}", index + 1, location_for_display(input));
		}
		return Ok(());
	}

	if inputs.len() > MANY_INPUTS {
		log::warn!(
			"merging {} inputs keeps {} containers open at once; if this fails with \"too many open files\", raise the limit, e.g. `ulimit -n {}`",
			inputs.len(),
			inputs.len(),
			inputs.len() + 64
		);
	}

	// Periodically log RSS so memory growth (e.g. toward an OOM kill, which the
	// process can't report itself) is visible in the merge's own output.
	let _memory_heartbeat = super::memory::start();

	let readers: Vec<SharedTileSource> = stream::iter(inputs.iter().enumerate())
		.map(|(index, input)| async move {
			runtime
				.reader_from_str(input)
				.await
				.with_context(|| format!("opening input {}: {}", index + 1, location_for_display(input)))
		})
		.buffered(OPEN_CONCURRENCY)
		.try_collect()
		.await?;

	let formats: Vec<TileFormat> = readers.iter().map(|r| *r.metadata().tile_format()).collect();
	check_formats(&inputs, &formats, arguments.strategy)?;

	// One input needs no stacking, unless it is to be overscaled.
	let source = if readers.len() == 1 && !arguments.auto_overscale {
		readers.into_iter().next().expect("one reader")
	} else {
		stack(readers, arguments.strategy, arguments.auto_overscale, runtime).await?
	};

	convert_tiles_container_to_str(source, parameters, &arguments.output, runtime.clone()).await?;

	log::info!("finished merging tiles");
	Ok(())
}

/// Refuses inputs that cannot be combined with `strategy`, naming the input
/// that differs, before any tile is read.
fn check_formats(inputs: &[String], formats: &[TileFormat], strategy: Strategy) -> Result<()> {
	let name = |index: usize| format!("input {} ({})", index + 1, location_for_display(&inputs[index]));
	let first = formats[0];

	for (index, format) in formats.iter().enumerate().skip(1) {
		if format.to_type() != first.to_type() {
			bail!(
				"{} has {} tiles ({format}), but {} has {} tiles ({first}): raster and vector tiles cannot be merged",
				name(index),
				format.to_type(),
				name(0),
				first.to_type()
			);
		}
	}

	match (strategy, first.to_type()) {
		(Strategy::First, _) => {
			if let Some(index) = formats.iter().position(|format| *format != first) {
				bail!(
					"--strategy first takes every tile as it is, so the inputs need the same tile format, but {} has {} and {} has {first}{}",
					name(index),
					formats[index],
					name(0),
					if first.to_type() == TileType::Raster {
						"; --strategy blend re-encodes raster tiles"
					} else {
						""
					}
				);
			}
		}
		(Strategy::Blend, TileType::Raster) | (Strategy::Layers, TileType::Vector) => {}
		(Strategy::Blend, tile_type) => {
			bail!("--strategy blend composites raster tiles, but the inputs have {tile_type} tiles ({first})")
		}
		(Strategy::Layers, tile_type) => {
			bail!("--strategy layers merges vector tiles, but the inputs have {tile_type} tiles ({first})")
		}
	}
	Ok(())
}

/// Builds the stacking operation for `strategy` over the opened `readers`.
///
/// It is built from VPL, so it is exactly what a hand-written `from_stacked`,
/// `from_stacked_raster` or `from_merged_vector` would give. But `from_container`
/// reads only paths and URLs, while an input may be any data source; so each
/// input is named by an `INPUT_SCHEME` URL that the factory's reader callback
/// answers with the reader already opened for it.
async fn stack(
	readers: Vec<SharedTileSource>,
	strategy: Strategy,
	auto_overscale: bool,
	runtime: &TilesRuntime,
) -> Result<SharedTileSource> {
	let sources = (0..readers.len())
		.map(|index| format!("from_container filename=\"{INPUT_SCHEME}://{index}\""))
		.collect::<Vec<_>>()
		.join(", ");
	let vpl = match strategy {
		Strategy::First => format!("from_stacked [ {sources} ]"),
		Strategy::Blend => format!("from_stacked_raster auto_overscale={auto_overscale} [ {sources} ]"),
		Strategy::Layers => format!("from_merged_vector [ {sources} ]"),
	};

	let readers = Arc::new(readers);
	let factory = PipelineFactory::new_default(
		DataLocation::from(std::env::current_dir()?),
		Box::new(
			move |location: DataLocation,
			      _ssh_identity: Option<PathBuf>|
			      -> BoxFuture<'static, Result<Box<dyn TileSource>>> {
				let readers = Arc::clone(&readers);
				Box::pin(async move {
					let index = match &location {
						DataLocation::Url(url) if url.scheme() == INPUT_SCHEME => {
							url.host_str().and_then(|host| host.parse().ok())
						}
						_ => None,
					};
					let reader = index
						.and_then(|index: usize| readers.get(index))
						.with_context(|| format!("not an input of this merge: {location}"))?;
					Ok(Box::new(Arc::clone(reader)) as Box<dyn TileSource>)
				})
			},
		),
		runtime.clone(),
	);

	Ok(Arc::from(factory.operation_from_vpl(&vpl).await?))
}

#[cfg(test)]
mod tests {
	use anyhow::Result;
	use assert_fs::TempDir;
	use rstest::rstest;
	use versatiles_core::TileFormat::{self, MVT, PNG, WEBP};

	use super::{Strategy, check_formats};
	use crate::tests::run_command;

	fn check(formats: &[TileFormat], strategy: Strategy) -> Result<()> {
		let inputs: Vec<String> = (0..formats.len()).map(|i| format!("in{i}.versatiles")).collect();
		check_formats(&inputs, formats, strategy)
	}

	#[rstest]
	#[case::first_raster(&[PNG, PNG], Strategy::First)]
	#[case::first_vector(&[MVT, MVT, MVT], Strategy::First)]
	#[case::blend_mixed_raster_formats(&[PNG, WEBP], Strategy::Blend)]
	#[case::layers(&[MVT, MVT], Strategy::Layers)]
	#[case::single(&[MVT], Strategy::Layers)]
	fn formats_that_merge(#[case] formats: &[TileFormat], #[case] strategy: Strategy) {
		check(formats, strategy).unwrap();
	}

	#[rstest]
	#[case::raster_with_vector(&[PNG, PNG, MVT], Strategy::First, "input 3 (in2.versatiles) has vector tiles (mvt), but input 1 (in0.versatiles) has raster tiles (png)")]
	#[case::first_mixed_formats(&[PNG, WEBP], Strategy::First, "input 2 (in1.versatiles) has webp and input 1 (in0.versatiles) has png; --strategy blend")]
	#[case::blend_vector(&[MVT, MVT], Strategy::Blend, "--strategy blend composites raster tiles, but the inputs have vector tiles")]
	#[case::layers_raster(&[PNG], Strategy::Layers, "--strategy layers merges vector tiles, but the inputs have raster tiles")]
	fn formats_that_do_not_merge(#[case] formats: &[TileFormat], #[case] strategy: Strategy, #[case] expected: &str) {
		let error = check(formats, strategy).unwrap_err().to_string();
		assert!(error.contains(expected), "{error}");
	}

	fn merge(args: &[&str]) -> Result<()> {
		run_command([&["versatiles", "merge"], args].concat()).map(drop)
	}

	#[test]
	fn merges_two_vector_containers() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let output = temp_dir.path().join("merged.versatiles");
		let output = output.to_str().unwrap();
		merge(&[
			"-o",
			output,
			"--max-zoom=3",
			"../testdata/berlin.mbtiles",
			"[,vpl](from_debug format=mvt)",
		])?;
		merge(&[
			"-o",
			output,
			"--strategy=layers",
			"--max-zoom=3",
			"../testdata/berlin.mbtiles",
			"[,vpl](from_debug format=mvt)",
		])?;
		Ok(())
	}

	#[test]
	fn blends_raster_inputs_of_different_formats() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let output = temp_dir.path().join("merged.versatiles");
		merge(&[
			"-o",
			output.to_str().unwrap(),
			"--strategy=blend",
			"--max-zoom=2",
			"[,vpl](from_debug format=png)",
			"[,vpl](from_debug format=webp)",
		])
	}

	#[test]
	fn a_single_input_is_converted() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let output = temp_dir.path().join("single.versatiles");
		merge(&["-o", output.to_str().unwrap(), "../testdata/berlin.mbtiles"])?;
		assert!(output.exists());
		Ok(())
	}

	/// The input that differs is named, and nothing is written.
	#[test]
	fn mixed_tile_types_are_refused() -> Result<()> {
		let temp_dir = TempDir::new()?;
		let output = temp_dir.path().join("merged.versatiles");
		let error = merge(&[
			"-o",
			output.to_str().unwrap(),
			"../testdata/berlin.mbtiles",
			"[,vpl](from_debug format=png)",
		])
		.unwrap_err();
		assert!(
			format!("{error:#}").contains("input 2 ([,vpl](from_debug format=png)) has raster tiles"),
			"{error:#}"
		);
		assert!(!output.exists());
		Ok(())
	}

	#[test]
	fn auto_overscale_needs_blend() {
		let error = merge(&["-o", "out.versatiles", "--auto-overscale", "a.versatiles"]).unwrap_err();
		assert!(format!("{error:#}").contains("--strategy blend"), "{error:#}");
	}

	/// `--dry-run` opens nothing, so inputs that do not exist are no error.
	#[test]
	fn dry_run_opens_nothing() -> Result<()> {
		merge(&[
			"-o",
			"out.versatiles",
			"--dry-run",
			"/nonexistent/a.versatiles",
			"/nonexistent/b.pmtiles",
		])
	}

	#[test]
	fn a_glob_that_matches_nothing_is_refused() {
		let error = merge(&["-o", "out.versatiles", "/nonexistent_dir_xyz/*.versatiles"]).unwrap_err();
		assert!(format!("{error:#}").contains("matched no files"), "{error:#}");
	}
}
