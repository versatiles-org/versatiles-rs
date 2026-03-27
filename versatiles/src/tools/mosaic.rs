use super::mosaic_tools::assemble;
use super::mosaic_tools::assemble2;
#[cfg(feature = "gdal")]
use super::mosaic_tools::tile;
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
	#[cfg(feature = "gdal")]
	Tile(tile::Tile),
	Assemble(assemble::Assemble),
	Assemble2(assemble2::Assemble2),
}

#[tokio::main]
pub async fn run(command: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	match &command.sub_command {
		#[cfg(feature = "gdal")]
		MosaicCommands::Tile(args) => tile::run(args, runtime).await?,
		MosaicCommands::Assemble(args) => assemble::run(args, runtime).await?,
		MosaicCommands::Assemble2(args) => assemble2::run(args, runtime).await?,
	}

	Ok(())
}
