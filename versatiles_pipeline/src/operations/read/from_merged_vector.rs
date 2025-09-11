//! # from_merged_vector operation
//!
//! Blends *multiple* **vector tile** sources by **concatenating layers** that
//! share the same name.  
//!  
//! * Sources are evaluated **in order** – later sources append their features
//!   after earlier ones within a layer.  
//! * All sources must provide Mapbox Vector Tiles (`*.mvt`).  
//! * The output is *always* a vector pyramid; raster data are not supported.
//!
//! The file contains:
//! 1. [`Args`] – the VPL/CLI parameters,  
//! 2. [`Operation`] – the runtime implementation,  
//! 3. Unit tests that verify layer merging, tile‐JSON updates, and
//!    pyramid handling.

use crate::{
	PipelineFactory,
	helpers::pack_vector_tile_stream,
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::{VPLNode, VPLPipeline},
};
use anyhow::{Result, bail, ensure};
use async_trait::async_trait;
use futures::{
	StreamExt,
	future::{BoxFuture, join_all},
	stream,
};
use imageproc::image::DynamicImage;
use std::collections::HashMap;
use versatiles_core::{tilejson::TileJSON, *};
use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Merges multiple vector tile sources.
/// Each resulting tile will contain all the features and properties from all the sources.
struct Args {
	/// All tile sources must provide vector tiles.
	sources: Vec<VPLPipeline>,
}

/// [`OperationTrait`] implementation that merges vector tiles “on the fly.”
///
/// * Keeps only metadata in memory; actual tile data stream straight through.  
/// * Performs no disk I/O itself – it relies entirely on the child pipelines.
#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
	tilejson: TileJSON,
	traversal: Traversal,
}

/// Combine several `VectorTile`s by merging layers with identical names.
///
/// If multiple sources provide a layer called `"roads"`, all road features
/// end up in the same output layer; layers unique to a source are copied as‐is.
fn merge_vector_tiles(tiles: Vec<VectorTile>) -> Result<VectorTile> {
	let mut layers = HashMap::<String, VectorTileLayer>::new();
	for tile in tiles.into_iter() {
		for new_layer in tile.layers {
			if let Some(layer) = layers.get_mut(&new_layer.name) {
				layer.add_from_layer(new_layer)?;
			} else {
				layers.insert(new_layer.name.clone(), new_layer);
			}
		}
	}
	Ok(VectorTile::new(layers.into_values().collect()))
}

impl ReadOperationTrait for Operation {
	fn build(
		vpl_node: VPLNode,
		factory: &PipelineFactory,
	) -> BoxFuture<'_, Result<Box<dyn OperationTrait>, anyhow::Error>>
	where
		Self: Sized + OperationTrait,
	{
		Box::pin(async move {
			let args = Args::from_vpl_node(&vpl_node)?;
			let sources = join_all(args.sources.into_iter().map(|c| factory.build_pipeline(c)))
				.await
				.into_iter()
				.collect::<Result<Vec<_>>>()?;

			ensure!(sources.len() > 1, "must have at least two sources");

			let mut tilejson = TileJSON::default();
			let first_parameters = sources.first().unwrap().parameters();
			let tile_format = first_parameters.tile_format;
			let tile_compression = first_parameters.tile_compression;
			let mut pyramid = TileBBoxPyramid::new_empty();
			let mut traversal = Traversal::new_any();

			for source in sources.iter() {
				tilejson.merge(source.tilejson())?;

				traversal.intersect(source.traversal())?;

				let parameters = source.parameters();
				pyramid.include_bbox_pyramid(&parameters.bbox_pyramid);

				ensure!(
					parameters.tile_format.get_type() == TileType::Vector,
					"all sources must be vector tiles"
				);
			}

			let parameters = TilesReaderParameters::new(tile_format, tile_compression, pyramid);
			tilejson.update_from_reader_parameters(&parameters);

			Ok(Box::new(Self {
				tilejson,
				parameters,
				sources,
				traversal,
			}) as Box<dyn OperationTrait>)
		})
	}
}

#[async_trait]
impl OperationTrait for Operation {
	/// Reader parameters (format, compression, pyramid) for the merged result.
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// `TileJSON` after combining metadata from every source.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal(&self) -> &Traversal {
		&self.traversal
	}

	/// Stream packed vector tiles intersecting `bbox`.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		pack_vector_tile_stream(self.get_vector_stream(bbox).await, &self.parameters)
	}

	/// Always errors – raster output is not supported.
	async fn get_image_stream(&self, _bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		bail!("this operation does not support image data");
	}

	/// Stream merged vector tiles for every coordinate in `bbox`.
	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		Ok(TileStream::from_streams(
			stream::iter(bboxes).map(move |bbox| async move {
				let mut tiles = TileBBoxContainer::<Vec<VectorTile>>::new_default(bbox);

				for source in self.sources.iter() {
					source
						.get_vector_stream(bbox)
						.await
						.unwrap()
						.for_each_sync(|(coord, tile)| {
							tiles.get_mut(&coord).unwrap().push(tile);
						})
						.await;
				}

				TileStream::from_vec(
					tiles
						.into_iter()
						.filter_map(|(c, v)| {
							if v.is_empty() {
								None
							} else {
								Some((c, merge_vector_tiles(v).unwrap()))
							}
						})
						.collect(),
				)
			}),
			1,
		))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_merged_vector"
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
	use crate::helpers::mock_vector_source::{MockVectorSource, arrange_tiles};
	use itertools::Itertools;
	use std::path::Path;

	pub fn check_tile(blob: &Blob) -> String {
		let tile = VectorTile::from_blob(blob).unwrap();
		assert_eq!(tile.layers.len(), 1);

		let layer = &tile.layers[0];
		assert_eq!(layer.name, "mock");

		layer
			.features
			.iter()
			.map(|vtf| {
				let p = vtf.to_feature(layer).unwrap().properties;

				p.get("filename").unwrap().to_string()
			})
			.join(",")
	}

	#[tokio::test]
	async fn test_operation_error() {
		let factory = PipelineFactory::new_dummy();
		let error = |command: &'static str| async {
			assert_eq!(
				factory.operation_from_vpl(command).await.unwrap_err().to_string(),
				"must have at least two sources"
			)
		};

		error("from_merged_vector").await;
		error("from_merged_vector [ ]").await;
		error("from_merged_vector [ from_container filename=1.pbf ]").await;
	}

	#[tokio::test]
	async fn test_unknown_argument() {
		assert_eq!(
			PipelineFactory::new_dummy()
				.operation_from_vpl(
					"from_merged_vector color=red [ from_container filename=1.pbf, from_container filename=2.pbf ]"
				)
				.await
				.unwrap_err()
				.to_string(),
			"The 'from_merged_vector' operation does not support the argument 'color'.\nOnly the following arguments are supported:\n'sources'"
		);
	}

	#[tokio::test]
	async fn test_tilejson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_merged_vector [ from_container filename=1.pbf, from_container filename=2.pbf ]")
			.await?;

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_get_tile_stream() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				r#"from_merged_vector [
					from_container filename="A.pbf" | filter bbox=[-130,-20,20,70],
					from_container filename="B.pbf" | filter bbox=[-20,-70,130,20]
				]"#,
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;
		let tiles = result.get_tile_stream(bbox).await?.to_vec().await;

		assert_eq!(
			arrange_tiles(tiles, |blob| {
				match check_tile(&blob).as_str() {
					"A.pbf" => "🟦",
					"B.pbf" => "🟨",
					"A.pbf,B.pbf" => "🟩",
					e => panic!("{}", e),
				}
			}),
			vec![
				"🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟩 🟩 🟨 🟨",
				"🟦 🟦 🟩 🟩 🟨 🟨",
				"❌ ❌ 🟨 🟨 🟨 🟨",
				"❌ ❌ 🟨 🟨 🟨 🟨"
			]
		);

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -130.78125, -70.140364, 130.78125, 70.140364 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_parameters() -> Result<()> {
		let factory = PipelineFactory::default(
			Path::new(""),
			Box::new(|filename: String| -> BoxFuture<Result<Box<dyn TilesReaderTrait>>> {
				Box::pin(async move {
					let mut pyramide = TileBBoxPyramid::new_empty();
					for c in filename[0..filename.len() - 4].chars() {
						pyramide.include_bbox(&TileBBox::new_full(c.to_digit(10).unwrap() as u8)?);
					}
					Ok(Box::new(MockVectorSource::new(
						&[("mock", &[&[("filename", &filename)]])],
						Some(pyramide),
					)) as Box<dyn TilesReaderTrait>)
				})
			}),
		);

		let result = factory
			.operation_from_vpl(
				r#"from_merged_vector [ from_container filename="12.pbf", from_container filename="23.pbf" ]"#,
			)
			.await?;

		let parameters = result.parameters();

		assert_eq!(parameters.tile_format, TileFormat::MVT);
		assert_eq!(parameters.tile_compression, TileCompression::Uncompressed);
		assert_eq!(
			format!("{}", parameters.bbox_pyramid),
			"[1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8)]"
		);

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 3,",
				"  \"minzoom\": 1,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
				"  \"tile_type\": \"vector\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_merge_tiles_multiple_layers() -> Result<()> {
		let vector_tile1 = VectorTile::new(vec![VectorTileLayer::new_standard("layer1")]);
		let vector_tile2 = VectorTile::new(vec![VectorTileLayer::new_standard("layer2")]);

		let merged_tile = merge_vector_tiles(vec![vector_tile1, vector_tile2])?;

		assert_eq!(merged_tile.layers.len(), 2);
		assert!(merged_tile.layers.iter().any(|l| l.name == "layer1"));
		assert!(merged_tile.layers.iter().any(|l| l.name == "layer2"));

		Ok(())
	}
}
