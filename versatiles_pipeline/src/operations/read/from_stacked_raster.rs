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
	vpl::{VPLNode, VPLPipeline},
};
use anyhow::{Result, ensure};
use async_trait::async_trait;
use std::{sync::Arc, vec};
use versatiles_container::{SharedTileSource, SourceType, Tile, TileSource, TileSourceMetadata, Traversal};
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

/// When `auto_overscale` is disabled, this sentinel value indicates that
/// all tiles are considered "native" (never overscaled). Using `u8::MAX`
/// ensures the comparison `native_level_max < request_level` is always false.
const NO_OVERSCALE_LEVEL: u8 = u8::MAX;

/// A wrapped tile source with its native zoom level metadata.
///
/// Used to track whether tiles from this source are "native" (from the source's
/// actual data) or "synthetic" (upscaled from a lower zoom level).
#[derive(Clone, Debug)]
struct SourceEntry {
	/// The tile source (possibly wrapped with `raster_overscale`)
	source: SharedTileSource,
	/// The maximum zoom level where this source has native (non-upscaled) tiles.
	/// For requests above this level, tiles are considered "overscaled".
	native_level_max: u8,
}

impl SourceEntry {
	/// Returns `true` if a tile at the given `level` would be synthetic (overscaled).
	fn is_overscaled(&self, level: u8) -> bool {
		self.native_level_max < level
	}

	/// Creates a `FilteredSourceEntry` for use at a specific zoom level.
	fn for_level(&self, level: u8) -> FilteredSourceEntry {
		FilteredSourceEntry {
			source: self.source.clone(),
			is_overscaled: self.is_overscaled(level),
		}
	}
}

/// A tile source with precomputed overscale status for a specific zoom level.
///
/// Created from [`SourceEntry`] when processing tiles at a particular level.
/// This avoids repeatedly computing the overscale status during tile iteration.
#[derive(Clone)]
struct FilteredSourceEntry {
	source: SharedTileSource,
	is_overscaled: bool,
}

/// [`TileSource`] implementation that overlays raster tiles "on the fly."
///
/// * Caches only metadata (`TileJSON`, `TileSourceMetadata`).
/// * Performs no disk I/O itself; all data come from the child pipelines.
#[derive(Debug)]
struct Operation {
	metadata: TileSourceMetadata,
	sources: Vec<SourceEntry>,
	source_types: Vec<Arc<SourceType>>,
	tilejson: TileJSON,
}

/// Fetches and blends tiles from all sources for a single coordinate.
///
/// Sources are processed in order (first source is background, later sources overlay).
/// Returns `None` if no native (non-overscaled) source contributed a tile, preventing
/// purely synthetic tiles from being generated where no real source data exists.
///
/// # Arguments
/// * `coord` - The tile coordinate to fetch
/// * `entries` - Sources with precomputed overscale status for this request level
async fn get_tile(coord: TileCoord, entries: Vec<FilteredSourceEntry>) -> Result<Option<(TileCoord, Tile)>> {
	let mut tile = Option::<Tile>::None;
	let mut has_native_tile = false;

	for entry in &entries {
		if let Some(mut tile_bg) = entry.source.get_tile(&coord).await? {
			if !entry.is_overscaled {
				has_native_tile = true;
			}
			if let Some(mut tile_fg) = tile {
				if tile_bg.as_image()?.is_empty() {
					tile_bg = tile_fg;
				} else {
					tile_bg.as_image_mut()?.overlay(tile_fg.as_image()?)?;
				}
			}
			tile = Some(tile_bg);
			if tile.as_mut().unwrap().as_image()?.is_opaque() {
				break;
			}
		}
	}

	// Only return a tile if at least one native (non-overscaled) source contributed.
	// This prevents generating purely synthetic tiles where no real data exists.
	if !has_native_tile {
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

		// Build source entries, optionally wrapping each source with raster_overscale
		let auto_overscale = args.auto_overscale.unwrap_or(false);
		let mut sources: Vec<SourceEntry> = vec![];

		if auto_overscale {
			use crate::operations::raster::raster_overscale;

			for source in original_sources {
				let native_level_max = source.metadata().bbox_pyramid.get_level_max().unwrap();
				let overscale_args = raster_overscale::Args {
					level_base: Some(native_level_max),
					level_max: Some(level_max),
					enable_climbing: Some(true),
				};
				let wrapped_source = raster_overscale::Operation::new(source, &overscale_args)?;
				sources.push(SourceEntry {
					source: Arc::new(Box::new(wrapped_source)),
					native_level_max,
				});
			}
		} else {
			// Without auto_overscale, all tiles are considered native
			for source in original_sources {
				sources.push(SourceEntry {
					source: Arc::new(source),
					native_level_max: NO_OVERSCALE_LEVEL,
				});
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
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::debug!("get_tile_stream {bbox:?}");

		// Filter sources to only those that overlap with the bbox,
		// and precompute whether each source is overscaled at this level
		let mut entries: Vec<FilteredSourceEntry> = self
			.sources
			.iter()
			.filter(|entry| entry.source.metadata().bbox_pyramid.overlaps_bbox(&bbox))
			.map(|entry| entry.for_level(bbox.level))
			.collect();

		if entries.is_empty() {
			return Ok(TileStream::empty());
		}

		// Return empty stream if no native (non-overscaled) source exists
		if entries.iter().all(|entry| entry.is_overscaled) {
			return Ok(TileStream::empty());
		}

		if entries.len() == 1 {
			let source = entries.pop().unwrap().source;
			let stream = source.get_tile_stream(bbox).await?;
			return Ok(stream);
		}

		let tile_format = self.metadata.tile_format;

		Ok(TileStream::from_bbox_async_parallel(bbox, move |c| {
			let entries = entries.clone();
			async move {
				let tile = get_tile(c, entries).await.unwrap();
				if let Some((_coord, mut tile)) = tile {
					tile.change_format(tile_format, None, None).unwrap();
					return Some((c, tile));
				}
				tile
			}
		}))
	}
}

crate::operations::macros::define_read_factory!("from_stacked_raster", Args, Operation);

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

	fn rgba_to_hex(rgba: &[u8]) -> String {
		use std::fmt::Write;
		rgba.iter().fold(String::new(), |mut acc, v| {
			write!(acc, "{v:02X}").unwrap();
			acc
		})
	}

	pub fn get_color(blob: &Blob) -> String {
		let image = DynamicImage::from_blob(blob, TileFormat::PNG).unwrap();
		let pixel = image.iter_pixels().next().unwrap();
		rgba_to_hex(pixel)
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

		let entries: Vec<FilteredSourceEntry> = input
			.iter()
			.map(|s| {
				// convert hex string to RGBA color
				let c: Vec<u8> = (0..4)
					.map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).unwrap())
					.collect();
				let source = DummyImageSource::from_color(&c, 4, PNG, None).unwrap();
				FilteredSourceEntry {
					source: Arc::new(Box::new(source) as Box<dyn TileSource>),
					is_overscaled: false,
				}
			})
			.collect();

		let coord = TileCoord::new(0, 0, 0).unwrap();
		let result = get_tile(coord, entries).await.unwrap();
		let color = result.unwrap().1.as_image().unwrap().average_color();
		assert_eq!(rgba_to_hex(&color), expected);
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
		let entries: Vec<FilteredSourceEntry> = vec![];
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let result = get_tile(coord, entries).await.unwrap();
		assert!(result.is_none());
	}

	// ============================================================================
	// Auto-overscale tests
	// ============================================================================

	#[rstest]
	// native_level_max = 8: levels 0-8 are native, 9+ are overscaled
	#[case(8, &[(0, false), (7, false), (8, false), (9, true), (15, true)])]
	// sentinel value: nothing is ever overscaled
	#[case(NO_OVERSCALE_LEVEL, &[(0, false), (30, false), (254, false)])]
	#[test]
	fn test_source_entry_is_overscaled(#[case] native_level_max: u8, #[case] expectations: &[(u8, bool)]) {
		use versatiles_core::TileFormat::PNG;

		let source = DummyImageSource::from_color(&[255, 0, 0], 4, PNG, None).unwrap();
		let entry = SourceEntry {
			source: Arc::new(Box::new(source) as Box<dyn TileSource>),
			native_level_max,
		};

		for &(level, expected) in expectations {
			assert_eq!(entry.is_overscaled(level), expected, "level {level}");
		}
	}

	#[rstest]
	#[case(&[true], false)] // all overscaled -> None
	#[case(&[true, true], false)] // all overscaled -> None
	#[case(&[false], true)] // single native source -> Some
	#[case(&[false, true], true)] // mixed (overscaled + native) -> Some
	#[case(&[true, false], true)] // mixed (overscaled + native) -> Some
	#[case(&[false, false], true)] // all native -> Some
	#[tokio::test]
	async fn test_get_tile_native_overscale_behavior(#[case] overscaled_flags: &[bool], #[case] expect_some: bool) {
		use versatiles_core::TileFormat::PNG;

		let entries: Vec<FilteredSourceEntry> = overscaled_flags
			.iter()
			.enumerate()
			.map(|(i, &is_overscaled)| {
				// Use semi-transparent for non-last sources to avoid short-circuiting
				let alpha = if i < overscaled_flags.len() - 1 { 128 } else { 255 };
				let source = DummyImageSource::from_color(&[255, 0, 0, alpha], 4, PNG, None).unwrap();
				FilteredSourceEntry {
					source: Arc::new(Box::new(source) as Box<dyn TileSource>),
					is_overscaled,
				}
			})
			.collect();

		let coord = TileCoord::new(0, 0, 0).unwrap();
		let result = get_tile(coord, entries).await.unwrap();
		assert_eq!(result.is_some(), expect_some);
	}

	#[rstest]
	#[case("true")]
	#[case("false")]
	#[tokio::test]
	async fn test_auto_overscale_vpl(#[case] value: &str) -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl(&format!(
				"from_stacked_raster auto_overscale={value} [ from_container filename=07.png ]"
			))
			.await?;
		assert_eq!(result.metadata().tile_format.to_type(), TileType::Raster);
		Ok(())
	}
}
