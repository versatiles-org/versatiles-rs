mod opencloudtiles;

use clap::{Args, Parser, Subcommand};
use opencloudtiles::{
	tools,
	types::{TileFormat, TilePrecompression},
};

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

	/// serve tiles via http
	Probe(Probe),

	/// serve tiles via http
	Compare(Compare),
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
	tile_format: Option<TileFormat>,

	/// set tile precompression
	#[arg(long, value_enum)]
	precompress: Option<TilePrecompression>,

	/// force to recompress tiles
	#[arg(long, value_enum)]
	recompress: bool,
}

#[derive(Args)]
pub struct Serve {
	/// (e.g. *.mbtiles, *.cloudtiles, *.tar)
	#[arg(num_args = 1.., required = true)]
	sources: Vec<String>,

	/// serve via port (default: 8080)
	#[arg(long, default_value = "8080")]
	port: u16,
}

#[derive(Args)]
pub struct Probe {
	file: String,
}

#[derive(Args)]
pub struct Compare {
	file1: String,
	file2: String,
}

fn main() {
	let cli = Cli::parse();

	let command = &cli.command;
	match command {
		Commands::Convert(arguments) => {
			tools::convert(&arguments);
		}
		Commands::Serve(arguments) => {
			tools::serve(&arguments);
		}
		Commands::Probe(arguments) => {
			tools::probe(&arguments);
		}
		Commands::Compare(arguments) => {
			tools::compare(&arguments);
		}
	}
}
