use crate::{
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::{VPLNode, VPLPipeline},
	PipelineFactory,
};
use anyhow::{ensure, Result};
use async_trait::async_trait;
use futures::future::{join_all, BoxFuture};
use imageproc::image::DynamicImage;
use versatiles_core::{tilejson::TileJSON, types::*, utils::recompress};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Overlays multiple tile sources, using the tile from the first source that provides it.
struct Args {
	/// All tile sources must have the same format.
	sources: Vec<VPLPipeline>,
	/// The tile format to use for the output tiles (default: format of the first source).
	format: Option<TileFormat>,
}

#[derive(Debug)]
struct Operation {
	parameters: TilesReaderParameters,
	sources: Vec<Box<dyn OperationTrait>>,
	tilejson: TileJSON,
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
			let parameters = sources.first().unwrap().get_parameters();
			let tile_format = args.format.unwrap_or(parameters.tile_format);
			let tile_compression = parameters.tile_compression;
			let tile_type = tile_format.get_type();

			let mut pyramid = TileBBoxPyramid::new_empty();

			for source in sources.iter() {
				tilejson.merge(source.get_tilejson())?;

				let parameters = source.get_parameters();
				pyramid.include_bbox_pyramid(&parameters.bbox_pyramid);

				ensure!(
					parameters.tile_format.get_type() == tile_type,
					"all sources must have the same tile type (raster or vector)"
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

impl Operation {
	/// Consolidates the duplicated logic of the three `*_stream` methods.
	async fn gather_stream<'a, T, FetchStream, MapFn>(
		&'a self,
		bbox: TileBBox,
		mut fetch_stream: FetchStream,
		mut map_fn: MapFn,
	) -> Result<TileStream<'a, T>>
	where
		T: Clone + Send + 'static,
		// Fetch a stream for the remaining bbox from one source.
		FetchStream: FnMut(&'a Box<dyn OperationTrait>, TileBBox) -> BoxFuture<'a, Result<TileStream<'a, T>>>
			+ Clone
			+ Copy
			+ Send
			+ 'a,
		// Optional post‑processing of each tile (e.g. recompression).
		MapFn: FnMut(T, &Box<dyn OperationTrait>) -> Result<T> + Copy + Send + 'a,
	{
		// Divide the requested bbox into manageable chunks.
		let sub_bboxes: Vec<TileBBox> = bbox.clone().iter_bbox_grid(32).collect();

		Ok(
			TileStream::from_stream_iter(sub_bboxes.into_iter().map(move |bbox| async move {
				let mut tiles: Vec<Option<(TileCoord3, T)>> = vec![None; bbox.count_tiles() as usize];

				for source in self.sources.iter() {
					let mut bbox_left = TileBBox::new_empty(bbox.level).unwrap();
					for (idx, slot) in tiles.iter().enumerate() {
						if slot.is_none() {
							bbox_left
								.include_coord3(&bbox.get_coord3_by_index(idx as u32).unwrap())
								.unwrap();
						}
					}
					if bbox_left.is_empty() {
						continue;
					}

					let stream = fetch_stream(source, bbox_left).await.unwrap();
					stream
						.for_each_sync(|(coord, item)| {
							let idx = bbox.get_tile_index3(&coord).unwrap();
							if tiles[idx].is_none() {
								let item = map_fn(item, source).unwrap();
								tiles[idx] = Some((coord, item));
							}
						})
						.await;
				}

				TileStream::from_vec(tiles.into_iter().flatten().collect())
			}))
			.await,
		)
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
		for source in self.sources.iter() {
			if let Some(mut blob) = source.get_tile_data(coord).await? {
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

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		// We need the desired output compression inside the closure, so copy it.
		let output_compression = self.parameters.tile_compression;
		self
			.gather_stream(
				bbox,
				|src, b| Box::pin(async move { src.get_tile_stream(b).await }),
				move |blob: Blob, src| recompress(blob, &src.get_parameters().tile_compression, &output_compression),
			)
			.await
	}

	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		for source in self.sources.iter() {
			let tile = source.get_image_data(coord).await?;
			if tile.is_some() {
				return Ok(tile);
			}
		}
		return Ok(None);
	}

	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		self
			.gather_stream(
				bbox,
				|src, b| Box::pin(async move { src.get_image_stream(b).await }),
				|img, _| Ok(img),
			)
			.await
	}

	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		for source in self.sources.iter() {
			let tile = source.get_vector_data(coord).await?;
			if tile.is_some() {
				return Ok(tile);
			}
		}
		return Ok(None);
	}

	async fn get_vector_stream(&self, bbox: TileBBox) -> Result<TileStream<VectorTile>> {
		self
			.gather_stream(
				bbox,
				|src, b| Box::pin(async move { src.get_vector_stream(b).await }),
				|tile, _| Ok(tile),
			)
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
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn OperationTrait>> {
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

		let filename = properties.get("filename").unwrap().to_string();
		Ok(filename[0..filename.len() - 4].to_string())
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

		error("from_overlayed").await;
		error("from_overlayed [ ]").await;
		error("from_overlayed [ from_container filename=1.pbf ]").await;
	}

	#[tokio::test]
	async fn test_operation_get_tile_data() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_overlayed [ from_container filename=1.pbf, from_container filename=2.pbf ]")
			.await?;

		let coord = TileCoord3::new(1, 2, 3)?;
		let blob = result.get_tile_data(&coord).await?.unwrap();

		assert_eq!(check_tile(&blob, &coord)?, "1");

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
				&[
					"from_overlayed [",
					"   from_container filename=\"🟦.pbf\" | filter_bbox bbox=[-130,-20,20,70],",
					"   from_container filename=\"🟨.pbf\" | filter_bbox bbox=[-20,-70,130,20]",
					"]",
				]
				.join(""),
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;
		let tiles = result.get_tile_stream(bbox).await?.collect().await;

		assert_eq!(
			arrange_tiles(tiles, |coord, blob| check_tile(&blob, &coord).unwrap()),
			vec![
				"🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟦 🟦 ❌ ❌",
				"🟦 🟦 🟦 🟦 🟨 🟨",
				"🟦 🟦 🟦 🟦 🟨 🟨",
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
}
