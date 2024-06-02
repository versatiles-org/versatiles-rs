//! # VersaTiles CLI
//!
//! VersaTiles is a command-line tool for converting, probing, and serving map tiles in various formats.
//!
//! ## Subcommands
//! - **Convert**: Convert between different tile containers.
//! - **Probe**: Show information about a tile container.
//! - **Serve**: Serve tiles via HTTP.
//!
//! ## Usage
//! ```sh
//! versatiles [OPTIONS] <COMMAND>
//! ```
//!
//! ## Example
//! ```sh
//! # Convert tiles between different formats
//! versatiles convert --input input_file --output output_file
//!
//! # Probe information about a tile container
//! versatiles probe --file tile_file
//!
//! # Serve tiles via HTTP
//! versatiles serve --port 8080 --dir /path/to/tiles
//! ```

// Import necessary modules and dependencies
mod container;
mod tools;
mod utils;

pub use versatiles_core::*;
#[cfg(feature = "full")]
pub use versatiles_geometry as geometry;

use anyhow::Result;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{ErrorLevel, Verbosity};

/// Command-line interface for VersaTiles
#[derive(Parser, Debug)]
#[command(
	author, // Set the author
	version, // Set the version
	about, // Set a short description
	long_about = None, // Disable long description
	propagate_version = false, // Enable version flag for subcommands
	disable_help_subcommand = true, // Disable help subcommand
)]
struct Cli {
	#[command(subcommand)]
	command: Commands, // Set subcommands

	#[command(flatten)]
	verbose: Verbosity<ErrorLevel>, // Set verbosity flag
}

/// Define subcommands for the command-line interface
#[derive(Subcommand, Debug)]
enum Commands {
	#[clap(alias = "converter")]
	/// Convert between different tile containers
	Convert(tools::convert::Subcommand),

	/// Show information about a tile container
	Probe(tools::probe::Subcommand),

	#[clap(alias = "server")]
	/// Serve tiles via http
	Serve(tools::serve::Subcommand),

	/// Show detailed help
	Help(tools::help::Subcommand),
}

/// Main function for running the command-line interface
fn main() -> Result<()> {
	let cli = Cli::parse();

	// Initialize logger and set log level based on verbosity flag
	env_logger::Builder::new()
		.filter_level(cli.verbose.log_level_filter())
		.format_timestamp(None)
		.init();

	run(cli)
}

/// Helper function for running subcommands
fn run(cli: Cli) -> Result<()> {
	match &cli.command {
		Commands::Convert(arguments) => tools::convert::run(arguments),
		Commands::Help(arguments) => tools::help::run(arguments),
		Commands::Probe(arguments) => tools::probe::run(arguments),
		Commands::Serve(arguments) => tools::serve::run(arguments),
	}
}

/// Unit tests for the command-line interface
#[cfg(test)]
mod tests {
	use crate::{run, Cli};
	use anyhow::Result;
	use clap::Parser;

	/// Function for running command-line arguments in tests
	pub fn run_command(arg_vec: Vec<&str>) -> Result<String> {
		let cli = Cli::try_parse_from(arg_vec)?;
		let msg = format!("{:?}", cli);
		run(cli)?;
		Ok(msg)
	}

	/// Test if VersaTiles generates help
	#[test]
	fn help() {
		let err = run_command(vec!["versatiles"]).unwrap_err().to_string();
		assert!(err.starts_with(
			"A toolbox for converting, checking and serving map tiles in various formats."
		));
	}

	/// Test for version
	#[test]
	fn version() {
		let err = run_command(vec!["versatiles", "-V"])
			.unwrap_err()
			.to_string();
		assert!(err.starts_with("versatiles "));
	}

	/// Test for subcommand 'convert'
	#[test]
	fn convert_subcommand() {
		let output = run_command(vec!["versatiles", "convert"])
			.unwrap_err()
			.to_string();
		assert!(
			output.starts_with("Convert between different tile containers"),
			"{output}"
		);
	}

	/// Test for subcommand 'probe'
	#[test]
	fn probe_subcommand() {
		let output = run_command(vec!["versatiles", "probe"])
			.unwrap_err()
			.to_string();
		assert!(
			output.starts_with("Show information about a tile container"),
			"{output}"
		);
	}

	/// Test for subcommand 'serve'
	#[test]
	fn serve_subcommand() {
		let output = run_command(vec!["versatiles", "serve"])
			.unwrap_err()
			.to_string();
		assert!(output.starts_with("Serve tiles via http"), "{output}");
	}
}
