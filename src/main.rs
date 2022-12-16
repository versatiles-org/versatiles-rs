mod container;

use clap::{arg, value_parser, Command};
use std::path::PathBuf;
use container::Tiles;

fn main() -> std::io::Result<()> {
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
		Some(("convert", sub_matches)) => {
			let filename_in = sub_matches.get_one::<PathBuf>("INPUT_FILE").unwrap();
			let filename_out = sub_matches.get_one::<PathBuf>("OUTPUT_FILE").unwrap();
			println!("convert from {:?} to {:?}", filename_in, filename_out);
			Tiles::convert(filename_in, filename_out)?;
			return Ok(());
		}
		_ => unreachable!("Exhausted list of subcommands and subcommand_required prevents `None`"),
	}
}
