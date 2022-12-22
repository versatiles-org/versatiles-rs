mod container;

use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use crate::container::*;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
	/// convert between different tile containers
	Convert(Convert),
}

#[derive(Args)]
pub struct Convert {
	/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
	input_file: PathBuf,

	/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
	output_file: PathBuf,

	/// minimal zoom level
	#[arg(long, value_name = "int")]
	min_zoom: Option<u64>,

	/// maximal zoom level
	#[arg(long, value_name = "int")]
	max_zoom: Option<u64>,

	/// precompress tiles
	#[arg(long, value_enum)]
	precompress: Option<container::TileCompression>,
}

fn main() -> std::io::Result<()> {
	let cli = Cli::parse();

	let command = &cli.command;
	match command {
		Commands::Convert(arguments) => {
			println!(
				"convert from {:?} to {:?}",
				arguments.input_file, arguments.output_file
			);
			Tools::convert(&arguments)?;
			return Ok(());
		}
	}
}
