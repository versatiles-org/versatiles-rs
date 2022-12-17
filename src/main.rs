mod container;

use clap::{Args, Parser, Subcommand};
use container::Tiles;
use std::path::PathBuf;

#[derive(Parser)]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
pub struct Cli {
	/// minimal zoom level
	#[arg(long, value_name = "int")]
	min_zoom: Option<u64>,

	/// maximal zoom level
	#[arg(long, value_name = "int")]
	max_zoom: Option<u64>,

	/// precompress tiles
	#[arg(long, value_enum)]
	precompression: Option<container::container::TileCompression>,

	#[command(subcommand)]
	command: Commands,
}

#[derive(Subcommand)]
enum Commands {
	/// convert between different tile containers
	Convert {
		/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
		input_file: PathBuf,

		/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
		output_file: PathBuf,
	},
}

#[derive(Args)]
struct Convert {
	name: Option<String>,
}

fn main() -> std::io::Result<()> {
	let cli = Cli::parse();

	let command = &cli.command;
	match command {
		Commands::Convert {
			input_file,
			output_file,
		} => {
			println!("convert from {:?} to {:?}", input_file, output_file);
			Tiles::convert(input_file, output_file, &cli)?;
			return Ok(());
		}
	}
}
