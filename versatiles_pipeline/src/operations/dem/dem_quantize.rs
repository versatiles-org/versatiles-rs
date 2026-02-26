use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileCoord, TileJSON, TileSchema, TileStream};
use versatiles_derive::context;
use versatiles_image::DynamicImage;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Quantize DEM (elevation) raster tiles by zeroing unnecessary low bits.
///
/// Computes a per-tile quantization mask from two physically meaningful criteria:
/// resolution relative to tile size, and maximum gradient distortion.
/// The stricter (smaller step) wins. Single-pass — no min/max scan needed.
struct Args {
	/// Minimum elevation resolution as fraction of tile ground size.
	/// E.g. 0.001 means for a 1000 m tile, keep 1 m resolution. Defaults to 0.001.
	resolution_ratio: Option<f64>,
	/// Maximum allowed gradient change in degrees due to quantization.
	/// Defaults to 1.0.
	max_gradient_error: Option<f64>,
	/// Override auto-detection of DEM encoding. Values: "mapbox", "terrarium".
	encoding: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DemEncoding {
	Mapbox,
	Terrarium,
}

#[derive(Debug)]
struct Operation {
	source: Box<dyn TileSource>,
	resolution_ratio: f64,
	max_gradient_error: f64,
	encoding: DemEncoding,
}

/// Returns the number of meters per raw DEM unit for the given encoding.
fn raw_unit_meters(encoding: DemEncoding) -> f64 {
	match encoding {
		DemEncoding::Mapbox => 0.1,            // 1 raw unit = 0.1 m
		DemEncoding::Terrarium => 1.0 / 256.0, // 1 raw unit = 1/256 m
	}
}

/// Compute per-channel bit masks for a tile based on its coordinates and quantization parameters.
///
/// Returns `(mask_r, mask_g, mask_b)` to be applied via bitwise AND to each pixel channel.
fn compute_masks_for_tile(
	coord: &TileCoord,
	resolution_ratio: f64,
	max_gradient_error: f64,
	encoding: DemEncoding,
) -> (u8, u8, u8) {
	let tile_ground_meters = coord.ground_size_meters();
	let pixel_meters = tile_ground_meters / 256.0;

	// Criterion 1: resolution ratio
	let max_step_resolution = resolution_ratio * tile_ground_meters;

	// Criterion 2: gradient limit
	let max_step_gradient = pixel_meters * f64::tan(max_gradient_error.to_radians());

	// Pick the stricter constraint
	let max_step_meters = max_step_resolution.min(max_step_gradient);

	// Convert to raw units
	let max_step_raw = max_step_meters / raw_unit_meters(encoding);

	// Compute zero_bits
	let zero_bits = if max_step_raw < 1.0 {
		0u32
	} else {
		#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
		// Safe: max_step_raw >= 1.0, so log2 >= 0 and floor fits in u32
		let bits = (max_step_raw + 1.0).log2().floor() as u32;
		bits
	};
	let zero_bits = zero_bits.min(24);

	let mask_24 = if zero_bits == 0 {
		0x00FF_FFFFu32
	} else {
		0x00FF_FFFFu32 & !((1u32 << zero_bits) - 1)
	};

	let mask_b = (mask_24 & 0xFF) as u8;
	let mask_g = ((mask_24 >> 8) & 0xFF) as u8;
	let mask_r = ((mask_24 >> 16) & 0xFF) as u8;
	(mask_r, mask_g, mask_b)
}

impl Operation {
	#[context("Building dem_quantize operation in VPL node {:?}", vpl_node.name)]
	async fn build(vpl_node: VPLNode, source: Box<dyn TileSource>, _factory: &PipelineFactory) -> Result<Operation>
	where
		Self: Sized + TileSource,
	{
		let args = Args::from_vpl_node(&vpl_node)?;

		let resolution_ratio = args.resolution_ratio.unwrap_or(0.001);
		let max_gradient_error = args.max_gradient_error.unwrap_or(1.0);

		let encoding = if let Some(ref enc_str) = args.encoding {
			match enc_str.as_str() {
				"mapbox" => DemEncoding::Mapbox,
				"terrarium" => DemEncoding::Terrarium,
				other => bail!("Unknown DEM encoding '{other}'; expected 'mapbox' or 'terrarium'"),
			}
		} else {
			match source.tilejson().tile_schema {
				Some(TileSchema::RasterDEMMapbox) => DemEncoding::Mapbox,
				Some(TileSchema::RasterDEMTerrarium) => DemEncoding::Terrarium,
				_ => bail!(
					"tile_schema is not a DEM encoding (mapbox/terrarium); use the 'encoding' parameter to specify one"
				),
			}
		};

		Ok(Self {
			source,
			resolution_ratio,
			max_gradient_error,
			encoding,
		})
	}
}

#[async_trait]
impl TileSource for Operation {
	fn source_type(&self) -> Arc<SourceType> {
		SourceType::new_processor("dem_quantize", self.source.source_type())
	}

	fn metadata(&self) -> &TileSourceMetadata {
		self.source.metadata()
	}

	fn tilejson(&self) -> &TileJSON {
		self.source.tilejson()
	}

	#[context("Failed to get tile stream for bbox: {:?}", bbox)]
	async fn get_tile_stream(&self, bbox: TileBBox) -> Result<TileStream<'static, Tile>> {
		log::debug!("get_tile_stream {bbox:?}");

		let resolution_ratio = self.resolution_ratio;
		let max_gradient_error = self.max_gradient_error;
		let encoding = self.encoding;

		Ok(self
			.source
			.get_tile_stream(bbox)
			.await?
			.map_parallel_try(move |coord, mut tile| {
				let (mask_r, mask_g, mask_b) =
					compute_masks_for_tile(&coord, resolution_ratio, max_gradient_error, encoding);

				let image = tile.as_image_mut()?;
				match image {
					DynamicImage::ImageRgb8(img) => {
						for p in img.pixels_mut() {
							p[0] &= mask_r;
							p[1] &= mask_g;
							p[2] &= mask_b;
						}
					}
					DynamicImage::ImageRgba8(img) => {
						for p in img.pixels_mut() {
							p[0] &= mask_r;
							p[1] &= mask_g;
							p[2] &= mask_b;
						}
					}
					_ => bail!("dem_quantize requires RGB8 or RGBA8 images"),
				}
				tile.set_format_quality(Some(100)); // max quality to preserve pixel values
				Ok(tile)
			})
			.unwrap_results())
	}
}

crate::operations::macros::define_transform_factory!("dem_quantize", Args, Operation);

#[cfg(test)]
mod tests {
	use super::*;
	use crate::factory::OperationFactoryTrait;
	use crate::helpers::dummy_image_source::DummyImageSource;
	use versatiles_core::{TileBBox, TileCoord, TileFormat};
	use versatiles_image::DynamicImage;
	use versatiles_image::traits::DynamicImageTraitConvert;

	/// Combine RGB channels into a single 24-bit raw value for test assertions.
	fn pixel_to_raw(r: u8, g: u8, b: u8) -> u32 {
		(u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
	}

	// ── compute_masks_for_tile unit tests ────────────────────────────────

	#[test]
	fn test_masks_zoom0_equator_mapbox() {
		// Zoom 0 equatorial tile: ground ≈ 40M m, pixel ≈ 156K m
		// With ratio=0.001: step ≈ 40K m → raw ≈ 400K → zero_bits ≈ 18
		// With gradient=1°: step ≈ 2.7K m → raw ≈ 27K → zero_bits ≈ 14
		// Gradient is stricter → zero_bits ≈ 14
		// 14 zero bits: B fully zeroed (bits 0-7), G partially (bits 8-13), R preserved (bits 16-23)
		let coord = TileCoord::new(0, 0, 0).unwrap();
		let (mr, mg, mb) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Mapbox);
		assert_eq!(mr, 0xFF, "R should be preserved at zoom 0 (only 14 bits zeroed)");
		assert_ne!(mg, 0xFF, "G should be partially masked at zoom 0");
		assert_eq!(mb, 0x00, "B should be fully zeroed at zoom 0");
	}

	#[test]
	fn test_masks_zoom14_equator_mapbox() {
		// Zoom 14 equatorial: ground ≈ 2446 m, pixel ≈ 9.56 m
		// ratio=0.001: step ≈ 2.45 m → raw ≈ 24.5 → zero_bits ≈ 4
		// gradient=1°: step ≈ 0.167 m → raw ≈ 1.67 → zero_bits = 1
		// Gradient is stricter → zero_bits ≈ 1
		let coord = TileCoord::new(14, 8192, 8192).unwrap();
		let (mr, mg, mb) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Mapbox);
		// Very few bits zeroed — masks should be close to 0xFF
		assert_eq!(mr, 0xFF, "R should be unmasked at zoom 14");
		assert_eq!(mg, 0xFF, "G should be unmasked at zoom 14");
		// B might lose 1 bit at most
		assert!(mb >= 0xFE, "B should lose at most 1 bit at zoom 14, got {mb:#04X}");
	}

	#[test]
	fn test_masks_zoom18_full_precision() {
		// Zoom 18: ground ≈ 153 m, pixel ≈ 0.6 m
		// ratio=0.001: step ≈ 0.15 m → raw ≈ 1.5 → zero_bits = 1
		// gradient=1°: step ≈ 0.01 m → raw ≈ 0.1 → zero_bits = 0
		// Gradient needs full precision → zero_bits = 0
		let coord = TileCoord::new(18, 131072, 131072).unwrap();
		let (mr, mg, mb) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Mapbox);
		assert_eq!(
			(mr, mg, mb),
			(0xFF, 0xFF, 0xFF),
			"Zoom 18 should preserve full precision"
		);
	}

	#[test]
	fn test_masks_terrarium_encoding() {
		// Terrarium has finer raw units (1/256 m vs 0.1 m), so same step allows
		// more zero bits in terrarium than mapbox
		let coord = TileCoord::new(8, 128, 128).unwrap();
		let (mr_m, mg_m, mb_m) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Mapbox);
		let (mr_t, mg_t, mb_t) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Terrarium);
		// Terrarium raw unit is ~25.6x smaller than Mapbox, so max_step_raw is ~25.6x larger
		// This means more zero_bits for Terrarium
		let mapbox_mask = pixel_to_raw(mr_m, mg_m, mb_m);
		let terrarium_mask = pixel_to_raw(mr_t, mg_t, mb_t);
		assert!(
			terrarium_mask <= mapbox_mask,
			"Terrarium should quantize more aggressively (more zeros) than Mapbox"
		);
	}

	#[test]
	fn test_masks_high_latitude_less_aggressive() {
		// At high latitude, ground size is smaller → less quantization
		let equator = TileCoord::new(8, 128, 128).unwrap();
		let high_lat = TileCoord::new(8, 128, 16).unwrap(); // near pole

		let (mr_eq, mg_eq, mb_eq) = compute_masks_for_tile(&equator, 0.001, 1.0, DemEncoding::Mapbox);
		let (mr_hl, mg_hl, mb_hl) = compute_masks_for_tile(&high_lat, 0.001, 1.0, DemEncoding::Mapbox);

		let mask_eq = pixel_to_raw(mr_eq, mg_eq, mb_eq);
		let mask_hl = pixel_to_raw(mr_hl, mg_hl, mb_hl);
		assert!(
			mask_hl >= mask_eq,
			"High latitude should quantize less aggressively (preserve more bits)"
		);
	}

	#[test]
	fn test_masks_stricter_ratio_preserves_more() {
		let coord = TileCoord::new(8, 128, 128).unwrap();
		let (mr1, mg1, mb1) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Mapbox);
		let (mr2, mg2, mb2) = compute_masks_for_tile(&coord, 0.0001, 1.0, DemEncoding::Mapbox);

		let mask1 = pixel_to_raw(mr1, mg1, mb1);
		let mask2 = pixel_to_raw(mr2, mg2, mb2);
		assert!(mask2 >= mask1, "Stricter resolution_ratio should preserve more bits");
	}

	#[test]
	fn test_masks_stricter_gradient_preserves_more() {
		let coord = TileCoord::new(8, 128, 128).unwrap();
		let (mr1, mg1, mb1) = compute_masks_for_tile(&coord, 0.001, 2.0, DemEncoding::Mapbox);
		let (mr2, mg2, mb2) = compute_masks_for_tile(&coord, 0.001, 0.5, DemEncoding::Mapbox);

		let mask1 = pixel_to_raw(mr1, mg1, mb1);
		let mask2 = pixel_to_raw(mr2, mg2, mb2);
		assert!(mask2 >= mask1, "Stricter max_gradient_error should preserve more bits");
	}

	// ── pixel_to_raw unit test ──────────────────────────────────────────

	#[test]
	fn test_pixel_to_raw() {
		assert_eq!(pixel_to_raw(0x12, 0x34, 0x56), 0x123456);
		assert_eq!(pixel_to_raw(0, 0, 0), 0);
		assert_eq!(pixel_to_raw(255, 255, 255), 0x00FF_FFFF);
	}

	// ── pixel processing tests ──────────────────────────────────────────

	fn raw_to_rgb(v: u32) -> (u8, u8, u8) {
		(
			u8::try_from((v >> 16) & 0xFF).unwrap(),
			u8::try_from((v >> 8) & 0xFF).unwrap(),
			u8::try_from(v & 0xFF).unwrap(),
		)
	}

	#[tokio::test]
	async fn test_quantize_rgb_tile() -> Result<()> {
		// Create a 2x2 RGB tile with known pixel values
		let raw_values: [u32; 4] = [100_000, 100_100, 100_200, 100_640];
		let mut raw_data: Vec<u8> = Vec::new();
		for v in &raw_values {
			let (r, g, b) = raw_to_rgb(*v);
			raw_data.extend_from_slice(&[r, g, b]);
		}
		let image = DynamicImage::from_raw(2, 2, raw_data)?;

		let source = DummyImageSource::from_image(image, TileFormat::PNG, None)?;
		let op = Operation {
			source: Box::new(source),
			resolution_ratio: 0.001,
			max_gradient_error: 1.0,
			encoding: DemEncoding::Mapbox,
		};

		let bbox = TileBBox::from_min_and_max(8, 56, 56, 56, 56)?;
		let mut tiles = op.get_tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let result_image = tiles[0].1.as_image_mut()?;
		// At zoom 8 equatorial with default params, some low bits should be zeroed
		match result_image {
			DynamicImage::ImageRgb8(img) => {
				// Compute expected masks for tile (8, 56, 56)
				let coord = TileCoord::new(8, 56, 56).unwrap();
				let (_mr, _mg, mb) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Mapbox);
				let zeroed_bits = !mb;
				for p in img.pixels() {
					assert_eq!(p[2] & zeroed_bits, 0, "Low bits of B channel should be zeroed");
				}
			}
			_ => bail!("Expected RGB8 image"),
		}
		Ok(())
	}

	#[tokio::test]
	async fn test_quantize_rgba_tile() -> Result<()> {
		// Create a 2x2 RGBA tile
		let raw_values: [u32; 4] = [100_000, 100_100, 100_200, 100_640];
		let mut raw_data: Vec<u8> = Vec::new();
		for v in &raw_values {
			let (r, g, b) = raw_to_rgb(*v);
			raw_data.extend_from_slice(&[r, g, b, 0xFF]);
		}
		let image = DynamicImage::from_raw(2, 2, raw_data)?;

		let source = DummyImageSource::from_image(image, TileFormat::PNG, None)?;
		let op = Operation {
			source: Box::new(source),
			resolution_ratio: 0.001,
			max_gradient_error: 1.0,
			encoding: DemEncoding::Mapbox,
		};

		let bbox = TileBBox::from_min_and_max(8, 56, 56, 56, 56)?;
		let mut tiles = op.get_tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let result_image = tiles[0].1.as_image_mut()?;
		match result_image {
			DynamicImage::ImageRgba8(img) => {
				let coord = TileCoord::new(8, 56, 56).unwrap();
				let (_mr, _mg, mb) = compute_masks_for_tile(&coord, 0.001, 1.0, DemEncoding::Mapbox);
				let zeroed_bits = !mb;
				for p in img.pixels() {
					assert_eq!(p[2] & zeroed_bits, 0, "Low bits of B channel should be zeroed");
					assert_eq!(p[3], 0xFF, "Alpha should be unchanged");
				}
			}
			_ => bail!("Expected RGBA8 image"),
		}
		Ok(())
	}

	// ── factory tests ───────────────────────────────────────────────────

	#[test]
	fn test_factory_get_tag_name() {
		let factory = Factory {};
		assert_eq!(factory.get_tag_name(), "dem_quantize");
	}

	#[test]
	fn test_factory_get_docs() {
		let factory = Factory {};
		let docs = factory.get_docs();
		assert!(docs.contains("resolution_ratio"));
		assert!(docs.contains("max_gradient_error"));
		assert!(docs.contains("encoding"));
	}

	// ── build tests ─────────────────────────────────────────────────────

	#[tokio::test]
	async fn test_build_with_mapbox_schema() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_quantize resolution_ratio=0.001",
			)
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}

	#[tokio::test]
	async fn test_build_with_terrarium_schema() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl(
				"from_debug format=png | meta_update schema=\"dem/terrarium\" | dem_quantize max_gradient_error=0.5",
			)
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}

	#[tokio::test]
	async fn test_build_with_encoding_override() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | dem_quantize encoding=mapbox")
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}

	#[tokio::test]
	async fn test_build_fails_without_dem_schema() {
		let factory = PipelineFactory::new_dummy();
		let result = factory.operation_from_vpl("from_debug format=png | dem_quantize").await;
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
			.operation_from_vpl("from_debug format=png | dem_quantize encoding=invalid")
			.await;
		assert!(result.is_err());
	}

	#[tokio::test]
	async fn test_source_type() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | dem_quantize encoding=mapbox")
			.await?;
		let source_type = op.source_type();
		assert!(source_type.to_string().contains("dem_quantize"));
		Ok(())
	}

	#[tokio::test]
	async fn test_defaults() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | dem_quantize encoding=terrarium")
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}
}
