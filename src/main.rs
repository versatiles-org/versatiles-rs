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
	/*
	let cmd = Command::new("cargo")
		.bin_name("cloudtiles")
		.subcommand_required(true)
		.subcommand(
			Command::new("convert")
				.about("convert between different tile containers")
				.arg(
					arg!([INPUT_FILE])
						.required(true)
						.value_parser(value_parser!(PathBuf)),
				)
				.arg(
					arg!([OUTPUT_FILE])
						.required(true)
						.value_parser(value_parser!(PathBuf)),
				),
		);
	let matches = cmd.get_matches();
	match matches.subcommand() {
		Some(("convert", sub_matches)) => {}
		_ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
	}
	*/
}
