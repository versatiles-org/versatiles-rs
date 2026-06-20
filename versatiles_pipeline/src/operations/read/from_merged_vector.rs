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
	operations::read::traits::ReadTileSource,
	vpl::{VPLNode, VPLPipeline},
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use futures::{StreamExt, future::join_all, stream};
use std::{collections::HashMap, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileBBoxMap, TileFormat, TileJSON, TilePyramid, TileStream, TileType};
use versatiles_derive::context;
use versatiles_geometry::vector_tile::{VectorTile, VectorTileLayer};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Merges multiple vector tile sources.
/// Each resulting tile will contain all the features and properties from all the sources.
struct Args {
	/// All tile sources must provide vector tiles.
	sources: Vec<VPLPipeline>,
}

/// [`TileSource`] implementation that merges vector tiles "on the fly."
///
/// * Keeps only metadata in memory; actual tile data stream straight through.
/// * Performs no disk I/O itself – it relies entirely on the child pipelines.
#[derive(Debug)]
struct Operation {
	metadata: TileSourceMetadata,
	sources: Arc<Vec<Box<dyn TileSource>>>,
	tilejson: TileJSON,
}

/// Combine several `VectorTile`s by merging layers with identical names.
///
/// If multiple sources provide a layer called `"roads"`, all road features
/// end up in the same output layer; layers unique to a source are copied as‐is.
#[context("Failed to merge vector tiles")]
fn merge_vector_tiles(tiles: Vec<VectorTile>) -> Result<VectorTile> {
	let mut layers = HashMap::<String, VectorTileLayer>::new();
	for tile in tiles {
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

/// Merge the source tiles for one coordinate into a single output [`Tile`].
///
/// Fast path: when only one source contributed this coordinate there is nothing to
/// merge, so the original tile is returned untouched — avoiding the decode + re-encode
/// round-trip entirely and preserving its exact bytes. (The writer re-compresses to the
/// target compression anyway, so a differing source compression is harmless.)
///
/// Slow path: decode every source tile, concatenate same-named layers via
/// [`merge_vector_tiles`], and re-encode to `format`.
#[context("Failed to merge tiles")]
fn merge_tiles(mut tiles: Vec<Tile>, format: TileFormat) -> Result<Tile> {
	if tiles.len() == 1 {
		return Ok(tiles.pop().expect("len == 1"));
	}

	let vector_tiles = tiles
		.into_iter()
		.map(Tile::into_vector)
		.collect::<Result<Vec<VectorTile>>>()?;
	Tile::from_vector(merge_vector_tiles(vector_tiles)?, format)
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_merged_vector operation")]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let sources = join_all(args.sources.into_iter().map(|c| factory.build_pipeline(c)))
			.await
			.into_iter()
			.collect::<Result<Vec<_>>>()?;

		ensure!(sources.len() > 1, "must have at least two sources");

		let mut tilejson = TileJSON::merge_all(sources.iter().map(|s| s.tilejson()))?;
		let first_parameters = sources.first().expect("already ensured sources.len() > 1").metadata();
		let tile_format = *first_parameters.tile_format();
		let tile_compression = *first_parameters.tile_compression();
		let mut pyramid = TilePyramid::new_empty();
		let mut traversal = Traversal::ANY;

		for source in &sources {
			let metadata = source.metadata();
			traversal.intersect(metadata.traversal())?;
			let src_pyramid = source.tile_pyramid().await?;
			pyramid.union(src_pyramid.as_ref());

			ensure!(
				metadata.tile_format().to_type() == TileType::Vector,
				"all sources must be vector tiles"
			);
		}

		let metadata = TileSourceMetadata::new(tile_format, tile_compression, traversal, Some(pyramid));
		metadata.update_tilejson(&mut tilejson);

		Ok(Box::new(Self {
			metadata,
			sources: Arc::new(sources),
			tilejson,
		}) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	/// Reader parameters (format, compression, pyramid) for the merged result.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// `TileJSON` after combining metadata from every source.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		let source_types: Vec<Arc<SourceType>> = self.sources.iter().map(|s| s.source_type()).collect();
		SourceType::new_composite("from_merged_vector", &source_types)
	}

	async fn tile_pyramid(&self) -> Result<Arc<TilePyramid>> {
		self
			.metadata
			.tile_pyramid()
			.ok_or_else(|| anyhow::anyhow!("tile_pyramid not set"))
	}

	#[context("Failed to get merged tile coord stream for bbox: {:?}", bbox)]
	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		let refs: Vec<&dyn TileSource> = self.sources.iter().map(|s| s.as_ref() as &dyn TileSource).collect();
		super::traits::union_tile_coord_streams(&refs, bbox).await
	}

	/// Stream merged vector tiles for every coordinate in `bbox`.
	///
	/// Two stages so the CPU work scales across cores while peak memory stays bounded:
	/// 1. **I/O** — read the raw, still-encoded source tiles per coordinate, one grid
	///    chunk at a time. At most `MERGE_READ_AHEAD` chunks are read concurrently and
	///    the chunk size is derived from a tile budget (see [`merge_grid_size`]), so the
	///    number of raw tiles held in memory is capped regardless of the bbox size —
	///    important because a single tile can be large and there may be many sources.
	/// 2. **CPU** — decode + merge + re-encode each coordinate's tiles on the blocking
	///    pool in parallel (`map_parallel`); single-source coordinates skip the
	///    decode/encode round-trip entirely (see [`merge_tiles`]).
	#[context("Failed to get merged tile stream for bbox: {:?}", bbox)]
	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("from_merged_vector::tile_stream {bbox:?}");
		// Each coordinate holds one tile per source until merged.
		let grid_size = super::traits::chunk_grid_size(self.sources.len());
		let bboxes: Vec<TileBBox> = bbox.iter_grid(grid_size).collect();
		let sources = Arc::clone(&self.sources);
		let format = *self.metadata.tile_format();

		// Stage 1: read raw source tiles per chunk (sources kept in order for a
		// deterministic merge). Bounded read-ahead caps resident raw tiles to
		// `READ_AHEAD × grid_size² × n_sources ≤ max_tiles_in_flight()`.
		let groups = TileStream::from_streams_bounded(
			stream::iter(bboxes).map(move |chunk_bbox| {
				let sources = Arc::clone(&sources);
				async move {
					let mut tiles = TileBBoxMap::<Vec<Tile>>::new_default(chunk_bbox).expect("grid cell fits in usize");

					for source in sources.iter() {
						source
							.tile_stream(chunk_bbox)
							.await
							.expect("tile_stream succeeded for requested bbox")
							.for_each(|coord, tile| {
								tiles.get_mut(&coord).expect("coord is within bbox").push(tile);
							})
							.await;
					}

					TileStream::from_vec(
						tiles
							.into_iter()
							.filter_map(|(coord, vec_tiles)| {
								if vec_tiles.is_empty() {
									None
								} else {
									Some((coord, vec_tiles))
								}
							})
							.collect(),
					)
				}
			}),
			super::traits::READ_AHEAD,
		);

		// Stage 2: merge in parallel across cores. Single-source coordinates skip the
		// decode/encode round-trip (see `merge_tiles`); only true overlaps are re-encoded.
		Ok(groups.map_parallel(move |_coord, vec_tiles| merge_tiles(vec_tiles, format).expect("valid tile merge")))
	}
}

crate::operations::macros::define_read_factory!("from_merged_vector", Args, Operation);

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::helpers::{arrange_tiles, dummy_vector_source::DummyVectorSource};
	use futures::future::BoxFuture;
	use itertools::Itertools;
	use pretty_assertions::assert_eq;
	use versatiles_container::{DataLocation, TileSource};
	use versatiles_core::{Blob, TileCompression, TileFormat, TilePyramid};

	pub fn check_tile(blob: &Blob) -> String {
		let tile = VectorTile::from_blob(blob).unwrap();
		assert_eq!(tile.layers.len(), 1);

		let layer = &tile.layers[0];
		assert_eq!(layer.name, "dummy");

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
				factory
					.operation_from_vpl(command)
					.await
					.unwrap_err()
					.chain()
					.last()
					.unwrap()
					.to_string(),
				"must have at least two sources"
			);
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
				.chain()
				.map(std::string::ToString::to_string)
				.collect::<Vec<_>>(),
			[
				"Failed to create reader from VPL",
				"Failed to build pipeline from VPL",
				"Failed to create read operation from VPL node",
				"Failed to build from_merged_vector operation",
				"The 'from_merged_vector' operation does not have a parameter 'color'.\nSupported parameters: 'sources'"
			]
		);
	}

	#[tokio::test]
	async fn test_tilejson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_merged_vector [ from_container filename=1.pbf, from_container filename=2.pbf ]")
			.await?;

		assert_eq!(
			result.tilejson().to_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [-180, -85.051129, 180, 85.051129],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy vector source\",",
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
	async fn test_operation_tile_stream() -> Result<()> {
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
		let tiles = result.tile_stream(bbox).await?.to_vec().await;

		assert_eq!(
			arrange_tiles(tiles, |tile| {
				match check_tile(&tile.into_blob(&TileCompression::Uncompressed).unwrap()).as_str() {
					"A.pbf" => "🟦",
					"B.pbf" => "🟨",
					"A.pbf,B.pbf" => "🟩",
					e => panic!("Unexpected tile: {e}"),
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
			result.tilejson().to_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [-130, -70, 130, 70],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy vector source\",",
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
		let factory = PipelineFactory::new_dummy_reader(Box::new(
			|location: DataLocation| -> BoxFuture<Result<Box<dyn TileSource>>> {
				Box::pin(async move {
					let mut pyramide = TilePyramid::new_empty();
					let filename = location.to_string();
					for c in filename[0..filename.len() - 4].chars() {
						pyramide.insert_bbox(&TileBBox::new_full(c.to_digit(10).unwrap() as u8)?)?;
					}
					Ok(Box::new(DummyVectorSource::new(
						&[("dummy", &[&[("filename", &filename)]])],
						Some(pyramide),
					)) as Box<dyn TileSource>)
				})
			},
		));

		let result = factory
			.operation_from_vpl(
				r#"from_merged_vector [ from_container filename="12.pbf", from_container filename="23.pbf" ]"#,
			)
			.await?;

		let parameters = result.metadata();

		assert_eq!(*parameters.tile_format(), TileFormat::MVT);
		assert_eq!(*parameters.tile_compression(), TileCompression::Uncompressed);
		assert_eq!(
			format!("{}", parameters.tile_pyramid().unwrap()),
			"[TileQuadtree { level: 1, root: Full }, TileQuadtree { level: 2, root: Full }, TileQuadtree { level: 3, root: Full }]"
		);

		assert_eq!(
			result.tilejson().to_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [-180, -85.051129, 180, 85.051129],",
				"  \"maxzoom\": 3,",
				"  \"minzoom\": 1,",
				"  \"name\": \"dummy vector source\",",
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
	async fn test_tilejson_combines_attribution_across_sources() -> Result<()> {
		// Each source advertises a different attribution; the merged source must
		// keep both credits (the bug was overwriting to a single one).
		let factory = PipelineFactory::new_dummy_reader(Box::new(
			|location: DataLocation| -> BoxFuture<Result<Box<dyn TileSource>>> {
				Box::pin(async move {
					let filename = location.to_string();
					let attribution = format!("© {filename}");
					Ok(Box::new(
						DummyVectorSource::new(&[("dummy", &[&[("filename", &filename)]])], None)
							.with_attribution(&attribution),
					) as Box<dyn TileSource>)
				})
			},
		));

		let result = factory
			.operation_from_vpl(
				r#"from_merged_vector [ from_container filename="a.pbf", from_container filename="b.pbf" ]"#,
			)
			.await?;

		assert_eq!(
			result.tilejson().string("attribution"),
			Some("© a.pbf · © b.pbf".to_string())
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

	#[test]
	fn test_merge_tiles_single_source_passes_through() -> Result<()> {
		// A coordinate covered by a single source must be returned untouched —
		// no decode/re-encode round-trip.
		let tile = Tile::from_vector(
			VectorTile::new(vec![VectorTileLayer::new_standard("layer1")]),
			TileFormat::MVT,
		)?;
		let merged = merge_tiles(vec![tile.clone()], TileFormat::MVT)?;
		assert_eq!(merged, tile, "single-source tile should pass through unchanged");
		Ok(())
	}

	#[test]
	fn test_merge_tiles_multiple_sources_are_combined() -> Result<()> {
		let tile1 = Tile::from_vector(
			VectorTile::new(vec![VectorTileLayer::new_standard("layer1")]),
			TileFormat::MVT,
		)?;
		let tile2 = Tile::from_vector(
			VectorTile::new(vec![VectorTileLayer::new_standard("layer2")]),
			TileFormat::MVT,
		)?;
		let merged = merge_tiles(vec![tile1, tile2], TileFormat::MVT)?.into_vector()?;
		assert_eq!(merged.layers.len(), 2);
		assert!(merged.layers.iter().any(|l| l.name == "layer1"));
		assert!(merged.layers.iter().any(|l| l.name == "layer2"));
		Ok(())
	}
}
