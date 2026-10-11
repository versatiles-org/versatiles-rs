//! # Raster Overscale Operation
//!
//! This operation generates raster tiles beyond the source's native maximum zoom level
//! by extracting and upscaling portions of existing tiles.
//!
//! ## How It Works
//!
//! Given a tile source with a maximum zoom level (e.g., z14), this operation can serve
//! tiles at higher zoom levels (e.g., z15, z16, z17...) by:
//!
//! 1. Finding the corresponding "parent" tile at the base level
//! 2. Extracting the relevant sub-region from that parent tile
//! 3. Upscaling the extracted region to full tile size
//!
//! For example, a z15 tile covers 1/4 the area of its z14 parent. The operation extracts
//! that quarter and scales it up to 256x256 (or whatever the tile size is).
//!
//! ## Tile Climbing
//!
//! When `enable_climbing` is enabled, the operation handles sparse tile sources gracefully.
//! If a tile doesn't exist at the base level, the algorithm "climbs" up the zoom hierarchy
//! (to parent tiles at lower zoom levels) until it finds an existing tile.
//!
//! **Example**: Requesting tile z16/1000/2000 with `level_base=14`:
//! - Without climbing: Only looks for z14/250/500. If missing, returns nothing.
//! - With climbing: Tries z14/250/500 → z13/125/250 → z12/62/125 → ... until found.
//!
//! This is useful for tile sources with incomplete coverage, where some areas may only
//! have tiles at lower zoom levels.
//!
//! ## Caching
//!
//! The operation maintains an LRU cache (512MB by default) of decoded source images.
//! This significantly improves performance when generating multiple high-zoom tiles
//! from the same parent tile, as the parent only needs to be fetched and decoded once.

mod fixed;
mod scale;

use anyhow::Result;
use versatiles_container::TileSource;
use versatiles_core::ZoomLevel;
use versatiles_derive::context;

pub use self::fixed::FixedOverscale;
use crate::{PipelineFactory, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Serves raster tiles above the source's native resolution by upscaling.
///
/// Tiles at `level_base` and below are passed through unchanged; above it, the
/// covering tile from `level_base` is cropped to the requested area and scaled
/// up. The result is blurry rather than detailed — the point is that a client
/// can keep zooming instead of hitting a blank map.
pub struct Args {
	/// Zoom level to upscale from. Defaults to the source's highest.
	pub level_base: Option<ZoomLevel>,

	/// Highest zoom level to serve. Defaults to `30`.
	#[vpl(default = "30")]
	pub level_max: Option<ZoomLevel>,

	/// Whether to climb to lower levels when the `level_base` tile is missing. Defaults to `false`.
	#[vpl(default = "false")]
	pub enable_climbing: Option<bool>,
}

/// Builds the operation behind `raster_overscale` in a pipeline.
pub struct Operation;

impl Operation {
	#[context("Building raster_overscale operation in VPL node {:?}", vpl_node.name)]
	async fn build(
		vpl_node: VPLNode,
		source: Box<dyn TileSource>,
		_factory: &PipelineFactory,
	) -> Result<FixedOverscale> {
		let args = Args::from_vpl_node(&vpl_node)?;
		FixedOverscale::new(
			source,
			args.level_base.map(u8::from),
			args.level_max.map(u8::from),
			args.enable_climbing.unwrap_or(false),
		)
	}
}

crate::operations::macros::define_transform_factory!("raster_overscale", Args, Operation, requires: Raster);

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test data is built from literal values")]
mod tests {
	use rstest::rstest;
	use versatiles_container::Tile;
	use versatiles_core::{TileBBox, TileCoord, TileFormat, TilePyramid};
	use versatiles_image::{DynamicImage, DynamicImageTraitConvert, DynamicImageTraitOperation};

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_image_source::DummyImageSource};

	fn make_gradient_image(channel_count: usize) -> DynamicImage {
		let s = 256;
		match channel_count {
			1 => DynamicImage::from_fn(s, s, |x, _| [x as u8]),
			2 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8]),
			3 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8, 255 - x as u8]),
			4 => DynamicImage::from_fn(s, s, |x, y| [x as u8, y as u8, 255 - x as u8, 255 - y as u8]),
			_ => panic!("unsupported channel count {channel_count}"),
		}
	}

	#[expect(
		clippy::cast_possible_truncation,
		clippy::cast_sign_loss,
		reason = "`c` is a u8 and `scale` a small u32, so the average is 0..=255"
	)]
	async fn get_avg(op: &FixedOverscale, coord: (u8, u8, u8), scale: u32) -> Vec<u8> {
		let (level, x, y) = coord;
		let coord = TileCoord::new(level, u32::from(x), u32::from(y))
			.unwrap()
			.to_tile_bbox();
		let mut tiles = op.tile_stream(coord).await.unwrap().to_vec().await;
		assert_eq!(tiles.len(), 1);
		let mut tile = tiles.pop().unwrap().1;
		let image = tile.as_image().unwrap();
		let avg = image.average_color();
		avg.into_iter()
			.map(|c| (f64::from(c) / f64::from(scale)).round() as u8)
			.collect()
	}

	async fn build_op(channel_count: usize) -> Result<FixedOverscale> {
		let image = make_gradient_image(channel_count);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		Operation::build(
			VPLNode::try_from_str("raster_overscale level_base=2")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await
	}

	#[rstest]
	#[case::l(1,[vec![16], vec![48], vec![16], vec![48]])]
	#[case::la(2,[vec![16,16], vec![48,16], vec![16,48], vec![48,48]])]
	#[case::rgb(3,[vec![16,16,48], vec![48,16,16], vec![16,48,48], vec![48,48,16]])]
	#[case::rgba(4,[vec![16,16,48,48], vec![48,16,16,48], vec![16,48,48,16], vec![48,48,16,16]])]
	#[tokio::test]
	async fn overscale_to_z3(#[case] channel_count: usize, #[case] expected: [Vec<u8>; 4]) {
		let op = build_op(channel_count).await.unwrap();

		let avg_colors = [
			get_avg(&op, (3, 2, 2), 4).await,
			get_avg(&op, (3, 3, 2), 4).await,
			get_avg(&op, (3, 2, 3), 4).await,
			get_avg(&op, (3, 3, 3), 4).await,
		];

		assert_eq!(avg_colors, expected);
	}

	#[tokio::test]
	async fn overscale_to_z4_rgb() -> Result<()> {
		let op = build_op(3).await?;

		for x in 0..4 {
			for y in 0..4 {
				assert_eq!(
					get_avg(&op, (4, 4 + x, 4 + y), 32).await,
					vec![1 + x * 2, 1 + y * 2, 7 - x * 2]
				);
			}
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_overscale_without_climbing() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with climbing disabled (default)
		let op = Operation::build(
			VPLNode::try_from_str("raster_overscale level_base=2")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;

		// Should work for tiles at level_base (z=2)
		let coord_base = TileCoord::new(2, 0, 0)?.to_tile_bbox();
		let tiles = op.tile_stream(coord_base).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should return tile at level_base");

		// Should work for tiles above level_base (extracted from level_base)
		let coord_high = TileCoord::new(3, 0, 0)?.to_tile_bbox();
		let tiles = op.tile_stream(coord_high).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should extract from level_base for high zoom");

		// Multiple high-zoom tiles should reuse cached base tile
		let coord_high2 = TileCoord::new(3, 1, 0)?.to_tile_bbox();
		let tiles2 = op.tile_stream(coord_high2).await?.to_vec().await;
		assert_eq!(tiles2.len(), 1, "Should also work for adjacent tile");

		Ok(())
	}

	#[tokio::test]
	async fn test_overscale_with_climbing_enabled() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with climbing ENABLED
		let op = Operation::build(
			VPLNode::try_from_str("raster_overscale level_base=2 enable_climbing=true")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;

		// Should work with climbing enabled
		let coord = TileCoord::new(3, 0, 0)?.to_tile_bbox();
		let tiles = op.tile_stream(coord).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should return tile with climbing enabled");

		Ok(())
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let op = build_op(3).await?;
		let source_type = op.source_type();
		assert!(source_type.to_string().contains("raster_overscale"));
		Ok(())
	}

	#[tokio::test]
	async fn test_metadata_and_tilejson() -> Result<()> {
		let op = build_op(3).await?;
		// metadata and tilejson should be available
		let metadata = op.metadata();
		assert!(metadata.tile_pyramid().unwrap().level_max().is_some());
		let _tilejson = op.tilejson();
		Ok(())
	}

	#[tokio::test]
	async fn test_level_max_parameter() -> Result<()> {
		let image = make_gradient_image(3);
		let source = Box::new(DummyImageSource::from_image(image, TileFormat::PNG, None)?);

		// Build with custom level_max
		let op = Operation::build(
			VPLNode::try_from_str("raster_overscale level_base=2 level_max=5")?,
			source,
			&PipelineFactory::new_dummy(),
		)
		.await?;

		// Should have extended max level
		let metadata = op.metadata();
		assert!(metadata.tile_pyramid().unwrap().level_max().unwrap() >= 5);
		Ok(())
	}

	#[tokio::test]
	async fn test_level_below_base_passthrough() -> Result<()> {
		let op = build_op(3).await?;

		// Request at level below level_base should pass through to source
		let coord = TileCoord::new(2, 1, 1)?.to_tile_bbox();
		let tiles = op.tile_stream(coord).await?.to_vec().await;
		assert_eq!(tiles.len(), 1, "Should passthrough to source for level <= level_base");
		Ok(())
	}

	// --- the mode `from_stacked_raster auto_overscale=true` relies on ---

	/// A source with holes: levels 1 to 4, not full, and inside its pyramid a
	/// tile missing here and there. Every tile has its own gradient, so both
	/// the tile an upscale came from and the region cut out of it show.
	fn sparse_source() -> Result<DummyImageSource> {
		let mut pyramid = TilePyramid::new_empty();
		pyramid.insert_bbox(&TileBBox::new_full(1)?)?;
		pyramid.insert_bbox(&TileBBox::from_min_and_max(2, 0, 0, 3, 2)?)?;
		pyramid.insert_bbox(&TileBBox::from_min_and_max(3, 1, 1, 6, 5)?)?;
		pyramid.insert_bbox(&TileBBox::from_min_and_max(4, 2, 2, 9, 9)?)?;

		DummyImageSource::new(
			|coord| {
				let (level, x, y) = (u32::from(coord.level), coord.x, coord.y);
				if (x + 2 * y + level) % 5 == 0 {
					return None;
				}
				let image = DynamicImage::from_fn(8, 8, |px, py| {
					[
						(level * 37 + px * 3) as u8,
						(x * 29 + py * 5) as u8,
						(y * 31 + px + py) as u8,
					]
				});
				Some(Tile::from_image(image, TileFormat::PNG).unwrap())
			},
			TileFormat::PNG,
			Some(pyramid),
		)
	}

	/// One line per level: how many tiles and coordinates the operation hands
	/// out, how many its pyramid announces, and a hash over which tiles they
	/// are and what is in them.
	async fn fingerprint(op: &dyn TileSource, level_max: u8) -> Result<String> {
		fn fnv(hash: &mut u64, bytes: &[u8]) {
			for byte in bytes {
				*hash ^= u64::from(*byte);
				*hash = hash.wrapping_mul(0x0100_0000_01b3);
			}
		}
		let key = |c: &TileCoord| (c.level, c.x, c.y);

		let pyramid = op.metadata().tile_pyramid().unwrap();
		let mut lines = Vec::new();
		for level in 0..=level_max {
			let bbox = TileBBox::new_full(level)?;
			let mut hash = 0xcbf2_9ce4_8422_2325_u64;

			let mut tiles = op.tile_stream(bbox).await?.to_vec().await;
			tiles.sort_by_key(|(coord, _)| key(coord));
			let tile_count = tiles.len();
			for (coord, tile) in tiles {
				fnv(&mut hash, format!("{:?}", key(&coord)).as_bytes());
				fnv(&mut hash, tile.into_image()?.as_bytes());
			}

			let mut coords: Vec<TileCoord> = op
				.tile_coord_stream(bbox)
				.await?
				.to_vec()
				.await
				.into_iter()
				.map(|(coord, ())| coord)
				.collect();
			coords.sort_by_key(key);
			for coord in &coords {
				fnv(&mut hash, format!("{:?}", key(coord)).as_bytes());
			}

			lines.push(format!(
				"level {level}: tiles={tile_count} coords={} pyramid={} hash={hash:016x}",
				coords.len(),
				pyramid.level_ref(level).count_tiles()
			));
		}
		Ok(lines.join("\n"))
	}

	/// Frozen before `raster_overscale` learned to fill gaps (#284), so that
	/// nothing in that work changes what a stack of sources is blended from.
	#[tokio::test]
	async fn the_stacking_mode_gives_what_it_always_gave() -> Result<()> {
		let op = FixedOverscale::for_stacking(Box::new(sparse_source()?), 4, 7)?;

		assert_eq!(
			fingerprint(&op, 8).await?,
			[
				"level 0: tiles=0 coords=0 pyramid=0 hash=cbf29ce484222325",
				"level 1: tiles=4 coords=4 pyramid=4 hash=c0bf84935b355a01",
				"level 2: tiles=10 coords=12 pyramid=12 hash=fd3cd4d04f3db28c",
				"level 3: tiles=24 coords=30 pyramid=30 hash=2b65ddbf30192d40",
				"level 4: tiles=51 coords=64 pyramid=64 hash=f0218e8b73d4abe5",
				"level 5: tiles=1024 coords=1024 pyramid=256 hash=b29d7d012c2f28c8",
				"level 6: tiles=4096 coords=4096 pyramid=1024 hash=da95c98490faa769",
				"level 7: tiles=16384 coords=16384 pyramid=4096 hash=96aa0f8a1f9d18bf",
				"level 8: tiles=0 coords=0 pyramid=0 hash=cbf29ce484222325",
			]
			.join("\n")
		);
		Ok(())
	}

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "raster_overscale");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("level_base"));
		assert!(docs.contains("level_max"));
		assert!(docs.contains("enable_climbing"));
	}
}
