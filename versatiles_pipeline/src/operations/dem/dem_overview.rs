use std::{fmt::Debug, sync::Arc};

use anyhow::{Result, bail};
use async_trait::async_trait;
use imageproc::image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TileStream, ZoomLevel};

use super::encoding::{DemEncoding, resolve_encoding, to_tile_schema};
use crate::{PipelineFactory, helpers::overview::OverviewCore, vpl::VPLNode};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generates lower-zoom DEM overview tiles by averaging 24-bit elevation values.
///
/// `raster_overview` averages the R, G and B channels independently, which is
/// meaningless for a DEM: the channels are one number split across three bytes,
/// so averaging them separately mixes the high byte of one pixel with the low
/// byte of another. This operation decodes each pixel to its 24-bit raw
/// elevation, averages that, and re-encodes.
///
/// Only the averaging differs. Read order, what a tile costs to build on
/// demand, and the memory the intermediate levels use are all as described for
/// [`raster_overview`](#raster_overview).
struct Args {
	/// Zoom level to build the overview from. Defaults to the source's highest.
	level: Option<ZoomLevel>,
	/// DEM encoding of the source. Defaults to the encoding its tile schema implies.
	encoding: Option<DemEncoding>,
}

#[derive(Debug)]
struct Operation {
	core: OverviewCore,
}

/// Downscale a DEM image by factor 2 by averaging 24-bit raw elevation values.
///
/// For each 2x2 block of pixels, the R/G/B channels are interpreted as a single
/// 24-bit integer (`raw = R*65536 + G*256 + B`), averaged, and re-encoded.
/// This is correct for all linear DEM encodings (Mapbox, Terrarium) because
/// `avg(elevation) = decode(avg(raw))` when the encoding is linear.
pub(super) fn dem_scale_down(image: &DynamicImage) -> Result<DynamicImage> {
	let half_w = image.width() / 2;
	let half_h = image.height() / 2;

	match image {
		DynamicImage::ImageRgb8(img) => {
			let mut out = RgbImage::new(half_w, half_h);
			for oy in 0..half_h {
				for ox in 0..half_w {
					let sx = ox * 2;
					let sy = oy * 2;
					let mut sum = 0u64;
					for dy in 0..2u32 {
						for dx in 0..2u32 {
							let Rgb([r, g, b]) = *img.get_pixel(sx + dx, sy + dy);
							sum += (u64::from(r) << 16) | (u64::from(g) << 8) | u64::from(b);
						}
					}
					let avg = (sum + 2) / 4; // round to nearest
					out.put_pixel(ox, oy, raw_to_rgb(avg));
				}
			}
			Ok(DynamicImage::ImageRgb8(out))
		}
		DynamicImage::ImageRgba8(img) => {
			let mut out = RgbaImage::new(half_w, half_h);
			for oy in 0..half_h {
				for ox in 0..half_w {
					let sx = ox * 2;
					let sy = oy * 2;
					let mut sum = 0u64;
					let mut alpha_sum = 0u64;
					let mut count = 0u64;

					for dy in 0..2u32 {
						for dx in 0..2u32 {
							let Rgba([r, g, b, a]) = *img.get_pixel(sx + dx, sy + dy);
							if a > 0 {
								sum += (u64::from(r) << 16) | (u64::from(g) << 8) | u64::from(b);
								count += 1;
							}
							alpha_sum += u64::from(a);
						}
					}

					let avg_alpha = ((alpha_sum + 2) / 4).min(255) as u8;

					if let Some(avg) = (sum + count / 2).checked_div(count) {
						let Rgb([cr, cg, cb]) = raw_to_rgb(avg);
						out.put_pixel(ox, oy, Rgba([cr, cg, cb, avg_alpha]));
					} else {
						out.put_pixel(ox, oy, Rgba([0, 0, 0, 0]));
					}
				}
			}
			Ok(DynamicImage::ImageRgba8(out))
		}
		_ => bail!("dem_overview requires RGB8 or RGBA8 images"),
	}
}

/// Convert a raw 24-bit value to an RGB pixel.
fn raw_to_rgb(raw: u64) -> Rgb<u8> {
	Rgb([
		((raw >> 16) & 0xFF) as u8,
		((raw >> 8) & 0xFF) as u8,
		(raw & 0xFF) as u8,
	])
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

		let encoding = resolve_encoding(args.encoding, source.tilejson().tile_schema.as_ref())?;

		let mut core = OverviewCore::new(
			source,
			args.level.map(u8::from),
			Arc::new(dem_scale_down),
			factory.runtime(),
		)?;

		core.tilejson.tile_schema = Some(to_tile_schema(encoding));

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
		SourceType::new_processor("dem_overview", self.core.source.source_type())
	}

	/// Overridden so a single tile is answered under a read budget, which the
	/// default implementation's route through `tile_stream` would not apply.
	async fn tile(&self, coord: &TileCoord) -> Result<Option<Tile>> {
		self.core.tile(coord).await
	}

	async fn tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("dem_overview::tile_stream {bbox:?}");
		self.core.tile_stream(bbox).await
	}

	async fn tile_coord_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, ()>> {
		self.core.tile_coord_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("dem_overview", Args, Operation, requires: Raster);

#[cfg(test)]
mod tests {
	use imageproc::image::{GenericImage, Pixel};
	use versatiles_core::{GeoBBox, TileCompression, TileFormat, TilePyramid, TileSchema};

	use super::*;
	use crate::{factory::OperationFactoryTrait, helpers::dummy_image_source::DummyImageSource};

	fn raw_to_rgb(v: u32) -> Rgb<u8> {
		Rgb([((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8, (v & 0xFF) as u8])
	}

	fn rgb_to_raw(Rgb([r, g, b]): Rgb<u8>) -> u32 {
		(u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
	}

	// ── dem_scale_down unit tests ─────────────────────────────────────

	#[test]
	fn test_dem_scale_down_rgb_uniform() {
		// 2x2 image with same value → output 1x1 with that value
		let raw = 100_000u32;
		let px = raw_to_rgb(raw);
		let mut img = RgbImage::new(2, 2);
		for y in 0..2 {
			for x in 0..2 {
				img.put_pixel(x, y, px);
			}
		}
		let result = dem_scale_down(&DynamicImage::ImageRgb8(img)).unwrap();
		let out = result.as_rgb8().unwrap();
		assert_eq!(rgb_to_raw(*out.get_pixel(0, 0)), raw);
	}

	#[test]
	fn test_dem_scale_down_rgb_averaging() {
		// 2x2 with values that span a channel boundary to verify correct 24-bit averaging
		// Pixel A: raw=65536 (R=1,G=0,B=0) and Pixel B: raw=65535 (R=0,G=255,B=255)
		// Standard channel-wise averaging would give wrong result, DEM averaging gives correct result
		let mut img = RgbImage::new(2, 2);
		img.put_pixel(0, 0, raw_to_rgb(65536)); // R=1, G=0, B=0
		img.put_pixel(1, 0, raw_to_rgb(65535)); // R=0, G=255, B=255
		img.put_pixel(0, 1, raw_to_rgb(65536));
		img.put_pixel(1, 1, raw_to_rgb(65535));

		let result = dem_scale_down(&DynamicImage::ImageRgb8(img)).unwrap();
		let out = result.as_rgb8().unwrap();
		let avg_raw = rgb_to_raw(*out.get_pixel(0, 0));
		// Average of 65536 and 65535 (twice each) = 65535.5, rounded = 65536
		assert_eq!(avg_raw, 65536);
	}

	#[test]
	fn test_dem_scale_down_rgb_vs_channel_average() {
		// Demonstrate that DEM averaging differs from naive channel averaging
		let mut img = RgbImage::new(2, 2);
		// All four pixels: two at raw=65536, two at raw=65280
		img.put_pixel(0, 0, raw_to_rgb(65536)); // R=1, G=0, B=0
		img.put_pixel(1, 0, raw_to_rgb(65280)); // R=0, G=254, B=192 (just below boundary)
		img.put_pixel(0, 1, raw_to_rgb(65536));
		img.put_pixel(1, 1, raw_to_rgb(65280));

		let result = dem_scale_down(&DynamicImage::ImageRgb8(img)).unwrap();
		let out = result.as_rgb8().unwrap();
		let dem_raw = rgb_to_raw(*out.get_pixel(0, 0));

		// Correct DEM average: (65536 + 65280 + 65536 + 65280) / 4 = 65408
		assert_eq!(dem_raw, 65408);

		// Channel-wise average would give: R=(1+0+1+0)/4=0, G=(0+254+0+254)/4=127, B=(0+192+0+192)/4=96
		// → raw = 0*65536 + 127*256 + 96 = 32608 (WRONG!)
		let channel_avg_raw = 32608;
		assert_ne!(
			dem_raw, channel_avg_raw,
			"DEM averaging should differ from channel-wise averaging"
		);
	}

	#[test]
	fn test_dem_scale_down_rgba_skips_transparent() {
		// RGBA: transparent pixels should be excluded from averaging
		let mut img = RgbaImage::new(2, 2);
		img.put_pixel(0, 0, Rgba([1, 0, 0, 255])); // raw=65536, opaque
		img.put_pixel(1, 0, Rgba([0, 0, 0, 0])); // transparent
		img.put_pixel(0, 1, Rgba([0, 0, 0, 0])); // transparent
		img.put_pixel(1, 1, Rgba([0, 0, 0, 0])); // transparent

		let result = dem_scale_down(&DynamicImage::ImageRgba8(img)).unwrap();
		let out = result.as_rgba8().unwrap();
		let p = out.get_pixel(0, 0);
		// Only one opaque pixel with raw=65536, so average = 65536
		let raw = (u32::from(p[0]) << 16) | (u32::from(p[1]) << 8) | u32::from(p[2]);
		assert_eq!(raw, 65536);
		// Alpha: (255 + 0 + 0 + 0) / 4 = 63.75 → rounded = 64
		assert_eq!(p[3], 64);
	}

	#[test]
	fn test_dem_scale_down_rgba_all_transparent() {
		let mut img = RgbaImage::new(2, 2);
		for y in 0..2 {
			for x in 0..2 {
				img.put_pixel(x, y, Rgba([0, 0, 0, 0]));
			}
		}
		let result = dem_scale_down(&DynamicImage::ImageRgba8(img)).unwrap();
		let out = result.as_rgba8().unwrap();
		assert_eq!(*out.get_pixel(0, 0), Rgba([0, 0, 0, 0]));
	}

	#[test]
	fn test_dem_scale_down_4x4_to_2x2() {
		// 4x4 image → 2x2 output
		let mut img = RgbImage::new(4, 4);
		for y in 0..4 {
			for x in 0..4 {
				let raw = 100_000 + (y * 4 + x) * 100;
				img.put_pixel(x, y, raw_to_rgb(raw));
			}
		}
		let result = dem_scale_down(&DynamicImage::ImageRgb8(img)).unwrap();
		assert_eq!(result.width(), 2);
		assert_eq!(result.height(), 2);

		let out = result.as_rgb8().unwrap();
		// Top-left 2x2 block: raw values 100000, 100100, 100400, 100500 → avg = 100250
		assert_eq!(rgb_to_raw(*out.get_pixel(0, 0)), 100250);
	}

	#[test]
	fn test_dem_scale_down_rejects_unsupported_format() {
		let img = DynamicImage::new_luma8(2, 2);
		assert!(dem_scale_down(&img).is_err());
	}

	// ── operation build/factory tests ─────────────────────────────────

	#[tokio::test]
	async fn test_build_with_mapbox_schema() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_overview")
			.await?;
		assert!(op.source_type().to_string().contains("dem_overview"));
		Ok(())
	}

	#[tokio::test]
	async fn test_build_with_terrarium_schema() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | meta_update schema=\"dem/terrarium\" | dem_overview")
			.await?;
		assert!(op.source_type().to_string().contains("dem_overview"));
		Ok(())
	}

	#[tokio::test]
	async fn test_build_with_encoding_override() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | dem_overview encoding=mapbox")
			.await?;
		assert!(op.source_type().to_string().contains("dem_overview"));
		Ok(())
	}

	#[tokio::test]
	async fn test_build_fails_without_dem_schema() {
		let factory = PipelineFactory::new_dummy();
		let result = factory.operation_from_vpl("from_debug format=png | dem_overview").await;
		assert!(result.is_err());
		let err_msg = format!("{:?}", result.unwrap_err());
		assert!(
			err_msg.contains("tile_schema is not a DEM encoding"),
			"Expected DEM schema error, got: {err_msg}"
		);
	}

	#[tokio::test]
	async fn test_build_fails_with_invalid_encoding() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_debug format=png | dem_overview encoding=invalid")
			.await;
		assert!(result.is_err());
	}

	// ── random tile access ───────────────────────────────────────────
	//
	// `dem_overview` shares `OverviewCore` with `raster_overview`, so it
	// inherits the ability to answer a tile asked for out of nowhere. Sharing
	// an implementation is not the same as being covered by its tests: nothing
	// here would notice if this operation stopped routing through the core, and
	// a DEM is the one most likely to be served live.

	/// A source with a bounded pyramid whose tiles differ from one another.
	///
	/// Distinct tiles matter for the comparison below: a DEM of one constant
	/// elevation encodes identically however its quadrants are arranged, so it
	/// cannot tell a correct composition from a wrong one.
	async fn make_dem_operation() -> Result<Box<dyn TileSource>> {
		PipelineFactory::new_dummy()
			.operation_from_vpl(
				"from_debug format=png | filter level_min=6 level_max=6 bbox=[2,48,3,49] | dem_overview encoding=terrarium",
			)
			.await
	}

	/// The northwest tile the operation covers at `level`.
	async fn min_tile_at(op: &dyn TileSource, level: u8) -> Result<TileCoord> {
		op.tile_pyramid().await?.level_ref(level).to_bbox().min_tile()
	}

	/// The same operation as [`make_dem_operation`], built directly so a test
	/// can reach its core.
	async fn make_dem_op_concrete() -> Result<Operation> {
		let pyramid = TilePyramid::from_geo_bbox(6, 6, &GeoBBox::new(2.0, 48.0, 3.0, 49.0).unwrap())?;
		let source = DummyImageSource::from_color(&[100u8, 150, 200], 256, TileFormat::PNG, Some(pyramid))?;

		Operation::build(
			VPLNode::try_from_str("dem_overview encoding=terrarium level=6").unwrap(),
			Box::new(source),
			&PipelineFactory::new_dummy(),
		)
		.await
	}

	/// `tile` must route through the core, where the read ceiling lives.
	///
	/// The trait supplies a default `tile` that takes the first item of a
	/// one-tile `tile_stream`, which works and is completely unbudgeted. If the
	/// override here were dropped, every other test in this file would still
	/// pass and the ceiling would silently stop applying to served tiles — so
	/// this asserts the one thing that distinguishes the two paths.
	#[tokio::test]
	async fn tile_is_answered_under_the_read_ceiling() -> Result<()> {
		let mut op = make_dem_op_concrete().await?;
		op.core.set_request_budget(Some(0));
		let coord = op
			.metadata()
			.tile_pyramid()
			.unwrap()
			.level_ref(4)
			.to_bbox()
			.min_tile()?;

		// Through the trait, which is how a server reaches it.
		let source: &dyn TileSource = &op;
		let error = source
			.tile(&coord)
			.await
			.expect_err("a ceiling of nothing should refuse a request that has to read");
		assert!(
			format!("{error:#}").contains("source tiles to build"),
			"should fail on the ceiling, not something else: {error:#}"
		);

		// The same tile in bulk is not subject to it.
		assert_eq!(source.tile_stream(coord.to_tile_bbox()).await?.to_vec().await.len(), 1);
		Ok(())
	}

	#[tokio::test]
	async fn a_single_tile_below_the_base_level_is_produced_on_demand() -> Result<()> {
		let op = make_dem_operation().await?;
		let coord = min_tile_at(op.as_ref(), 4).await?;

		assert!(
			op.tile(&coord).await?.is_some(),
			"{coord:?} is inside the pyramid the operation advertises, so it has to be produced on request"
		);
		Ok(())
	}

	#[tokio::test]
	async fn both_streams_agree_on_how_many_tiles_exist() -> Result<()> {
		let op = make_dem_operation().await?;
		let pyramid = op.tile_pyramid().await?;

		for level in [4, 6, 7] {
			versatiles_container::testing::assert_stream_counts_agree(op.as_ref(), pyramid.level_ref(level).to_bbox())
				.await?;
		}
		Ok(())
	}

	#[tokio::test]
	async fn reading_an_overview_tile_twice_returns_it_twice() -> Result<()> {
		let op = make_dem_operation().await?;
		let coord = min_tile_at(op.as_ref(), 4).await?;

		let first = op.tile(&coord).await?.expect("first read");
		let second = op.tile(&coord).await?.expect("second read");

		assert_eq!(
			first.into_blob(&TileCompression::Uncompressed)?,
			second.into_blob(&TileCompression::Uncompressed)?,
			"two reads of {coord:?} disagree"
		);
		Ok(())
	}

	/// Elevations must not depend on how the tile was asked for.
	///
	/// The DEM-specific risk: this operation averages 24-bit elevations rather
	/// than channels, and a tile composed on demand goes through the same
	/// `scale_fn` as one built by a pass. If the two ever diverged, the numbers
	/// would be wrong rather than merely different, and nothing else here looks
	/// at the pixels of a composed tile.
	#[tokio::test]
	async fn a_tile_is_the_same_whether_it_was_streamed_or_asked_for() -> Result<()> {
		let streamed = make_dem_operation().await?;
		let coord = min_tile_at(streamed.as_ref(), 3).await?;

		let metadata = streamed.metadata().clone();
		let traversal = metadata
			.preferred_traversal()
			.unwrap_or_else(|| metadata.traversal())
			.clone();
		let pyramid = streamed.tile_pyramid().await?;

		let mut from_pass = None;
		for bbox in traversal.traverse_pyramid(pyramid.as_ref())? {
			for (c, tile) in streamed.tile_stream(bbox).await?.to_vec().await {
				if c == coord {
					from_pass = Some(tile);
				}
			}
		}
		let from_pass = from_pass.expect("the pass covers the whole pyramid, including this tile");

		// A second operation, so the cold read cannot benefit from the pass.
		let asked = make_dem_operation().await?;
		let on_demand = asked.tile(&coord).await?.expect("the same tile, asked for directly");

		assert_eq!(
			from_pass.into_blob(&TileCompression::Uncompressed)?,
			on_demand.into_blob(&TileCompression::Uncompressed)?,
			"{coord:?} differs between the pass and a direct request"
		);
		Ok(())
	}

	#[tokio::test]
	async fn the_operation_prefers_depth_first_without_requiring_it() -> Result<()> {
		let op = make_dem_operation().await?;
		let metadata = op.metadata();

		assert!(metadata.traversal().is_any(), "tiles may be asked for in any order");

		let preferred = metadata.preferred_traversal().expect("a preferred order");
		assert_eq!(preferred.order(), &versatiles_container::TraversalOrder::DepthFirst);
		Ok(())
	}

	#[tokio::test]
	async fn test_streaming_generates_lower_zoom_tiles() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_debug format=png | filter level_min=4 level_max=4 bbox=[2,48,3,49] | dem_overview encoding=terrarium",
			)
			.await?;

		// Stream every block the operation hands out, in the order it hands them
		// out. That order is no longer load-bearing — a level builds whatever it
		// is missing — but it is what a conversion does.
		let traversal = op.metadata().traversal().clone();
		let pyramid = op.tile_pyramid().await?;
		let bboxes = traversal.traverse_pyramid(pyramid.as_ref())?;

		let mut tiles_at_2 = Vec::new();
		for bbox in &bboxes {
			let tiles: Vec<_> = op.tile_stream(*bbox).await?.to_vec().await;
			for (coord, tile) in tiles {
				if coord.level == 2 {
					tiles_at_2.push((coord, tile));
				}
			}
		}

		assert!(!tiles_at_2.is_empty(), "overview should generate tiles at level 2");
		Ok(())
	}

	#[test]
	fn test_factory_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.tag_name(), "dem_overview");
	}

	#[test]
	fn test_factory_docs() {
		let factory = Factory {};
		let docs = factory.docs();
		assert!(docs.contains("level"));
		assert!(docs.contains("encoding"));
	}

	// ── full pipeline integration test ────────────────────────────────

	#[tokio::test]
	async fn test_overview_generates_lower_zoom_tiles() -> Result<()> {
		// Create a DEM source image with known elevation values
		let raw = 100_000u32;
		let px = raw_to_rgb(raw);
		let tile_size = 256u32;
		let level_base = 6u8;

		let pyramid = TilePyramid::from_geo_bbox(
			level_base,
			level_base,
			&GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap(),
		)
		.unwrap();

		let mut img = DynamicImage::new_rgb8(tile_size, tile_size);
		for y in 0..tile_size {
			for x in 0..tile_size {
				img.put_pixel(x, y, px.to_rgba());
			}
		}

		let source = DummyImageSource::from_image(img, TileFormat::PNG, Some(pyramid))?;
		let mut tilejson = source.tilejson().clone();
		tilejson.tile_schema = Some(TileSchema::RasterDEMMapbox);

		// Build via VPL to test the full pipeline
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(&format!(
				"from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_overview level={level_base}"
			))
			.await?;

		// Metadata should extend pyramid to level 0
		let pyramid = op.tile_pyramid().await?;
		assert_eq!(pyramid.level_min(), Some(0));
		Ok(())
	}
}
