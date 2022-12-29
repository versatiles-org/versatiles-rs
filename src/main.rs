mod opencloudtiles;

use clap::{Args, Parser, Subcommand};
use opencloudtiles::*;
use std::path::PathBuf;

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

	/// serve tiles via http
	Serve(Serve),
}

#[derive(Args)]
pub struct Convert {
	/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
	input_file: String,

	/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
	output_file: String,

	/// minimal zoom level
	#[arg(long, value_name = "int")]
	min_zoom: Option<u64>,

	/// maximal zoom level
	#[arg(long, value_name = "int")]
	max_zoom: Option<u64>,

	/// bounding box: lon_min lat_min lon_max lat_max
	#[arg(long, value_name = "float", num_args = 4, value_delimiter = ',')]
	bbox: Option<Vec<f32>>,

	/// set new tile format
	#[arg(long, value_enum)]
	tile_format: Option<opencloudtiles::types::TileFormat>,

	/// force to recompress tiles
	#[arg(long, value_enum)]
	recompress: bool,
}

#[derive(Args)]
pub struct Serve {
	/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
	#[arg(value_delimiter = ' ')]
	sources: Vec<String>,

	/// serve via port (default: 8080)
	#[arg(long, default_value = "8080")]
	port: u16,
}

fn main() {
	let cli = Cli::parse();

	let command = &cli.command;
	match command {
		Commands::Convert(arguments) => {
			println!(
				"convert from {:?} to {:?}",
				arguments.input_file, arguments.output_file
			);
			Tools::convert(&arguments);
		}
		Commands::Serve(arguments) => {
			println!(
				"serve from {:?} to http://localhost:{:?}/",
				arguments.sources, arguments.port
			);
			Tools::serve(&arguments);
		}
	}
}
