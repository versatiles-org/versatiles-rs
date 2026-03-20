use super::raster_tools::{convert, merge};
use anyhow::Result;
use versatiles_container::TilesRuntime;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	#[command(subcommand)]
	sub_command: RasterCommands,
}

#[derive(clap::Subcommand, Debug)]
enum RasterCommands {
	Convert(convert::Convert),
	Merge(merge::Merge),
}

#[tokio::main]
pub async fn run(command: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	match &command.sub_command {
		RasterCommands::Convert(args) => convert::run(args, runtime).await?,
		RasterCommands::Merge(args) => merge::run(args, runtime).await?,
	}

	Ok(())
}
