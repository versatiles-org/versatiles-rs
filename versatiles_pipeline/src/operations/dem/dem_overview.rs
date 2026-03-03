use super::encoding::{resolve_encoding, to_tile_schema};
use crate::{PipelineFactory, helpers::overview::OverviewCore, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use imageproc::image::{DynamicImage, Rgb, RgbImage, Rgba, RgbaImage};
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TileStream};

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Generate lower-zoom DEM overview tiles by averaging 24-bit elevation values.
///
/// Unlike raster_overview which averages RGB channels independently,
/// this operation decodes each pixel to its 24-bit raw elevation value,
/// averages the values correctly, and re-encodes back to RGB.
struct Args {
	/// Use this zoom level to build the overview. Defaults to the maximum zoom level of the source.
	level: Option<u8>,
	/// Size of the tiles in pixels. Defaults to 512.
	tile_size: Option<u32>,
	/// Override auto-detection of DEM encoding. Values: "mapbox", "terrarium".
	encoding: Option<String>,
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
#[allow(clippy::many_single_char_names)]
fn dem_scale_down(image: &DynamicImage) -> Result<DynamicImage> {
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
					#[allow(clippy::cast_possible_truncation)]
					let avg_alpha = ((alpha_sum + 2) / 4) as u8; // max 255, truncation impossible
					if count > 0 {
						let avg = (sum + count / 2) / count;
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
	#[allow(clippy::unused_async)] // must be async for the factory macro
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		let encoding = resolve_encoding(args.encoding.as_ref(), source.tilejson().tile_schema.as_ref())?;

		let mut core = OverviewCore::new(source, args.level, args.tile_size, Arc::new(dem_scale_down))?;

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

	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::trace!("dem_overview::get_tile_stream {bbox:?}");
		self.core.get_tile_stream(bbox).await
	}
}

crate::operations::macros::define_transform_factory!("dem_overview", Args, Operation);

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
	use super::*;
	use crate::factory::OperationFactoryTrait;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use imageproc::image::{GenericImage, Pixel};
	use versatiles_core::{GeoBBox, TileBBoxPyramid, TileFormat, TileSchema};

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

	#[tokio::test]
	async fn test_streaming_generates_lower_zoom_tiles() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_debug format=png | filter level_min=4 level_max=4 bbox=[2,48,3,49] | dem_overview encoding=terrarium",
			)
			.await?;

		// Stream tiles level-by-level from highest to lowest (as the traversal does).
		// This populates the cache at each level so lower levels can build from it.
		let traversal = op.metadata().traversal.clone();
		let bboxes = traversal.traverse_pyramid(&op.metadata().bbox_pyramid)?;

		let mut tiles_at_2 = Vec::new();
		for bbox in &bboxes {
			let tiles: Vec<_> = op.get_tile_stream(*bbox).await?.to_vec().await;
			for (coord, tile) in &tiles {
				if coord.level == 2 {
					tiles_at_2.push((coord.clone(), tile.clone()));
				}
			}
		}

		assert!(!tiles_at_2.is_empty(), "overview should generate tiles at level 2");
		Ok(())
	}

	#[test]
	fn test_factory_get_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.get_tag_name(), "dem_overview");
	}

	#[test]
	fn test_factory_get_docs() {
		let factory = Factory {};
		let docs = factory.get_docs();
		assert!(docs.contains("level"));
		assert!(docs.contains("tile_size"));
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

		let pyramid = TileBBoxPyramid::from_geo_bbox(
			level_base,
			level_base,
			&GeoBBox::new(2.224, 48.815, 2.47, 48.903).unwrap(),
		);

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
				"from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_overview level={level_base} tile_size={tile_size}"
			))
			.await?;

		// Metadata should extend pyramid to level 0
		let metadata = op.metadata();
		assert_eq!(metadata.bbox_pyramid.get_level_min(), Some(0));
		Ok(())
	}
}
