use super::mosaic_tools::assemble;
#[cfg(feature = "gdal")]
use super::mosaic_tools::tile;
use anyhow::Result;
use versatiles_container::TilesRuntime;

#[derive(clap::Args, Debug)]
#[command(
	arg_required_else_help = true,
	disable_version_flag = true,
	about = "Tile and assemble image mosaics",
	long_about = "\
Tile and assemble image mosaics.

Two workflows for image-based tile pipelines:

* `tile` — turn a single georeferenced raster (e.g. GeoTIFF, JPEG2000) into a
  tile container, generating all overview levels with smart WebP compression.
  Requires GDAL.

* `assemble` — merge many pre-tiled containers into one mosaic. Containers
  listed earlier overlay those listed later (paint algorithm). Translucent
  tiles are composited in a two-pass pipeline; opaque tiles pass through
  unchanged to avoid recompression.

Typical pipeline: produce per-scene containers with `mosaic tile`, then
combine them into a seamless output with `mosaic assemble`."
)]
pub struct Subcommand {
	#[command(subcommand)]
	sub_command: MosaicCommands,
}

#[derive(clap::Subcommand, Debug)]
enum MosaicCommands {
	#[cfg(feature = "gdal")]
	Tile(tile::Tile),
	Assemble(assemble::Assemble),
}

#[tokio::main]
pub async fn run(command: &Subcommand, runtime: &TilesRuntime) -> Result<()> {
	match &command.sub_command {
		#[cfg(feature = "gdal")]
		MosaicCommands::Tile(args) => tile::run(args, runtime).await?,
		MosaicCommands::Assemble(args) => assemble::run(args, runtime).await?,
	}

	Ok(())
}
