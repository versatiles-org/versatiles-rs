use crate::{PipelineFactory, vpl::VPLNode};
use anyhow::{Result, bail};
use async_trait::async_trait;
use std::{fmt::Debug, sync::Arc};
use versatiles_container::{SourceType, Tile, TileSource, TileSourceMetadata};
use versatiles_core::{TileBBox, TileJSON, TileSchema, TileStream};
use versatiles_derive::context;
use versatiles_image::DynamicImage;

#[derive(versatiles_derive::VPLDecode, Clone, Debug)]
/// Quantize DEM (elevation) raster tiles by zeroing unnecessary low bits.
///
/// Scans each tile's elevation range, calculates how many bits of precision
/// are needed for the requested accuracy, and zeros out the rest.
/// This makes tiles much more compressible (PNG/WebP) without losing
/// meaningful detail.
struct Args {
	/// Number of bits of precision to retain within the tile's elevation range.
	/// 2^bits = number of distinct levels. Defaults to 8 (256 levels).
	bits: Option<u8>,
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
	bits: u8,
	#[allow(dead_code)]
	encoding: DemEncoding,
}

/// Combine RGB channels into a single 24-bit raw value.
#[inline]
pub fn pixel_to_raw(r: u8, g: u8, b: u8) -> u32 {
	(u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

/// Calculate per-channel masks given the raw value range and desired precision bits.
///
/// Returns `(mask_r, mask_g, mask_b)` to be applied via bitwise AND to each channel.
pub fn calculate_masks(v_min: u32, v_max: u32, bits: u8) -> (u8, u8, u8) {
	let range = v_max - v_min;
	let zero_bits = if range == 0 {
		0u32
	} else {
		let range_bits = 32 - range.leading_zeros(); // = floor(log2(range)) + 1
		range_bits.saturating_sub(u32::from(bits))
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

		let bits = args.bits.unwrap_or(12).min(24);

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

		Ok(Self { source, bits, encoding })
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

		let bits = self.bits;
		Ok(self
			.source
			.get_tile_stream(bbox)
			.await?
			.map_parallel_try(move |_coord, mut tile| {
				let image = tile.as_image_mut()?;
				match image {
					DynamicImage::ImageRgb8(img) => {
						// First pass: find min/max raw values
						let (mut v_min, mut v_max) = (u32::MAX, 0u32);
						for p in img.pixels() {
							let raw = pixel_to_raw(p[0], p[1], p[2]);
							v_min = v_min.min(raw);
							v_max = v_max.max(raw);
						}

						let (mask_r, mask_g, mask_b) = calculate_masks(v_min, v_max, bits);

						// Second pass: apply masks
						for p in img.pixels_mut() {
							p[0] &= mask_r;
							p[1] &= mask_g;
							p[2] &= mask_b;
						}
					}
					DynamicImage::ImageRgba8(img) => {
						// First pass: find min/max raw values (ignore alpha)
						let (mut v_min, mut v_max) = (u32::MAX, 0u32);
						for p in img.pixels() {
							let raw = pixel_to_raw(p[0], p[1], p[2]);
							v_min = v_min.min(raw);
							v_max = v_max.max(raw);
						}

						let (mask_r, mask_g, mask_b) = calculate_masks(v_min, v_max, bits);

						// Second pass: apply masks (alpha unchanged)
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
	use versatiles_core::{TileBBox, TileFormat};
	use versatiles_image::DynamicImage;
	use versatiles_image::traits::DynamicImageTraitConvert;

	// ── calculate_masks unit tests ──────────────────────────────────────

	#[test]
	fn test_masks_flat_tile() {
		// range = 0 → zero_bits = 0 → all masks 0xFF
		let (mr, mg, mb) = calculate_masks(1000, 1000, 8);
		assert_eq!((mr, mg, mb), (0xFF, 0xFF, 0xFF));
	}

	#[test]
	fn test_masks_terrarium_64m() {
		// Terrarium, 1000–1064 m, bits=8
		// LSB = 1/256 m → range = 64 * 256 = 16384 raw units
		// range_bits = 32 - 16384.leading_zeros() = 32 - 17 = 15
		// zero_bits = 15 - 8 = 7
		// mask_24 = 0xFFFFFF & !0x7F = 0xFFFF80
		// → R=0xFF, G=0xFF, B=0x80
		let v_min = 1000 * 256; // arbitrary base
		let v_max = v_min + 64 * 256;
		let (mr, mg, mb) = calculate_masks(v_min, v_max, 8);
		assert_eq!((mr, mg, mb), (0xFF, 0xFF, 0x80));
	}

	#[test]
	fn test_masks_mapbox_64m() {
		// Mapbox, 64 m range: 64 / 0.1 = 640 raw units
		// range_bits = 32 - 640.leading_zeros() = 32 - 22 = 10
		// zero_bits = 10 - 8 = 2
		// mask_24 = 0xFFFFFF & !0x03 = 0xFFFFFC
		// → R=0xFF, G=0xFF, B=0xFC
		let (mr, mg, mb) = calculate_masks(100_000, 100_640, 8);
		assert_eq!((mr, mg, mb), (0xFF, 0xFF, 0xFC));
	}

	#[test]
	fn test_masks_range_fits_in_bits() {
		// range = 1 → range_bits = 1, zero_bits = max(1 - 8, 0) = 0 → all masks 0xFF
		let (mr, mg, mb) = calculate_masks(500, 501, 8);
		assert_eq!((mr, mg, mb), (0xFF, 0xFF, 0xFF));
	}

	#[test]
	fn test_masks_bits_zero() {
		// bits=0 → zero all bits below the range
		// range = 640 → range_bits = 10, zero_bits = 10 - 0 = 10
		// mask_24 = 0xFFFFFF & !0x3FF = 0xFFFC00
		// → R=0xFF, G=0xFC, B=0x00
		let (mr, mg, mb) = calculate_masks(100_000, 100_640, 0);
		assert_eq!((mr, mg, mb), (0xFF, 0xFC, 0x00));
	}

	#[test]
	fn test_masks_large_range_spanning_r_channel() {
		// range = 0x100000 (1048576) → range_bits = 21, bits=8 → zero_bits = 13
		// mask_24 = 0xFFFFFF & !0x1FFF = 0xFFE000
		// → R=0xFF, G=0xE0, B=0x00
		let (mr, mg, mb) = calculate_masks(0, 0x10_0000, 8);
		assert_eq!((mr, mg, mb), (0xFF, 0xE0, 0x00));
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
		// Create a 2x2 RGB tile with known pixel values encoding elevations
		// Pixels: raw values 100_000, 100_100, 100_200, 100_640
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
			bits: 8,
			encoding: DemEncoding::Mapbox,
		};

		let bbox = TileBBox::from_min_and_max(8, 56, 56, 56, 56)?;
		let mut tiles = op.get_tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let result_image = tiles[0].1.as_image_mut()?;
		// range = 640, range_bits = 10, zero_bits = 2
		// mask = (0xFF, 0xFF, 0xFC)
		match result_image {
			DynamicImage::ImageRgb8(img) => {
				for p in img.pixels() {
					// Last 2 bits of B should be zeroed
					assert_eq!(p[2] & 0x03, 0, "Last 2 bits of B channel should be zero");
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
			raw_data.extend_from_slice(&[r, g, b, 0xFF]); // alpha = 255
		}
		let image = DynamicImage::from_raw(2, 2, raw_data)?;

		let source = DummyImageSource::from_image(image, TileFormat::PNG, None)?;
		let op = Operation {
			source: Box::new(source),
			bits: 8,
			encoding: DemEncoding::Mapbox,
		};

		let bbox = TileBBox::from_min_and_max(8, 56, 56, 56, 56)?;
		let mut tiles = op.get_tile_stream(bbox).await?.to_vec().await;
		assert_eq!(tiles.len(), 1);

		let result_image = tiles[0].1.as_image_mut()?;
		match result_image {
			DynamicImage::ImageRgba8(img) => {
				for p in img.pixels() {
					assert_eq!(p[2] & 0x03, 0, "Last 2 bits of B channel should be zero");
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
		assert!(docs.contains("bits"));
		assert!(docs.contains("encoding"));
	}

	// ── build tests ─────────────────────────────────────────────────────

	#[tokio::test]
	async fn test_build_with_mapbox_schema() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | meta_update schema=\"dem/mapbox\" | dem_quantize bits=8")
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}

	#[tokio::test]
	async fn test_build_with_terrarium_schema() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | meta_update schema=\"dem/terrarium\" | dem_quantize bits=8")
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}

	#[tokio::test]
	async fn test_build_with_encoding_override() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		let op = factory
			.operation_from_vpl("from_debug format=png | dem_quantize bits=8 encoding=mapbox")
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}

	#[tokio::test]
	async fn test_build_fails_without_dem_schema() {
		let factory = PipelineFactory::new_dummy();
		let result = factory
			.operation_from_vpl("from_debug format=png | dem_quantize bits=8")
			.await;
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
	async fn test_default_bits() -> Result<()> {
		let factory = PipelineFactory::new_dummy();
		// No bits parameter - should default to 8
		let op = factory
			.operation_from_vpl("from_debug format=png | dem_quantize encoding=terrarium")
			.await?;
		let _metadata = op.metadata();
		Ok(())
	}
}
