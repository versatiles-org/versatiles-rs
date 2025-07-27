//! # from_overlayed operation
//!
//! Selects the **first** non‑empty tile from a chain of sources that all
//! share the *same* tile type (raster *or* vector).  Think of it as a
//! “transparent overlay”: the moment a source can deliver a tile for the
//! requested coordinate, downstream sources are ignored for that tile.
//!
//! * Sources are evaluated in the **order** provided in the VPL list.  
//! * No blending occurs – it is a *winner‑takes‑first* strategy.  
//! * All sources must expose an identical tile type and compression; only
//!   their spatial coverage may differ.
//!
//! The file provides:
//! 1. [`Args`] – CLI / VPL configuration,  
//! 2. [`Operation`] – the runtime implementation,  
//! 3. Tests that verify error handling and overlay semantics.

use crate::{
	PipelineFactory,
	operations::read::traits::ReadOperationTrait,
	traits::*,
	vpl::{VPLNode, VPLPipeline},
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use futures::future::{BoxFuture, join_all};
use imageproc::image::DynamicImage;
use versatiles_core::{tilejson::TileJSON, types::*, utils::recompress};
use versatiles_geometry::vector_tile::VectorTile;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Overlays multiple tile sources, using the tile from the first source that provides it.
struct Args {
	/// All tile sources must have the same format.
	sources: Vec<VPLPipeline>,
}

#[derive(Debug)]
/// Implements [`OperationTrait`] by performing *short‑circuit* look‑ups
/// across multiple sources.
///
/// The struct keeps only metadata (`TileJSON`, `TilesReaderParameters`) in
/// memory; actual tile data are streamed directly from the first source that
/// contains them.
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
			let parameters = sources.first().unwrap().parameters();
			let tile_format = parameters.tile_format;
			let tile_compression = parameters.tile_compression;

			let mut pyramid = TileBBoxPyramid::new_empty();

			for source in sources.iter() {
				tilejson.merge(source.tilejson())?;

				let parameters = source.parameters();
				pyramid.include_bbox_pyramid(&parameters.bbox_pyramid);

				ensure!(
					parameters.tile_format == tile_format,
					"all sources must have the same tile format"
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
	/// Internal helper that collapses the common logic of
	/// `get_tile_stream`, `get_image_stream`, and `get_vector_stream`.
	///
	/// For each sub‑bbox it:
	/// 1. Allocates a slot for every coordinate,  
	/// 2. Iterates through the sources in priority order,  
	/// 3. Fills empty slots with whichever source provides the tile first,  
	/// 4. Optionally post‑processes each tile (`map_fn`),  
	/// 5. Emits a complete, gap‑free `TileStream`.
	///
	/// The generic parameters allow us to reuse the routine for both raster
	/// and vector content while avoiding code duplication.
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
	/// Reader parameters (format, compression, pyramid) for the overlay result.
	fn parameters(&self) -> &TilesReaderParameters {
		&self.parameters
	}

	/// Combined `TileJSON` after merging metadata from all sources.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn traversal_orders(&self) -> TraversalOrderSet {
		self
			.sources
			.iter()
			.map(|source| source.traversal_orders())
			.fold(TraversalOrderSet::new_all(), |acc, set| acc & set)
	}

	/// Fetch a *packed* tile for `coord`, recompressing if necessary.
	async fn get_tile_data(&self, coord: &TileCoord3) -> Result<Option<Blob>> {
		for source in self.sources.iter() {
			if let Some(mut blob) = source.get_tile_data(coord).await? {
				blob = recompress(
					blob,
					&source.parameters().tile_compression,
					&self.parameters.tile_compression,
				)?;
				return Ok(Some(blob));
			}
		}
		return Ok(None);
	}

	/// Stream packed tiles intersecting `bbox` using the overlay strategy.
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream> {
		// We need the desired output compression inside the closure, so copy it.
		let output_compression = self.parameters.tile_compression;
		self
			.gather_stream(
				bbox,
				|src, b| Box::pin(async move { src.get_tile_stream(b).await }),
				move |blob: Blob, src| recompress(blob, &src.parameters().tile_compression, &output_compression),
			)
			.await
	}

	/// Retrieve a single raster tile, stopping at the first source that has it.
	async fn get_image_data(&self, coord: &TileCoord3) -> Result<Option<DynamicImage>> {
		for source in self.sources.iter() {
			let tile = source.get_image_data(coord).await?;
			if tile.is_some() {
				return Ok(tile);
			}
		}
		return Ok(None);
	}

	/// Stream raster tiles for every coordinate in `bbox` via overlay.
	async fn get_image_stream(&self, bbox: TileBBox) -> Result<TileStream<DynamicImage>> {
		self
			.gather_stream(
				bbox,
				|src, b| Box::pin(async move { src.get_image_stream(b).await }),
				|img, _| Ok(img),
			)
			.await
	}

	/// Retrieve a single vector tile, stopping at the first source that has it.
	async fn get_vector_data(&self, coord: &TileCoord3) -> Result<Option<VectorTile>> {
		for source in self.sources.iter() {
			let tile = source.get_vector_data(coord).await?;
			if tile.is_some() {
				return Ok(tile);
			}
		}
		return Ok(None);
	}

	/// Stream vector tiles for every coordinate in `bbox` via overlay.
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
	use versatiles_image::EnhancedDynamicImageTrait;

	use super::*;
	use crate::helpers::mock_vector_source::arrange_tiles;
	use std::sync::LazyLock;

	static RESULT_PATTERN: LazyLock<Vec<String>> = LazyLock::new(|| {
		vec![
			"🟦 🟦 🟦 🟦 ❌ ❌".to_string(),
			"🟦 🟦 🟦 🟦 ❌ ❌".to_string(),
			"🟦 🟦 🟦 🟦 🟨 🟨".to_string(),
			"🟦 🟦 🟦 🟦 🟨 🟨".to_string(),
			"❌ ❌ 🟨 🟨 🟨 🟨".to_string(),
			"❌ ❌ 🟨 🟨 🟨 🟨".to_string(),
		]
	});

	pub fn check_vector_blob(blob: Blob) -> String {
		use versatiles_geometry::vector_tile::VectorTile;
		let tile = VectorTile::from_blob(&blob).unwrap();
		check_vector(tile)
	}

	pub fn check_image_blob(blob: Blob) -> String {
		let tile = DynamicImage::from_blob(&blob, TileFormat::PNG).unwrap();
		check_image(tile)
	}

	pub fn check_vector(tile: VectorTile) -> String {
		assert_eq!(tile.layers.len(), 1);

		let layer = &tile.layers[0];
		assert_eq!(layer.name, "mock");
		assert_eq!(layer.features.len(), 1);

		let feature = &layer.features[0].to_feature(layer).unwrap();
		let properties = &feature.properties;

		let filename = properties.get("filename").unwrap().to_string();
		filename[0..filename.len() - 4].to_string()
	}

	pub fn check_image(image: DynamicImage) -> String {
		use versatiles_image::EnhancedDynamicImageTrait;
		let pixel = image.average_color();
		match pixel.as_slice() {
			[0, 0, 255] => "🟦".to_string(),
			[255, 255, 0] => "🟨".to_string(),
			_ => panic!("Unexpected pixel color: {pixel:?}"),
		}
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
	async fn test_tilejson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				&[
					"from_overlayed [",
					"   from_container filename=\"1.pbf\" | filter_bbox bbox=[-11,-12,3,4],",
					"   from_container filename=\"2.pbf\" | filter_bbox bbox=[-5,-6,7,8]",
					"]",
				]
				.join(""),
			)
			.await?;

		let coord = TileCoord3::new(7, 8, 4)?;
		let blob = result.get_tile_data(&coord).await?.unwrap();
		assert_eq!(check_vector_blob(blob), "1");

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [ -11.25, -12.554564, 7.03125, 8.407168 ],",
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
	async fn test_operation_vector() -> Result<()> {
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
		assert_eq!(arrange_tiles(tiles, check_vector_blob), *RESULT_PATTERN);

		let tiles = result.get_vector_stream(bbox).await?.collect().await;
		assert_eq!(arrange_tiles(tiles, check_vector), *RESULT_PATTERN);

		let c1 = TileCoord3::new(7, 7, 4)?;
		let c2 = TileCoord3::new(9, 7, 4)?;
		assert_eq!(check_vector_blob(result.get_tile_data(&c1).await?.unwrap()), "🟦");
		assert_eq!(check_vector_blob(result.get_tile_data(&c2).await?.unwrap()), "🟨");
		assert_eq!(check_vector(result.get_vector_data(&c1).await?.unwrap()), "🟦");
		assert_eq!(check_vector(result.get_vector_data(&c2).await?.unwrap()), "🟨");

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_image() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(
				&[
					"from_overlayed [",
					"   from_container filename=\"00f.png\" | filter_bbox bbox=[-130,-20,20,70],",
					"   from_container filename=\"ff0.png\" | filter_bbox bbox=[-20,-70,130,20]",
					"]",
				]
				.join(""),
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;

		let tiles = result.get_tile_stream(bbox).await?.collect().await;
		assert_eq!(arrange_tiles(tiles, check_image_blob), *RESULT_PATTERN);

		let tiles = result.get_image_stream(bbox).await?.collect().await;
		assert_eq!(arrange_tiles(tiles, check_image), *RESULT_PATTERN);

		let c1 = TileCoord3::new(7, 7, 4)?;
		let c2 = TileCoord3::new(9, 7, 4)?;
		assert_eq!(check_image_blob(result.get_tile_data(&c1).await?.unwrap()), "🟦");
		assert_eq!(check_image_blob(result.get_tile_data(&c2).await?.unwrap()), "🟨");
		assert_eq!(check_image(result.get_image_data(&c1).await?.unwrap()), "🟦");
		assert_eq!(check_image(result.get_image_data(&c2).await?.unwrap()), "🟨");

		Ok(())
	}
}
