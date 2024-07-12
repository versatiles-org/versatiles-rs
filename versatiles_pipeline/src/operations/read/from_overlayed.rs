use crate::{
	traits::*,
	types::{Blob, TileBBox, TileCompression, TileCoord3, TileStream, TilesReaderParameters},
	utils::recompress,
	vpl::{VPLNode, VPLPipeline},
	PipelineFactory,
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures::future::{join_all, BoxFuture};

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
			let mut tile_compression = parameters.tile_compression;

			for source in sources.iter() {
				let parameters = source.get_parameters();
				pyramid.include_bbox_pyramid(&parameters.bbox_pyramid);
				ensure!(
					parameters.tile_format == tile_format,
					"all sources must have the same tile format"
				);
				if parameters.tile_compression != tile_compression {
					tile_compression = TileCompression::Uncompressed;
				}
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

	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		for source in self.sources.iter() {
			let result = source.get_tile_data(coord).await?;
			if let Some(mut blob) = result {
				blob = recompress(
					blob,
					&source.get_parameters().tile_compression,
					&self.parameters.tile_compression,
				)?;
				return Ok(Some(blob));
			}
		}
		return Ok(None);
	}

	async fn get_bbox_tile_stream(&self, bbox: TileBBox) -> TileStream {
		let output_compression = &self.parameters.tile_compression;
		let bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		TileStream::from_stream_iter(bboxes.into_iter().map(move |bbox| async move {
			let mut tiles: Vec<Option<(TileCoord3, Blob)>> = Vec::new();
			tiles.resize(bbox.count_tiles() as usize, None);

			for source in self.sources.iter() {
				let mut bbox_left = TileBBox::new_empty(bbox.level).unwrap();
				for (index, t) in tiles.iter().enumerate() {
					if t.is_none() {
						bbox_left.include_coord2(&bbox.get_coord2_by_index(index as u32).unwrap())
					}
				}
				if bbox_left.is_empty() {
					continue;
				}

				source
					.get_bbox_tile_stream(bbox_left)
					.await
					.for_each_sync(|(coord, mut blob)| {
						let index = bbox.get_tile_index3(&coord);
						if tiles[index].is_none() {
							blob = recompress(
								blob,
								&source.get_parameters().tile_compression,
								output_compression,
							)
							.unwrap();
							tiles[index] = Some((coord, blob));
						}
					})
					.await;
			}

			TileStream::from_vec(tiles.into_iter().flatten().collect())
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
		"from_overlayed"
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
	use super::*;
	use crate::helpers::mock_vector_source::arrange_tiles;

	pub fn check_tile(blob: &Blob, coord: &TileCoord3) -> Result<String> {
		use versatiles_geometry::{vector_tile::VectorTile, GeoValue};

		let tile = VectorTile::from_blob(blob)?;
		assert_eq!(tile.layers.len(), 1);

		let layer = &tile.layers[0];
		assert_eq!(layer.name, "mock");
		assert_eq!(layer.features.len(), 1);

		let feature = &layer.features[0].to_feature(layer)?;
		let properties = &feature.properties;

		assert_eq!(properties.get("x").unwrap(), &GeoValue::from(coord.x));
		assert_eq!(properties.get("y").unwrap(), &GeoValue::from(coord.y));
		assert_eq!(properties.get("z").unwrap(), &GeoValue::from(coord.z));

		Ok(properties.get("filename").unwrap().to_string())
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

		error("from_overlayed").await;
		error("from_overlayed [ ]").await;
		error("from_overlayed [ from_container filename=1 ]").await;
	}

	#[tokio::test]
	async fn test_operation_get_tile_data() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				"from_overlayed [ from_container filename=1, from_container filename=2 ]",
			)
			.await?;

		let coord = TileCoord3::new(1, 2, 3)?;
		let blob = result.get_tile_data(&coord).await?.unwrap();

		assert_eq!(check_tile(&blob, &coord)?, "1");

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_get_bbox_tile_stream() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				&[
					"from_overlayed [",
					"   from_container filename=\"🟦\" | filter_bbox bbox=[-180,-20,20,85],",
					"   from_container filename=\"🟨\" | filter_bbox bbox=[-20,-85,180,20]",
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
			arrange_tiles(tiles, |coord, blob| check_tile(&blob, &coord).unwrap()),
			vec![
				"🟦 🟦 🟦 🟦 🟦 ❌ ❌ ❌",
				"🟦 🟦 🟦 🟦 🟦 ❌ ❌ ❌",
				"🟦 🟦 🟦 🟦 🟦 ❌ ❌ ❌",
				"🟦 🟦 🟦 🟦 🟦 🟨 🟨 🟨",
				"🟦 🟦 🟦 🟦 🟦 🟨 🟨 🟨",
				"❌ ❌ ❌ 🟨 🟨 🟨 🟨 🟨",
				"❌ ❌ ❌ 🟨 🟨 🟨 🟨 🟨",
				"❌ ❌ ❌ 🟨 🟨 🟨 🟨 🟨"
			]
		);

		Ok(())
	}
}
