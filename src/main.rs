#![feature(test)]

mod opencloudtiles;
use clap::{Parser, Subcommand};
use clap_verbosity_flag::{InfoLevel, Verbosity};
use opencloudtiles::tools;

#[derive(Parser)]
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

#[derive(Subcommand)]
pub enum Commands {
	/// Compare two tile containers
	Compare(tools::compare::Subcommand),

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

	let command = &cli.command;
	match command {
		Commands::Compare(arguments) => tools::compare::run(arguments),
		Commands::Convert(arguments) => tools::convert::run(arguments),
		Commands::Probe(arguments) => tools::probe::run(arguments),
		Commands::Serve(arguments) => tools::serve::run(arguments),
	}
}
