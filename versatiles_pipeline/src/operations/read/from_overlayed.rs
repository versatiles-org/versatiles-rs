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
					"all children must have the same tile format"
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

	async fn get_tile_data(&mut self, coord: &TileCoord3) -> Result<Option<Blob>> {
		for source in self.sources.iter_mut() {
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
				source
					.get_bbox_tile_stream(bbox.clone())
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
	use versatiles_geometry::{vector_tile::VectorTile, GeoValue};

	use super::*;

	fn check_tile(blob: &Blob, coord: &TileCoord3) -> Result<String> {
		let tile = VectorTile::from_blob(blob)?;
		assert_eq!(tile.layers.len(), 1);

		let layer = &tile.layers[0];
		assert_eq!(layer.name, "mock");
		assert_eq!(layer.features.len(), 1);

		let feature = &layer.features[0].to_feature(layer)?;
		let properties = feature.properties.as_ref().unwrap();

		assert_eq!(properties.get("x").unwrap(), &GeoValue::from(coord.x));
		assert_eq!(properties.get("y").unwrap(), &GeoValue::from(coord.y));
		assert_eq!(properties.get("z").unwrap(), &GeoValue::from(coord.z));

		Ok(properties.get("filename").unwrap().to_string())
	}

	fn arrange_tiles<T: ToString>(
		tiles: Vec<(TileCoord3, Blob)>,
		cb: impl Fn(TileCoord3, Blob) -> T,
	) -> Vec<String> {
		let mut bbox = TileBBox::new_empty(tiles.get(0).unwrap().0.z).unwrap();
		tiles.iter().for_each(|t| bbox.include_tile(t.0.x, t.0.y));

		let mut result: Vec<Vec<String>> = (0..bbox.height())
			.into_iter()
			.map(|_| {
				(0..bbox.width())
					.into_iter()
					.map(|_| String::from(""))
					.collect()
			})
			.collect();

		for (coord, blob) in tiles.into_iter() {
			let x = (coord.x - bbox.x_min) as usize;
			let y = (coord.y - bbox.y_min) as usize;
			result[y][x] = cb(coord, blob).to_string();
		}
		result
			.into_iter()
			.map(|r| r.join(","))
			.collect::<Vec<String>>()
	}

	#[tokio::test]
	async fn test_operation_error1() -> Result<()> {
		let vpl_node = VPLNode::from("from_overlayed");
		let factory = PipelineFactory::new_dummy();

		assert_eq!(
			&Operation::build(vpl_node, &factory)
				.await
				.unwrap_err()
				.to_string(),
			"must have at least two sources"
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_error2() {
		let factory = PipelineFactory::new_dummy();
		let result = factory.operation_from_vpl("from_overlayed [ ]").await;

		assert_eq!(
			&result.unwrap_err().to_string(),
			"must have at least two sources"
		);
	}

	#[tokio::test]
	async fn test_operation_error3() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_overlayed [ from_container filename=1 ]")
			.await;

		assert_eq!(
			&result.unwrap_err().to_string(),
			"must have at least two sources"
		);
	}

	#[tokio::test]
	async fn test_operation_get_tile_data() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let mut result = factory
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
					"   from_container filename=1 | filter_bbox bbox=[-180,-20,20,85],",
					"   from_container filename=2 | filter_bbox bbox=[-20,-85,180,20],",
					"   from_container filename=3",
					"]",
				]
				.join(""),
			)
			.await?;

		let bbox = TileBBox::new(2, 0, 0, 3, 3)?;
		let tiles = result
			.get_bbox_tile_stream(bbox.clone())
			.await
			.collect()
			.await;

		assert_eq!(
			arrange_tiles(tiles, |coord, blob| check_tile(&blob, &coord).unwrap()),
			vec!["1,1,1,3", "1,1,1,2", "1,1,1,2", "3,2,2,2"]
		);

		Ok(())
	}
}
