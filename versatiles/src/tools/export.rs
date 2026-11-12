use anyhow::Result;
use versatiles_container::TilesRuntime;

use super::export_tools::tile;

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	about = "Export tiles as ordinary files",
	long_about = "\
Export tiles as ordinary files.

`convert` keeps data as tiles, including a geographic subset with --bbox.
`export` takes data out of the tile container: a tile as a file of its own.

The format of an output file follows its extension."
)]
pub struct Subcommand {
	#[command(subcommand)]
	sub_command: ExportCommands,
}

#[derive(clap::Subcommand, Debug)]
enum ExportCommands {
	Tile(tile::Tile),
}

#[tokio::main]
pub async fn run(command: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	match &command.sub_command {
		ExportCommands::Tile(args) => tile::run(args, runtime).await,
	}
}
