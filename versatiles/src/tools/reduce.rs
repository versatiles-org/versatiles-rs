//! `versatiles reduce` — strip a tileset down to what one style draws.
//!
//! Takes a MapLibre style, renders the VPL pipeline that reduces a tileset to it, and either prints
//! that pipeline or runs it.
//!
//! The work itself belongs to the `vector_reduce_to_style` operation, not here: reading the style
//! and choosing the filters is something a VPL document should be able to do without a subcommand,
//! and having one implementation means the CLI and a hand-written pipeline cannot disagree. What
//! this adds is the convenience of naming an input and an output.
//!
//! `--print` composes with what already exists, since inline VPL is a first-class input:
//!
//! ```text
//! versatiles reduce --print -s style.json in.versatiles | versatiles convert "[,vpl]-" out.versatiles
//! versatiles reduce --print -s style.json in.versatiles | versatiles convert --dry-run "[,vpl]-" x
//! ```

use std::path::PathBuf;

use anyhow::{Result, bail};
use versatiles_container::{TileSource, TilesConverterParameters, TilesRuntime, convert_tiles_container_to_str};
use versatiles_derive::context;
use versatiles_pipeline::{PipelineReader, VPLNode, VPLPipeline};

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	/// Input tile container (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(verbatim_doc_comment)]
	input_file: String,

	/// Output tile container path or SFTP URL. Required unless --print is given.
	/// Supported formats: *.versatiles, *.tar, *.pmtiles, *.mbtiles or a directory.
	#[arg(verbatim_doc_comment)]
	output_file: Option<String>,

	/// MapLibre style JSON file the tileset should be reduced to.
	#[arg(long, short = 's', value_name = "file", verbatim_doc_comment)]
	style: PathBuf,

	/// print the rendered VPL pipeline and exit, without reading or writing any tiles
	#[arg(long, verbatim_doc_comment)]
	print: bool,
}

#[tokio::main]
pub async fn run(arguments: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	let pipeline = build_pipeline(arguments)?;

	if arguments.print {
		println!("{pipeline}");
		return Ok(());
	}

	let Some(output_file) = &arguments.output_file else {
		bail!("an output file is required unless --print is given");
	};

	log::info!("reduce {:?} to {output_file:?}", arguments.input_file);

	// Same reasoning as `convert`: a tile dropped by a read error would make the reduced output
	// indistinguishable from a correct one, and the whole point is that the output is trustworthy.
	runtime.set_abort_on_error(true);

	let _memory_heartbeat = super::memory::start();

	// Built from the pipeline itself rather than from its text. `reader_from_str` would want the
	// `[,vpl](…)` wrapper, and re-parsing an expression that contains its own parentheses and
	// quotes is a round-trip with nothing to gain — the structure is already here.
	let reader = PipelineReader::from_pipeline(pipeline, "reduce", &std::env::current_dir()?, runtime.clone())
		.await?
		.into_shared();

	convert_tiles_container_to_str(
		reader,
		TilesConverterParameters::default(),
		output_file,
		runtime.clone(),
	)
	.await?;

	log::info!("finished reducing tiles");
	Ok(())
}

/// Renders the whole pipeline, source included.
///
/// The source node is part of the output because the text has to be runnable on its own — piped
/// into `convert`, or pasted into a VPL document — and a transform with nothing to transform is
/// neither.
///
/// Note that this reads no files and opens no container: it renders the *text*, and the style is
/// read when the pipeline is built. That is what lets `--print` answer without touching the input,
/// and it is why a style that does not parse is reported by the run rather than by the print.
#[context("Failed to build the reduction pipeline")]
fn build_pipeline(arguments: &Subcommand) -> Result<VPLPipeline> {
	let style = arguments
		.style
		.to_str()
		.ok_or_else(|| anyhow::anyhow!("the style path is not valid UTF-8: {:?}", arguments.style))?;

	Ok(VPLPipeline::new(vec![
		node("from_container", "filename", &arguments.input_file),
		node("vector_reduce_to_style", "style", style),
	]))
}

/// A VPL node with a single property.
fn node(name: &str, key: &str, value: &str) -> VPLNode {
	VPLNode {
		name: name.to_string(),
		properties: [(key.to_string(), vec![value.to_string()])].into_iter().collect(),
		sources: Vec::new(),
	}
}

// ───────────────────────── TESTS ─────────────────────────
#[cfg(test)]
mod tests {
	use pretty_assertions::assert_eq;

	use super::*;
	use crate::tests::run_command;

	fn testdata(name: &str) -> String {
		format!("{}/../testdata/{name}", env!("CARGO_MANIFEST_DIR"))
	}

	/// The pipeline `--print` would write, without going through stdout.
	fn pipeline_for(input: &str, style: &str) -> String {
		let arguments = Subcommand {
			input_file: input.to_string(),
			output_file: None,
			style: PathBuf::from(style),
			print: true,
		};
		build_pipeline(&arguments).unwrap().to_string()
	}

	#[test]
	fn the_pipeline_is_a_source_and_one_operation() {
		// The source node is part of the output so the text runs on its own — piped into
		// `convert`, or pasted into a VPL document.
		let text = pipeline_for(&testdata("berlin.versatiles"), &testdata("styles/colorful.json"));
		assert!(text.starts_with("from_container filename="), "got: {text}");
		assert!(text.contains("| vector_reduce_to_style style="), "got: {text}");
	}

	#[test]
	fn the_pipeline_passes_check() {
		// What `convert --dry-run` would say about it, asked directly: every operation resolves and
		// every parameter validates against the operation's own metadata.
		let text = pipeline_for(&testdata("berlin.versatiles"), &testdata("styles/colorful.json"));
		let pipeline = versatiles_pipeline::vpl::parse_vpl(&text).unwrap();
		let problems: Vec<String> = versatiles_pipeline::check_pipeline(&pipeline)
			.into_iter()
			.map(|p| p.message)
			.collect();
		assert_eq!(problems, Vec::<String>::new(), "in: {text}");
	}

	#[test]
	fn paths_needing_quotes_survive() {
		// Both paths go through the VPL serializer like any other value, so a space in one is the
		// serializer's problem rather than a broken pipeline.
		let text = pipeline_for("/tmp/my tiles.versatiles", "/tmp/my style.json");
		assert!(text.contains("filename='/tmp/my tiles.versatiles'"), "got: {text}");
		assert!(text.contains("style='/tmp/my style.json'"), "got: {text}");
		assert!(versatiles_pipeline::vpl::parse_vpl(&text).is_ok(), "got: {text}");
	}

	#[test]
	fn running_without_print_needs_an_output_file() {
		let err = run_command(vec![
			"versatiles",
			"reduce",
			"-s",
			&testdata("styles/colorful.json"),
			&testdata("berlin.versatiles"),
		])
		.unwrap_err();
		assert!(err.to_string().contains("output file is required"), "got: {err}");
	}

	#[test]
	fn printing_does_not_read_the_style() {
		// `--print` renders text and opens nothing, so a style that does not exist yet still prints
		// a pipeline. The run mode is where it is read, and where a bad one is reported.
		let text = pipeline_for(&testdata("berlin.versatiles"), "does-not-exist.json");
		assert!(text.contains("style=does-not-exist.json"), "got: {text}");
	}
}
