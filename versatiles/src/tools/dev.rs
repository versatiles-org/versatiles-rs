use super::dev_tools::{export_outline, measure_tile_sizes, print_tilejson};
use anyhow::Result;

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
pub async fn run(command: &Subcommand) -> Result<()> {
	match &command.sub_command {
		DevCommands::MeasureTileSizes(args) => measure_tile_sizes::run(args).await?,
		DevCommands::ExportOutline(args) => export_outline::run(args).await?,
		DevCommands::PrintTilejson(args) => print_tilejson::run(args).await?,
	};

	Ok(())
}
