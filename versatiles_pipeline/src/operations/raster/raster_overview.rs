use std::{fmt::Debug, sync::Arc};

use anyhow::Result;
use async_trait::async_trait;
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TileStream, ZoomLevel};
use versatiles_image::traits::DynamicImageTraitOperation;

use crate::{PipelineFactory, helpers::overview::OverviewCore, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generates the lower zoom levels of a raster pyramid by downscaling.
struct Args {
	/// Zoom level to build the overview from. Defaults to the source's highest.
	level: Option<ZoomLevel>,
}

#[derive(Debug)]
struct Operation {
	core: OverviewCore,
}

impl Operation {
	#[expect(
		clippy::unused_async,
		clippy::unused_async_trait_impl,
		reason = "the factory trait declares `build` async; this implementation has nothing to await"
	)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;
		let core = OverviewCore::new(
			source,
			args.level.map(u8::from),
			Arc::new(|img| img.scaled_down(2)),
			factory.runtime(),
		)?;

		Ok(Self { core })
	}
}

#[async_trait]
impl TileSource for Operation {
	fn metadata(&self) -> &TileSourceMetadata {
		&self.core.metadata
	}

	fn tilejson(&self) -> &TileJSON {
		&self.core.tilejson
	}

	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("raster_overview", self.core.source.source_type())
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("raster_overview::tile_stream {bbox:?}");
		self.core.tile_stream(bbox).await
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.core.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("raster_overview", Args, Operation, requires: Raster);

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
mod tests {
	use imageproc::image::{DynamicImage, GenericImage, GenericImageView, Rgb, RgbImage, Rgba};
	use versatiles_container::{Traversal, TraversalOrder, testing::assert_stream_counts_agree};
	use versatiles_core::{Blob, GeoBBox, TileCompression, TileCoord, TileFormat, TilePyramid};

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_image_source::DummyImageSource};

	/// The extent every fixture is built over: central Paris.
	fn paris() -> GeoBBox {
		GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap()
	}

	async fn make_operation(tile_size: u32, level_base: u8) -> Operation {
		let pyramid = TilePyramid::from_geo_bbox(level_base, level_base, &paris()).unwrap();

		return Operation::build(
			VPLNode::try_from_str(&format!("raster_overview level={level_base}")).unwrap(),
			Box::new(DummyImageSource::from_color(&[255, 0, 0], tile_size, TileFormat::PNG, Some(pyramid)).unwrap()),
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap();
	}

	/// An operation over a source whose tiles differ per coordinate.
	///
	/// `make_operation` paints every tile the same solid red, which cannot tell
	/// a correctly composed overview from a wrongly composed one — every
	/// arrangement of identical quadrants encodes to identical bytes. These
	/// fixtures exist to compare tiles, so the source has to vary.
	async fn make_varied_operation(tile_size: u32, level_base: u8) -> Operation {
		let pyramid = TilePyramid::from_geo_bbox(level_base, level_base, &paris()).unwrap();

		let mut source = DummyImageSource::new(
			move |coord| {
				let color = Rgb([
					(coord.x % 251) as u8,
					(coord.y % 251) as u8,
					coord.level.wrapping_mul(8),
				]);
				let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(tile_size, tile_size, color));
				Tile::from_image(image, TileFormat::PNG).ok()
			},
			TileFormat::PNG,
			Some(pyramid),
		)
		.unwrap();
		source.tilejson_mut().set_tile_size(tile_size).unwrap();

		Operation::build(
			VPLNode::try_from_str(&format!("raster_overview level={level_base}")).unwrap(),
			Box::new(source),
			&PipelineFactory::new_dummy(),
		)
		.await
		.unwrap()
	}

	/// Stream every block `traversal` asks for, in the order it asks for it,
	/// down to `level_stop`.
	async fn pass_in(op: &Operation, traversal: &Traversal, level_stop: u8) -> Result<Vec<(TileCoord, Tile)>> {
		let pyramid = op.metadata().tile_pyramid().unwrap();
		let bboxes = traversal.traverse_pyramid(&pyramid)?;

		let mut tiles = Vec::new();
		for bbox in bboxes {
			if bbox.level() < level_stop {
				continue;
			}
			tiles.extend(op.tile_stream(bbox).await?.to_vec().await);
		}
		Ok(tiles)
	}

	/// The pass a converter performs: the order the operation asks to be read
	/// in, which is what `traverse_all_tiles` picks when the destination leaves
	/// the choice open.
	async fn depth_first_pass(op: &Operation, level_stop: u8) -> Result<Vec<(TileCoord, Tile)>> {
		let metadata = op.metadata().clone();
		let traversal = metadata
			.preferred_traversal()
			.unwrap_or_else(|| metadata.traversal())
			.clone();
		pass_in(op, &traversal, level_stop).await
	}

	/// The northwest tile the operation covers at `level`.
	fn min_tile_at(op: &Operation, level: u8) -> Result<TileCoord> {
		op.metadata()
			.tile_pyramid()
			.unwrap()
			.level_ref(level)
			.to_bbox()
			.min_tile()
	}

	/// How many blocks the cache is holding across both its tiers.
	async fn cached_blocks(op: &Operation) -> u64 {
		op.core.cache.entry_count().await
	}

	/// The encoded bytes of a tile, for comparing two tiles for equality.
	fn bytes_of(tile: Tile) -> Result<Blob> {
		tile.into_blob(&TileCompression::Uncompressed)
	}

	// ---------------------------------------------------------------------
	// Phase 0 regressions: random tile access.
	//
	// The operation composes each overview level from a staging cache that the
	// level above *removes* as it consumes it, so a tile is only ever produced
	// for a consumer walking the pyramid in post-order depth-first. Every test
	// below describes behaviour a `TileSource` is expected to have, and every
	// one of them fails on the current implementation.
	// ---------------------------------------------------------------------

	/// Depth-first is what this operation *wants*, not what it demands.
	///
	/// Declaring it as a requirement is what used to force a PMTiles archive to
	/// give up clustering or write its tile data twice, for an order the
	/// operation can do without — expensively, but correctly.
	#[tokio::test]
	async fn the_operation_prefers_depth_first_without_requiring_it() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata();

		assert!(
			metadata.traversal().is_any(),
			"the operation should accept tiles being asked for in any order"
		);

		let preferred = metadata.preferred_traversal().expect("a preferred order");
		assert_eq!(preferred.order(), &TraversalOrder::DepthFirst);
		assert_eq!(preferred.min_size()?, 16);
		assert_eq!(preferred.max_size()?, 16);

		Ok(())
	}

	/// Reading in an order the operation did not ask for still gives the same
	/// tiles — only slower.
	///
	/// This is what a PMTiles archive does: it needs Hilbert order, which no
	/// amount of preference can change. It is allowed to, and the result has to
	/// be indistinguishable.
	#[tokio::test]
	async fn a_different_order_produces_the_same_tiles() -> Result<()> {
		let preferred = make_varied_operation(256, 6).await;
		let mut from_preferred = depth_first_pass(&preferred, 0).await?;

		let hilbert = make_varied_operation(256, 6).await;
		let mut from_hilbert = pass_in(&hilbert, &Traversal::new(TraversalOrder::PMTiles, 1, 16)?, 0).await?;

		assert!(!from_preferred.is_empty(), "the pass should have produced tiles");
		assert_eq!(
			from_preferred.len(),
			from_hilbert.len(),
			"the two orders produced different numbers of tiles"
		);

		from_preferred.sort_by_key(|(coord, _)| (coord.level, coord.x, coord.y));
		from_hilbert.sort_by_key(|(coord, _)| (coord.level, coord.x, coord.y));

		for ((coord_a, tile_a), (coord_b, tile_b)) in from_preferred.into_iter().zip(from_hilbert) {
			assert_eq!(coord_a, coord_b);
			assert_eq!(
				bytes_of(tile_a)?,
				bytes_of(tile_b)?,
				"{coord_a:?} differs between orders"
			);
		}

		Ok(())
	}

	/// A single tile below the base level, asked for out of the blue.
	///
	/// This is what a tile server does — `ServerTileSource::get_data` calls
	/// `tile()` per request — so on the current implementation `versatiles
	/// serve` answers every level below `level_base` with a blank tile.
	#[tokio::test]
	async fn a_single_tile_below_the_base_level_is_produced_on_demand() -> Result<()> {
		let op = make_operation(256, 6).await;
		let coord = min_tile_at(&op, 4)?;

		let tile = op.tile(&coord).await?;

		assert!(
			tile.is_some(),
			"{coord:?} lies inside the pyramid the operation advertises, so it has to be produced on request"
		);
		Ok(())
	}

	/// The two streams must yield the same number of items for any bbox.
	///
	/// Required by the `TileSource` docs and relied on by the converter's
	/// progress accounting: `tile_coord_stream` computes coordinates below the
	/// base level from the source, while `tile_stream` answers from the staging
	/// cache, so the two disagree exactly where the cache is cold.
	#[tokio::test]
	async fn both_streams_agree_on_how_many_tiles_exist() -> Result<()> {
		let op = make_operation(256, 6).await;
		let pyramid = op.metadata().tile_pyramid().unwrap();

		for level in [4, 6, 7] {
			assert_stream_counts_agree(&op, pyramid.level_ref(level).to_bbox()).await?;
		}
		Ok(())
	}

	/// Reading a tile must not consume it.
	///
	/// The cache is drained by `remove`, so the first read of an overview tile
	/// takes the level below it away and the second read finds nothing. The
	/// pass below stops at level 5 deliberately: it leaves the level-5 cache
	/// populated, which is the state in which the first read succeeds.
	#[tokio::test]
	async fn reading_an_overview_tile_twice_returns_it_twice() -> Result<()> {
		let op = make_varied_operation(256, 6).await;
		depth_first_pass(&op, 5).await?;

		let coord = min_tile_at(&op, 4)?;

		let first = op.tile(&coord).await?;
		let second = op.tile(&coord).await?;

		assert!(first.is_some(), "the first read of {coord:?} should produce a tile");
		assert!(
			second.is_some(),
			"the second read of {coord:?} produced nothing — reading consumed the tile"
		);
		assert_eq!(
			bytes_of(first.unwrap())?,
			bytes_of(second.unwrap())?,
			"two reads of {coord:?} disagree"
		);
		Ok(())
	}

	/// A bbox wider than one block is a request, not a contract violation.
	///
	/// `tile_stream` rounds the request to the 16-tile block grid and then
	/// asserts the result is exactly one block, so anything wider aborts the
	/// process instead of returning tiles.
	#[tokio::test]
	async fn a_bbox_wider_than_one_block_is_served_rather_than_asserted_on() -> Result<()> {
		let op = make_operation(256, 8).await;

		// Two blocks across and two down, positioned so the data is inside it.
		let mut min = min_tile_at(&op, 7)?;
		min.floor(32);
		let bbox = TileBBox::from_min_and_size(7, min.x, min.y, 32, 32)?;

		let tiles = op.tile_stream(bbox).await?.to_vec().await;

		assert!(!tiles.is_empty(), "expected the tiles covering the data in {bbox:?}");
		for (coord, _) in &tiles {
			assert_eq!(coord.level, 7);
		}
		Ok(())
	}

	/// Both paths to the same tile have to produce the same bytes.
	///
	/// The depth-first pass composes from the staging cache and random access
	/// has to compose from whatever it can reach; if the two ever diverge, one
	/// of them is wrong and nothing else here would say which.
	#[tokio::test]
	async fn a_tile_is_the_same_whether_it_was_streamed_or_asked_for() -> Result<()> {
		let streamed = make_varied_operation(256, 6).await;
		let coord = min_tile_at(&streamed, 3)?;

		let from_pass = depth_first_pass(&streamed, 0)
			.await?
			.into_iter()
			.find(|(c, _)| *c == coord)
			.map(|(_, tile)| tile)
			.expect("the depth-first pass covers the whole pyramid, including this tile");

		// A second operation, so the cold read cannot benefit from the pass.
		let asked = make_varied_operation(256, 6).await;
		let on_demand = asked.tile(&coord).await?.expect("the same tile, asked for directly");

		assert_eq!(
			bytes_of(from_pass)?,
			bytes_of(on_demand)?,
			"{coord:?} differs between the depth-first pass and a direct request"
		);
		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_at_base_populates_cache() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata();
		let level_bbox = metadata.tile_pyramid().unwrap().level_ref(6).to_bbox();
		let bbox = TileBBox::from_min_and_size(6, level_bbox.x_min()?, level_bbox.y_min()?, 1, 1)?;

		// Fetch at base level — should populate the cache with scaled-down entries
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		// Cache should now contain entries for the base-level block
		assert!(
			cached_blocks(&op).await > 0,
			"cache should be populated after base-level fetch"
		);

		Ok(())
	}

	#[tokio::test]
	async fn tile_stream_builds_lower_zoom_from_cache() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata().clone();
		let level_bbox = metadata.tile_pyramid().unwrap().level_ref(6).to_bbox();

		// First, fetch all base-level tiles to populate the cache
		let base_bbox = level_bbox;
		let _base_tiles = op.tile_stream(base_bbox).await?.to_vec().await;
		assert!(cached_blocks(&op).await > 0, "cache should be populated");

		// Now fetch at level 5 — should compose from cached half-size images
		let lvl5_bbox = metadata.tile_pyramid().unwrap().level_ref(5).to_bbox();
		let tiles_lvl5 = op.tile_stream(lvl5_bbox).await?.to_vec().await;
		// Should produce at least one tile at level 5
		assert!(!tiles_lvl5.is_empty(), "should produce tiles at level 5 from cache");

		for (coord, _tile) in &tiles_lvl5 {
			assert_eq!(coord.level, 5);
		}

		Ok(())
	}

	/// Helper: create a solid half-size RGBA image where every pixel has the same color.
	fn solid_half(size: u32, r: u8, g: u8, b: u8, a: u8) -> DynamicImage {
		let color = Rgba([r, g, b, a]);
		let mut img = DynamicImage::new_rgba8(size, size);
		for y in 0..size {
			for x in 0..size {
				img.put_pixel(x, y, color);
			}
		}
		img
	}

	#[tokio::test]
	async fn build_images_from_cache_composes_quadrants_with_correct_pixels() -> Result<()> {
		let op = make_operation(256, 6).await;
		let half_size = op.core.tile_size / 2; // 128

		// Manually populate the cache with known half-size (128×128) images for a 32×32 block at level 6.
		// Each tile at (x, y) gets a unique solid color: R=x, G=y, B=42, A=255.
		let block_key = TileCoord::new(6, 0, 0)?;
		let mut entries: Vec<(TileCoord, Option<Blob>)> = Vec::new();
		for y in 0u32..32 {
			for x in 0u32..32 {
				let coord = TileCoord::new(6, x, y)?;
				let img = solid_half(half_size, x as u8, y as u8, 42, 255);
				let blob = versatiles_image::format::png::encode(&img, Some(0))?;
				entries.push((coord, Some(blob)));
			}
		}
		op.core.cache.produce(block_key, std::sync::Arc::new(entries)).await;

		// Request composed images at level 5 for a 2×2 bbox
		let bbox_lvl5 = TileBBox::from_min_and_size(5, 0, 0, 2, 2)?;
		let result = op.core.compose_from_children(bbox_lvl5).await?;
		let items: Vec<_> = result.into_iter().filter(|(_, img)| img.is_some()).collect();
		assert!(!items.is_empty());

		for (coord, img_opt) in items {
			assert_eq!(coord.level, 5);
			let img = img_opt.unwrap();
			assert_eq!(img.dimensions(), (256, 256));

			// Level-5 tile at (x0, y0) is composed from level-6 children placed in quadrants:
			//   (2*x0,   2*y0)   → top-left     (0..128, 0..128)
			//   (2*x0+1, 2*y0)   → top-right    (128..256, 0..128)
			//   (2*x0,   2*y0+1) → bottom-left  (0..128, 128..256)
			//   (2*x0+1, 2*y0+1) → bottom-right (128..256, 128..256)
			let x0 = coord.x as u8;
			let y0 = coord.y as u8;
			assert_eq!(img.get_pixel(0, 0).0, [x0 * 2, y0 * 2, 42, 255]);
			assert_eq!(img.get_pixel(128, 0).0, [x0 * 2 + 1, y0 * 2, 42, 255]);
			assert_eq!(img.get_pixel(0, 128).0, [x0 * 2, y0 * 2 + 1, 42, 255]);
			assert_eq!(img.get_pixel(128, 128).0, [x0 * 2 + 1, y0 * 2 + 1, 42, 255]);
		}

		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = make_operation(256, 6).await;
		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_overview"));
		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_and_tilejson() -> Result<()> {
		let op = make_operation(256, 6).await;
		// metadata and tilejson should be available
		let metadata = op.metadata();
		// After building overview, pyramid should extend to level 0
		assert!(metadata.tile_pyramid().unwrap().level_min().is_some());
		let _tilejson = op.tilejson();
		Ok(())
	}

	#[tokio::test]
	async fn test_tile_stream_at_base_level() -> Result<()> {
		let op = make_operation(256, 6).await;
		// Request tiles at base level within the pyramid bbox
		// The GeoBBox(2.224, 48.815, 2.47, 48.903) at level 6 covers tile (33, 22)
		let metadata = op.metadata();
		let level_bbox = metadata.tile_pyramid().unwrap().level_ref(6).to_bbox();
		let bbox = TileBBox::from_min_and_size(6, level_bbox.x_min()?, level_bbox.y_min()?, 1, 1)?;
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn test_level_above_base_passthrough() -> Result<()> {
		let op = make_operation(256, 6).await;
		// Request at level above level_base should pass through to source
		// Use coordinates that would be valid children of the base level bbox
		let metadata = op.metadata();
		let level_bbox = metadata.tile_pyramid().unwrap().level_ref(6).to_bbox();
		let bbox = TileBBox::from_min_and_size(7, level_bbox.x_min()? * 2, level_bbox.y_min()? * 2, 1, 1)?;
		let tiles = op.tile_stream(bbox).await?.to_vec().await;
		// May or may not have tiles depending on source, but should not error
		assert!(tiles.len() <= 1);
		Ok(())
	}

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "raster_overview");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("level"));
	}

	#[tokio::test]
	async fn test_default_parameters() -> Result<()> {
		// Test with default parameters (no level or tile_size specified)
		let pyramid = TilePyramid::from_geo_bbox(6, 6, &GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap()).unwrap();

		let op = Operation::build(
			VPLNode::try_from_str("raster_overview").unwrap(),
			Box::new(DummyImageSource::from_color(&[255, 0, 0], 512, TileFormat::PNG, Some(pyramid)).unwrap()),
			&PipelineFactory::new_dummy(),
		)
		.await?;

		// Should default to level_base from source max and tile_size 512
		assert_eq!(op.core.tile_size, 512);
		assert_eq!(op.core.level_base, 6);
		Ok(())
	}

	/// The cache is bounded by the weight of what it holds, not by entry count.
	///
	/// Blocks differ in size by orders of magnitude between a dense base level
	/// and a nearly empty overview, so a bound counted in entries would not be
	/// a bound on memory at all.
	#[tokio::test]
	async fn the_cache_weighs_what_it_holds() -> Result<()> {
		let op = make_operation(256, 6).await;
		assert_eq!(op.core.cache.total_bytes().await, 0, "nothing has been cached yet");

		let base_bbox = op.metadata().tile_pyramid().unwrap().level_ref(6).to_bbox();
		op.tile_stream(base_bbox).await?.to_vec().await;

		assert!(
			op.core.cache.total_bytes().await > 0,
			"the cache is not reporting the size of the blocks it holds"
		);

		Ok(())
	}

	/// Building the levels below must not consume the blocks they were built from.
	///
	/// The staging map this replaced was drained as it was read, which is what
	/// made a second read of the same tile return nothing. The inverse is the
	/// property worth pinning down.
	#[tokio::test]
	async fn blocks_stay_cached_after_the_level_above_has_been_built() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata().clone();

		let base_bbox = metadata.tile_pyramid().unwrap().level_ref(6).to_bbox();
		op.tile_stream(base_bbox).await?.to_vec().await;

		let after_base = cached_blocks(&op).await;
		assert!(after_base > 0, "the base level should have been cached");

		for level in (0..6).rev() {
			let lvl_bbox = metadata.tile_pyramid().unwrap().level_ref(level).to_bbox();
			op.tile_stream(lvl_bbox).await?.to_vec().await;
		}

		assert!(
			cached_blocks(&op).await >= after_base,
			"building every level below the base emptied the cache"
		);

		Ok(())
	}

	/// A cold cache is a slow path, not an empty answer.
	///
	/// Nothing is cached here, so every block this needs is computed from the
	/// source on the way down.
	#[tokio::test]
	async fn compose_from_children_computes_what_is_not_cached() -> Result<()> {
		let op = make_operation(256, 6).await;

		let bbox = min_tile_at(&op, 5)?.to_tile_bbox();
		let result = op.core.compose_from_children(bbox).await?;

		let items: Vec<_> = result.into_iter().filter(|(_, img)| img.is_some()).collect();
		assert!(
			!items.is_empty(),
			"a cold cache should be filled, not reported as empty"
		);

		Ok(())
	}

	#[tokio::test]
	async fn compose_from_children_with_none_entries() -> Result<()> {
		let op = make_operation(256, 6).await;
		let half_size = op.core.tile_size / 2;

		// Insert entries where some are None (missing tiles)
		let block_key = TileCoord::new(6, 0, 0)?;
		let mut entries: Vec<(TileCoord, Option<Blob>)> = Vec::new();
		for y in 0u32..16 {
			for x in 0u32..16 {
				let coord = TileCoord::new(6, x, y)?;
				// Only populate even x,y tiles; odd ones are None
				let blob = if x % 2 == 0 && y % 2 == 0 {
					Some(versatiles_image::format::png::encode(
						&solid_half(half_size, x as u8, y as u8, 42, 255),
						Some(0),
					)?)
				} else {
					None
				};
				entries.push((coord, blob));
			}
		}
		op.core.cache.produce(block_key, std::sync::Arc::new(entries)).await;

		let bbox = TileBBox::from_min_and_size(5, 0, 0, 8, 8)?;
		let result = op.core.compose_from_children(bbox).await?;

		// Should still produce some composed tiles without errors
		let items: Vec<_> = result.into_iter().collect();
		assert!(!items.is_empty(), "should produce results even with None entries");

		Ok(())
	}

	#[test]
	fn estimate_entry_bytes_with_blobs_and_nones() {
		use crate::helpers::overview::estimate_entry_bytes;

		let coord = TileCoord::new(6, 0, 0).unwrap();

		// None entries = 16 bytes each
		let entries_none: Vec<(TileCoord, Option<Blob>)> = vec![(coord, None), (coord, None)];
		assert_eq!(estimate_entry_bytes(&entries_none), 32);

		// Blob with known length
		let blob = Blob::from(vec![0u8; 100]);
		let entries_blob: Vec<(TileCoord, Option<Blob>)> = vec![(coord, Some(blob))];
		assert_eq!(estimate_entry_bytes(&entries_blob), 100);

		// Mixed
		let blob2 = Blob::from(vec![0u8; 50]);
		let entries_mixed: Vec<(TileCoord, Option<Blob>)> = vec![(coord, None), (coord, Some(blob2))];
		assert_eq!(estimate_entry_bytes(&entries_mixed), 16 + 50);

		// Empty
		let entries_empty: Vec<(TileCoord, Option<Blob>)> = vec![];
		assert_eq!(estimate_entry_bytes(&entries_empty), 0);
	}

	#[tokio::test]
	async fn full_pipeline_produces_tiles_at_every_level() -> Result<()> {
		let op = make_operation(256, 6).await;
		let metadata = op.metadata().clone();

		// Fetch base level
		let base_bbox = metadata.tile_pyramid().unwrap().level_ref(6).to_bbox();
		let base_tiles = op.tile_stream(base_bbox).await?.to_vec().await;
		assert!(!base_tiles.is_empty(), "base level should have tiles");

		// Walk every level from 5 down to 0 and verify tiles are produced
		for level in (0..6).rev() {
			let lvl_bbox = metadata.tile_pyramid().unwrap().level_ref(level).to_bbox();
			let tiles = op.tile_stream(lvl_bbox).await?.to_vec().await;
			assert!(!tiles.is_empty(), "level {level} should produce at least one tile");
			for (coord, _) in &tiles {
				assert_eq!(coord.level, level, "tile should be at level {level}");
			}
		}

		Ok(())
	}

	#[test]
	fn debug_format_includes_fields() {
		let rt = tokio::runtime::Runtime::new().unwrap();
		let op = rt.block_on(make_operation(256, 6));
		let debug = format!("{op:?}");
		assert!(debug.contains("level_base"), "debug should include level_base");
		assert!(debug.contains("tile_size"), "debug should include tile_size");
	}

	#[tokio::test]
	async fn tile_512_overview_produces_tiles() -> Result<()> {
		// Test with a different tile size (512) to ensure it's not hardcoded
		let op = make_operation(512, 4).await;
		assert_eq!(op.core.tile_size, 512);

		let metadata = op.metadata().clone();
		let base_bbox = metadata.tile_pyramid().unwrap().level_ref(4).to_bbox();
		let base_tiles = op.tile_stream(base_bbox).await?.to_vec().await;
		assert!(!base_tiles.is_empty());

		// Build level 3 from cache
		let lvl3_bbox = metadata.tile_pyramid().unwrap().level_ref(3).to_bbox();
		let tiles = op.tile_stream(lvl3_bbox).await?.to_vec().await;
		assert!(!tiles.is_empty(), "should produce overview tiles with tile_size=512");

		Ok(())
	}
	/// A depth-first pass must read every source tile exactly once.
	///
	/// This is the whole reason the cache exists. Composing a level consumes
	/// the blocks above it, and a traversal hands out blocks *clipped* to the
	/// pyramid rather than whole ones — so a caching rule that only recognised
	/// whole blocks would quietly cache nothing at the edges of the data and
	/// rebuild entire subtrees from the source, with every other test here
	/// still passing. Counting the reads is what notices.
	#[tokio::test]
	async fn a_depth_first_pass_reads_every_source_tile_exactly_once() -> Result<()> {
		use std::sync::{
			Arc,
			atomic::{AtomicUsize, Ordering},
		};

		let base = 7u8;
		// Wide enough that the base level spans several blocks and is clipped
		// by most of them; a fixture the size of one block cannot show this.
		let geo = GeoBBox::new(-20.0, 20.0, 30.0, 55.0).unwrap();
		let pyramid = TilePyramid::from_geo_bbox(base, base, &geo).unwrap();
		let base_tiles = pyramid.level_ref(base).to_bbox().count_tiles();

		let reads = Arc::new(AtomicUsize::new(0));
		let counter = reads.clone();

		let mut source = DummyImageSource::new(
			move |coord| {
				counter.fetch_add(1, Ordering::Relaxed);
				let color = Rgb([(coord.x % 251) as u8, (coord.y % 251) as u8, 7]);
				let image = DynamicImage::ImageRgb8(RgbImage::from_pixel(256, 256, color));
				Tile::from_image(image, TileFormat::PNG).ok()
			},
			TileFormat::PNG,
			Some(pyramid),
		)
		.unwrap();
		source.tilejson_mut().set_tile_size(256).unwrap();

		let op = Operation::build(
			VPLNode::try_from_str(&format!("raster_overview level={base}")).unwrap(),
			Box::new(source),
			&PipelineFactory::new_dummy(),
		)
		.await?;

		let produced = depth_first_pass(&op, 0).await?;
		assert!(!produced.is_empty(), "the pass should have produced tiles");

		assert_eq!(
			reads.load(Ordering::Relaxed) as u64,
			base_tiles,
			"the pass rebuilt part of the pyramid from the source instead of from the cache"
		);

		// Every block is produced just before the level below consumes it, so
		// none of them should have had to be built on demand.
		assert_eq!(
			op.core.cache.stats().0,
			0,
			"a block was rebuilt: the reservation did not hold it until its consumer arrived"
		);

		Ok(())
	}
}
