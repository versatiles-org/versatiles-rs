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
mod tools;

use anyhow::Result;
use clap::{Parser, Subcommand};
use log::LevelFilter;

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

	#[arg(
		long,
		short = 'q',
		action = clap::ArgAction::Count,
		global = true,
		help = "Decrease logging verbosity",
		long_help = "Decrease the logging verbosity level.",
		conflicts_with = "verbose",
		display_order = 100,
	)]
	quiet: u8,

	#[arg(
		long,
		short = 'v',
		action = clap::ArgAction::Count,
		global = true,
		help = "Increase logging verbosity\n(add more 'v' for greater detail, e.g., '-vvvv' for trace-level logs).",
		long_help = "Increase the logging verbosity level. Each 'v' increases the log level by one step:\n\
			- `-v` enables warnings\n\
			- `-vv` enables informational logs\n\
			- `-vvv` enables debugging logs\n\
			- `-vvvv` enables trace logs, which give the most detailed information about the tool's execution.",
		display_order = 100,
	)]
	verbose: u8,
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
	/// Serve tiles via HTTP
	Serve(tools::serve::Subcommand),

	/// Show detailed help
	Help(tools::help::Subcommand),

	#[clap(hide = true)]
	/// Internal command for development
	Dev(tools::dev::Subcommand),
}

/// Main function for running the command-line interface
fn main() -> Result<()> {
	let cli = Cli::parse();

	// Initialize logger and set log level based on verbosity flag
	let verbosity = cli.verbose as i16 - cli.quiet as i16;
	let log_level = match verbosity {
		i16::MIN..=-1 => LevelFilter::Off,
		0 => LevelFilter::Warn,
		1 => LevelFilter::Info,
		2 => LevelFilter::Debug,
		3..=i16::MAX => LevelFilter::Trace,
	};

	env_logger::Builder::new()
		.filter_level(log_level)
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
		Commands::Dev(arguments) => tools::dev::run(arguments),
	}
}

/// Unit tests for the command-line interface
#[cfg(test)]
mod tests {
	use crate::{Cli, run};
	use anyhow::Result;
	use clap::Parser;

	/// Function for running command-line arguments in tests
	pub fn run_command(arg_vec: Vec<&str>) -> Result<String> {
		let cli = Cli::try_parse_from(arg_vec)?;
		let msg = format!("{cli:?}");
		run(cli)?;
		Ok(msg)
	}

	/// Test if VersaTiles generates help
	#[test]
	fn help() {
		let err = run_command(vec!["versatiles"]).unwrap_err().to_string();
		assert!(err.starts_with("A toolbox for converting, checking and serving map tiles in various formats."));
	}

	/// Test for version
	#[test]
	fn version() {
		let err = run_command(vec!["versatiles", "-V"]).unwrap_err().to_string();
		assert!(err.starts_with("versatiles "));
	}

	/// Test for subcommand 'convert'
	#[test]
	fn convert_subcommand() {
		let output = run_command(vec!["versatiles", "convert"]).unwrap_err().to_string();
		assert!(
			output.starts_with("Convert between different tile containers"),
			"{output}"
		);
	}

	/// Test for subcommand 'probe'
	#[test]
	fn probe_subcommand() {
		let output = run_command(vec!["versatiles", "probe"]).unwrap_err().to_string();
		assert!(
			output.starts_with("Show information about a tile container"),
			"{output}"
		);
	}

	/// Test for subcommand 'serve'
	#[test]
	fn serve_subcommand() {
		let output = run_command(vec!["versatiles", "serve"]).unwrap_err().to_string();
		assert!(output.starts_with("Serve tiles via HTTP"), "{output}");
	}
}
