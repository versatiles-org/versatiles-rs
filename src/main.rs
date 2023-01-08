mod opencloudtiles;

use clap::{Args, Parser, Subcommand};
use opencloudtiles::{
	lib::{Precompression, TileFormat},
	tools,
};

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
}

#[derive(Subcommand)]
pub enum Commands {
	/// Convert between different tile containers
	Convert(Convert),

	/// Serve tiles via http
	Serve(Serve),

	/// Show information about a tile container
	Probe(Probe),

	/// Compare two tile containers
	Compare(Compare),
}

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Convert {
	/// supported container formats: *.cloudtiles, *.tar, *.mbtiles
	#[arg()]
	input_file: String,

	/// supported container formats: *.cloudtiles, *.tar
	#[arg()]
	output_file: String,

	/// minimum zoom level
	#[arg(long, value_name = "int")]
	min_zoom: Option<u64>,

	/// maximum zoom level
	#[arg(long, value_name = "int")]
	max_zoom: Option<u64>,

	/// bounding box: lon_min lat_min lon_max lat_max
	#[arg(long, short, value_name = "float", num_args = 4, value_delimiter = ',')]
	bbox: Option<Vec<f32>>,

	/// flip input vertically
	#[arg(long)]
	flip_input: bool,

	/// convert tiles to new format
	#[arg(long, short, value_enum)]
	tile_format: Option<TileFormat>,

	/// set new precompression
	#[arg(long, short, value_enum)]
	precompress: Option<Precompression>,

	/// force recompression, e.g. to improve an existing gzip compression.
	#[arg(long, short, value_enum)]
	force_recompression: bool,
}

#[derive(Args)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	verbatim_doc_comment
)]
pub struct Serve {
	/// one or more tile containers you want to serve
	/// supported container formats are: *.cloudtiles, *.tar, *.mbtiles
	/// the url will be generated automatically:
	///    e.g. "ukraine.cloudtiles" will be served at url "/tiles/ukraine/..."
	/// you can add a name by using a "#":
	///    e.g. "overlay.tar#iran-revolution" will serve "overlay.tar" at url "/tiles/iran-revolution/..."
	#[arg(num_args = 1.., required = true, verbatim_doc_comment)]
	sources: Vec<String>,

	/// serve via port
	#[arg(short, long, default_value = "8080")]
	port: u16,

	/// serve static content at "/static/..." from folder
	#[arg(
		short = 's',
		long,
		conflicts_with = "static_tar",
		value_name = "folder"
	)]
	static_folder: Option<String>,

	/// serve static content at "/static/..." from tar file
	#[arg(
		short = 't',
		long,
		conflicts_with = "static_folder",
		value_name = "file"
	)]
	static_tar: Option<String>,
}

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Probe {
	/// tile container you want to probe
	/// supported container formats are: *.cloudtiles, *.tar, *.mbtiles
	#[arg(required = true, verbatim_doc_comment)]
	file: String,
}

#[derive(Args)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Compare {
	file1: String,
	file2: String,
}

fn main() {
	let cli = Cli::parse();

	let command = &cli.command;
	match command {
		Commands::Convert(arguments) => {
			tools::convert(arguments);
		}
		Commands::Serve(arguments) => {
			tools::serve(arguments);
		}
		Commands::Probe(arguments) => {
			tools::probe(arguments);
		}
		Commands::Compare(arguments) => {
			tools::compare(arguments);
		}
	}
}
