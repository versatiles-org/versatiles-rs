use super::mosaic_tools::{assemble, tile};
use anyhow::Result;
use versatiles_container::TilesRuntime;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	#[command(subcommand)]
	sub_command: MosaicCommands,
}

#[derive(clap::Subcommand, Debug)]
enum MosaicCommands {
	Tile(tile::Tile),
	Assemble(assemble::Assemble),
}

#[tokio::main]
pub async fn run(command: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	match &command.sub_command {
		MosaicCommands::Tile(args) => tile::run(args, runtime).await?,
		MosaicCommands::Assemble(args) => assemble::run(args, runtime).await?,
	}

	Ok(())
}
