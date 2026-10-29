use anyhow::Result;
use versatiles_container::TileSource;
use versatiles_core::ZoomLevel;
use versatiles_derive::context;

use super::encoding::{DemEncoding, resolve_encoding, to_tile_schema};
use crate::{PipelineFactory, operations::raster::raster_overscale::FillOverscale, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Creates the DEM tiles a source lacks by upscaling the tiles it has, as heights.
///
/// A tile the source has is passed on untouched. A tile it lacks is created
/// from the nearest tile it has at a lower zoom level: the area is cut out and
/// its heights are interpolated up to a full tile, then encoded without loss.
/// This is how a DEM reaches the zoom levels a map asks for when its data
/// ends lower, for a hillshade or contour lines that stay sharp at the edge
/// of the data instead of disappearing.
///
/// It is `raster_overscale` for elevation tiles, with the same rules for
/// where tiles are created: inside the source's bounds, up to `level_max`,
/// and never below the source's lowest level. Unlike `raster_overscale`, it
/// always scales heights, never colours, so a source without a DEM tile schema
/// needs `encoding` rather than being scaled as an image; the result carries
/// the tile schema of that encoding.
///
/// A following `raster_format` must keep `quality=100`, since a lossy encoder
/// changes the heights.
struct Args {
	/// Highest zoom level to create tiles at. Defaults to the source's highest.
	level_max: Option<ZoomLevel>,
	/// DEM encoding of the source. Defaults to the encoding its tile schema implies.
	encoding: Option<DemEncoding>,
}

struct Operation;

impl Operation {
	#[context("Building dem_overscale operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, factory: &PipelineFactory) -> Result<FillOverscale> {
		let args = Args::from_vpl_node(&vpl_node)?;
		let encoding = resolve_encoding(args.encoding, source.tilejson().tile_schema.as_ref())?;
		FillOverscale::for_elevation(
			source,
			args.level_max.map(u8::from),
			to_tile_schema(encoding),
			factory.runtime(),
		)
		.await
	}
}

crate::operations::macros::define_transform_factory!("dem_overscale", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use versatiles_container::Tile;
	use versatiles_core::{TileCompression, TileCoord, TileFormat, TileSchema};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

	use super::*;
	use crate::factory::OperationFactoryTrait;

	/// Elevation tiles up to level 3, as the source for every test.
	const DEM: &str = "from_debug format=webp | meta_update schema=\"dem/mapbox\" | filter level_max=3";

	async fn created_tile(vpl: &str) -> Result<Tile> {
		let op = PipelineFactory::new_dummy().operation_from_vpl(vpl).await?;
		let bbox = TileCoord::new(5, 9, 9)?.to_tile_bbox();
		let mut tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "{vpl}");
		Ok(tiles.remove(0).1)
	}

	#[test]
	fn factory_docs_name_the_parameters() {
		let docs = Factory {}.docs();
		for name in ["level_max", "encoding"] {
			assert!(docs.contains(name), "{name} missing from {docs}");
		}
	}

	/// The same tiles `raster_overscale` creates for a source it recognises as
	/// elevation, which is the code both run.
	#[tokio::test]
	async fn it_creates_the_tiles_raster_overscale_creates_for_heights() -> Result<()> {
		let dem = created_tile(&format!("{DEM} | dem_overscale level_max=5")).await?;
		let raster = created_tile(&format!("{DEM} | raster_overscale level_max=5")).await?;
		assert_eq!(dem.into_image()?.to_rgb8(), raster.into_image()?.to_rgb8());
		Ok(())
	}

	/// Encoded without loss: the heights that come back are the ones that
	/// went in.
	#[tokio::test]
	async fn the_heights_survive_encoding() -> Result<()> {
		let tile = created_tile(&format!("{DEM} | dem_overscale level_max=5")).await?;
		let image = tile.clone().into_image()?;
		let blob = tile.into_blob(&TileCompression::Uncompressed)?;
		let decoded = DynamicImage::from_blob(&blob, TileFormat::WEBP)?;
		assert!(decoded.to_rgb8() == image.to_rgb8(), "heights changed");
		Ok(())
	}

	/// With `encoding`, a source that declares no DEM schema is still scaled
	/// as heights, where `raster_overscale` would scale it as colours, and the
	/// result says what it holds.
	#[tokio::test]
	async fn encoding_makes_a_source_without_a_schema_heights() -> Result<()> {
		let plain = "from_debug format=webp | filter level_max=3";
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!("{plain} | dem_overscale level_max=5 encoding=terrarium"))
			.await?;
		assert_eq!(op.tilejson().tile_schema, Some(TileSchema::RasterDEMTerrarium));

		let heights = created_tile(&format!("{plain} | dem_overscale level_max=5 encoding=terrarium")).await?;
		let colors = created_tile(&format!("{plain} | raster_overscale level_max=5")).await?;
		assert!(
			heights.into_image()?.to_rgb8() != colors.into_image()?.to_rgb8(),
			"scaled as colours"
		);
		Ok(())
	}

	#[tokio::test]
	async fn a_source_that_is_not_mapbox_or_terrarium_is_refused() {
		let factory = PipelineFactory::new_dummy();
		for vpl in [
			"from_debug format=png | dem_overscale",
			"from_debug format=png | meta_update schema=\"dem/versatiles\" | dem_overscale",
		] {
			let error = factory.operation_from_vpl(vpl).await.unwrap_err();
			assert!(format!("{error:#}").contains("encoding"), "{vpl}: {error:#}");
		}
	}

	#[tokio::test]
	async fn it_reports_itself_under_its_own_name() -> Result<()> {
		let op = PipelineFactory::new_dummy()
			.operation_from_vpl(&format!("{DEM} | dem_overscale level_max=5"))
			.await?;
		assert!(format!("{:?}", op.source_type()).contains("dem_overscale"));
		Ok(())
	}
}
