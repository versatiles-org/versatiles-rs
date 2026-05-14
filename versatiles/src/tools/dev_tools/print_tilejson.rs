use anyhow::Result;
use std::io::Write;
use versatiles_container::TilesRuntime;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
/// Print the TileJSON metadata of a container to stdout.
pub struct PrintTilejson {
	/// Tile container to read (path, URL, or data source expression).
	/// Run `versatiles help source` for syntax details.
	#[arg(value_name = "INPUT_FILE", verbatim_doc_comment)]
	input: String,

	/// Pretty-print the output
	#[arg(long, default_value_t = false, short = 'p')]
	pretty: bool,
}

pub async fn run(args: &PrintTilejson, runtime: &TilesRuntime) -> Result<()> {
	let tilejson = fetch_tilejson(args, runtime).await?;
	std::io::stdout().write_all(tilejson.as_bytes())?;
	Ok(())
}

async fn fetch_tilejson(args: &PrintTilejson, runtime: &TilesRuntime) -> Result<String> {
	let reader = runtime.reader_from_str(&args.input).await?;

	Ok(if args.pretty {
		reader.tilejson().to_pretty_lines(80).join("\n")
	} else {
		reader.tilejson().stringify()
	})
}

#[cfg(test)]
mod tests {
	use super::*;
	use versatiles::runtime::create_test_runtime;

	#[tokio::test]
	async fn test_print_tilejson() {
		let runtime = create_test_runtime();
		let output = fetch_tilejson(
			&PrintTilejson {
				input: "../testdata/berlin.mbtiles".into(),
				pretty: false,
			},
			&runtime,
		)
		.await
		.unwrap();
		// Compact form: single-line JSON, no inserted whitespace between
		// keys/values.
		assert!(output.starts_with("{\""), "expected compact JSON, got: {output}");
		assert!(!output.contains('\n'), "compact form should not contain newlines");
		assert!(output.contains("\"bounds\":[13.3,52.45,13.46,52.55]"));
		assert!(output.contains("\"vector_layers\":["));
		assert!(output.contains("\"tilejson\":\"3.0.0\""));
		// Spot-check at least one known shortbread layer survives the round-trip.
		assert!(output.contains("\"id\":\"place_labels\""));
	}

	#[tokio::test]
	async fn test_pretty_print_tilejson() {
		let runtime = create_test_runtime();
		let output = fetch_tilejson(
			&PrintTilejson {
				input: "../testdata/berlin.mbtiles".into(),
				pretty: true,
			},
			&runtime,
		)
		.await
		.unwrap();
		// Pretty form: newlines + 2-space indentation.
		assert!(output.starts_with("{\n  \""), "expected pretty JSON, got: {output}");
		assert!(output.contains("\n  \"vector_layers\": ["));
		assert!(output.contains("\n  \"tilejson\": \"3.0.0\""));
		assert!(output.contains("\"id\": \"place_labels\""));
	}
}
