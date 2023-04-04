pub mod tools;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};

#[derive(Parser, Debug)]
#[command(
	author,
	version,
	about,
	long_about = None,
	propagate_version = true,
	disable_help_subcommand = true,
	disable_help_flag = true,
)]
pub struct Cli {
	#[command(subcommand)]
	command: Commands,

	#[command(flatten)]
	verbose: Verbosity<InfoLevel>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
	// /// Compare two tile containers
	// Compare(tools::compare::Subcommand),
	/// Convert between different tile containers
	Convert(tools::convert::Subcommand),

	/// Show information about a tile container
	Probe(tools::probe::Subcommand),

	/// Serve tiles via http
	Serve(tools::serve::Subcommand),
}

fn main() {
	let cli = Cli::parse();

	env_logger::Builder::new()
		.filter_level(cli.verbose.log_level_filter())
		.init();

	run(cli);
}

fn run(cli: Cli) {
	match &cli.command {
		//Commands::Compare(arguments) => tools::compare::run(arguments),
		Commands::Convert(arguments) => tools::convert::run(arguments),
		Commands::Probe(arguments) => tools::probe::run(arguments),
		Commands::Serve(arguments) => tools::serve::run(arguments),
	}
}

#[cfg(test)]
mod tests {
	use crate::{run, Cli};
	use clap::Parser;
	use core::result::Result;

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

	#[test]
	fn help() {
		let err = run_command(vec!["versatiles"]).unwrap_err();
		assert!(err.starts_with("A toolbox for converting, checking and serving map tiles in various formats."));
		assert!(err.contains("\nUsage: versatiles [OPTIONS] <COMMAND>"));
	}

	#[test]
	fn version() {
		let err = run_command(vec!["versatiles", "-V"]).unwrap_err();
		assert!(err.starts_with("versatiles "));
	}
}
