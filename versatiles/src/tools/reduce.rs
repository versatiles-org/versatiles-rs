//! `versatiles reduce` — strip a tileset down to what one style draws.
//!
//! Takes the data requirement `@versatiles/style` exports, renders it into a VPL pipeline, and
//! either prints that pipeline or runs it.
//!
//! Printing is not a debugging affordance. Studio's design note is explicit that the reduction
//! should land in the VPL document someone is editing "rather than running it opaquely", so the
//! text is the product and running it is the convenience. `--print` also composes with what
//! already exists, since inline VPL is a first-class input:
//!
//! ```text
//! versatiles reduce --print -r style.req.json in.versatiles | versatiles convert "[,vpl]-" out.versatiles
//! versatiles reduce --print -r style.req.json in.versatiles | versatiles convert --dry-run "[,vpl]-" x
//! ```

use std::{fs, path::PathBuf};

use anyhow::{Result, bail};
use versatiles_container::{TileSource, TilesConverterParameters, TilesRuntime, convert_tiles_container_to_str};
use versatiles_derive::context;
use versatiles_pipeline::{
	PipelineReader, VPLNode, VPLPipeline,
	reduce::{Requirement, operations},
};

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

	/// JSON file describing what the style needs, as exported by @versatiles/style.
	#[arg(long, short = 'r', value_name = "file", verbatim_doc_comment)]
	requirements: PathBuf,

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

/// Reads the requirement and renders the whole pipeline, source included.
///
/// The source node is part of the output because the text has to be runnable on its own — piped
/// into `convert`, or pasted into a VPL document — and a list of transforms with nothing to
/// transform is neither.
#[context("Failed to build the reduction pipeline")]
fn build_pipeline(arguments: &Subcommand) -> Result<VPLPipeline> {
	let json = fs::read_to_string(&arguments.requirements)?;
	let requirement = Requirement::from_json(&json)?;

	let mut nodes = vec![source_node(&arguments.input_file)];
	nodes.extend(operations(&requirement));

	Ok(VPLPipeline::new(nodes))
}

/// The `from_container` node that reads the input.
fn source_node(filename: &str) -> VPLNode {
	VPLNode {
		name: "from_container".to_string(),
		properties: [("filename".to_string(), vec![filename.to_string()])]
			.into_iter()
			.collect(),
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
	fn pipeline_for(requirement: &str) -> String {
		let arguments = Subcommand {
			input_file: testdata("berlin.versatiles"),
			output_file: None,
			requirements: PathBuf::from(testdata(requirement)),
			print: true,
		};
		build_pipeline(&arguments).unwrap().to_string()
	}

	#[test]
	fn the_pipeline_starts_with_the_source() {
		// The source node is part of the output so the text runs on its own — piped into
		// `convert`, or pasted into a VPL document.
		let text = pipeline_for("reduce/streets.json");
		assert!(text.starts_with("from_container filename="), "got: {text}");
		assert!(text.contains("vector_filter_layers"), "got: {text}");
		assert!(text.contains("vector_filter_properties"), "got: {text}");
	}

	#[test]
	fn the_pipeline_passes_check() {
		// What `convert --dry-run` would say about it, asked directly: every operation resolves,
		// every parameter validates against the operation's own metadata, and the CEL compiles.
		for requirement in ["reduce/minimal.json", "reduce/streets.json", "reduce/operators.json"] {
			let text = pipeline_for(requirement);
			let pipeline = versatiles_pipeline::vpl::parse_vpl(&text).unwrap();
			let problems: Vec<String> = versatiles_pipeline::check_pipeline(&pipeline)
				.into_iter()
				.map(|p| p.message)
				.collect();
			assert_eq!(problems, Vec::<String>::new(), "in: {text}");
		}
	}

	#[test]
	fn an_input_path_needing_quotes_survives() {
		// The path goes through the VPL serializer like any other value, so a space in it is the
		// serializer's problem rather than a broken pipeline.
		let arguments = Subcommand {
			input_file: "/tmp/my tiles.versatiles".to_string(),
			output_file: None,
			requirements: PathBuf::from(testdata("reduce/minimal.json")),
			print: true,
		};
		let text = build_pipeline(&arguments).unwrap().to_string();
		assert!(text.contains("filename='/tmp/my tiles.versatiles'"), "got: {text}");
		assert!(versatiles_pipeline::vpl::parse_vpl(&text).is_ok(), "got: {text}");
	}

	#[test]
	fn running_without_print_needs_an_output_file() {
		let err = run_command(vec![
			"versatiles",
			"reduce",
			"-r",
			&testdata("reduce/minimal.json"),
			&testdata("berlin.versatiles"),
		])
		.unwrap_err();
		assert!(err.to_string().contains("output file is required"), "got: {err}");
	}

	#[test]
	fn a_missing_requirement_file_is_reported() {
		let err = run_command(vec![
			"versatiles",
			"reduce",
			"--print",
			"-r",
			&testdata("reduce/does-not-exist.json"),
			&testdata("berlin.versatiles"),
		])
		.unwrap_err();
		assert!(
			err.chain().any(|e| e.to_string().contains("reduction pipeline")),
			"got: {err:?}"
		);
	}
}
