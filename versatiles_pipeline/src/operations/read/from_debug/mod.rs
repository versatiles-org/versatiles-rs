mod image;
mod vector;

use crate::{
	helpers::{pack_image_tile, pack_image_tile_stream, pack_vector_tile, pack_vector_tile_stream},
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::VPLNode,
	PipelineFactory,
};
use anyhow::{bail, Result};
use async_trait::async_trait;
use futures::future::BoxFuture;
use image::create_debug_image;
use imageproc::image::DynamicImage;
use std::fmt::Debug;
use vector::create_debug_vector_tile;
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Produces debugging tiles, each showing their coordinates as text.
struct Args {
	/// tile format: "mvt", "avif", "jpg", "png" or "webp"
	format: String,
}

#[derive(Debug)]
pub struct Operation {
	tilejson: TileJSON,
	parameters: TilesReaderParameters,
}

impl Operation {
	pub fn from_parameters(tile_format: TileFormat) -> Result<Box<dyn OperationTrait>> {
		let parameters = TilesReaderParameters::new(
			tile_format,
			TileCompression::Uncompressed,
			TileBBoxPyramid::new_full(31),
		);

		let mut tilejson = TileJSON::default();

		if tile_format == TileFormat::MVT {
			tilejson.merge(&TileJSON::try_from(
				r#"{"vector_layers":[
					{"id":"background","minzoom":0,"maxzoom":30},
					{"id":"debug_x","minzoom":0,"maxzoom":30},
					{"id":"debug_y","minzoom":0,"maxzoom":30},
					{"id":"debug_z","minzoom":0,"maxzoom":30}
				]}"#,
			)?)?;
		}

		tilejson.update_from_reader_parameters(&parameters);

		Ok(Box::new(Self { tilejson, parameters }) as Box<dyn OperationTrait>)
	}
	pub fn from_vpl_node(vpl_node: &VPLNode) -> Result<Box<dyn OperationTrait>> {
		let args = Args::from_vpl_node(vpl_node)?;
		Self::from_parameters(TileFormat::parse_str(&args.format)?)
	}
}

impl ReadOperationTrait for Operation {
	fn build(vpl_node: VPLNode, _factory: &PipelineFactory) -> BoxFuture<'_, Result<Box<dyn OperationTrait>>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move { Operation::from_vpl_node(&vpl_node) })
	}
}

#[async_trait]
impl OperationTrait for Operation {
	fn get_parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	fn get_tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		Ok(Some(create_debug_image(coord)))
	}

	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		Ok(Some(create_debug_vector_tile(coord)?))
	}

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		match self.parameters.tile_format {
			TileFormat::AVIF | TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP => pack_image_tile(
				self.get_image_data(coord).await,
				self.parameters.tile_format,
				self.parameters.tile_compression,
			),
			TileFormat::MVT => pack_vector_tile(
				self.get_vector_data(coord).await,
				self.parameters.tile_format,
				self.parameters.tile_compression,
			),
			_ => bail!("tile format '{}' is not implemented yet", self.parameters.tile_format),
		}
	}

	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		match self.parameters.tile_format {
			TileFormat::AVIF | TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP => Ok(
				TileStream::from_coord_iter_parallel(bbox.into_iter_coords(), move |c| Some(create_debug_image(&c))),
			),
			_ => bail!("tile format '{}' is not implemented yet", self.parameters.tile_format),
		}
	}

	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		match self.parameters.tile_format {
			TileFormat::MVT => Ok(TileStream::from_coord_iter_parallel(
				bbox.into_iter_coords(),
				move |c| create_debug_vector_tile(&c).ok(),
			)),
			_ => bail!("tile format '{}' is not implemented yet", self.parameters.tile_format),
		}
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Blob>> {
		let tile_format = self.parameters.tile_format;
		let tile_compression = self.parameters.tile_compression;

		match tile_format {
			TileFormat::AVIF | TileFormat::JPG | TileFormat::PNG | TileFormat::WEBP => {
				pack_image_tile_stream(self.get_image_stream(bbox).await, tile_format, tile_compression)
			}
			TileFormat::MVT => pack_vector_tile_stream(self.get_vector_stream(bbox).await, tile_format, tile_compression),
			_ => bail!("tile format '{}' is not implemented yet", tile_format),
		}
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_debug"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, factory).await
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	//use pretty_assertions::assert_eq;

	async fn test(format: &str, len: u64, tilejson: &[&str]) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let operation = factory
			.operation_from_vpl(&format!("from_debug format={format}"))
			.await?;

		let coord = TileCoord3 { x: 1, y: 2, z: 3 };
		let blob = operation.get_tile_data(&coord).await?.unwrap();

		assert_eq!(blob.len(), len, "for '{format}'");
		assert_eq!(
			operation.get_tilejson().as_pretty_lines(100),
			tilejson,
			"for '{format}'"
		);

		let mut stream = operation.get_tile_stream(TileBBox::new(3, 1, 1, 2, 3)?).await?;

		let mut n = 0;
		while let Some((coord, blob)) = stream.next().await {
			assert!(!blob.is_empty(), "for '{format}'");
			assert!(coord.x >= 1 && coord.x <= 2, "for '{format}'");
			assert!(coord.y >= 1 && coord.y <= 3, "for '{format}'");
			assert_eq!(coord.z, 3, "for '{format}'");
			n += 1;
		}
		assert_eq!(n, 6, "for '{format}'");

		Ok(())
	}

	#[tokio::test]
	async fn test_build_tile_png() {
		test(
			"png",
			5207,
			&[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 31,",
				"  \"minzoom\": 0,",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}",
			],
		)
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn test_build_tile_jpg() {
		test(
			"jpg",
			11782,
			&[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 31,",
				"  \"minzoom\": 0,",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/jpeg\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}",
			],
		)
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn test_build_tile_webp() {
		test(
			"webp",
			2656,
			&[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 31,",
				"  \"minzoom\": 0,",
				"  \"tile_content\": \"raster\",",
				"  \"tile_format\": \"image/webp\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tilejson\": \"3.0.0\"",
				"}",
			],
		)
		.await
		.unwrap();
	}

	#[tokio::test]
	async fn test_build_tile_vector() {
		test(
			"mvt",
			1732,
			&[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 31,",
				"  \"minzoom\": 0,",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tilejson\": \"3.0.0\",",
				"  \"vector_layers\": [",
				"    { \"fields\": {  }, \"id\": \"background\", \"maxzoom\": 30, \"minzoom\": 0 },",
				"    { \"fields\": {  }, \"id\": \"debug_x\", \"maxzoom\": 30, \"minzoom\": 0 },",
				"    { \"fields\": {  }, \"id\": \"debug_y\", \"maxzoom\": 30, \"minzoom\": 0 },",
				"    { \"fields\": {  }, \"id\": \"debug_z\", \"maxzoom\": 30, \"minzoom\": 0 }",
				"  ]",
				"}",
			],
		)
		.await
		.unwrap();
	}
}
