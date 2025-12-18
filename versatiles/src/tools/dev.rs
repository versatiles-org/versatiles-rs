use super::dev_tools::{export_outline, measure_tile_sizes, print_tilejson};
use anyhow::Result;
use versatiles_container::TilesRuntime;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_help_flag = true, disable_version_flag = true)]
pub struct Subcommand {
	#[command(subcommand)]
	sub_command: DevCommands,
}

#[derive(clap::Subcommand, Debug)]
enum DevCommands {
	MeasureTileSizes(measure_tile_sizes::MeasureTileSizes),
	ExportOutline(export_outline::ExportOutline),
	PrintTilejson(print_tilejson::PrintTilejson),
}

#[tokio::main]
pub async fn run(command: &Subcommand, runtime: TilesRuntime) -> Result<()> {
	match &command.sub_command {
		DevCommands::MeasureTileSizes(args) => measure_tile_sizes::run(args, runtime).await?,
		DevCommands::ExportOutline(args) => export_outline::run(args, runtime).await?,
		DevCommands::PrintTilejson(args) => print_tilejson::run(args, runtime).await?,
	};

	Ok(())
}
