use crate::{
	helpers::{pack_vector_tile, pack_vector_tile_stream},
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::{VPLNode, VPLPipeline},
	PipelineFactory,
};
use anyhow::{bail, ensure, Result};
use async_trait::async_trait;
use futures::future::{join_all, BoxFuture};
use imageproc::image::DynamicImage;
use std::collections::HashMap;
use versatiles_core::{tilejson::TileJSON, types::*};
use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Merges multiple vector tile sources. Each layer will contain all features from the same layer of all sources.
struct Args {
	/// All tile sources must provide vector tiles.
	sources: Vec<VPLPipeline>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
	tilejson: TileJSON,
}

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
			let first_parameters = sources.first().unwrap().get_parameters();
			let tile_format = first_parameters.tile_format;
			let tile_compression = first_parameters.tile_compression;
			let mut pyramid = TileBBoxPyramid::new_empty();

			for source in sources.iter() {
				tilejson.merge(source.get_tilejson())?;

				let parameters = source.get_parameters();
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
			}) as Box<dyn OperationTrait>)
		})
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

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		pack_vector_tile(self.get_vector_data(coord).await, &self.parameters)
	}

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		pack_vector_tile_stream(self.get_vector_stream(bbox).await, &self.parameters)
	}

	async fn get_image_data(&self, _coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		bail!("this operation does not support image data");
	}

	async fn get_image_stream(&self, _bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		bail!("this operation does not support image data");
	}

	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		let mut vector_tiles: Vec<VectorTile> = vec![];
		for source in self.sources.iter() {
			let vector_tile = source.get_vector_data(coord).await?;
			if let Some(vector_tile) = vector_tile {
				vector_tiles.push(vector_tile);
			}
		}

		Ok(if vector_tiles.is_empty() {
			None
		} else {
			Some(merge_vector_tiles(vector_tiles)?)
		})
	}

	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		Ok(
			TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
				let mut tiles: Vec<Vec<VectorTile>> = Vec::new();
				tiles.resize(bbox.count_tiles() as usize, vec![]);

				for source in self.sources.iter() {
					source
						.get_vector_stream(bbox)
						.await
						.unwrap()
						.for_each_sync(|(coord, tile)| {
							tiles[bbox.get_tile_index3(&coord).unwrap()].push(tile);
						})
						.await;
				}

				TileStream::from_vec(
					tiles
						.into_iter()
						.enumerate()
						.filter_map(|(i, v)| {
							if v.is_empty() {
								None
							} else {
								Some((
									bbox.get_coord3_by_index(i as u32).unwrap(),
									merge_vector_tiles(v).unwrap(),
								))
							}
						})
						.collect(),
				)
			}))
			.await,
		)
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"merge_vectortiles"
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
	use crate::helpers::mock_vector_source::{arrange_tiles, MockVectorSource};
	use itertools::Itertools;
	use std::{ops::BitXor, path::Path};

	pub fn check_tile(blob: &Blob, coord: &TileCoord3) -> String {
		use versatiles_geometry::GeoValue;

		let tile = VectorTile::from_blob(blob).unwrap();
		assert_eq!(tile.layers.len(), 1);

		let layer = &tile.layers[0];
		assert_eq!(layer.name, "mock");

		layer
			.features
			.iter()
			.map(|vtf| {
				let p = vtf.to_feature(layer).unwrap().properties;

				assert_eq!(p.get("x").unwrap(), &GeoValue::from(coord.x));
				assert_eq!(p.get("y").unwrap(), &GeoValue::from(coord.y));
				assert_eq!(p.get("z").unwrap(), &GeoValue::from(coord.z));

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

		error("merge_vectortiles").await;
		error("merge_vectortiles [ ]").await;
		error("merge_vectortiles [ from_container filename=1.pbf ]").await;
	}

	#[tokio::test]
	async fn test_operation_get_tile_data() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("merge_vectortiles [ from_container filename=1.pbf, from_container filename=2.pbf ]")
			.await?;

		let coord = TileCoord3::new(1, 2, 3)?;
		let blob = result.get_tile_data(&coord).await?.unwrap();

		assert_eq!(check_tile(&blob, &coord), "1.pbf,2.pbf");

		assert_eq!(
			result.get_tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
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
				r#"merge_vectortiles [
					from_container filename="A.pbf" | filter_bbox bbox=[-130,-20,20,70],
					from_container filename="B.pbf" | filter_bbox bbox=[-20,-70,130,20]
				]"#,
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;
		let tiles = result.get_tile_stream(bbox).await?.collect().await;

		assert_eq!(
			arrange_tiles(tiles, |coord, blob| {
				match check_tile(&blob, &coord).as_str() {
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
			result.get_tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -130.78125, -70.140364, 130.78125, 70.140364 ],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
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
				r#"merge_vectortiles [ from_container filename="12.pbf", from_container filename="23.pbf" ]"#,
			)
			.await?;

		let parameters = result.get_parameters();

		assert_eq!(parameters.tile_format, TileFormat::MVT);
		assert_eq!(parameters.tile_compression, TileCompression::Uncompressed);
		assert_eq!(
			format!("{}", parameters.bbox_pyramid),
			"[1: [0,0,1,1] (4), 2: [0,0,3,3] (16), 3: [0,0,7,7] (64)]"
		);

		for level in 0..=4 {
			assert!(
				result
					.get_tile_data(&TileCoord3::new(0, 0, level)?)
					.await?
					.is_some()
					.bitxor(!(1..=3).contains(&level)),
				"level: {level}"
			);
		}

		assert_eq!(
			result.get_tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -180, -85.051129, 180, 85.051129 ],",
				"  \"maxzoom\": 3,",
				"  \"minzoom\": 1,",
				"  \"name\": \"mock vector source\",",
				"  \"tile_content\": \"vector\",",
				"  \"tile_format\": \"vnd.mapbox-vector-tile\",",
				"  \"tile_schema\": \"other\",",
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
