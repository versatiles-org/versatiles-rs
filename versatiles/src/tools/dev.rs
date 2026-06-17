use super::dev_tools::{count_tiles, export_outline, measure_tile_sizes, print_tilejson, shortbread, vector_layers};
use anyhow::Result;
use versatiles_container::TilesRuntime;

#[derive(clap::Args, Debug)]
#[command(arg_required_else_help = true, disable_version_flag = true)]
pub struct Subcommand {
	#[command(subcommand)]
	sub_command: DevCommands,
}

#[derive(clap::Subcommand, Debug)]
enum DevCommands {
	CountTiles(count_tiles::CountTiles),
	MeasureTileSizes(measure_tile_sizes::MeasureTileSizes),
	ExportOutline(export_outline::ExportOutline),
	PrintTilejson(print_tilejson::PrintTilejson),
	VectorLayers(vector_layers::VectorLayersTool),
	CheckShortbread(shortbread::CheckShortbread),
}

#[tokio::main]
pub async fn run(command: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	match &command.sub_command {
		DevCommands::CountTiles(args) => count_tiles::run(args, runtime).await?,
		DevCommands::MeasureTileSizes(args) => measure_tile_sizes::run(args, runtime).await?,
		DevCommands::ExportOutline(args) => export_outline::run(args, runtime).await?,
		DevCommands::PrintTilejson(args) => print_tilejson::run(args, runtime).await?,
		DevCommands::VectorLayers(args) => vector_layers::run(args, runtime).await?,
		DevCommands::CheckShortbread(args) => shortbread::run(args, runtime).await?,
	}

	Ok(())
}
