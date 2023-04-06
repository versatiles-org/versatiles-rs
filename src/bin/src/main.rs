// Import necessary modules and dependencies
pub mod tools;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};

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
pub struct Cli {
	#[command(subcommand)]
	command: Commands, // Set subcommands

	#[command(flatten)]
	verbose: Verbosity<InfoLevel>, // Set verbosity flag
}

// Define subcommands for the command-line interface
#[derive(Subcommand, Debug)]
pub enum Commands {
	/// Convert between different tile containers
	Convert(tools::convert::Subcommand),

	/// Show information about a tile container
	Probe(tools::probe::Subcommand),

	/// Serve tiles via http
	Serve(tools::serve::Subcommand),
}

// Main function for running the command-line interface
fn main() {
	let cli = Cli::parse();

	// Initialize logger and set log level based on verbosity flag
	env_logger::Builder::new()
		.filter_level(cli.verbose.log_level_filter())
		.init();

	run(cli);
}

// Helper function for running subcommands
fn run(cli: Cli) {
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
				run(cli);
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
}
