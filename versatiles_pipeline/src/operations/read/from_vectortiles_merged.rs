use std::collections::HashMap;

use crate::{
	traits::*,
	types::{Blob, TileBBox, TileCompression, TileCoord3, TileStream, TilesReaderParameters},
	vpl::{VPLNode, VPLPipeline},
	PipelineFactory,
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures::future::{join_all, BoxFuture};
use versatiles_core::{types::TileFormat, utils::decompress};
use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Overlays multiple tile sources, using the tile from the first source that provides it.
struct Args {
	/// All tile sources must have the same format.
	sources: Vec<VPLPipeline>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
	meta: Option<Blob>,
}

fn merge_tiles(blobs: Vec<Blob>) -> Result<Blob> {
	let mut layers = HashMap::<String, VectorTileLayer>::new();
	for blob in blobs.into_iter() {
		let tile = VectorTile::from_blob(&blob)?;
		for new_layer in tile.layers {
			if let Some(layer) = layers.get_mut(&new_layer.name) {
				layer.add_from_layer(new_layer)?;
			} else {
				layers.insert(new_layer.name.clone(), new_layer);
			}
		}
	}
	VectorTile::new(layers.into_values().collect()).to_blob()
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

			let meta = sources.first().unwrap().get_meta();
			let parameters = sources.first().unwrap().get_parameters();
			let mut pyramid = parameters.bbox_pyramid.clone();
			let tile_format = parameters.tile_format;
			let tile_compression = TileCompression::Uncompressed;

			for source in sources.iter() {
				let parameters = source.get_parameters();
				pyramid.include_bbox_pyramid(&parameters.bbox_pyramid);
				ensure!(
					tile_format == TileFormat::PBF,
					"all sources must be vector tiles"
				);
			}

			let parameters = TilesReaderParameters::new(tile_format, tile_compression, pyramid);

			Ok(Box::new(Self {
				meta,
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

	fn get_meta(&self) -> Option<Blob> {
		self.meta.clone()
	}

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		let mut blobs: Vec<Blob> = vec![];
		for source in self.sources.iter_mut() {
			let result = source.get_tile_data(coord).await?;
			if let Some(mut blob) = result {
				blob = decompress(blob, &source.get_parameters().tile_compression)?;
				blobs.push(blob);
			}
		}
		if blobs.is_empty() {
			return Ok(None);
		} else {
			return Ok(Some(merge_tiles(blobs)?));
		}
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
			let mut tiles: Vec<Vec<Blob>> = Vec::new();
			tiles.resize(bbox.count_tiles() as usize, vec![]);

			for source in self.sources.iter() {
				source
					.get_bbox_tile_stream(bbox.clone())
					.await
					.for_each_sync(|(coord, mut blob)| {
						let index = bbox.get_tile_index3(&coord);
						blob = decompress(blob, &source.get_parameters().tile_compression).unwrap();
						tiles[index].push(blob);
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
								merge_tiles(v).unwrap(),
							))
						}
					})
					.collect(),
			)
		}))
		.await
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_vectortiles_merged"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(
		&self,
		vpl_node: VPLNode,
		factory: &'a PipelineFactory,
	) -> Result<Box<dyn OperationTrait>> {
		Operation::build(vpl_node, factory).await
	}
}
#[cfg(test)]
mod tests {
	use itertools::Itertools;

	use super::*;
	use crate::helpers::mock_vector_source::arrange_tiles;

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
				factory
					.operation_from_vpl(command)
					.await
					.unwrap_err()
					.to_string(),
				"must have at least two sources"
			)
		};

		error("from_vectortiles_merged").await;
		error("from_vectortiles_merged [ ]").await;
		error("from_vectortiles_merged [ from_container filename=1 ]").await;
	}

	#[tokio::test]
	async fn test_operation_get_tile_data() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let mut result = factory
			.operation_from_vpl(
				"from_vectortiles_merged [ from_container filename=1, from_container filename=2 ]",
			)
			.await?;

		let coord = TileCoord3::new(1, 2, 3)?;
		let blob = result.get_tile_data(&coord).await?.unwrap();

		assert_eq!(check_tile(&blob, &coord), "1,2");

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_get_bbox_tile_stream() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				&[
					"from_vectortiles_merged [",
					"   from_container filename=\"A\" | filter_bbox bbox=[-180,-45,90,85],",
					"   from_container filename=\"B\" | filter_bbox bbox=[-90,-85,180,45]",
					"]",
				]
				.join(""),
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;
		let tiles = result
			.get_bbox_tile_stream(bbox.clone())
			.await
			.collect()
			.await;

		assert_eq!(
			arrange_tiles(tiles, |coord, blob| {
				match check_tile(&blob, &coord).as_str() {
					"A" => "🟦",
					"B" => "🟨",
					"A,B" => "🟩",
					e => panic!("{}", e),
				}
			}),
			vec![
				"🟦 🟦 🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟩 🟩 🟩 🟩 🟨 🟨",
				"🟦 🟦 🟩 🟩 🟩 🟩 🟨 🟨",
				"🟦 🟦 🟩 🟩 🟩 🟩 🟨 🟨",
				"🟦 🟦 🟩 🟩 🟩 🟩 🟨 🟨",
				"❌ ❌ 🟨 🟨 🟨 🟨 🟨 🟨",
				"❌ ❌ 🟨 🟨 🟨 🟨 🟨 🟨"
			]
		);

		Ok(())
	}
}
