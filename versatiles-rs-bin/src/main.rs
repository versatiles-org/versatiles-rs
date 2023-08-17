mod tools;

use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use shared::Result;

// Define the command-line interface using the clap crate
#[derive(Parser, Debug)]
#[command(
	author, // Set the author
	version, // Set the version
	about, // Set a short description
	long_about = None, // Disable long description
	propagate_version = true, // Enable version flag for subcommands
	disable_help_subcommand = true, // Disable help subcommand
	disable_help_flag = true, // Disable help flag
)]
struct Cli {
	#[command(subcommand)]
	command: Commands, // Set subcommands

	#[command(flatten)]
	verbose: Verbosity<InfoLevel>, // Set verbosity flag
}

// Define subcommands for the command-line interface
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
}

// Main function for running the command-line interface
fn main() -> Result<()> {
	let cli = Cli::parse();

	// Initialize logger and set log level based on verbosity flag
	env_logger::Builder::new()
		.filter_level(cli.verbose.log_level_filter())
		.init();

	run(cli)
}

// Helper function for running subcommands
fn run(cli: Cli) -> Result<()> {
	match &cli.command {
		Commands::Convert(arguments) => tools::convert::run(arguments),
		Commands::Probe(arguments) => tools::probe::run(arguments),
		Commands::Serve(arguments) => tools::serve::run(arguments),
	}
}

// Unit tests for the command-line interface
#[cfg(test)]
mod tests {
	use crate::{run, Cli};
	use clap::Parser;
	use core::result::Result;

	// Function for running command-line arguments in tests
	pub fn run_command(arg_vec: Vec<&str>) -> Result<String, String> {
		match Cli::try_parse_from(arg_vec) {
			Ok(cli) => {
				let msg = format!("{:?}", cli);
				run(cli).unwrap();
				Ok(msg)
			}
			Err(error) => Err(error.render().to_string()),
		}
	}

	// Test if versatiles generates help
	#[test]
	fn help() {
		let err = run_command(vec!["versatiles"]).unwrap_err();
		assert!(err.starts_with("A toolbox for converting, checking and serving map tiles in various formats."));
		assert!(err.contains("\nUsage: versatiles [OPTIONS] <COMMAND>"));
	}

	// Test for version
	#[test]
	fn version() {
		let err = run_command(vec!["versatiles", "-V"]).unwrap_err();
		assert!(err.starts_with("versatiles "));
	}

	// Test for subcommand 'convert'
	#[test]
	fn convert_subcommand() {
		let output = run_command(vec!["versatiles", "convert"]).unwrap_err();
		assert!(output.starts_with("Convert between different tile containers"));
	}

	// Test for subcommand 'probe'
	#[test]
	fn probe_subcommand() {
		let output = run_command(vec!["versatiles", "probe"]).unwrap_err();
		assert!(output.starts_with("Show information about a tile container"));
	}

	// Test for subcommand 'serve'
	#[test]
	fn serve_subcommand() {
		let output = run_command(vec!["versatiles", "serve"]).unwrap_err();
		assert!(output.starts_with("Serve tiles via http"));
	} // Add required imports at the top of the test module
}
