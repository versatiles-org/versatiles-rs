//! # from_stacked_raster operation
//!
//! Combines *multiple* raster tile sources by **alpha‑blending** the tiles for
//! each coordinate.  
//!  
//! * Sources are evaluated **in the order given** – later sources overlay
//!   earlier ones.  
//! * Every source **must** produce raster tiles in the *same* resolution.  
//!
//! This file contains both the [`Args`] struct used by the VPL parser and the
//! [`Operation`] implementation that performs the blending.

use crate::{
	PipelineFactory,
	operations::read::traits::ReadTileSource,
	traits::{OperationFactoryTrait, ReadOperationFactoryTrait},
	vpl::{VPLNode, VPLPipeline},
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::{sync::Arc, vec};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
use versatiles_core::{TileBBox, TileBBoxPyramid, TileCoord, TileFormat, TileJSON, TileStream, TileType};
use versatiles_derive::context;
use versatiles_image::traits::{DynamicImageTraitInfo, DynamicImageTraitOperation};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Overlays multiple raster tile sources on top of each other.
struct Args {
	/// All tile sources must provide raster tiles in the same resolution.
	/// The first source overlays the others.
	sources: Vec<VPLPipeline>,

	/// The tile format to use for the output tiles.
	/// Default: format of the first source.
	format: Option<TileFormat>,

	/// Whether to automatically overscale tiles when a source does not
	/// provide tiles at the requested zoom level.
	/// Default: `false`.
	auto_overscale: Option<bool>,
}

/// [`TileSource`] implementation that overlays raster tiles "on the fly."
///
/// * Caches only metadata (`TileJSON`, `TileSourceMetadata`).
/// * Performs no disk I/O itself; all data come from the child pipelines.
#[derive(Debug)]
struct Operation {
	metadata: TileSourceMetadata,
	sources: Vec<(Arc<Box<dyn TileSource>>, u8)>,
	source_types: Vec<Arc<SourceType>>,
	tilejson: TileJSON,
}

/// Fetches tiles from all sources for a sub-bbox, collects them, stacks overlapping
/// tiles, and returns a stream of the resulting tiles.
#[allow(unused_variables)]
async fn get_tile(
	coord: TileCoord,
	sources: Vec<(Arc<Box<dyn TileSource>>, bool)>,
) -> Result<Option<(TileCoord, Tile)>> {
	let mut tile = Option::<Tile>::None;
	let mut non_overscaled_sources_used = false;

	for source in &sources {
		if let Some(mut tile_bg) = source.0.get_tile(&coord).await? {
			if tile_bg.as_image()?.is_empty() {
				continue;
			}
			if !source.1 {
				non_overscaled_sources_used = true;
			}
			if let Some(mut image_fg) = tile {
				tile_bg.as_image_mut()?.overlay(image_fg.as_image()?)?;
			}
			tile = Some(tile_bg);
			if tile.as_mut().unwrap().as_image()?.is_opaque() {
				break;
			}
		}
	}

	if !non_overscaled_sources_used {
		return Ok(None);
	}

	Ok(tile.map(|t| (coord, t)))
}

impl ReadTileSource for Operation {
	#[context("Failed to build from_stacked_raster operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, factory: &PipelineFactory) -> Result<Box<dyn TileSource>>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		let mut original_sources: Vec<Box<dyn TileSource>> = vec![];
		let mut source_types: Vec<Arc<SourceType>> = vec![];
		for source in args.sources {
			let s = factory.build_pipeline(source).await?;
			source_types.push(s.source_type());
			original_sources.push(s);
		}

		ensure!(!original_sources.is_empty(), "must have at least one source");

		let mut tilejson = TileJSON::default();

		let first_source_metadata = original_sources.first().unwrap().metadata();
		let tile_format = args.format.unwrap_or(first_source_metadata.tile_format);
		ensure!(
			tile_format.to_type() == TileType::Raster,
			"output format must be a raster format"
		);
		let tile_compression = first_source_metadata.tile_compression;

		let mut pyramid = TileBBoxPyramid::new_empty();
		let mut traversal = Traversal::new_any();

		for source in &original_sources {
			tilejson.merge(source.tilejson())?;

			let metadata = source.metadata();
			traversal.intersect(&metadata.traversal)?;
			pyramid.include_bbox_pyramid(&metadata.bbox_pyramid);

			ensure!(
				metadata.tile_format.to_type() == TileType::Raster,
				"all sources must be raster tiles"
			);
		}

		let metadata = TileSourceMetadata::new(tile_format, tile_compression, pyramid, traversal);
		metadata.update_tilejson(&mut tilejson);
		let level_max = metadata.bbox_pyramid.get_level_max().unwrap();

		let auto_overscale = args.auto_overscale.unwrap_or(false);
		let mut sources: Vec<(Arc<Box<dyn TileSource>>, u8)> = vec![];
		if auto_overscale {
			use crate::operations::raster::raster_overscale;

			for source in original_sources {
				let level_base = source.metadata().bbox_pyramid.get_level_max().unwrap();
				let args = raster_overscale::Args {
					level_base: Some(level_base),
					level_max: Some(level_max),
					enable_climbing: Some(false),
				};
				let operation = raster_overscale::Operation::new(source, args)?;
				sources.push((Arc::new(Box::new(operation)), level_base));
			}
		} else {
			for source in original_sources {
				sources.push((Arc::new(source), 30));
			}
		}

		Ok(Box::new(Self {
			metadata,
			sources,
			source_types,
			tilejson,
		}) as Box<dyn TileSource>)
	}
}

#[async_trait]
impl TileSource for Operation {
	/// Reader parameters (format, compression, pyramid) for the *blended* result.
	fn metadata(&self) -> &TileSourceMetadata {
		&self.metadata
	}

	/// Combined `TileJSON` derived from all sources.
	fn tilejson(&self) -> &TileJSON {
		&self.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_composite("from_stacked_raster", &self.source_types)
	}

	/// Stream packed raster tiles intersecting `bbox`.
	#[context("Failed to get stacked raster tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<Tile>> {
		log::debug!("get_stream {bbox:?}");

		// Filter sources to only those that overlap with the bbox
		let sources: Vec<(Arc<Box<dyn TileSource>>, bool)> = self
			.sources
			.iter()
			.filter(|s| s.0.metadata().bbox_pyramid.overlaps_bbox(&bbox))
			.cloned()
			.map(|s| (s.0, s.1 < bbox.level))
			.collect();

		if sources.is_empty() {
			return Ok(TileStream::empty());
		}

		let tile_format = self.metadata.tile_format;

		Ok(TileStream::from_bbox_async_parallel(bbox, move |c| {
			let sources = sources.clone();
			async move {
				let tile = get_tile(c, sources).await.unwrap();
				if let Some((_coord, mut tile)) = tile {
					tile.change_format(tile_format, None, None).unwrap();
					return Some((c, tile));
				}
				tile
			}
		}))
	}
}

pub struct Factory {}

impl OperationFactoryTrait for Factory {
	fn get_docs(&self) -> String {
		Args::get_docs()
	}
	fn get_tag_name(&self) -> &str {
		"from_stacked_raster"
	}
}

#[async_trait]
impl ReadOperationFactoryTrait for Factory {
	async fn build<'a>(&self, vpl_node: VPLNode, factory: &'a PipelineFactory) -> Result<Box<dyn TileSource>> {
		Operation::build(vpl_node, factory).await
	}
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::helpers::{arrange_tiles, dummy_image_source::DummyImageSource};
	use futures::future::BoxFuture;
	use pretty_assertions::assert_eq;
	use rstest::rstest;
	use versatiles_container::TileSource;
	use versatiles_core::{Blob, TileCompression, TileCompression::Uncompressed, TileFormat};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert};

	pub fn get_color(blob: &Blob) -> String {
		let image = DynamicImage::from_blob(blob, TileFormat::PNG).unwrap();
		let pixel = image.iter_pixels().next().unwrap();
		pixel.iter().fold(String::new(), |mut acc, v| {
			use std::fmt::Write;
			write!(acc, "{v:02X}").unwrap();
			acc
		})
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
				"must have at least one source"
			);
		};

		error("from_stacked_raster").await;
		error("from_stacked_raster [ ]").await;
	}

	#[tokio::test]
	async fn test_tilejson() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_stacked_raster [ from_container filename=07.png, from_container filename=F7.png ]")
			.await?;

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [-180, -85.051129, 180, 85.051129],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
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
				r#"from_stacked_raster [
					from_container filename="00F7.png" | filter bbox=[-130,-20,20,70],
					from_container filename="FF07.png" | filter bbox=[-20,-70,130,20]
				]"#,
			)
			.await?;

		let bbox = TileBBox::new_full(3)?;
		let tiles = result.get_tile_stream(bbox).await?.to_vec().await;

		assert_eq!(
			arrange_tiles(tiles, |mut tile| {
				match get_color(tile.as_blob(Uncompressed).unwrap()).as_str() {
					"0000FF77" => "🟦",
					"FFFF0077" => "🟨",
					"5858A6B6" => "🟩",
					e => panic!("{}", e),
				}
				.to_string()
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
				"  \"bounds\": [-130.78125, -70.140364, 130.78125, 70.140364],",
				"  \"maxzoom\": 8,",
				"  \"minzoom\": 0,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	#[tokio::test]
	async fn test_operation_parameters() -> Result<()> {
		let factory =
			PipelineFactory::new_dummy_reader(Box::new(|filename: String| -> BoxFuture<Result<Box<dyn TileSource>>> {
				Box::pin(async move {
					let mut pyramide = TileBBoxPyramid::new_empty();
					for c in filename[0..filename.len() - 4].chars() {
						pyramide.include_bbox(&TileBBox::new_full(c.to_digit(10).unwrap() as u8)?);
					}
					Ok(
						Box::new(DummyImageSource::from_color(&[0, 0, 0], 4, TileFormat::PNG, Some(pyramide)).unwrap())
							as Box<dyn TileSource>,
					)
				})
			}));

		let result = factory
			.operation_from_vpl(
				r#"from_stacked_raster [ from_container filename="12.png", from_container filename="23.png" ]"#,
			)
			.await?;

		let parameters = result.metadata();

		assert_eq!(parameters.tile_format, TileFormat::PNG);
		assert_eq!(parameters.tile_compression, TileCompression::Uncompressed);
		assert_eq!(
			format!("{}", parameters.bbox_pyramid),
			"[1: [0,0,1,1] (2x2), 2: [0,0,3,3] (4x4), 3: [0,0,7,7] (8x8)]"
		);

		assert_eq!(
			result.tilejson().as_pretty_lines(100),
			[
				"{",
				"  \"bounds\": [-180, -85.051129, 180, 85.051129],",
				"  \"maxzoom\": 3,",
				"  \"minzoom\": 1,",
				"  \"name\": \"dummy raster source\",",
				"  \"tile_format\": \"image/png\",",
				"  \"tile_schema\": \"rgb\",",
				"  \"tile_type\": \"raster\",",
				"  \"tilejson\": \"3.0.0\"",
				"}"
			]
		);

		Ok(())
	}

	/// Tests tile blending with various layer configurations:
	/// - Single source (opaque and semi-transparent)
	/// - Two sources with transparent/semi-transparent/opaque foreground
	/// - Three sources with layered blending and short-circuit behavior
	#[rstest]
	#[case(&["FF0000FF"], "FF0000FF")] // single opaque red
	#[case(&["00FF0080"], "00FF0080")] // single semi-transparent green
	#[case(&["FF000000", "00FF00FF"], "00FF00FF")] // transparent over opaque
	#[case(&["FF000077", "00FF0077"], "A65800B6")] // semi-transparent blend
	#[case(&["FF0000FF", "00FF00FF"], "FF0000FF")] // opaque short-circuits
	#[case(&["FFFFFF80", "000000FF"], "808080FE")] // half-white over black
	#[case(&["FF000080", "00FF0080", "0000FFFF"], "7F3E3FFF")] // three-layer blend
	#[case(&["FF000080", "00FF00FF", "0000FFFF"], "7F7E00FF")] // middle opaque short-circuits
	#[tokio::test]
	async fn test_get_tile_multiple_layers(#[case] input: &[&str], #[case] expected: &str) {
		use versatiles_core::TileFormat::PNG;

		for s in input {
			assert_eq!(s.len(), 8);
		}

		let sources: Vec<(Arc<Box<dyn TileSource>>, bool)> = input
			.iter()
			.map(|s| {
				// convert hex string to RGBA color
				let c: Vec<u8> = (0..4)
					.map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
					.collect();
				let source = DummyImageSource::from_color(&c, 4, PNG, None).unwrap();
				(Arc::new(Box::new(source) as Box<dyn TileSource>), false)
			})
			.collect();

		let coord = TileCoord::new(0, 0, 0).unwrap();
		let result = get_tile(coord, sources).await.unwrap();
		let color = result.unwrap().1.as_image().unwrap().average_color();
		let color_string = color.iter().fold(String::new(), |mut acc, v| {
			use std::fmt::Write;
			write!(acc, "{v:02X}").unwrap();
			acc
		});

		assert_eq!(color_string, expected);
	}

	#[tokio::test]
	async fn test_reuses_original_blob_single_source() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let stacked = factory
			.operation_from_vpl("from_stacked_raster [ from_container filename=00F7.png ]")
			.await?;
		// Build the plain source to compare raw tiles
		let plain = factory.operation_from_vpl("from_container filename=00F7.png").await?;

		let bbox = TileBBox::new_full(3)?;
		let stacked_tiles = stacked.get_tile_stream(bbox).await?.to_vec().await;
		let plain_tiles = plain.get_tile_stream(bbox).await?.to_vec().await;

		// Convert to maps for easy lookup
		use std::collections::HashMap;
		let mut map_stacked: HashMap<_, _> = stacked_tiles.into_iter().collect();
		let map_plain: HashMap<_, _> = plain_tiles.into_iter().collect();

		// For every key present in the plain source, the stacked version must be byte-identical
		for (coord, mut tile_plain) in map_plain {
			if let Some(mut tile_stacked) = map_stacked.remove(&coord) {
				assert_eq!(
					tile_stacked.as_blob(Uncompressed).unwrap(),
					tile_plain.as_blob(Uncompressed).unwrap()
				);
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_reencodes_on_blend() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let stacked = factory
			.operation_from_vpl(
				"from_stacked_raster [ from_container filename=00F7.png, from_container filename=FF07.png ]",
			)
			.await?;
		let src1 = factory.operation_from_vpl("from_container filename=00F7.png").await?;
		let src2 = factory.operation_from_vpl("from_container filename=FF07.png").await?;

		let bbox = TileBBox::new_full(3)?;
		let coord = TileCoord::new(3, 2, 2)?; // a tile that lies in the overlap area in our dummy dataset

		let stacked_tile = stacked.get_tile_stream(bbox).await?.to_map().await.remove(&coord);
		let tile1 = src1.get_tile_stream(bbox).await?.to_map().await.remove(&coord);
		let tile2 = src2.get_tile_stream(bbox).await?.to_map().await.remove(&coord);

		if let Some(mut stacked_tile) = stacked_tile {
			// If both sources produced a tile here, blended output must differ from each single-source blob
			if let (Some(mut tile1), Some(mut tile2)) = (tile1, tile2) {
				assert_ne!(
					stacked_tile.as_blob(Uncompressed).unwrap(),
					tile1.as_blob(Uncompressed).unwrap()
				);
				assert_ne!(
					stacked_tile.as_blob(Uncompressed).unwrap(),
					tile2.as_blob(Uncompressed).unwrap()
				);
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_get_tile_empty_sources_returns_none() {
		let sources: Vec<(Arc<Box<dyn TileSource>>, bool)> = vec![];
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let result = get_tile(coord, sources).await.unwrap();
		assert!(result.is_none());
	}
}
